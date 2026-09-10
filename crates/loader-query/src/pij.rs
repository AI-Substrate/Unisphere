//! Optional, bounded Pij seat-to-native-session resolution.

use serde_json::{Map, Value};
use std::{
    fmt,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, TryRecvError},
    thread,
    time::{Duration, Instant},
};
use unisphere_core::query::HarnessId;

const DEFAULT_MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// Per-call resource limits for a Pij identity lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PijLookupLimits {
    pub max_output_bytes: usize,
    pub timeout: Duration,
}

impl Default for PijLookupLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl PijLookupLimits {
    fn validate(self) -> Result<Self, PijLookupError> {
        if self.max_output_bytes == 0
            || self.max_output_bytes > MAX_OUTPUT_BYTES
            || self.timeout.is_zero()
            || self.timeout > MAX_TIMEOUT
        {
            return Err(PijLookupError::InvalidLimits);
        }
        Ok(self)
    }
}

/// One current Pij seat mapping. Callers pin this snapshot for one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPijSession {
    pub pij_id: String,
    pub harness: HarnessId,
    pub native_session_id: String,
    pub cwd: Option<PathBuf>,
    pub machine: Option<String>,
}

/// Payload-free Pij lookup failures. Diagnostics from the child process are discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PijLookupError {
    MissingExecutable,
    LookupUnavailable,
    UnknownSeat,
    NativeSessionUnavailable,
    UnsupportedHarness,
    InvalidResponse,
    InvalidInput,
    OutputLimitExceeded,
    Timeout,
    InvalidLimits,
}

impl PijLookupError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::MissingExecutable => "UNI-PIJ-MISSING",
            Self::LookupUnavailable => "UNI-PIJ-UNAVAILABLE",
            Self::UnknownSeat => "UNI-PIJ-UNKNOWN",
            Self::NativeSessionUnavailable => "UNI-PIJ-NATIVE-ID",
            Self::UnsupportedHarness => "UNI-PIJ-HARNESS",
            Self::InvalidResponse => "UNI-PIJ-RESPONSE",
            Self::InvalidInput => "UNI-PIJ-INPUT",
            Self::OutputLimitExceeded => "UNI-PIJ-OUTPUT-LIMIT",
            Self::Timeout => "UNI-PIJ-TIMEOUT",
            Self::InvalidLimits => "UNI-PIJ-LIMITS",
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::MissingExecutable => "Pij integration is not available",
            Self::LookupUnavailable => "Pij lookup is unavailable",
            Self::UnknownSeat => "Pij seat is unknown",
            Self::NativeSessionUnavailable => "Pij seat has no recorded native session",
            Self::UnsupportedHarness => "Pij seat uses an unsupported harness",
            Self::InvalidResponse => "Pij returned an invalid response",
            Self::InvalidInput => "Pij lookup input is invalid",
            Self::OutputLimitExceeded => "Pij response exceeded the output limit",
            Self::Timeout => "Pij lookup timed out",
            Self::InvalidLimits => "Pij lookup limits are invalid",
        }
    }
}

impl fmt::Display for PijLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for PijLookupError {}

