//! Session-status target resolution: a Pij seat or tmux pane → an explicit
//! [`StatusTarget`].
//!
//! Imperative shell for the CLI path only; the status SDK never calls it. Every
//! capability is injected: a [`CommandRunner`] (`pij`, `tmux`), a
//! [`ProcessTable`] (liveness and the pane's process tree) and a [`StatusFs`]
//! (harness session records under `home`). Nothing is written.
//!
//! - `--pij ID`: one `pij list --json`; the seat's recorded process
//!   `(pid, proc_start)` must still be running, else the binding is dead.
//! - `--pane %N`: every live Pij seat recorded on that pane, plus the native
//!   lookup (tmux pane → bounded walk of the pane's process tree → the harness's
//!   own session record). The first answer is the target; every other distinct
//!   answer is returned as a conflict, never silently dropped.
//!
//! Native pane lookup supports Claude Code (`~/.claude/sessions/<pid>.json`).

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        mpsc::{self, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;
use unisphere_core::status::{
    ResolveBasis, ResolveConflict, Resolved, StatusFailure, StatusFailureKind, StatusQuery,
    StatusTarget, TargetResolver,
};

/// Deepest descendant of the pane process examined for a harness record.
const MAX_WALK_DEPTH: usize = 4;
/// Most pane processes examined for a harness record.
const MAX_WALK_PROCESSES: usize = 64;
/// Largest harness session record read.
const MAX_RECORD_BYTES: usize = 64 * 1024;
const CLAUDE_CODE: &str = "claude-code";

// ---------------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------------

/// Output of one completed external command. Diagnostics (stderr) are discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
}

/// Why an external command produced no output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    /// The program is not installed.
    NotFound,
    /// The program could not be started or its output could not be read.
    Failed,
    Timeout,
    OutputLimit,
}

/// Runs one external program with arguments; no shell.
pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError>;
}

/// One running process. `started` is its local start time as the number
/// `YYYYMMDDhhmmss`, the form Pij records as `proc.proc_start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub started: Option<u64>,
}

/// Read-only view of running processes.
pub trait ProcessTable: Send + Sync {
    /// Processes whose controlling terminal is `tty` (e.g. `/dev/ttys003`).
    fn on_tty(&self, tty: &str) -> Result<Vec<ProcessInfo>, CommandError>;
    /// One process; `None` when it is not running.
    fn process(&self, pid: u32) -> Result<Option<ProcessInfo>, CommandError>;
}

/// Read-only file access for harness session records.
pub trait StatusFs: Send + Sync {
    /// The whole file when it has at most `max_bytes`; `Ok(None)` when absent.
    fn read(&self, path: &Path, max_bytes: usize) -> io::Result<Option<Vec<u8>>>;
}

// ---------------------------------------------------------------------------
// Resolver
// ---------------------------------------------------------------------------

/// [`TargetResolver`] over Pij's seat list and native tmux/harness records.
pub struct StatusTargetResolver {
    runner: Arc<dyn CommandRunner>,
    procs: Arc<dyn ProcessTable>,
    fs: Arc<dyn StatusFs>,
    home: PathBuf,
}

impl StatusTargetResolver {
    /// `home` is the user's home directory holding harness session records
    /// (`home/.claude/sessions`).
    pub fn new(
        runner: Arc<dyn CommandRunner>,
        procs: Arc<dyn ProcessTable>,
        fs: Arc<dyn StatusFs>,
        home: PathBuf,
    ) -> Self {
        Self {
            runner,
            procs,
            fs,
            home,
        }
    }

