//! CLI prep frontend through a fake `PrepApi`: argv → requests, envelopes, human
//! summary, exit codes, the content opt-in refusal and documented examples.
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde_json::Value;
use unisphere_cli::{
    CliContext, ParsedCommand, PrepCommand, emit_parse_failure, parse, run_docs, run_prep,
    run_prep_compact, run_prep_record,
};
use unisphere_core::{
    PipelineError, PipelineErrorKind, SnapshotLimits,
    prep::{
        NativeAddress, PrepApi, PrepCommit, PrepCompactReport, PrepCompactRequest, PrepRecord,
        PrepRecordRequest, PrepReplaceReason, PrepReport, PrepRequest, PrepSetReport,
        PrepSkipCounts, PrepSourceOutcome, PrepSourceSet, PrepSourceStatus, PrepTableCounts,
        derived_root_label,
    },
};

const PREP_TOPIC: &str = include_str!("../docs/prep.md");
const README: &str = include_str!("../../../README.md");
const CLI_GUIDE: &str = include_str!("../../../docs/cli.md");
const SDK_GUIDE: &str = include_str!("../../../docs/sdk.md");

fn context() -> CliContext {
    CliContext {
        cwd: PathBuf::from("/work"),
        stdout_is_terminal: false,
        version: "prep-test".into(),
    }
}

fn argv(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn parsed(values: &[&str]) -> ParsedCommand {
    match parse(argv(values), &context()) {
        Ok(command) => command,
        Err(failure) => panic!("{values:?} did not parse: {}", failure.code()),
    }
}

fn prep(values: &[&str]) -> PrepCommand {
    match parsed(values) {
        ParsedCommand::Prep(command) => command,
        _ => panic!("not a prep command"),
    }
}

fn refusal(values: &[&str]) -> &'static str {
    match parse(argv(values), &context()) {
        Ok(_) => panic!("{values:?} parsed"),
        Err(failure) => failure.code(),
    }
}

#[derive(Default)]
struct FakePrep {
    report: Option<Result<PrepReport, PipelineErrorKind>>,
    compact: Option<Result<PrepCompactReport, PipelineErrorKind>>,
    record: Option<Result<PrepRecord, PipelineErrorKind>>,
    preps: Mutex<Vec<PrepRequest>>,
    compacts: Mutex<Vec<PrepCompactRequest>>,
    records: Mutex<Vec<PrepRecordRequest>>,
}

fn answer<T: Clone>(value: &Option<Result<T, PipelineErrorKind>>) -> Result<T, PipelineError> {
    value
        .clone()
        .expect("unexpected port call")
        .map_err(|kind| PipelineError::new(kind, None))
}

impl PrepApi for FakePrep {
    fn prep(&self, request: &PrepRequest) -> Result<PrepReport, PipelineError> {
        self.preps.lock().unwrap().push(request.clone());
        answer(&self.report)
    }
    fn compact(&self, request: &PrepCompactRequest) -> Result<PrepCompactReport, PipelineError> {
        self.compacts.lock().unwrap().push(request.clone());
        answer(&self.compact)
    }
    fn record(&self, request: &PrepRecordRequest) -> Result<PrepRecord, PipelineError> {
        self.records.lock().unwrap().push(request.clone());
        answer(&self.record)
    }
}

fn outcome(source: &str, status: PrepSourceStatus, pending: u64) -> PrepSourceOutcome {
    PrepSourceOutcome {
        source: source.into(),
        status,
        generation: 2,
        bytes_read: 10,
        rows: 1,
        committed_offset: 10,
        pending_tail_bytes: pending,
        error: (status == PrepSourceStatus::Unreadable).then(|| "UNI-READ".into()),
    }
}

