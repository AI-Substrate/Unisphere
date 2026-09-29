//! CLI `sessions status` through `FakeStatusApi` + `FakeTargetResolver`: argv →
//! ordered queries, one result or failure per query, envelopes, exit codes,
//! the human table and documented examples.
use std::{ffi::OsString, path::PathBuf};

use serde_json::Value;
use unisphere_cli::{
    CliContext, OutputMode, ParsedCommand, SessionStatusCommand, emit_parse_failure, parse,
    run_docs, run_status,
};
use unisphere_core::status::{
    Basis, Fact, ModelSwitchStatus, ResolveBasis, ResolveConflict, Resolved, SessionStatus,
    StatusFailure, StatusFailureKind, StatusQuery, StatusTarget,
};
use unisphere_testkit::status::{FakeStatusApi, FakeTargetResolver};

const TOPIC: &str = include_str!("../docs/session-status.md");
const CLI_GUIDE: &str = include_str!("../../../docs/cli.md");
const SDK_GUIDE: &str = include_str!("../../../docs/sdk.md");
const NOW: i64 = 1_790_000_000_000;

fn context(terminal: bool) -> CliContext {
    CliContext {
        cwd: PathBuf::from("/work"),
        stdout_is_terminal: terminal,
        version: "status-test".into(),
    }
}