    fn by_pij(&self, query: &StatusQuery, id: &str) -> Result<Resolved, StatusFailure> {
        if !valid_pij_id(id) {
            return Err(failure(
                StatusFailureKind::PijUnknownSeat,
                "the Pij seat id is not valid",
            ));
        }
        let seats = self.seats()?;
        let seat = seats.iter().find(|seat| seat.id == id).ok_or_else(|| {
            failure(
                StatusFailureKind::PijUnknownSeat,
                "Pij lists no seat with this id",
            )
        })?;
        let harness = adapter_id(&seat.harness).ok_or_else(|| {
            failure(
                StatusFailureKind::UnsupportedHarness,
                "the seat's harness has no Unisphere adapter",
            )
        })?;
        let session = seat.session.clone().ok_or_else(|| {
            failure(
                StatusFailureKind::PijNoSession,
                "the seat has no recorded native session",
            )
        })?;
        if !self.live(seat.proc_)? {
            return Err(failure(
                StatusFailureKind::DeadBinding,
                "the seat's recorded process is not running",
            ));
        }
        Ok(Resolved {
            query: query.clone(),
            target: target(harness, session),
            pij_id: Some(seat.id.clone()),
            pane: seat.pane.clone(),
            basis: ResolveBasis::PijRegistry,
            conflicts: Vec::new(),
        })
    }

    fn by_pane(&self, query: &StatusQuery, pane: &str) -> Result<Resolved, StatusFailure> {
        if !valid_pane(pane) {
            return Err(failure(
                StatusFailureKind::PaneNotFound,
                "the pane id is not of the form %N",
            ));
        }
        // (basis, target, pij id) in precedence order: live Pij seats (most
        // recent first), then native records (shallowest process first).
        let mut answers: Vec<(ResolveBasis, StatusTarget, Option<String>)> = Vec::new();
        let mut dead = false;
        let mut no_session = false;
        let mut unsupported = false;
        // Pij unavailable is not a pane failure: the native lookup still answers.
        if let Ok(mut seats) = self.seats() {
            seats.retain(|seat| seat.pane.as_deref() == Some(pane));
            seats.sort_by_key(|seat| std::cmp::Reverse(seat.last_event_at));
            for seat in seats {
                let Some(harness) = adapter_id(&seat.harness) else {
                    unsupported = true;
                    continue;
                };
                let Some(session) = seat.session else {
                    no_session = true;
                    continue;
                };
                if !self.live(seat.proc_).unwrap_or(false) {
                    dead = true;
                    continue;
                }
                answers.push((
                    ResolveBasis::PijRegistry,
                    target(harness, session),
                    Some(seat.id),
                ));
            }
        }
        let native = self.native_pane(pane);
        if let Ok(targets) = &native {
            answers.extend(
                targets
                    .iter()
                    .cloned()
                    .map(|target| (ResolveBasis::NativePane, target, None)),
            );
        }

        let mut answers = answers.into_iter();
        let Some((basis, chosen, pij_id)) = answers.next() else {
            return Err(match native {
                Err(NativeMiss::PaneMissing) => failure(
                    StatusFailureKind::PaneNotFound,
                    "tmux lists no pane with this id",
                ),
                Err(NativeMiss::ProcessesUnavailable) => failure(
                    StatusFailureKind::PaneNotFound,
                    "the pane's processes could not be listed",
                ),
                _ if dead => failure(
                    StatusFailureKind::DeadBinding,
                    "every Pij seat on this pane has a dead recorded process",
                ),
                _ if no_session => failure(
                    StatusFailureKind::PijNoSession,
                    "the Pij seat on this pane has no recorded native session",
                ),
                _ if unsupported => failure(
                    StatusFailureKind::UnsupportedHarness,
                    "the Pij seat on this pane uses a harness with no Unisphere adapter",
                ),
                _ => failure(
                    StatusFailureKind::UnsupportedHarness,
                    "no supported harness session record was found under this pane",
                ),
            });
        };
        let mut seen = BTreeSet::from([key(&chosen)]);
        let conflicts = answers
            .filter(|(_, target, _)| seen.insert(key(target)))
            .map(|(basis, target, _)| ResolveConflict { basis, target })
            .collect();
        Ok(Resolved {
            query: query.clone(),
            target: chosen,
            pij_id,
            pane: Some(pane.to_owned()),
            basis,
            conflicts,
        })
    }

