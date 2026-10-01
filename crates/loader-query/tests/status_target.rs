//! Status target resolution with injected pij/tmux/process/filesystem fakes:
//! Pij registry + liveness, dead bindings, duplicate panes, the native pane
//! child walk, returned conflicts and distinct failures.
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde_json::{Value, json};
use unisphere_core::status::{
    ResolveBasis, ResolveConflict, StatusFailureKind, StatusQuery, StatusTarget, TargetResolver,
};
use unisphere_loader_query::status_target::{
    CommandError, CommandOutput, CommandRunner, ProcessInfo, ProcessTable, PsProcessTable,
    StatusFs, StatusTargetResolver,
};

const HOME: &str = "/home/u";
const PANE_LIST: &str = "%3 100 /dev/ttys003\n%7 700 /dev/ttys007\n";

#[derive(Default)]
struct FakeRunner {
    answers: BTreeMap<String, Result<CommandOutput, CommandError>>,
    calls: Mutex<Vec<String>>,
}

impl FakeRunner {
    fn with(mut self, program: &str, answer: Result<(bool, &str), CommandError>) -> Self {
        self.answers.insert(
            program.to_owned(),
            answer.map(|(success, stdout)| CommandOutput {
                success,
                stdout: stdout.as_bytes().to_vec(),
            }),
        );
        self
    }
}

impl CommandRunner for FakeRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{program} {}", args.join(" ")));
        self.answers
            .get(program)
            .cloned()
            .unwrap_or(Err(CommandError::NotFound))
    }
}

#[derive(Default)]
struct FakeProcs {
    running: Vec<ProcessInfo>,
    ttys: BTreeMap<String, Vec<u32>>,
}

impl FakeProcs {
    fn run(mut self, pid: u32, ppid: u32, started: u64, tty: &str) -> Self {
        self.running.push(ProcessInfo {
            pid,
            ppid,
            started: Some(started),
        });
        self.ttys.entry(tty.to_owned()).or_default().push(pid);
        self
    }
}

impl ProcessTable for FakeProcs {
    fn on_tty(&self, tty: &str) -> Result<Vec<ProcessInfo>, CommandError> {
        let pids = self.ttys.get(tty).cloned().unwrap_or_default();
        Ok(self
            .running
            .iter()
            .filter(|process| pids.contains(&process.pid))
            .copied()
            .collect())
    }

    fn process(&self, pid: u32) -> Result<Option<ProcessInfo>, CommandError> {
        Ok(self
            .running
            .iter()
            .find(|process| process.pid == pid)
            .copied())
    }
}

#[derive(Default)]
struct FakeFs {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl FakeFs {
    fn claude(mut self, pid: u32, session: &str, pane: &str) -> Self {
        let record = json!({"pid": pid, "sessionId": session, "tmux": format!("main:@1.{pane}"),
            "procStart": "Tue Sep 29 03:13:12 2026"});
        self.files.insert(
            Path::new(HOME).join(format!(".claude/sessions/{pid}.json")),
            record.to_string().into_bytes(),
        );
        self
    }
}

impl StatusFs for FakeFs {
    fn read(&self, path: &Path, max_bytes: usize) -> io::Result<Option<Vec<u8>>> {
        let bytes = self.files.get(path).cloned();
        if bytes.as_ref().is_some_and(|bytes| bytes.len() > max_bytes) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "too large"));
        }
        Ok(bytes)
    }
}

fn seat(
    id: &str,
    harness: &str,
    session: Option<&str>,
    pane: &str,
    proc_: Option<(u32, u64)>,
    at: i64,
) -> Value {
    json!({"id": id, "harness": harness, "session": session, "pane": pane,
        "proc": proc_.map(|(pid, start)| json!({"pid": pid, "proc_start": start})),
        "last_event_at": at, "state": "idle"})
}

fn pij_list(seats: &[Value]) -> String {
    json!({"ok": true, "command": "pij list", "v": 2, "data": {"seats": seats}}).to_string()
}

fn resolver(runner: FakeRunner, procs: FakeProcs, fs: FakeFs) -> StatusTargetResolver {
    StatusTargetResolver::new(
        Arc::new(runner),
        Arc::new(procs),
        Arc::new(fs),
        PathBuf::from(HOME),
    )
}

fn claude(session: &str) -> StatusTarget {
    StatusTarget {
        harness: "claude-code".into(),
        session_id: session.into(),
        transcript: None,
    }
}

