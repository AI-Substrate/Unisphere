//! Pure VS Code Copilot folds into canonical prep tables.
//!
//! Supplied snapshots in, metadata facts out; no filesystem, clock or
//! environment. Two folds share the catalogue harness id: one for whole JSON
//! documents, one for native mutation journals. A journal is reduced to its
//! current document with this crate's replay before folding, so a journal and
//! its equivalent document produce equal rows and facts.
//!
//! A snapshot source is interpreted per revision: a changed revision is a new
//! generation, so one fold session folds exactly one revision. Rules:
//!
//! - Every request is a trigger (`human`, or `other` when the request is
//!   system-initiated). A request with a non-null `response` is one call and
//!   opens the next turn; a request without one leaves its trigger unopened.
//! - A call carries `responseId`/`requestId`, the requested `modelId`,
//!   `promptTokens` as input and `completionTokens` as output. VS Code records
//!   no per-call cache counters, so cache fields are null with basis `none`;
//!   whole-turn `modelTotals` have a different scope and are not mapped.
//! - `stop_reason` is the native response state (`modelState.value`) by name,
//!   else `cancelled` when `isCanceled` is true.
//! - Every `toolInvocationSerialized` part is a tool use; a complete one also
//!   has a result sighting whose outcome is the native `resultDetails.isError`
//!   when present, else unknown. Tool input, output and duration are not
//!   recorded, so they stay null.
//! - Timestamps are native epoch milliseconds; `ts` is their exact RFC 3339
//!   UTC rendering. The dialect has no native compaction or model-switch
//!   marker, so those facts are absent, never inferred.
//! - `native_key` is the JSON pointer of the row's native location in the
//!   current document (for example `/requests/0/response/2`).
//!
//! Message text is read only to count characters and derive the body hash. It
//! leaves the fold only as `triggers.content_head`, under explicit opt-in.

use serde_json::{Map, Value, json};
use unisphere_core::{
    NativeSnapshot, PipelineError, PipelineErrorKind, SnapshotFormat,
    prep::{
        CacheWriteBasis, CallSighting, ContextSample, PREP_CHECKPOINT_FORMAT, PrepCallRow,
        PrepCheckpoint, PrepFold, PrepFoldSession, PrepInput, PrepOptions, PrepRows,
        PrepSourceKind, PrepSourceMeta, PrepToolUseRow, PrepTriggerRow, PrepTurnRow, SessionFacts,
        ToolOutcome, ToolSighting, TurnOrigin,
    },
};

use crate::{DESCRIPTOR, journal, tool_family};

/// Interpretation policy of whole JSON documents; bump on any rule change.
pub const DOCUMENT_PREP_POLICY_VERSION: &str = "vscode-copilot/document-prep-v1";
/// Interpretation policy of reduced JSON journals; bump on any rule change.
pub const JOURNAL_PREP_POLICY_VERSION: &str = "vscode-copilot/journal-prep-v1";

/// VS Code Copilot chat-session fold, one value per native representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VsCodeCopilotPrepFold {
    /// `*/chatSessions/*.json` whole documents.
    Document,
    /// `*/chatSessions/*.jsonl` mutation journals.
    Journal,
}

fn invalid_data() -> PipelineError {
    PipelineError::new(PipelineErrorKind::InvalidData, None)
}

fn invalid_input() -> PipelineError {
    PipelineError::new(PipelineErrorKind::InvalidInput, None)
}

impl PrepFold for VsCodeCopilotPrepFold {
    fn harness(&self) -> &'static str {
        DESCRIPTOR.id
    }
    fn policy(&self) -> &'static str {
        match self {
            Self::Document => DOCUMENT_PREP_POLICY_VERSION,
            Self::Journal => JOURNAL_PREP_POLICY_VERSION,
        }
    }
    fn kind(&self) -> PrepSourceKind {
        PrepSourceKind::Snapshot
    }
    fn pattern(&self) -> &'static str {
        match self {
            Self::Document => "*/chatSessions/*.json",
            Self::Journal => "*/chatSessions/*.jsonl",
        }
    }
    /// `<workspace storage id>/chatSessions/<file>`: the workspace storage
    /// directory is the project; VS Code sessions have no subagent files.
    fn describe(&self, file: &str) -> PrepSourceMeta {
        PrepSourceMeta {
            is_sub: false,
            agent_id: None,
            project: file
                .split_once('/')
                .map(|(project, _)| project)
                .filter(|project| !project.is_empty())
                .map(str::to_owned),
        }
    }
    fn open(
        &self,
        _meta: &PrepSourceMeta,
        source: &str,
        generation: u32,
        saved: Option<&PrepCheckpoint>,
    ) -> Result<Box<dyn PrepFoldSession>, PipelineError> {
        let mut state = State {
            fold: *self,
            source: source.to_owned(),
            generation,
            revision: None,
            session: SessionFacts::default(),
        };
        match saved {
            None => {}
            Some(checkpoint)
                if checkpoint.format == PREP_CHECKPOINT_FORMAT
                    && checkpoint.policy == self.policy() =>
            {
                state.load(&checkpoint.fold).ok_or_else(invalid_data)?;
            }
            Some(_) => return Err(invalid_data()),
        }
        Ok(Box::new(state))
    }
}