    /// Current seats from one `pij list --json`.
    fn seats(&self) -> Result<Vec<Seat>, StatusFailure> {
        let unavailable = |message: &str| failure(StatusFailureKind::PijUnavailable, message);
        let output = self
            .runner
            .run("pij", &["list", "--json"])
            .map_err(|error| match error {
                CommandError::NotFound => unavailable("the pij CLI is not installed"),
                CommandError::Timeout => unavailable("pij list timed out"),
                CommandError::OutputLimit => unavailable("pij list output exceeded the limit"),
                CommandError::Failed => unavailable("pij list could not run"),
            })?;
        let response: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| unavailable("pij list returned invalid JSON"))?;
        if !output.success || response.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(unavailable("pij list reported a failure"));
        }
        let seats = response
            .pointer("/data/seats")
            .and_then(Value::as_array)
            .ok_or_else(|| unavailable("pij list returned no seat list"))?;
        Ok(seats.iter().filter_map(Seat::parse).collect())
    }

    /// Whether the recorded `(pid, proc_start)` is still the running process.
    fn live(&self, proc_: Option<(u32, u64)>) -> Result<bool, StatusFailure> {
        let Some((pid, started)) = proc_ else {
            return Ok(false);
        };
        let process = self.procs.process(pid).map_err(|_| {
            failure(
                StatusFailureKind::PijUnavailable,
                "the process table could not be read to check the seat's process",
            )
        })?;
        Ok(process.is_some_and(|process| process.started == Some(started)))
    }

    /// tmux pane → the pane's process tree → harness session records.
    fn native_pane(&self, pane: &str) -> Result<Vec<StatusTarget>, NativeMiss> {
        let output = self
            .runner
            .run(
                "tmux",
                &[
                    "list-panes",
                    "-a",
                    "-F",
                    "#{pane_id} #{pane_pid} #{pane_tty}",
                ],
            )
            .map_err(|_| NativeMiss::PaneMissing)?;
        // No tmux server means no panes.
        if !output.success {
            return Err(NativeMiss::PaneMissing);
        }
        let listing = String::from_utf8_lossy(&output.stdout);
        let (root, tty) = listing
            .lines()
            .find_map(|line| {
                let mut fields = line.split_whitespace();
                (fields.next() == Some(pane)).then_some(())?;
                let pid = fields.next()?.parse::<u32>().ok()?;
                Some((pid, fields.next()?.to_owned()))
            })
            .ok_or(NativeMiss::PaneMissing)?;
        let processes = self
            .procs
            .on_tty(&tty)
            .map_err(|_| NativeMiss::ProcessesUnavailable)?;

        let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for process in &processes {
            children.entry(process.ppid).or_default().push(process.pid);
        }
        let mut targets = Vec::new();
        let mut queue = VecDeque::from([(root, 0)]);
        let mut visited = BTreeSet::new();
        while let Some((pid, depth)) = queue.pop_front() {
            if visited.len() >= MAX_WALK_PROCESSES || !visited.insert(pid) {
                continue;
            }
            if let Some(target) = self.claude_record(pid, pane) {
                targets.push(target);
            }
            if depth < MAX_WALK_DEPTH {
                for child in children.get(&pid).into_iter().flatten() {
                    queue.push_back((*child, depth + 1));
                }
            }
        }
        if targets.is_empty() {
            Err(NativeMiss::NoHarness)
        } else {
            Ok(targets)
        }
    }

    /// Claude Code's own record for the live pane process `pid`, when it names
    /// this pane. (`procStart` is not compared: Claude writes it in UTC while
    /// `ps` reports local time; the pid being a live pane descendant suffices.)
    fn claude_record(&self, pid: u32, pane: &str) -> Option<StatusTarget> {
        let path = self
            .home
            .join(".claude/sessions")
            .join(format!("{pid}.json"));
        let bytes = self.fs.read(&path, MAX_RECORD_BYTES).ok()??;
        let record: Value = serde_json::from_slice(&bytes).ok()?;
        if record.get("pid").and_then(Value::as_u64) != Some(u64::from(pid)) {
            return None;
        }
        // `tmux` is `session:@window.%pane`; a record for another pane is stale.
        if let Some(recorded) = record.get("tmux").and_then(Value::as_str)
            && recorded.rsplit('.').next() != Some(pane)
        {
            return None;
        }
        let session = record
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|session| !session.is_empty())?;
        Some(target(CLAUDE_CODE.to_owned(), session.to_owned()))
    }
}