fn report(unreadable: bool) -> PrepReport {
    let mut by_status = BTreeMap::from([
        ("new".to_owned(), 1),
        ("appended".to_owned(), 1),
        ("replaced".to_owned(), 1),
        ("unchanged".to_owned(), 7),
        ("skipped".to_owned(), 2),
        ("missing".to_owned(), 1),
    ]);
    let mut sources = vec![
        outcome("claude-code/default/a.jsonl", PrepSourceStatus::New, 0),
        outcome(
            "claude-code/default/b.jsonl",
            PrepSourceStatus::Appended,
            37,
        ),
        outcome(
            "claude-code/default/c.jsonl",
            PrepSourceStatus::Replaced {
                reason: PrepReplaceReason::Truncated,
            },
            0,
        ),
        outcome(
            "claude-code/default/gone.jsonl",
            PrepSourceStatus::Missing,
            0,
        ),
    ];
    if unreadable {
        by_status.insert("unreadable".into(), 1);
        sources.push(outcome(
            "claude-code/default/locked.jsonl",
            PrepSourceStatus::Unreadable,
            0,
        ));
    }
    PrepReport {
        target: "/work/out".into(),
        table_schema_version: 2,
        run: 4,
        sets: vec![
            PrepSetReport {
                harness: "claude-code".into(),
                label: "default".into(),
                root: "/home/.claude/projects".into(),
                policy: Some("claude-code/prep-v3".into()),
                supported: true,
                discovered: 13,
                skipped: PrepSkipCounts {
                    symlinks: 3,
                    hidden: 5,
                    unreadable_entries: 1,
                },
                dirs_listed: 2,
                dirs_reused: 9,
                by_status,
            },
            PrepSetReport {
                harness: "codex".into(),
                label: "default".into(),
                root: "/home/.codex/sessions".into(),
                policy: None,
                supported: false,
                discovered: 4,
                skipped: PrepSkipCounts::default(),
                dirs_listed: 0,
                dirs_reused: 0,
                by_status: BTreeMap::from([("unsupported".to_owned(), 4)]),
            },
        ],
        bytes_read: 30,
        pending_tail_bytes: 37,
        rows_written: PrepTableCounts {
            calls: 5,
            turns: 2,
            triggers: 2,
            events: 1,
            tool_uses: 3,
        },
        commit: PrepCommit {
            parts_written: vec!["tables/calls/p1.parquet".into()],
            ..PrepCommit::default()
        },
        commits: 1,
        sources,
    }
}

fn envelope(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("one JSON envelope")
}

#[test]
fn frozen_argv_parses_into_the_port_request() {
    let command = prep(&[
        "unisphere",
        "prep",
        "--target",
        "out",
        "--root",
        "claude-code:alt=/home/.claude-alt/projects/",
        "--root",
        "claude-code=archive/./projects",
        "--harness",
        "codex",
        "--harness",
        "claude-code",
        "--harness",
        "codex",
        "--no-default-roots",
        "--include-content",
        "--max-record-bytes",
        "1024",
        "--max-batch-bytes",
        "4096",
        "--max-snapshot-bytes",
        "1048576",
        "--max-snapshot-records",
        "50",
        "--threads",
        "3",
        "--max-run-bytes",
        "65536",
        "--modified-since",
        "2026-01-01T00:00:00Z",
        "--human",
    ]);
    assert_eq!(command.target, Path::new("/work/out"));
    assert_eq!(command.harnesses, ["claude-code", "codex"]);
    assert!(command.no_default_roots);
    assert!(!command.wants_default_root("claude-code"));
    assert!(command.selects("codex") && !command.selects("cursor"));
    let archive = PathBuf::from("/work/archive/projects");
    assert_eq!(
        command.explicit_sets(),
        [
            PrepSourceSet {
                harness: "claude-code".into(),
                label: "alt".into(),
                root: "/home/.claude-alt/projects".into(),
            },
            PrepSourceSet {
                harness: "claude-code".into(),
                label: derived_root_label(&archive),
                root: archive,
            },
        ]
    );
    let roots = vec![PrepSourceSet {
        harness: "claude-code".into(),
        label: "default".into(),
        root: "/home/.claude/projects".into(),
    }];
    let api = FakePrep {
        report: Some(Ok(report(false))),
        ..FakePrep::default()
    };
    let mut stdout = Vec::new();
    assert_eq!(
        run_prep(&command, roots.clone(), &api, &mut stdout, &mut Vec::new()),
        0
    );
    let requests = api.preps.lock().unwrap();
    let [request] = requests.as_slice() else {
        panic!("one prep call")
    };
    assert_eq!(request.target, Path::new("/work/out"));
    assert_eq!(
        request.roots, roots,
        "shell-resolved roots pass through verbatim"
    );
    assert!(request.options.include_content);
    assert_eq!(request.limits.read.max_record_bytes, 1024);
    assert_eq!(request.limits.read.max_batch_bytes, 4096);
    assert_eq!(
        request.limits.snapshot,
        SnapshotLimits {
            max_records: 50,
            max_record_bytes: 1_048_576,
            max_snapshot_bytes: 1_048_576,
        },
        "one snapshot record is bounded by the snapshot"
    );
    assert_eq!(request.threads, 3);
    assert_eq!(request.max_run_bytes, 65_536);
    assert_eq!(request.modified_since_ns, Some(1_767_225_600_000_000_000));
}