fn argv(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn status_command(values: &[&str], terminal: bool) -> SessionStatusCommand {
    match parse(argv(values), &context(terminal)) {
        Ok(ParsedCommand::SessionStatus(command)) => command,
        Ok(_) => panic!("{values:?} is not sessions status"),
        Err(failure) => panic!("{values:?} did not parse: {}", failure.code()),
    }
}

fn target(harness: &str, session: &str) -> StatusTarget {
    StatusTarget {
        harness: harness.into(),
        session_id: session.into(),
        transcript: None,
    }
}

/// What the SDK answers: the target including the transcript it read.
fn answered(harness: &str, session: &str) -> SessionStatus {
    let mut status = SessionStatus::empty(StatusTarget {
        transcript: Some(PathBuf::from(format!("/t/{session}.jsonl"))),
        ..target(harness, session)
    });
    status.model.current = Some(Fact::new("claude-opus-4-7".into(), Basis::Native));
    status.timeline.idle_seconds = Some(125);
    status.turns.total = 12;
    status.turns.last_hour_total = 3;
    status
}

fn resolved(query: StatusQuery, session: &str, basis: ResolveBasis) -> Resolved {
    Resolved {
        query,
        target: target("claude-code", session),
        pij_id: Some("pij-a".into()),
        pane: Some("%3".into()),
        basis,
        conflicts: Vec::new(),
    }
}

fn run(
    command: &SessionStatusCommand,
    api: &FakeStatusApi,
    resolver: &FakeTargetResolver,
) -> (u8, String, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let exit = run_status(command, api, resolver, NOW, &mut stdout, &mut stderr);
    (
        exit,
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap(),
    )
}

#[test]
fn queries_keep_argv_order_and_pair_sessions_with_harnesses() {
    let command = status_command(
        &[
            "unisphere",
            "sessions",
            "status",
            "--pane",
            "%3",
            "--session",
            "s1",
            "--pij",
            "pij-a",
            "--harness",
            "claude-code",
            "--session",
            "s2",
            "--harness",
            "codex",
        ],
        false,
    );
    assert_eq!(
        command.queries,
        vec![
            StatusQuery::Pane("%3".into()),
            StatusQuery::Target(target("claude-code", "s1")),
            StatusQuery::Pij("pij-a".into()),
            StatusQuery::Target(target("codex", "s2")),
        ]
    );
    let shared = status_command(
        &[
            "unisphere",
            "sessions",
            "status",
            "--harness",
            "claude-code",
            "--session",
            "s1",
            "--session",
            "s2",
        ],
        false,
    );
    assert_eq!(
        shared.queries,
        vec![
            StatusQuery::Target(target("claude-code", "s1")),
            StatusQuery::Target(target("claude-code", "s2")),
        ],
        "one --harness applies to every --session"
    );
}

#[test]
fn json_is_the_default_when_piped_and_flags_choose_explicitly() {
    let base = ["unisphere", "sessions", "status", "--pij", "pij-a"];
    assert_eq!(status_command(&base, false).mode, OutputMode::Json);
    assert_eq!(status_command(&base, true).mode, OutputMode::Human);
    let with = |flag| [&base[..], &[flag]].concat();
    assert_eq!(status_command(&with("--json"), true).mode, OutputMode::Json);
    assert_eq!(
        status_command(&with("--human"), false).mode,
        OutputMode::Human
    );
}

#[test]
fn malformed_queries_are_refused_before_any_port_call() {
    for values in [
        &["unisphere", "sessions", "status"][..],
        &["unisphere", "sessions", "status", "--session", "s1"],
        &[
            "unisphere",
            "sessions",
            "status",
            "--harness",
            "claude-code",
        ],
        &[
            "unisphere",
            "sessions",
            "status",
            "--pane",
            "%3",
            "--harness",
            "claude-code",
        ],
        &[
            "unisphere",
            "sessions",
            "status",
            "--session",
            "s1",
            "--session",
            "s2",
            "--harness",
            "a",
            "--harness",
            "b",
            "--harness",
            "c",
        ],
        &["unisphere", "sessions", "status", "--pane", "3"],
        &["unisphere", "sessions", "status", "--pane", "%3a"],
        &["unisphere", "sessions", "status", "--pij", "pij a"],
        &[
            "unisphere",
            "sessions",
            "status",
            "--session",
            "s1",
            "--harness",
            "Claude Code",
        ],
    ] {
        let failure = match parse(argv(values), &context(false)) {
            Ok(_) => panic!("{values:?} parsed"),
            Err(failure) => failure,
        };
        assert_eq!(failure.code(), "UNI-CLI-STATUS-TARGET", "{values:?}");
        let mut stderr = Vec::new();
        assert_eq!(
            emit_parse_failure(
                &failure,
                OutputMode::JsonDiagnostic,
                &mut Vec::new(),
                &mut stderr
            ),
            2
        );
    }
}

#[test]
fn every_query_yields_one_result_or_failure_in_order() {
    let command = status_command(
        &[
            "unisphere",
            "sessions",
            "status",
            "--session",
            "s-explicit",
            "--harness",
            "claude-code",
            "--pij",
            "pij-a",
            "--pij",
            "pij-gone",
            "--session",
            "s-x",
            "--harness",
            "gemini",
            "--pane",
            "%3",
        ],
        false,
    );
    let pane_resolved = Resolved {
        conflicts: vec![ResolveConflict {
            basis: ResolveBasis::NativePane,
            target: target("claude-code", "s-other"),
        }],
        ..resolved(
            StatusQuery::Pane("%3".into()),
            "s-pane",
            ResolveBasis::PijRegistry,
        )
    };
    let resolver = FakeTargetResolver::new()
        .with(
            StatusQuery::Pij("pij-a".into()),
            Ok(resolved(
                StatusQuery::Pij("pij-a".into()),
                "s-pij",
                ResolveBasis::PijRegistry,
            )),
        )
        .with(
            StatusQuery::Pij("pij-gone".into()),
            Err(StatusFailure::new(StatusFailureKind::DeadBinding, "gone")),
        )
        .with(StatusQuery::Pane("%3".into()), Ok(pane_resolved.clone()));
    let unsupported = target("gemini", "s-x");
    let api = FakeStatusApi::new()
        .with(
            Ok(answered("claude-code", "s-explicit")),
            &target("claude-code", "s-explicit"),
        )
        .with(
            Ok(answered("claude-code", "s-pij")),
            &target("claude-code", "s-pij"),
        )
        .with(
            Err(StatusFailure::new(
                StatusFailureKind::UnsupportedHarness,
                "no binding",
            )),
            &unsupported,
        )
        .with(
            Ok(answered("claude-code", "s-pane")),
            &target("claude-code", "s-pane"),
        );

    let (exit, stdout, _) = run(&command, &api, &resolver);
    assert_eq!(exit, 3, "some queries failed");
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["command"], "sessions.status");
    assert_eq!(value["data"]["failed"], 2);
    let results = value["data"]["results"].as_array().unwrap();
    assert_eq!(results.len(), 5);

    // --session: explicit resolution filled by the CLI, transcript copied in.
    let explicit = &results[0]["status"]["resolved"];
    assert_eq!(results[0]["ok"], true);
    assert_eq!(explicit["basis"], "explicit");
    assert_eq!(explicit["target"]["transcript"], "/t/s-explicit.jsonl");
    assert_eq!(
        explicit["query"],
        serde_json::json!({"target": {"harness": "claude-code", "session_id": "s-explicit"}})
    );

    // --pij: resolver answer, pij id and pane kept.
    let pij = &results[1]["status"]["resolved"];
    assert_eq!(pij["pij_id"], "pij-a");
    assert_eq!(pij["basis"], "pij_registry");
    assert_eq!(pij["target"]["session_id"], "s-pij");
    assert_eq!(pij["target"]["transcript"], "/t/s-pij.jsonl");

    // Resolution failure: no resolved block.
    assert_eq!(results[2]["ok"], false);
    assert_eq!(results[2]["query"], serde_json::json!({"pij": "pij-gone"}));
    assert_eq!(results[2]["resolved"], Value::Null);
    assert_eq!(results[2]["error"]["code"], "UNI-STATUS-DEAD-BINDING");
    assert_eq!(results[2]["error"]["kind"], "dead_binding");
    assert_eq!(
        results[2]["error"]["fix"],
        StatusFailureKind::DeadBinding.recovery()
    );

    // Status failure after resolution: resolved kept for context.
    assert_eq!(
        results[3]["error"]["code"],
        "UNI-STATUS-UNSUPPORTED-HARNESS"
    );
    assert_eq!(results[3]["resolved"]["basis"], "explicit");

    // --pane: conflicts returned with the answer.
    let pane = &results[4]["status"]["resolved"];
    assert_eq!(pane["conflicts"][0]["basis"], "native_pane");
    assert_eq!(pane["conflicts"][0]["target"]["session_id"], "s-other");

    let asked: Vec<String> = api
        .calls()
        .into_iter()
        .map(|(t, now)| {
            assert_eq!(now, NOW, "now_ms is passed through");
            t.session_id
        })
        .collect();
    assert_eq!(
        asked,
        ["s-explicit", "s-pij", "s-x", "s-pane"],
        "failed resolutions never reach the status port"
    );
}