fn pij(id: &str) -> StatusQuery {
    StatusQuery::Pij(id.into())
}

fn pane(id: &str) -> StatusQuery {
    StatusQuery::Pane(id.into())
}

fn failure_kind(resolver: &StatusTargetResolver, query: &StatusQuery) -> StatusFailureKind {
    match resolver.resolve(query) {
        Ok(resolved) => panic!("{query:?} resolved to {resolved:?}"),
        Err(failure) => failure.kind,
    }
}

/// The standard machine: seats of every failure shape plus one live seat.
fn registry() -> String {
    pij_list(&[
        seat(
            "pij-live",
            "claude",
            Some("s-live"),
            "%3",
            Some((200, 20260929131312)),
            5,
        ),
        seat(
            "pij-dead",
            "claude",
            Some("s-dead"),
            "%9",
            Some((300, 20260929131312)),
            5,
        ),
        seat(
            "pij-reused",
            "claude",
            Some("s-reused"),
            "%9",
            Some((400, 20260101000000)),
            5,
        ),
        seat(
            "pij-nosession",
            "claude",
            None,
            "%8",
            Some((500, 20260929131312)),
            5,
        ),
        seat("pij-noproc", "claude", Some("s-noproc"), "%8", None, 5),
        seat(
            "pij-gemini",
            "gemini",
            Some("s-g"),
            "%8",
            Some((600, 20260929131312)),
            5,
        ),
        seat(
            "pij-omp",
            "omp",
            Some("s-omp"),
            "%12",
            Some((700, 20260929131312)),
            5,
        ),
    ])
}

fn machine() -> FakeProcs {
    FakeProcs::default()
        .run(100, 1, 20260929131311, "/dev/ttys003")
        .run(200, 100, 20260929131312, "/dev/ttys003")
        .run(400, 1, 20260929131312, "/dev/ttys009")
        .run(500, 1, 20260929131312, "/dev/ttys008")
        .run(600, 1, 20260929131312, "/dev/ttys008")
        .run(700, 1, 20260929131312, "/dev/ttys012")
}

#[test]
fn pij_seat_resolves_through_the_registry_when_its_process_is_live() {
    let runner = FakeRunner::default()
        .with("pij", Ok((true, &registry())))
        .with("tmux", Ok((true, PANE_LIST)));
    let resolved = resolver(runner, machine(), FakeFs::default())
        .resolve(&pij("pij-live"))
        .unwrap();
    assert_eq!(resolved.target, claude("s-live"));
    assert_eq!(resolved.basis, ResolveBasis::PijRegistry);
    assert_eq!(resolved.pij_id.as_deref(), Some("pij-live"));
    assert_eq!(resolved.pane.as_deref(), Some("%3"));
    assert!(resolved.conflicts.is_empty());

    let runner = FakeRunner::default().with("pij", Ok((true, &registry())));
    let omp = resolver(runner, machine(), FakeFs::default())
        .resolve(&pij("pij-omp"))
        .unwrap();
    assert_eq!(
        omp.target.harness, "oh-my-pi",
        "pij harness names map to adapter ids"
    );
}