impl TargetResolver for StatusTargetResolver {
    fn resolve(&self, query: &StatusQuery) -> Result<Resolved, StatusFailure> {
        match query {
            StatusQuery::Target(target) => Ok(Resolved {
                query: query.clone(),
                target: target.clone(),
                pij_id: None,
                pane: None,
                basis: ResolveBasis::Explicit,
                conflicts: Vec::new(),
            }),
            StatusQuery::Pij(id) => self.by_pij(query, id),
            StatusQuery::Pane(pane) => self.by_pane(query, pane),
        }
    }
}

enum NativeMiss {
    PaneMissing,
    ProcessesUnavailable,
    NoHarness,
}

struct Seat {
    id: String,
    harness: String,
    session: Option<String>,
    pane: Option<String>,
    proc_: Option<(u32, u64)>,
    last_event_at: i64,
}

impl Seat {
    /// Rows without an id or harness are not seats and are ignored.
    fn parse(row: &Value) -> Option<Self> {
        let text = |field: &str| {
            row.get(field)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let proc_ = row.get("proc").and_then(|proc_| {
            let pid = u32::try_from(proc_.get("pid")?.as_u64()?).ok()?;
            Some((pid, proc_.get("proc_start")?.as_u64()?))
        });
        Some(Self {
            id: text("id")?,
            harness: text("harness")?,
            session: text("session"),
            pane: text("pane"),
            proc_,
            last_event_at: row
                .get("last_event_at")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MIN),
        })
    }
}

fn target(harness: String, session_id: String) -> StatusTarget {
    StatusTarget {
        harness,
        session_id,
        transcript: None,
    }
}

fn key(target: &StatusTarget) -> (String, String) {
    (target.harness.clone(), target.session_id.clone())
}

fn failure(kind: StatusFailureKind, message: &str) -> StatusFailure {
    StatusFailure::new(kind, message)
}

/// Pij harness name → Unisphere adapter id.
fn adapter_id(pij_harness: &str) -> Option<String> {
    let id = match pij_harness {
        "claude" => CLAUDE_CODE,
        "omp" => "oh-my-pi",
        "codex" => "codex",
        "pi" => "pi",
        "copilot" => "copilot-cli",
        _ => return None,
    };
    Some(id.to_owned())
}

fn valid_pij_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn valid_pane(pane: &str) -> bool {
    pane.strip_prefix('%')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// `ps -o lstart` text (`Tue Sep 29 13:13:12 2026`; day and month in either
/// order) → `YYYYMMDDhhmmss`.
fn parse_lstart(text: &str) -> Option<u64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let fields: Vec<&str> = text.split_whitespace().collect();
    let [_, first, second, clock, year] = fields.as_slice() else {
        return None;
    };
    let month_of = |name: &str| MONTHS.iter().position(|month| *month == name);
    let (month, day) = match (month_of(first), month_of(second)) {
        (Some(month), None) => (month, second),
        (None, Some(month)) => (month, first),
        _ => return None,
    };
    let day: u64 = day.parse().ok().filter(|day| (1..=31).contains(day))?;
    let year: u64 = year.parse().ok()?;
    let mut clock = clock.split(':').map(|part| part.parse::<u64>().ok());
    let (Some(Some(hour)), Some(Some(minute)), Some(Some(second)), None) =
        (clock.next(), clock.next(), clock.next(), clock.next())
    else {
        return None;
    };
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let month = month as u64 + 1;
    Some(((((year * 100 + month) * 100 + day) * 100 + hour) * 100 + minute) * 100 + second)
}