#[derive(Debug, Clone)]
struct State {
    fold: VsCodeCopilotPrepFold,
    source: String,
    generation: u32,
    /// Revision folded by this generation; `None` until the snapshot is folded.
    revision: Option<String>,
    session: SessionFacts,
}

impl State {
    fn save(&self) -> Value {
        json!({
            "revision": self.revision,
            "session": serde_json::to_value(&self.session).unwrap_or(Value::Null),
        })
    }

    /// Strict inverse of [`State::save`]; anything else is refused.
    fn load(&mut self, value: &Value) -> Option<()> {
        let object = value.as_object()?;
        if object.len() != 2 {
            return None;
        }
        self.revision = match object.get("revision")? {
            Value::Null => None,
            Value::String(revision) if !revision.is_empty() => Some(revision.clone()),
            _ => return None,
        };
        self.session = serde_json::from_value(object.get("session")?.clone()).ok()?;
        Some(())
    }

    /// The current session document of `snapshot` in this fold's representation.
    fn document(&self, snapshot: &NativeSnapshot) -> Result<Value, PipelineError> {
        match (self.fold, &snapshot.source.format) {
            (VsCodeCopilotPrepFold::Document, SnapshotFormat::JsonDocument) => {
                let [record] = snapshot.records.as_slice() else {
                    return Err(invalid_data());
                };
                if record.key != "document" {
                    return Err(invalid_data());
                }
                serde_json::from_slice(&record.bytes).map_err(|_| invalid_data())
            }
            (VsCodeCopilotPrepFold::Journal, SnapshotFormat::JsonJournal) => {
                journal::reduce(snapshot)
            }
            _ => Err(invalid_input()),
        }
    }
}

impl PrepFoldSession for State {
    fn checkpoint(&self) -> PrepCheckpoint {
        PrepCheckpoint {
            format: PREP_CHECKPOINT_FORMAT,
            policy: self.fold.policy().to_owned(),
            fold: self.save(),
        }
    }

    fn facts(&self) -> SessionFacts {
        self.session.clone()
    }

    /// Folds one whole snapshot. The folded revision again yields no rows; any
    /// other revision is a new generation and is refused here.
    fn fold(&mut self, input: &PrepInput, options: PrepOptions) -> Result<PrepRows, PipelineError> {
        let PrepInput::Snapshot(snapshot) = input else {
            return Err(invalid_input());
        };
        if snapshot.revision.is_empty() {
            return Err(invalid_data());
        }
        if let Some(revision) = &self.revision {
            return if *revision == snapshot.revision {
                Ok(PrepRows::default())
            } else {
                Err(invalid_input())
            };
        }
        let document = self.document(snapshot)?;
        let Some(session) = document.as_object() else {
            return Err(invalid_data());
        };
        let mut folder = Folder {
            source: &self.source,
            generation: self.generation,
            options,
            rows: PrepRows::default(),
            facts: SessionFacts::default(),
            turn_no: 0,
            last_call_ts: None,
        };
        folder.session(session, snapshot.source.session_id.as_deref())?;
        // Only a successful fold changes the state.
        self.revision = Some(snapshot.revision.clone());
        self.session = folder.facts;
        Ok(folder.rows)
    }
}

struct Folder<'a> {
    source: &'a str,
    generation: u32,
    options: PrepOptions,
    rows: PrepRows,
    facts: SessionFacts,
    turn_no: i64,
    /// `None` before the first call; then the latest call's native timestamp.
    last_call_ts: Option<Option<i64>>,
}