#[test]
fn pij_failures_are_distinct() {
    let runner = || FakeRunner::default().with("pij", Ok((true, &registry())));
    let resolver = |runner| resolver(runner, machine(), FakeFs::default());
    for (id, kind) in [
        ("pij-missing", StatusFailureKind::PijUnknownSeat),
        ("pij-nosession", StatusFailureKind::PijNoSession),
        ("pij-dead", StatusFailureKind::DeadBinding),
        ("pij-reused", StatusFailureKind::DeadBinding),
        ("pij-noproc", StatusFailureKind::DeadBinding),
        ("pij-gemini", StatusFailureKind::UnsupportedHarness),
        ("--pane", StatusFailureKind::PijUnknownSeat),
    ] {
        assert_eq!(failure_kind(&resolver(runner()), &pij(id)), kind, "{id}");
    }
    for runner in [
        FakeRunner::default(),
        FakeRunner::default().with("pij", Err(CommandError::Timeout)),
        FakeRunner::default().with("pij", Ok((true, "not json"))),
        FakeRunner::default().with("pij", Ok((false, r#"{"ok":false,"v":2,"error":"daemon"}"#))),
        FakeRunner::default().with("pij", Ok((true, r#"{"ok":true,"v":2,"data":{}}"#))),
    ] {
        assert_eq!(
            failure_kind(&resolver(runner), &pij("pij-live")),
            StatusFailureKind::PijUnavailable
        );
    }
}

#[test]
fn pane_prefers_live_seats_and_skips_dead_duplicate_rows() {
    // %3 has a dead older row and a live row; the dead row is not an answer.
    let seats = pij_list(&[
        seat(
            "pij-old",
            "claude",
            Some("s-old"),
            "%3",
            Some((900, 20260101000000)),
            9,
        ),
        seat(
            "pij-live",
            "claude",
            Some("s-live"),
            "%3",
            Some((200, 20260929131312)),
            5,
        ),
    ]);
    let runner = FakeRunner::default()
        .with("pij", Ok((true, &seats)))
        .with("tmux", Ok((true, PANE_LIST)));
    let fs = FakeFs::default().claude(200, "s-live", "%3");
    let resolved = resolver(runner, machine(), fs)
        .resolve(&pane("%3"))
        .unwrap();
    assert_eq!(resolved.target, claude("s-live"));
    assert_eq!(resolved.basis, ResolveBasis::PijRegistry);
    assert_eq!(resolved.pij_id.as_deref(), Some("pij-live"));
    assert_eq!(resolved.pane.as_deref(), Some("%3"));
    assert!(
        resolved.conflicts.is_empty(),
        "the agreeing native record is no conflict"
    );
}

#[test]
fn disagreements_are_returned_as_conflicts_not_picked() {
    // Two live seats on %3 and a native record naming a third session.
    let seats = pij_list(&[
        seat(
            "pij-a",
            "claude",
            Some("s-a"),
            "%3",
            Some((200, 20260929131312)),
            1,
        ),
        seat(
            "pij-b",
            "claude",
            Some("s-b"),
            "%3",
            Some((100, 20260929131311)),
            2,
        ),
    ]);
    let runner = FakeRunner::default()
        .with("pij", Ok((true, &seats)))
        .with("tmux", Ok((true, PANE_LIST)));
    let fs = FakeFs::default().claude(200, "s-native", "%3");
    let resolved = resolver(runner, machine(), fs)
        .resolve(&pane("%3"))
        .unwrap();
    assert_eq!(
        resolved.target,
        claude("s-b"),
        "most recent live seat first"
    );
    assert_eq!(resolved.pij_id.as_deref(), Some("pij-b"));
    assert_eq!(
        resolved.conflicts,
        vec![
            ResolveConflict {
                basis: ResolveBasis::PijRegistry,
                target: claude("s-a"),
            },
            ResolveConflict {
                basis: ResolveBasis::NativePane,
                target: claude("s-native"),
            },
        ]
    );
}

#[test]
fn native_pane_walks_the_pane_process_tree_to_the_harness_record() {
    // pane 100 (shell) → 150 (wrapper) → 200 (claude); pij is not installed.
    let procs = FakeProcs::default()
        .run(100, 1, 1, "/dev/ttys003")
        .run(150, 100, 1, "/dev/ttys003")
        .run(200, 150, 1, "/dev/ttys003")
        .run(201, 200, 1, "/dev/ttys003");
    let fs = FakeFs::default()
        .claude(200, "s-native", "%3")
        // Stale record: a reused pid whose record names another pane.
        .claude(150, "s-stale", "%44");
    let runner = FakeRunner::default().with("tmux", Ok((true, PANE_LIST)));
    let resolved = resolver(runner, procs, fs).resolve(&pane("%3")).unwrap();
    assert_eq!(resolved.target, claude("s-native"));
    assert_eq!(resolved.basis, ResolveBasis::NativePane);
    assert_eq!(resolved.pij_id, None);
    assert_eq!(resolved.pane.as_deref(), Some("%3"));
    assert!(resolved.conflicts.is_empty());
}

#[test]
fn native_walk_is_bounded_in_depth() {
    // Claude six levels below the pane process is beyond the walk.
    let mut procs = FakeProcs::default().run(100, 1, 1, "/dev/ttys003");
    for pid in 101..=106 {
        procs = procs.run(pid, pid - 1, 1, "/dev/ttys003");
    }
    let fs = FakeFs::default().claude(106, "s-deep", "%3");
    let runner = FakeRunner::default().with("tmux", Ok((true, PANE_LIST)));
    assert_eq!(
        failure_kind(&resolver(runner, procs, fs), &pane("%3")),
        StatusFailureKind::UnsupportedHarness
    );
}

#[test]
fn pane_failures_are_distinct() {
    let tmux = || {
        FakeRunner::default().with(
            "tmux",
            Ok((
                true,
                "%3 100 /dev/ttys003\n%8 800 /dev/ttys008\n%9 900 /dev/ttys009\n",
            )),
        )
    };
    let with_pij = || tmux().with("pij", Ok((true, &registry())));
    let procs = || {
        machine()
            .run(800, 1, 1, "/dev/ttys008")
            .run(900, 1, 1, "/dev/ttys009")
    };
    let cases: [(FakeRunner, &str, StatusFailureKind); 7] = [
        (with_pij(), "%404", StatusFailureKind::PaneNotFound),
        (with_pij(), "3", StatusFailureKind::PaneNotFound),
        (
            FakeRunner::default().with("tmux", Ok((false, ""))),
            "%3",
            StatusFailureKind::PaneNotFound,
        ),
        (with_pij(), "%9", StatusFailureKind::DeadBinding),
        (with_pij(), "%8", StatusFailureKind::DeadBinding),
        (
            tmux().with(
                "pij",
                Ok((
                    true,
                    &pij_list(&[seat(
                        "pij-n",
                        "claude",
                        None,
                        "%8",
                        Some((500, 20260929131312)),
                        1,
                    )]),
                )),
            ),
            "%8",
            StatusFailureKind::PijNoSession,
        ),
        (tmux(), "%3", StatusFailureKind::UnsupportedHarness),
    ];
    for (runner, id, kind) in cases {
        assert_eq!(
            failure_kind(&resolver(runner, procs(), FakeFs::default()), &pane(id)),
            kind,
            "{id}"
        );
    }
}

#[test]
fn explicit_targets_pass_through_without_any_lookup() {
    let runner = Arc::new(FakeRunner::default());
    let resolver = StatusTargetResolver::new(
        runner.clone(),
        Arc::new(FakeProcs::default()),
        Arc::new(FakeFs::default()),
        PathBuf::from(HOME),
    );
    let query = StatusQuery::Target(claude("s-x"));
    let resolved = resolver.resolve(&query).unwrap();
    assert_eq!(resolved.basis, ResolveBasis::Explicit);
    assert_eq!(resolved.target, claude("s-x"));
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[test]
fn ps_process_table_parses_c_locale_start_times_in_the_pij_form() {
    let ps = "  200   100 Tue Sep 29 13:13:12 2026\n  201   200 Wed  1 Oct 09:05:00 2026\n";
    let runner = Arc::new(FakeRunner::default().with("ps", Ok((true, ps))));
    let table = PsProcessTable::new(runner.clone());
    let processes = table.on_tty("/dev/ttys003").unwrap();
    assert_eq!(
        processes,
        vec![
            ProcessInfo {
                pid: 200,
                ppid: 100,
                started: Some(20260929131312)
            },
            ProcessInfo {
                pid: 201,
                ppid: 200,
                started: Some(20261001090500)
            },
        ]
    );
    assert_eq!(table.process(201).unwrap().map(|p| p.ppid), Some(200));
    assert_eq!(table.process(999).unwrap(), None);
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls[0], "ps -o pid=,ppid=,lstart= -t ttys003");
    assert_eq!(calls[1], "ps -o pid=,ppid=,lstart= -p 201");

    let empty = PsProcessTable::new(Arc::new(FakeRunner::default().with("ps", Ok((false, "")))));
    assert_eq!(
        empty.process(5).unwrap(),
        None,
        "ps exits 1 when nothing matches"
    );

    // procps-ng 4 (ubuntu:24.04): tmux reports `/dev/pts/N`; only `/dev/` is
    // stripped, and its wider right-aligned columns parse the same.
    let procps = Arc::new(FakeRunner::default().with(
        "ps",
        Ok((true, "     11      10 Tue Sep 29 23:43:52 2026\n")),
    ));
    let linux = PsProcessTable::new(procps.clone());
    assert_eq!(
        linux.on_tty("/dev/pts/0").unwrap(),
        vec![ProcessInfo {
            pid: 11,
            ppid: 10,
            started: Some(20260929234352)
        }]
    );
    assert_eq!(
        procps.calls.lock().unwrap()[0],
        "ps -o pid=,ppid=,lstart= -t pts/0"
    );
}