#[test]
fn all_answered_exits_zero() {
    let command = status_command(
        &[
            "unisphere",
            "sessions",
            "status",
            "--session",
            "s1",
            "--harness",
            "claude-code",
        ],
        false,
    );
    let api = FakeStatusApi::new().with(
        Ok(answered("claude-code", "s1")),
        &target("claude-code", "s1"),
    );
    let (exit, stdout, stderr) = run(&command, &api, &FakeTargetResolver::new());
    assert_eq!(exit, 0);
    assert!(stderr.is_empty());
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["failed"], 0);
    let status: SessionStatus =
        serde_json::from_value(value["data"]["results"][0]["status"].clone()).unwrap();
    assert_eq!(status.schema_version, 1);
    assert_eq!(status.resolved.unwrap().basis, ResolveBasis::Explicit);
}

#[test]
fn human_table_has_one_row_per_query_then_conflicts_and_failures() {
    let command = status_command(
        &[
            "unisphere",
            "sessions",
            "status",
            "--pane",
            "%3",
            "--pij",
            "pij-gone",
            "--human",
        ],
        false,
    );
    let mut pane_status = answered("claude-code", "s-pane");
    pane_status.model.pending_switch = Some(ModelSwitchStatus {
        requested: "sonnet".into(),
        at_ms: Some(NOW),
    });
    let resolver = FakeTargetResolver::new().with(
        StatusQuery::Pane("%3".into()),
        Ok(Resolved {
            conflicts: vec![ResolveConflict {
                basis: ResolveBasis::NativePane,
                target: target("claude-code", "s-other"),
            }],
            ..resolved(
                StatusQuery::Pane("%3".into()),
                "s-pane",
                ResolveBasis::PijRegistry,
            )
        }),
    );
    let api = FakeStatusApi::new().with(Ok(pane_status), &target("claude-code", "s-pane"));
    let (exit, stdout, _) = run(&command, &api, &resolver);
    assert_eq!(exit, 3);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines[0].starts_with("QUERY"), "{stdout}");
    assert!(lines[1].starts_with("%3"), "{stdout}");
    assert!(
        lines[1].contains("claude-opus-4-7 -> sonnet (pending)"),
        "{stdout}"
    );
    assert!(lines[1].contains("2m05s"), "{stdout}");
    assert!(lines[1].contains("12 (3/1h)"), "{stdout}");
    assert!(
        lines[2].starts_with("pij-gone") && lines[2].contains("UNI-STATUS-PIJ-UNKNOWN-SEAT"),
        "{stdout}"
    );
    assert!(
        stdout.contains("%3: conflict (native_pane): claude-code s-other"),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "Next: {}",
            StatusFailureKind::PijUnknownSeat.recovery()
        )),
        "{stdout}"
    );
}

/// `unisphere sessions status` lines inside ```sh fences.
fn documented_examples(markdown: &str) -> Vec<Vec<OsString>> {
    let mut examples = Vec::new();
    let mut in_sh = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_sh = trimmed == "```sh";
            continue;
        }
        if in_sh && trimmed.starts_with("unisphere sessions status") {
            examples.push(trimmed.split_whitespace().map(OsString::from).collect());
        }
    }
    examples
}

#[test]
fn topic_is_registered_and_every_documented_example_parses() {
    let ParsedCommand::Docs(docs) = parse(
        argv(&["unisphere", "docs", "get", "session-status", "--json"]),
        &context(false),
    )
    .unwrap() else {
        panic!("docs route")
    };
    let mut stdout = Vec::new();
    assert_eq!(run_docs(&docs, &mut stdout, &mut Vec::new()), 0);
    let value: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["data"]["text"], TOPIC);

    for (name, markdown, minimum) in [
        ("session-status topic", TOPIC, 5),
        ("docs/cli.md", CLI_GUIDE, 2),
        ("docs/sdk.md", SDK_GUIDE, 1),
    ] {
        let examples = documented_examples(markdown);
        assert!(
            examples.len() >= minimum,
            "{name}: {} examples",
            examples.len()
        );
        for example in examples {
            assert!(
                matches!(
                    parse(example.clone(), &context(false)),
                    Ok(ParsedCommand::SessionStatus(_))
                ),
                "{name}: example does not parse: {example:?}"
            );
        }
    }
}