#[test]
fn defaults_are_metadata_only_with_catalogue_roots_for_every_harness() {
    let command = prep(&["unisphere", "prep", "--target", "/t"]);
    assert!(command.roots.is_empty() && command.harnesses.is_empty());
    assert!(command.wants_default_root("claude-code") && command.wants_default_root("codex"));
    let request = command.request(Vec::new());
    assert!(!request.options.include_content);
    assert_eq!(request.limits.read.max_record_bytes, 16 * 1024 * 1024);
    assert_eq!(request.limits.read.max_batch_bytes, 16 * 1024 * 1024);
    assert_eq!(request.threads, 8);
    assert_eq!(request.max_run_bytes, 256 * 1024 * 1024);
    assert_eq!(request.limits.snapshot, SnapshotLimits::default());
    assert_eq!(request.modified_since_ns, None);

    let filtered = prep(&[
        "unisphere",
        "prep",
        "--target",
        "/t",
        "--harness",
        "claude-code",
    ]);
    assert!(filtered.wants_default_root("claude-code") && !filtered.wants_default_root("codex"));
}

#[test]
fn record_limit_follows_a_lowered_batch_limit_unless_given() {
    let lowered = prep(&[
        "unisphere",
        "prep",
        "--target",
        "/t",
        "--max-batch-bytes",
        "4194304",
    ])
    .request(Vec::new());
    assert_eq!(lowered.limits.read.max_record_bytes, 4_194_304);
    assert_eq!(lowered.limits.read.max_batch_bytes, 4_194_304);

    let explicit = prep(&[
        "unisphere",
        "prep",
        "--target",
        "/t",
        "--max-batch-bytes",
        "4194304",
        "--max-record-bytes",
        "1048576",
    ])
    .request(Vec::new());
    assert_eq!(explicit.limits.read.max_record_bytes, 1_048_576);
}

#[test]
fn equivalent_root_spellings_share_one_derived_label() {
    let plain = prep(&[
        "unisphere",
        "prep",
        "--target",
        "/t",
        "--root",
        "claude-code=/r/x",
    ]);
    let slash = prep(&[
        "unisphere",
        "prep",
        "--target",
        "/t",
        "--root",
        "claude-code=/r/./x/",
    ]);
    assert_eq!(plain.explicit_sets(), slash.explicit_sets());
}