// ---------------------------------------------------------------------------
// System adapters
// ---------------------------------------------------------------------------

/// Runs programs found on `PATH`, bounded by a timeout and an output limit, in
/// the `C` locale so machine output (such as `ps` dates) is stable.
#[derive(Debug, Clone, Copy)]
pub struct SystemCommandRunner {
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl Default for SystemCommandRunner {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            max_output_bytes: 4 * 1024 * 1024,
        }
    }
}

impl CommandRunner for SystemCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        let deadline = Instant::now() + self.timeout;
        let mut child = Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => CommandError::NotFound,
                _ => CommandError::Failed,
            })?;
        let Some(stdout) = child.stdout.take() else {
            stop(&mut child);
            return Err(CommandError::Failed);
        };
        let limit = self.max_output_bytes;
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut output = Vec::new();
            let result = stdout
                .take(limit as u64 + 1)
                .read_to_end(&mut output)
                .map(|_| output);
            let _ = sender.send(result);
        });
        let mut output = None;
        loop {
            if output.is_none() {
                match receiver.try_recv() {
                    Ok(Ok(bytes)) if bytes.len() > limit => {
                        stop(&mut child);
                        return Err(CommandError::OutputLimit);
                    }
                    Ok(Ok(bytes)) => output = Some(bytes),
                    Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                        stop(&mut child);
                        return Err(CommandError::Failed);
                    }
                    Err(TryRecvError::Empty) => {}
                }
            }
            match child.try_wait() {
                Ok(Some(status)) if output.is_some() => {
                    return Ok(CommandOutput {
                        success: status.success(),
                        stdout: output.unwrap_or_default(),
                    });
                }
                Ok(_) => {}
                Err(_) => {
                    stop(&mut child);
                    return Err(CommandError::Failed);
                }
            }
            if Instant::now() >= deadline {
                stop(&mut child);
                return Err(CommandError::Timeout);
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// [`ProcessTable`] over `ps -o pid=,ppid=,lstart=` (POSIX `ps`, C locale).
pub struct PsProcessTable {
    runner: Arc<dyn CommandRunner>,
}

impl PsProcessTable {
    pub fn new(runner: Arc<dyn CommandRunner>) -> Self {
        Self { runner }
    }

    fn ps(&self, selector: &str, value: &str) -> Result<Vec<ProcessInfo>, CommandError> {
        let output = self
            .runner
            .run("ps", &["-o", "pid=,ppid=,lstart=", selector, value])?;
        // `ps` exits non-zero when nothing matches.
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                let pid = fields.next()?.parse().ok()?;
                let ppid = fields.next()?.parse().ok()?;
                let started = parse_lstart(&fields.collect::<Vec<_>>().join(" "));
                Some(ProcessInfo { pid, ppid, started })
            })
            .collect())
    }
}

impl ProcessTable for PsProcessTable {
    fn on_tty(&self, tty: &str) -> Result<Vec<ProcessInfo>, CommandError> {
        self.ps("-t", tty.strip_prefix("/dev/").unwrap_or(tty))
    }

    fn process(&self, pid: u32) -> Result<Option<ProcessInfo>, CommandError> {
        Ok(self
            .ps("-p", &pid.to_string())?
            .into_iter()
            .find(|process| process.pid == pid))
    }
}

/// [`StatusFs`] over the local filesystem, read-only.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemFs;

impl StatusFs for SystemFs {
    fn read(&self, path: &Path, max_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(max_bytes as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file exceeds the read limit",
            ));
        }
        Ok(Some(bytes))
    }
}