impl Folder<'_> {
    fn session(
        &mut self,
        session: &Map<String, Value>,
        selected: Option<&str>,
    ) -> Result<(), PipelineError> {
        self.facts.session_id = string(session, "sessionId").filter(|id| !id.is_empty());
        if selected.is_some_and(|selected| self.facts.session_id.as_deref() != Some(selected)) {
            return Err(invalid_input());
        }
        match session.get("version") {
            None => {}
            Some(version) if matches!(version.as_u64(), Some(2 | 3)) => {}
            Some(_) => return Err(invalid_data()),
        }
        let created = self.stamp(session.get("creationDate"));
        self.observe(created);
        let requests = match session.get("requests") {
            None => &[][..],
            Some(Value::Array(requests)) => requests.as_slice(),
            Some(_) => return Err(invalid_data()),
        };
        self.facts.records = requests.len() as u64;
        for (index, request) in requests.iter().enumerate() {
            match request.as_object() {
                Some(request) => self.request(request, index),
                None => self.facts.skipped.malformed += 1,
            }
        }
        Ok(())
    }

    /// Native epoch milliseconds; a present but unusable value is counted.
    fn stamp(&mut self, value: Option<&Value>) -> Option<i64> {
        match value {
            None | Some(Value::Null) => None,
            Some(value) => {
                let ms = value
                    .as_u64()
                    .and_then(|ms| i64::try_from(ms).ok())
                    .filter(|ms| *ms <= MAX_RFC3339_MS);
                if ms.is_none() {
                    self.facts.skipped.bad_timestamp += 1;
                }
                ms
            }
        }
    }

    /// Widen the session's native event span.
    fn observe(&mut self, ms: Option<i64>) {
        let Some(ms) = ms else { return };
        if self.facts.first_event_ms.is_none_or(|first| ms < first) {
            self.facts.first_event_ms = Some(ms);
            self.facts.first_event_ts = Some(rfc3339(ms));
        }
        if self.facts.last_event_ms.is_none_or(|last| ms > last) {
            self.facts.last_event_ms = Some(ms);
            self.facts.last_event_ts = Some(rfc3339(ms));
        }
    }

    fn request(&mut self, request: &Map<String, Value>, index: usize) {
        let key = format!("/requests/{index}");
        let ts_ms = self.stamp(request.get("timestamp"));
        self.observe(ts_ms);
        let text = match request.get("message") {
            Some(Value::String(text)) => Some(text.as_str()),
            Some(Value::Object(message)) => message.get("text").and_then(Value::as_str),
            _ => None,
        };
        let chars = text.map(|text| len_i64(text.chars().count()));
        let body_key = text.map(|text| format!("{:016x}", fnv(text.as_bytes())));
        let origin = if request.get("isSystemInitiated") == Some(&Value::Bool(true)) {
            TurnOrigin::Other
        } else {
            TurnOrigin::Human
        };
        self.rows.triggers.push(PrepTriggerRow {
            source: self.source.to_owned(),
            generation: self.generation,
            native_offset: None,
            native_key: Some(format!("{key}/message")),
            ts: ts_ms.map(rfc3339),
            ts_ms,
            kind: origin,
            sender: None,
            pij_msg_id: None,
            chars: chars.unwrap_or(0),
            body_key: body_key.clone(),
            next_turn_no: self.turn_no + 1,
            content_head: text
                .filter(|_| self.options.include_content)
                .map(|text| text.chars().take(200).collect()),
        });
        // An unanswered request is not an invented empty call.
        let Some(response) = request.get("response").filter(|value| !value.is_null()) else {
            return;
        };
        let call_ts = self.stamp(request.get("responseTimestamp"));
        self.observe(call_ts);
        self.turn_no += 1;
        let call_key = format!("{key}/response");
        let response_id = string(request, "responseId");
        let model = string(request, "modelId");
        let stop_reason = stop_reason(request);
        let input = count(request.get("promptTokens"));
        let gap_ms = match self.last_call_ts {
            None => Some(-1),
            Some(previous) => previous
                .zip(call_ts)
                .map(|(previous, now)| now.saturating_sub(previous)),
        };
        self.last_call_ts = Some(call_ts);
        self.rows.turns.push(PrepTurnRow {
            source: self.source.to_owned(),
            generation: self.generation,
            native_offset: None,
            native_key: Some(call_key.clone()),
            turn_no: self.turn_no,
            started_ts: call_ts.map(rfc3339),
            started_ts_ms: call_ts,
            first_call_offset: None,
            origin,
            sender: None,
            pij_msg_id: None,
            opener_offset: None,
            opener_ts_ms: ts_ms,
            opener_chars: chars,
            body_key,
        });
        self.rows.calls.push(PrepCallRow {
            source: self.source.to_owned(),
            generation: self.generation,
            native_offset: None,
            native_key: Some(call_key.clone()),
            sighting: CallSighting::First,
            msg_id: response_id.clone(),
            request_id: string(request, "requestId"),
            ts: call_ts.map(rfc3339),
            ts_ms: call_ts,
            model: model.clone(),
            stop_reason: stop_reason.clone(),
            input,
            cw_1h: None,
            cw_5m: None,
            cache_read: None,
            output: count(request.get("completionTokens")),
            cache_write_basis: CacheWriteBasis::None,
            is_sidechain: false,
            gap_ms,
            turn_no: Some(self.turn_no),
            call_in_turn: Some(1),
            records: 1,
        });
        self.facts.turns += 1;
        self.facts.calls += 1;
        self.facts.latest_context = Some(ContextSample {
            ts_ms: call_ts,
            model,
            stop_reason,
            input,
            cache_read: None,
            cache_write: None,
            total: None,
        });
        let Some(parts) = response.as_array() else {
            return;
        };
        for (part_index, part) in parts.iter().enumerate() {
            if let Some(part) = part.as_object().filter(|part| {
                part.get("kind").and_then(Value::as_str) == Some("toolInvocationSerialized")
            }) {
                self.tool(
                    part,
                    format!("{call_key}/{part_index}"),
                    response_id.as_deref(),
                );
            }
        }
    }

    fn tool(&mut self, part: &Map<String, Value>, key: String, call_msg_id: Option<&str>) {
        let name = string(part, "toolId").filter(|name| !name.is_empty());
        let row = PrepToolUseRow {
            source: self.source.to_owned(),
            generation: self.generation,
            native_offset: None,
            native_key: Some(key),
            sighting: ToolSighting::Use,
            tool_use_id: string(part, "toolCallId").filter(|id| !id.is_empty()),
            call_msg_id: call_msg_id.map(str::to_owned),
            ts: None,
            ts_ms: None,
            family: name.as_deref().and_then(tool_family).map(str::to_owned),
            name,
            input_hash: None,
            input_bytes: None,
            result_offset: None,
            result_bytes: None,
            outcome: None,
            duration_ms: None,
            turn_no: Some(self.turn_no),
        };
        // A complete invocation has a result; its success is native only when
        // `resultDetails.isError` is recorded.
        let result = (part.get("isComplete") == Some(&Value::Bool(true))).then(|| {
            let outcome = match part
                .get("resultDetails")
                .and_then(|details| details.get("isError"))
            {
                Some(Value::Bool(true)) => ToolOutcome::Error,
                Some(Value::Bool(false)) => ToolOutcome::Ok,
                _ => ToolOutcome::Unknown,
            };
            PrepToolUseRow {
                sighting: ToolSighting::Result,
                outcome: Some(outcome),
                ..row.clone()
            }
        });
        self.rows.tool_uses.push(row);
        self.rows.tool_uses.extend(result);
    }
}