#[test]
fn invalid_combinations_exit_2_before_any_port_call() {
    let cases: &[(&[&str], &str)] = &[
        (&["--no-default-roots"], "UNI-CLI-PREP-SCOPE"),
        (
            &["--harness", "codex", "--root", "claude-code=/r"],
            "UNI-CLI-PREP-SCOPE",
        ),
        (&["--harness", "Claude"], "UNI-CLI-PREP-SCOPE"),
        (
            &["--root", "claude-code=/a", "--root", "claude-code=/a/"],
            "UNI-CLI-PREP-ROOT",
        ),
        (
            &["--root", "claude-code:x=/a", "--root", "claude-code:x=/b"],
            "UNI-CLI-PREP-ROOT",
        ),
        (&["--root", "claude-code:default=/a"], "UNI-CLI-PREP-ROOT"),
        (&["--root", "claude-code:a/b=/a"], "UNI-CLI-PREP-ROOT"),
        (&["--root", "claude-code:=/a"], "UNI-CLI-PREP-ROOT"),
        (&["--root", "claude-code"], "UNI-CLI-PREP-ROOT"),
        (&["--root", "claude-code="], "UNI-CLI-PREP-ROOT"),
        (&["--root", ".hidden=/a"], "UNI-CLI-PREP-ROOT"),
        (&["--threads", "0"], "UNI-CLI-PREP-LIMITS"),
        (&["--max-run-bytes", "0"], "UNI-CLI-PREP-LIMITS"),
        (&["--max-record-bytes", "0"], "UNI-CLI-PREP-LIMITS"),
        (
            &["--max-record-bytes", "10", "--max-batch-bytes", "9"],
            "UNI-CLI-PREP-LIMITS",
        ),
        (&["--modified-since", "yesterday"], "UNI-CLI-PREP-LIMITS"),
        (&["--max-snapshot-bytes", "0"], "UNI-CLI-PREP-LIMITS"),
        (&["--max-snapshot-records", "0"], "UNI-CLI-PREP-LIMITS"),
    ];
    for (extra, code) in cases {
        let mut values = vec!["unisphere", "prep", "--target", "/t"];
        values.extend_from_slice(extra);
        assert_eq!(refusal(&values), *code, "{extra:?}");
    }
    // Same harness in two sets under different directories is a legitimate second account.
    prep(&[
        "unisphere",
        "prep",
        "--target",
        "/t",
        "--root",
        "claude-code:a=/a",
        "--root",
        "claude-code:b=/b",
    ]);
    for values in [
        &["unisphere", "prep"][..],
        &[
            "unisphere",
            "prep",
            "--target",
            "/t",
            "compact",
            "--target",
            "/t",
        ],
        &["unisphere", "prep", "compact"],
        &[
            "unisphere",
            "prep",
            "record",
            "--target",
            "/t",
            "--source",
            "k",
            "--include-content",
        ],
        &[
            "unisphere",
            "prep",
            "record",
            "--target",
            "/t",
            "--source",
            "k",
            "--offset",
            "1",
            "--key",
            "x",
            "--include-content",
        ],
    ] {
        assert_eq!(refusal(values), "UNI-CLI-ARGUMENT", "{values:?}");
    }
    let failure = parse(
        argv(&["unisphere", "prep", "--target", "/t", "--threads", "0"]),
        &context(),
    )
    .err()
    .unwrap();
    let mut stdout = Vec::new();
    assert_eq!(
        emit_parse_failure(
            &failure,
            unisphere_cli::OutputMode::Json,
            &mut stdout,
            &mut Vec::new()
        ),
        2
    );
    assert_eq!(envelope(&stdout)["error"]["code"], "UNI-CLI-PREP-LIMITS");
}

#[test]
fn record_without_content_opt_in_is_refused_with_an_actionable_next_action() {
    let failure = parse(
        argv(&[
            "unisphere",
            "prep",
            "record",
            "--target",
            "/t",
            "--source",
            "claude-code/default/a.jsonl",
            "--offset",
            "0",
            "--json",
        ]),
        &context(),
    )
    .err()
    .expect("refused at parse, before any port exists");
    assert_eq!(failure.code(), "UNI-CLI-PREP-CONTENT");
    let mut stdout = Vec::new();
    assert_eq!(
        emit_parse_failure(
            &failure,
            unisphere_cli::OutputMode::Json,
            &mut stdout,
            &mut Vec::new()
        ),
        2
    );
    let value = envelope(&stdout);
    assert_eq!(value["ok"], false);
    assert!(
        value["next_action"]
            .as_str()
            .unwrap()
            .contains("--include-content")
    );

    // A hand-built command without the opt-in is refused too, with no port call.
    let ParsedCommand::PrepRecord(mut command) = parsed(&[
        "unisphere",
        "prep",
        "record",
        "--target",
        "/t",
        "--source",
        "claude-code/default/a.jsonl",
        "--offset",
        "0",
        "--include-content",
        "--json",
    ]) else {
        panic!("record route")
    };
    command.include_content = false;
    let api = FakePrep::default();
    let mut stdout = Vec::new();
    assert_eq!(
        run_prep_record(&command, &api, &mut stdout, &mut Vec::new()),
        2
    );
    assert!(api.records.lock().unwrap().is_empty());
    let value = envelope(&stdout);
    assert_eq!(value["command"], "prep.record");
    assert_eq!(value["error"]["code"], "UNI-INPUT");
    let argv = value["next_action"]["argv"].as_array().unwrap();
    assert_eq!(argv[..3], ["unisphere", "prep", "record"]);
}