/// Resolve `id` through exactly one `pij state <id> --json` invocation.
///
/// The supplied executable is the only program invoked directly. Pij retains
/// ownership of its ordinary daemon address and authentication mechanics.
pub fn resolve_pij(
    executable: Option<&Path>,
    id: &str,
    limits: PijLookupLimits,
) -> Result<ResolvedPijSession, PijLookupError> {
    let limits = limits.validate()?;
    if id.is_empty() || id.trim() != id || id.chars().any(|character| character.is_control()) {
        return Err(PijLookupError::InvalidInput);
    }
    let deadline = Instant::now()
        .checked_add(limits.timeout)
        .ok_or(PijLookupError::InvalidLimits)?;
    let executable = executable.ok_or(PijLookupError::MissingExecutable)?;

    let mut child = Command::new(executable)
        .args(["state", id, "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                PijLookupError::MissingExecutable
            } else {
                PijLookupError::LookupUnavailable
            }
        })?;
    let Some(stdout) = child.stdout.take() else {
        stop_child(&mut child);
        return Err(PijLookupError::LookupUnavailable);
    };
    let read_limit = u64::try_from(limits.max_output_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = thread::Builder::new()
        .name("pij-lookup-output".into())
        .spawn(move || {
            let mut output = Vec::new();
            let result = stdout
                .take(read_limit)
                .read_to_end(&mut output)
                .map(|_| output);
            let _ = sender.send(result);
        });
    if reader.is_err() {
        stop_child(&mut child);
        return Err(PijLookupError::LookupUnavailable);
    }

    let mut output = None;
    let status = loop {
        if output.is_none() {
            match receiver.try_recv() {
                Ok(Ok(bytes)) if bytes.len() > limits.max_output_bytes => {
                    stop_child(&mut child);
                    return Err(PijLookupError::OutputLimitExceeded);
                }
                Ok(Ok(bytes)) => output = Some(bytes),
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                    stop_child(&mut child);
                    return Err(PijLookupError::LookupUnavailable);
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        let status = match child.try_wait() {
            Ok(status) => status,
            Err(_) => {
                stop_child(&mut child);
                return Err(PijLookupError::LookupUnavailable);
            }
        };
        if let (Some(status), Some(_)) = (status, output.as_ref()) {
            break status;
        }
        if Instant::now() >= deadline {
            stop_child(&mut child);
            return Err(PijLookupError::Timeout);
        }
        thread::sleep(POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())));
    };
    let output = output.ok_or(PijLookupError::LookupUnavailable)?;

    parse_response(id, status, &output)
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn parse_response(
    requested_id: &str,
    status: ExitStatus,
    output: &[u8],
) -> Result<ResolvedPijSession, PijLookupError> {
    let response: Value = serde_json::from_slice(output).map_err(|_| {
        if status.success() {
            PijLookupError::InvalidResponse
        } else {
            PijLookupError::LookupUnavailable
        }
    })?;
    let response = response
        .as_object()
        .ok_or(PijLookupError::InvalidResponse)?;
    let ok = response
        .get("ok")
        .and_then(Value::as_bool)
        .ok_or(PijLookupError::InvalidResponse)?;
    if !ok {
        return if response.get("error").and_then(Value::as_str) == Some("not_found") {
            Err(PijLookupError::UnknownSeat)
        } else {
            Err(PijLookupError::LookupUnavailable)
        };
    }
    if !status.success() {
        return Err(PijLookupError::LookupUnavailable);
    }

    let data = response
        .get("data")
        .and_then(Value::as_object)
        .ok_or(PijLookupError::InvalidResponse)?;
    let returned_id = required_string(data, "id")?;
    if returned_id != requested_id {
        return Err(PijLookupError::InvalidResponse);
    }
    let harness = map_harness(required_string(data, "harness")?)?;
    let native_session_id = match data.get("session") {
        Some(Value::Null) => return Err(PijLookupError::NativeSessionUnavailable),
        Some(Value::String(value)) if !value.is_empty() => value.clone(),
        _ => return Err(PijLookupError::InvalidResponse),
    };
    let cwd = optional_string(data, "cwd")?.map(PathBuf::from);
    let machine = optional_string(data, "machine")?.map(str::to_owned);

    Ok(ResolvedPijSession {
        pij_id: returned_id.to_owned(),
        harness,
        native_session_id,
        cwd,
        machine,
    })
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, PijLookupError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(PijLookupError::InvalidResponse)
}

fn optional_string<'a>(
    object: &'a Map<String, Value>,
    field: &str,
) -> Result<Option<&'a str>, PijLookupError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.is_empty() => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(PijLookupError::InvalidResponse),
    }
}

fn map_harness(value: &str) -> Result<HarnessId, PijLookupError> {
    let harness = match value {
        "claude" => "claude-code",
        "copilot" => "copilot-cli",
        "codex" => "codex",
        "omp" => "oh-my-pi",
        "pi" => "pi",
        _ => return Err(PijLookupError::UnsupportedHarness),
    };
    HarnessId::new(harness).map_err(|_| PijLookupError::InvalidResponse)
}