/// Native `ResponseModelState` by name, else the legacy cancellation flag.
fn stop_reason(request: &Map<String, Value>) -> Option<String> {
    let state = request
        .get("modelState")
        .and_then(|state| state.get("value"))
        .and_then(Value::as_u64)
        .and_then(|value| match value {
            0 => Some("pending"),
            1 => Some("complete"),
            2 => Some("cancelled"),
            3 => Some("failed"),
            4 => Some("needs_input"),
            _ => None,
        });
    state
        .or_else(|| (request.get("isCanceled") == Some(&Value::Bool(true))).then_some("cancelled"))
        .map(str::to_owned)
}

fn string(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// A native non-negative token counter; anything else is not recorded.
fn count(value: Option<&Value>) -> Option<i64> {
    value?.as_i64().filter(|n| *n >= 0)
}

fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// 9999-12-31T23:59:59.999Z, the last instant with a four-digit year.
const MAX_RFC3339_MS: i64 = 253_402_300_799_999;

/// Exact RFC 3339 UTC rendering of non-negative epoch milliseconds.
fn rfc3339(ms: i64) -> String {
    let (days, day_ms) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // Civil date from days since 1970-01-01 (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        day_ms / 3_600_000,
        day_ms / 60_000 % 60,
        day_ms / 1_000 % 60,
        day_ms % 1_000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_renders_exact_utc_instants() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_700_000_000_001), "2023-11-14T22:13:20.001Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(rfc3339(MAX_RFC3339_MS), "9999-12-31T23:59:59.999Z");
    }
}