#[test]
fn machine_envelope_carries_the_whole_report_and_unreadable_exits_3() {
    let command = prep(&["unisphere", "prep", "--target", "/work/out", "--json"]);
    for (unreadable, exit) in [(false, 0), (true, 3)] {
        let api = FakePrep {
            report: Some(Ok(report(unreadable))),
            ..FakePrep::default()
        };
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_prep(&command, Vec::new(), &api, &mut stdout, &mut stderr),
            exit
        );
        assert!(stderr.is_empty());
        let value = envelope(&stdout);
        assert_eq!(
            (
                value["ok"].clone(),
                value["command"].clone(),
                value["v"].clone()
            ),
            (true.into(), "prep".into(), 1.into())
        );
        assert_eq!(
            value["data"],
            serde_json::to_value(report(unreadable)).unwrap()
        );
        let set = &value["data"]["sets"][0];
        assert_eq!(set["skipped"]["symlinks"], 3);
        assert_eq!(set["skipped"]["hidden"], 5);
        assert_eq!(value["data"]["sets"][1]["supported"], false);
        assert_eq!(value["data"]["pending_tail_bytes"], 37);
        let action = &value["next_action"];
        assert!(!action["summary"].as_str().unwrap().is_empty());
        if unreadable {
            assert_eq!(
                action["argv"],
                serde_json::json!(["unisphere", "prep", "--target"])
            );
            assert_eq!(action["required_inputs"], serde_json::json!(["target"]));
        }
    }
}

#[test]
fn human_summary_reports_coverage_statuses_skips_and_pending_tails() {
    let command = prep(&["unisphere", "prep", "--target", "/work/out", "--human"]);
    let api = FakePrep {
        report: Some(Ok(report(true))),
        ..FakePrep::default()
    };
    let mut stdout = Vec::new();
    assert_eq!(
        run_prep(&command, Vec::new(), &api, &mut stdout, &mut Vec::new()),
        3
    );
    let text = String::from_utf8(stdout).unwrap();
    for expected in [
        "run 4, table schema v2",
        "claude-code/default  /home/.claude/projects",
        "discovered 13: 1 new, 1 appended, 1 replaced, 7 unchanged, 2 skipped, 1 missing, 1 unreadable",
        "symlinks 3, hidden 5, unreadable entries 1",
        "codex/default",
        "unsupported: no prep binding",
        "discovered 4: 4 unsupported",
        "rows written: calls 5, turns 2, triggers 2, events 1, tool_uses 3",
        "Pending tails: 37 byte(s)",
        "claude-code/default/b.jsonl  generation 2; pending tail 37 byte(s)",
        "replaced (truncated)",
        "missing",
        "unreadable             claude-code/default/locked.jsonl",
        "Next: restore read access",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in\n{text}");
    }
    assert!(
        !text.contains("a.jsonl"),
        "new sources without tails are counted, not listed"
    );
}

#[test]
fn port_failures_map_to_usage_or_error_exits() {
    let command = prep(&["unisphere", "prep", "--target", "/t", "--json"]);
    for (kind, exit) in [
        (PipelineErrorKind::InvalidInput, 2),
        (PipelineErrorKind::Write, 1),
    ] {
        let api = FakePrep {
            report: Some(Err(kind)),
            ..FakePrep::default()
        };
        let mut stdout = Vec::new();
        assert_eq!(
            run_prep(&command, Vec::new(), &api, &mut stdout, &mut Vec::new()),
            exit
        );
        let value = envelope(&stdout);
        assert_eq!(value["ok"], false);
        assert_eq!(value["command"], "prep");
        assert!(!value["next_action"]["summary"].as_str().unwrap().is_empty());
    }
    let human = prep(&["unisphere", "prep", "--target", "/t", "--human"]);
    let api = FakePrep {
        report: Some(Err(PipelineErrorKind::Read)),
        ..FakePrep::default()
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    assert_eq!(
        run_prep(&human, Vec::new(), &api, &mut stdout, &mut stderr),
        1
    );
    assert!(stdout.is_empty());
    assert!(String::from_utf8(stderr).unwrap().starts_with("UNI-READ: "));
}

#[test]
fn compact_and_record_render_their_reports() {
    let ParsedCommand::PrepCompact(compact) =
        parsed(&["unisphere", "prep", "compact", "--target", "out", "--json"])
    else {
        panic!("compact route")
    };
    let compacted = PrepCompactReport {
        target: "/work/out".into(),
        parts_before: 9,
        parts_after: 5,
        ..PrepCompactReport::default()
    };
    let api = FakePrep {
        compact: Some(Ok(compacted.clone())),
        ..FakePrep::default()
    };
    let mut stdout = Vec::new();
    assert_eq!(
        run_prep_compact(&compact, &api, &mut stdout, &mut Vec::new()),
        0
    );
    assert_eq!(
        api.compacts.lock().unwrap()[0].target,
        Path::new("/work/out")
    );
    let value = envelope(&stdout);
    assert_eq!(value["command"], "prep.compact");
    assert_eq!(value["data"], serde_json::to_value(&compacted).unwrap());

    let ParsedCommand::PrepRecord(record) = parsed(&[
        "unisphere",
        "prep",
        "record",
        "--target",
        "out",
        "--source",
        "claude-code/default/a.jsonl",
        "--key",
        "k1",
        "--include-content",
        "--json",
    ]) else {
        panic!("record route")
    };
    for (bytes, encoding, text) in [
        (b"{\"type\":\"x\"}".to_vec(), "utf8", "{\"type\":\"x\"}"),
        (vec![0xff, 0x00, 0x41], "hex", "ff0041"),
    ] {
        let api = FakePrep {
            record: Some(Ok(PrepRecord {
                source: "claude-code/default/a.jsonl".into(),
                path: "/r/a.jsonl".into(),
                address: NativeAddress {
                    offset: None,
                    key: Some("k1".into()),
                },
                bytes: bytes.clone(),
            })),
            ..FakePrep::default()
        };
        let mut stdout = Vec::new();
        assert_eq!(
            run_prep_record(&record, &api, &mut stdout, &mut Vec::new()),
            0
        );
        let request = api.records.lock().unwrap()[0].clone();
        assert!(request.include_content);
        assert_eq!(request.address.key.as_deref(), Some("k1"));
        assert_eq!(request.address.offset, None);
        assert_eq!(request.max_bytes, 16 * 1024 * 1024);
        let value = envelope(&stdout);
        assert_eq!(value["command"], "prep.record");
        assert_eq!(value["data"]["encoding"], encoding);
        assert_eq!(value["data"]["record"], text);
        assert_eq!(value["data"]["bytes"], bytes.len());
    }

    let ParsedCommand::PrepRecord(human) = parsed(&[
        "unisphere",
        "prep",
        "record",
        "--target",
        "out",
        "--source",
        "s",
        "--offset",
        "7",
        "--include-content",
        "--human",
    ]) else {
        panic!("record route")
    };
    let api = FakePrep {
        record: Some(Ok(PrepRecord {
            source: "s".into(),
            path: "/r/s".into(),
            address: NativeAddress {
                offset: Some(7),
                key: None,
            },
            bytes: b"{\"a\":1}".to_vec(),
        })),
        ..FakePrep::default()
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    assert_eq!(run_prep_record(&human, &api, &mut stdout, &mut stderr), 0);
    assert_eq!(
        stdout, b"{\"a\":1}\n",
        "human record is the raw native line"
    );
    assert!(String::from_utf8(stderr).unwrap().starts_with("Next: "));
}

/// `unisphere …` argv inside ```sh fences, with `$TARGET`/`$HOME` bound, up to
/// the first `|` (the external engine).
fn documented_examples(markdown: &str) -> Vec<Vec<OsString>> {
    let mut examples = Vec::new();
    let mut in_sh = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_sh = trimmed == "```sh";
            continue;
        }
        if in_sh && trimmed.starts_with("unisphere prep") {
            let bound = trimmed
                .replace("$TARGET", "/fixtures/prep-target")
                .replace("$HOME", "/fixtures/home");
            assert!(!bound.contains('$'), "unbound placeholder in {trimmed:?}");
            examples.push(
                bound
                    .split_whitespace()
                    .take_while(|word| *word != "|")
                    .map(OsString::from)
                    .collect(),
            );
        }
    }
    examples
}

#[test]
fn prep_topic_is_registered_and_every_documented_example_parses() {
    let ParsedCommand::Docs(docs) = parsed(&["unisphere", "docs", "get", "prep", "--json"]) else {
        panic!("docs route")
    };
    let mut stdout = Vec::new();
    assert_eq!(run_docs(&docs, &mut stdout, &mut Vec::new()), 0);
    let value = envelope(&stdout);
    assert_eq!(value["data"]["id"], "prep");
    assert_eq!(value["data"]["text"], PREP_TOPIC);

    for (name, markdown, minimum) in [
        ("prep topic", PREP_TOPIC, 13),
        ("README.md", README, 1),
        ("docs/cli.md", CLI_GUIDE, 3),
        ("docs/sdk.md", SDK_GUIDE, 1),
    ] {
        let examples = documented_examples(markdown);
        assert!(
            examples.len() >= minimum,
            "{name}: {} prep examples",
            examples.len()
        );
        for example in examples {
            assert!(
                parse(example.clone(), &context()).is_ok(),
                "{name}: example does not parse: {example:?}"
            );
        }
    }
}
