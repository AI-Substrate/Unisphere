use std::{collections::BTreeMap, ffi::OsString, io::Write, path::PathBuf};

use serde_json::Value;
use unisphere_cli::{
    CliContext, DocsCommand, OutputMode, ParsedCommand, PijTarget, QueryCommand, diagnostic_mode,
    emit_parse_failure, emit_pij_failure, emit_pij_resolution, emit_query_failure, parse,
    run_catalog, run_config, run_docs, run_help, run_query, run_schema, run_version,
};
use unisphere_core::query::{
    ActionReason, Completeness, ContentAccess, Coverage, Dataset, Digest, EntityId, EntityKind,
    FieldId, FieldValue, Filter, OperationKind, Predicate, ProjectedRow, QueryAction,
    QueryDescription, QueryFailure, QueryFailureCode, QueryResponse, QueryScope, QueryWriter,
    RecoveryAction, ResultUniverse, SavedFormat, SourceId, SourceSelector, UniverseBasis,
};
use unisphere_core::{Configuration, InspectionReport};
use unisphere_output_query::ProjectedQueryWriter;
use unisphere_testkit::{FakeInspector, query::FakeQueryApi};

const EXAMPLE_CASES: &str = include_str!("../../testkit/fixtures/query-docs/example-cases.json");

fn context() -> CliContext {
    CliContext {
        cwd: PathBuf::from(if cfg!(windows) {
            r"C:\query-test"
        } else {
            "/query-test"
        }),
        stdout_is_terminal: false,
        version: "query-test".into(),
    }
}

fn argv(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn entity(kind: EntityKind, name: &[u8]) -> String {
    EntityId::derive(kind, [name]).to_string()
}

fn emitted_action(bytes: &[u8]) -> Vec<OsString> {
    let envelope: Value = serde_json::from_slice(bytes).unwrap();
    envelope["next_action"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| OsString::from(value.as_str().unwrap()))
        .collect()
}

fn parse_csv(bytes: &[u8]) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = Vec::new();
    let mut quoted = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'"' if quoted && bytes.get(index + 1) == Some(&b'"') => {
                field.push(b'"');
                index += 1;
            }
            b'"' => quoted = !quoted,
            b',' if !quoted => record.push(String::from_utf8(std::mem::take(&mut field)).unwrap()),
            b'\r' if !quoted && bytes.get(index + 1) == Some(&b'\n') => {
                record.push(String::from_utf8(std::mem::take(&mut field)).unwrap());
                records.push(std::mem::take(&mut record));
                index += 1;
            }
            byte => field.push(byte),
        }
        index += 1;
    }
    assert!(!quoted && field.is_empty() && record.is_empty());
    records
}

#[test]
fn diagnostic_mode_is_selected_before_parse_consumes_argv() {
    assert_eq!(
        diagnostic_mode(&argv(&["unisphere", "--human", "--json"]), true),
        OutputMode::Json
    );
    assert_eq!(
        diagnostic_mode(&argv(&["unisphere"]), true),
        OutputMode::Human
    );
    assert_eq!(
        diagnostic_mode(
            &argv(&["unisphere", "sessions", "list", "--format", "jsonl"]),
            false,
        ),
        OutputMode::JsonDiagnostic
    );
    let args = argv(&[
        "unisphere",
        "sessions",
        "list",
        "--repo",
        ".",
        "--format",
        "jsonl",
        "--unknown",
    ]);
    let mode = diagnostic_mode(&args, false);
    let failure = parse(args, &context()).err().expect("invalid raw query");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        emit_parse_failure(&failure, mode, &mut stdout, &mut stderr),
        2
    );
    assert!(stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&stderr).unwrap()["ok"],
        false
    );
}

#[test]
fn pij_selector_produces_explicit_pending_targets() {
    for (root, dataset) in [
        ("sources", Dataset::Sources),
        ("sessions", Dataset::Sessions),
        ("turns", Dataset::Turns),
        ("messages", Dataset::Messages),
        ("tools", Dataset::Tools),
        ("events", Dataset::Events),
    ] {
        let ParsedCommand::PijQuery(command) = parse(
            argv(&[
                "unisphere",
                root,
                "list",
                "--pij",
                "pij-example-seat",
                "--format",
                "json",
            ]),
            &context(),
        )
        .unwrap() else {
            panic!("shared Pij dataset route");
        };
        assert_eq!(command.target, PijTarget::Query);
        assert_eq!(command.query.request.dataset, dataset);
    }

    let ParsedCommand::PijQuery(source_check) = parse(
        argv(&[
            "unisphere",
            "sources",
            "check",
            "--pij",
            "pij-example-seat",
            "--format",
            "json",
        ]),
        &context(),
    )
    .unwrap() else {
        panic!("Pij source check route");
    };
    assert_eq!(source_check.target, PijTarget::SourceCheck);
    assert_eq!(
        source_check.query.request.operation.kind(),
        OperationKind::List
    );

    let ParsedCommand::PijQuery(session_show) = parse(
        argv(&[
            "unisphere",
            "sessions",
            "show",
            "--pij",
            "pij-example-seat",
            "--format",
            "json",
        ]),
        &context(),
    )
    .unwrap() else {
        panic!("Pij session show route");
    };
    assert_eq!(session_show.id, "pij-example-seat");
    assert_eq!(session_show.target, PijTarget::SessionShow);
    assert_eq!(
        session_show.query.request.operation.kind(),
        OperationKind::List
    );
    assert!(matches!(
        session_show.query.request.scope,
        QueryScope::Repository { ref path, .. } if path == &context().cwd
    ));

    let turn = entity(EntityKind::Turn, b"pij-turn");
    let ParsedCommand::PijQuery(query) = parse(
        argv(&[
            "unisphere",
            "turns",
            "show",
            &turn,
            "--pij",
            "pij-example-seat",
            "--format",
            "json",
        ]),
        &context(),
    )
    .unwrap() else {
        panic!("ordinary Pij query route");
    };
    assert_eq!(query.target, PijTarget::Query);
    assert_eq!(query.query.request.operation.kind(), OperationKind::Show);
}

#[test]
fn pij_selector_rejects_contradictory_or_unsafe_identity_inputs() {
    let session = entity(EntityKind::Session, b"conflicting-session");
    let conflicts = [
        vec!["--repo", "."],
        vec!["--source", "/source"],
        vec!["--input", "saved.json"],
        vec!["--repo-scope", "exact"],
        vec!["--session", session.as_str()],
        vec!["--native-id", "native"],
        vec!["--harness", "claude-code"],
        vec!["--exclude-harness", "claude-code"],
        vec!["--source-adapter", "claude-code"],
        vec!["--exclude-source-adapter", "claude-code"],
    ];
    for conflict in conflicts {
        let mut args = argv(&["unisphere", "tools", "list", "--pij", "pij-example-seat"]);
        args.extend(conflict.into_iter().map(OsString::from));
        let failure = parse(args, &context())
            .err()
            .expect("conflicting Pij selector");
        assert_eq!(failure.code(), "UNI-CLI-PIJ-CONFLICT");
    }

    for id in ["", "-option", "has space", "line\nbreak"] {
        let failure = parse(
            vec![
                "unisphere".into(),
                "sessions".into(),
                "list".into(),
                format!("--pij={id}").into(),
            ],
            &context(),
        )
        .err()
        .expect("invalid Pij ID");
        assert!(matches!(
            failure.code(),
            "UNI-CLI-PIJ-ID" | "UNI-CLI-ARGUMENT"
        ));
    }

    assert!(
        parse(
            argv(&[
                "unisphere",
                "sessions",
                "show",
                &session,
                "--pij",
                "pij-example-seat",
            ]),
            &context(),
        )
        .is_err()
    );
}

#[test]
fn pij_turn_range_stays_pending_and_duration_threshold_is_finite_float() {
    let ParsedCommand::PijQuery(pending) = parse(
        argv(&[
            "unisphere",
            "tools",
            "extract",
            "--pij",
            "pij-example-seat",
            "--range",
            "1:3",
            "--min-duration",
            "500.25",
            "--format",
            "jsonl",
        ]),
        &context(),
    )
    .unwrap() else {
        panic!("pending Pij query");
    };
    assert!(pending.query.request.turn_range.is_some());
    let duration = pending
        .query
        .request
        .filters
        .iter()
        .find(|filter| filter.field == FieldId::DurationMs)
        .expect("duration filter");
    assert!(matches!(duration.values.as_slice(), [FieldValue::Float(value)] if *value == 500.25));

    assert!(
        parse(
            argv(&[
                "unisphere",
                "tools",
                "list",
                "--repo",
                ".",
                "--min-duration",
                "-0.1",
            ]),
            &context(),
        )
        .is_err()
    );
}

#[test]
fn complete_catalogue_grammar_parses_to_typed_commands() {
    let source = entity(EntityKind::Source, b"source");
    let session = entity(EntityKind::Session, b"session");
    let turn = entity(EntityKind::Turn, b"turn");
    let message = entity(EntityKind::Message, b"message");
    let tool = entity(EntityKind::Tool, b"tool");
    let event = entity(EntityKind::Event, b"event");
    let cases = vec![
        vec!["unisphere", "adapters", "list", "--json"],
        vec![
            "unisphere",
            "sources",
            "list",
            "--repo",
            ".",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "sources",
            "check",
            "--source",
            &source,
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "sessions",
            "list",
            "--repo",
            ".",
            "--harness",
            "claude-code",
            "--source-adapter",
            "claude-code",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "sessions",
            "show",
            &session,
            "--repo",
            ".",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "sessions",
            "tree",
            &session,
            "--repo",
            ".",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "sessions",
            "stats",
            "--repo",
            ".",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "sessions",
            "extract",
            "--repo",
            ".",
            "--session",
            &session,
            "--include-content",
            "--format",
            "markdown",
        ],
        vec![
            "unisphere",
            "sessions",
            "export",
            "--adapter",
            "claude-code",
            "--input",
            "native.jsonl",
        ],
        vec![
            "unisphere",
            "turns",
            "list",
            "--repo",
            ".",
            "--session",
            &session,
            "--min-tool-calls",
            "2",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "turns",
            "show",
            &turn,
            "--repo",
            ".",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "turns",
            "stats",
            "--repo",
            ".",
            "--group-by",
            "session_id",
            "--metrics",
            "count",
            "--sort=-count",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "turns",
            "extract",
            "--repo",
            ".",
            "--has-tool-family",
            "shell",
            "--has-errors",
            "--context-before",
            "1",
            "--include-content",
            "--format",
            "jsonl",
        ],
        vec![
            "unisphere",
            "messages",
            "list",
            "--repo",
            ".",
            "--role",
            "user",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "messages",
            "show",
            &message,
            "--repo",
            ".",
            "--include-content",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "messages",
            "extract",
            "--repo",
            ".",
            "--role",
            "user",
            "--since",
            "2026-09-01",
            "--until",
            "2026-09-02",
            "--time-field",
            "timestamp",
            "--include-content",
            "--format",
            "text",
        ],
        vec![
            "unisphere",
            "tools",
            "list",
            "--repo",
            ".",
            "--status",
            "failed",
            "--include-content",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "tools",
            "show",
            &tool,
            "--repo",
            ".",
            "--include-content",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "tools",
            "stats",
            "--repo",
            ".",
            "--tool-family",
            "shell",
            "--metrics",
            "count,measured_count,missing_duration_count,failures,mean_ms,p50_ms,p95_ms",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "tools",
            "extract",
            "--repo",
            ".",
            "--tool-family",
            "shell",
            "--include-content",
            "--columns",
            "id,command,duration_ms,status",
            "--format",
            "jsonl",
        ],
        vec![
            "unisphere",
            "events",
            "list",
            "--repo",
            ".",
            "--kind",
            "tool_start",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "events",
            "show",
            &event,
            "--repo",
            ".",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "events",
            "extract",
            "--repo",
            ".",
            "--since",
            "2026-09-02",
            "--until",
            "2026-09-03",
            "--format",
            "jsonl",
        ],
        vec!["unisphere", "schema", "show", "tools", "--json"],
        vec!["unisphere", "docs", "list", "--json"],
        vec!["unisphere", "docs", "get", "tool-analysis", "--human"],
        vec!["unisphere", "config", "check", "--json"],
    ];
    assert_eq!(cases.len(), 27);
    for case in cases {
        assert!(
            parse(argv(&case), &context()).is_ok(),
            "failed grammar leaf: {case:?}"
        );
    }
}

#[test]
fn seven_staged_examples_parse_after_declared_bindings() {
    let manifest: Value = serde_json::from_str(EXAMPLE_CASES).unwrap();
    let session = entity(EntityKind::Session, b"bound-session");
    for case in manifest["cases"].as_array().unwrap() {
        let bound = case["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|argument| match argument.as_str().unwrap() {
                "${REPO}" => OsString::from("/fixtures/project"),
                "${SAVED_INPUT}" => OsString::from("/fixtures/saved-sessions.json"),
                "${SESSION_ID}" => OsString::from(&session),
                argument => OsString::from(argument),
            })
            .collect::<Vec<_>>();
        assert!(
            parse(bound, &context()).is_ok(),
            "example did not parse: {}",
            case["id"]
        );
    }
}

#[test]
fn parsed_static_and_config_commands_execute_without_reparsing() {
    let context = context();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let ParsedCommand::Config(config) =
        parse(argv(&["unisphere", "config", "check", "--json"]), &context).unwrap()
    else {
        panic!("config route");
    };
    let inspector = FakeInspector::new(Ok(InspectionReport {
        configuration: Configuration {
            source_roots: Vec::new(),
        },
    }));
    assert_eq!(
        run_config(&config, &context, &inspector, &mut stdout, &mut stderr),
        0
    );
    assert_eq!(inspector.requests().len(), 1);
    let mut next = emitted_action(&stdout);
    next.push("/bound/repository".into());
    assert!(matches!(parse(next, &context), Ok(ParsedCommand::Query(_))));

    stdout.clear();
    let ParsedCommand::Catalog(catalog) =
        parse(argv(&["unisphere", "adapters", "list", "--json"]), &context).unwrap()
    else {
        panic!("catalog route");
    };
    assert_eq!(run_catalog(&catalog, &[], &mut stdout, &mut stderr), 0);
    assert!(parse(emitted_action(&stdout), &context).is_ok());

    stdout.clear();
    let ParsedCommand::Help(help) =
        parse(argv(&["unisphere", "--help", "--json"]), &context).unwrap()
    else {
        panic!("help route");
    };
    assert_eq!(run_help(&help, &mut stdout, &mut stderr), 0);
    assert!(matches!(
        parse(emitted_action(&stdout), &context),
        Ok(ParsedCommand::Docs(_))
    ));

    stdout.clear();
    let ParsedCommand::Version { mode } =
        parse(argv(&["unisphere", "--version", "--json"]), &context).unwrap()
    else {
        panic!("version route");
    };
    assert_eq!(run_version(&context, mode, &mut stdout, &mut stderr), 0);
    assert!(matches!(
        parse(emitted_action(&stdout), &context),
        Ok(ParsedCommand::Help(_))
    ));
}

#[test]
fn static_lookup_errors_name_valid_alternatives() {
    let context = context();

    let field = parse(
        argv(&[
            "unisphere",
            "sessions",
            "list",
            "--repo",
            ".",
            "--columns",
            "not_a_field",
            "--format",
            "json",
        ]),
        &context,
    )
    .err()
    .unwrap();
    assert_eq!(field.code(), "UNI-QUERY-FIELD");
    assert!(field.alternatives().iter().any(|value| value == "id"));
    let schema = parse(
        argv(&["unisphere", "schema", "show", "unknown", "--json"]),
        &context,
    )
    .err()
    .unwrap();
    assert_eq!(schema.code(), "UNI-CLI-DATASET");
    assert!(
        schema
            .alternatives()
            .iter()
            .any(|value| value == "sessions")
    );

    let ParsedCommand::Docs(command) = parse(
        argv(&["unisphere", "docs", "get", "unknown", "--json"]),
        &context,
    )
    .unwrap() else {
        panic!("docs route");
    };
    let mut stderr = Vec::new();
    assert_eq!(run_docs(&command, &mut Vec::new(), &mut stderr), 2);
    let error: Value = serde_json::from_slice(&stderr).unwrap();
    assert!(
        error["error"]["alternatives"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value == "start"))
    );
}

#[test]
fn native_and_query_session_routes_are_unambiguous() {
    assert!(matches!(
        parse(
            argv(&["unisphere", "sessions", "list", "--root", "."]),
            &context()
        ),
        Ok(ParsedCommand::NativeRootList(_))
    ));
    let git_executable = if cfg!(windows) {
        r"C:\Git\git.exe"
    } else {
        "/usr/bin/git"
    };
    let git_list = parse(
        argv(&[
            "unisphere",
            "sessions",
            "list",
            "--adapter",
            "git-ai",
            "--repo",
            ".",
            "--notes-ref",
            "refs/notes/ai",
            "--commit",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--git-executable",
            git_executable,
            "--max-notes",
            "12",
            "--max-records",
            "34",
            "--max-note-bytes",
            "56",
            "--max-total-bytes",
            "78",
            "--max-listing-bytes",
            "90",
            "--command-timeout-ms",
            "1234",
        ]),
        &context(),
    );
    let Ok(ParsedCommand::NativeGitNotesList(git_list)) = git_list else {
        panic!("native Git Notes list route");
    };
    assert_eq!(git_list.notes_ref, "refs/notes/ai");
    assert_eq!(git_list.commits.len(), 1);
    assert_eq!(git_list.max_notes, 12);
    assert_eq!(git_list.max_records, 34);
    assert_eq!(git_list.command_timeout_ms, 1234);

    let git_export = parse(
        argv(&[
            "unisphere",
            "sessions",
            "export",
            "--adapter",
            "git-ai",
            "--repo",
            ".",
            "--include-content",
            "--max-notes",
            "12",
        ]),
        &context(),
    );
    let Ok(ParsedCommand::NativeExport(git_export)) = git_export else {
        panic!("native Git Notes export route");
    };
    assert_eq!(git_export.notes_ref.as_deref(), Some("refs/notes/ai"));
    assert_eq!(git_export.max_records, Some(10_000));
    let query = parse(
        argv(&[
            "unisphere",
            "sessions",
            "list",
            "--repo",
            ".",
            "--source-adapter",
            "git-ai",
            "--format",
            "json",
        ]),
        &context(),
    );
    assert!(matches!(query, Ok(ParsedCommand::Query(_))));

    for invalid in [
        vec![
            "unisphere",
            "sessions",
            "list",
            "--root",
            ".",
            "--repo",
            ".",
        ],
        vec![
            "unisphere",
            "sessions",
            "list",
            "--adapter",
            "git-ai",
            "--repo",
            ".",
            "--source-adapter",
            "git-ai",
        ],
        vec![
            "unisphere",
            "sessions",
            "list",
            "--repo",
            ".",
            "--adapter",
            "claude-code",
        ],
        vec![
            "unisphere",
            "sessions",
            "list",
            "--repo",
            ".",
            "--json",
            "--format",
            "json",
        ],
        vec![
            "unisphere",
            "sessions",
            "list",
            "--root",
            ".",
            "--notes-ref",
            "refs/notes/ai",
        ],
        vec![
            "unisphere",
            "sessions",
            "export",
            "--adapter",
            "claude-code",
            "--input",
            "x",
            "--max-notes",
            "1",
        ],
    ] {
        let failure = parse(argv(&invalid), &context())
            .err()
            .expect("mixed route fails");
        assert!(!failure.next_action().is_empty());
    }
}

#[test]
fn bundled_docs_and_schema_are_real_static_routes() {
    let ParsedCommand::Docs(command @ DocsCommand::List { .. }) =
        parse(argv(&["unisphere", "docs", "list", "--json"]), &context()).unwrap()
    else {
        panic!("docs list route");
    };
    let mut output = Vec::new();
    assert_eq!(run_docs(&command, &mut output, &mut Vec::new()), 0);
    let envelope: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["command"], "docs.list");
    assert!(
        envelope["data"]["topics"]
            .as_array()
            .is_some_and(|topics| topics.len() == 14)
    );
    assert!(envelope["next_action"]["argv"].is_array());

    let ParsedCommand::Schema(command) = parse(
        argv(&["unisphere", "schema", "show", "tools", "--json"]),
        &context(),
    )
    .unwrap() else {
        panic!("schema route");
    };
    output.clear();
    assert_eq!(run_schema(&command, &mut output, &mut Vec::new()), 0);
    let envelope: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(envelope["data"]["dataset"], "tools");
    assert!(
        envelope["data"]["formats"]
            .as_array()
            .is_some_and(|formats| !formats.is_empty())
    );
}

fn response(dataset: Dataset, operation: OperationKind, action: QueryAction) -> QueryResponse {
    QueryResponse {
        schema_version: 1,
        dataset,
        query: QueryDescription {
            dataset,
            operation,
            scope_digest: Digest::of_bytes(b"scope"),
        },
        rows: Vec::new(),
        coverage: Coverage::default(),
        universe: ResultUniverse {
            source_view_digest: None,
            selection_digest: Digest::of_bytes(b"selection"),
            columns_digest: Digest::of_bytes(b"columns"),
            columns: vec![FieldId::Id, FieldId::SourceRefs],
            applied_limit: None,
            rows_complete_for_selection: Completeness::Complete,
            partitions_complete: Completeness::Complete,
            basis: UniverseBasis::LiveView,
            bounded_by_input: false,
        },
        matched: 0,
        emitted: 0,
        next_cursor: None,
        next_action: action,
    }
}

fn query_command(values: &[&str]) -> QueryCommand {
    let ParsedCommand::Query(command) = parse(argv(values), &context()).unwrap() else {
        panic!("query route");
    };
    command
}

#[test]
fn raw_rows_stay_clean_and_guidance_uses_diagnostic_channel() {
    let command = query_command(&[
        "unisphere",
        "messages",
        "extract",
        "--repo",
        ".",
        "--format",
        "jsonl",
    ]);
    let query = FakeQueryApi::new(Ok(response(
        Dataset::Messages,
        OperationKind::Extract,
        QueryAction::ReadSchema {
            dataset: Dataset::Messages,
            reason: ActionReason::EmptySelection,
        },
    )));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_query(
            &command,
            &context(),
            &query,
            &ProjectedQueryWriter,
            &mut stdout,
            &mut stderr
        ),
        0
    );
    assert!(stdout.is_empty(), "JSONL has no fake guidance row");
    let summary: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(summary["data"]["emitted"], 0);
    assert_eq!(summary["next_action"]["argv"][1], "schema");
}

#[test]
fn csv_rows_parse_separately_from_loss_and_coverage_diagnostics() {
    let command = query_command(&[
        "unisphere",
        "tools",
        "extract",
        "--repo",
        ".",
        "--format",
        "csv",
        "--csv-safety",
        "raw",
    ]);
    let mut result = response(
        Dataset::Tools,
        OperationKind::Extract,
        QueryAction::InspectCoverage {
            reason: ActionReason::PartialEvidence,
        },
    );
    result.rows = vec![
        ProjectedRow::new(
            Dataset::Tools,
            EntityId::derive(EntityKind::Tool, [b"csv-tool".as_slice()]),
            Vec::new(),
            BTreeMap::new(),
            &ContentAccess::default(),
        )
        .unwrap(),
    ];
    result.matched = 1;
    result.emitted = 1;
    result.coverage.discovered_sources = 2;
    result.coverage.loaded_sources = 1;
    result.coverage.selected_sources = 1;
    result.coverage.source_read_complete = false;
    result.universe.rows_complete_for_selection = Completeness::Subset;
    result.universe.partitions_complete = Completeness::Unknown;
    result.universe.basis = UniverseBasis::SavedSelection;

    let query = FakeQueryApi::new(Ok(result));
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_query(
            &command,
            &context(),
            &query,
            &ProjectedQueryWriter,
            &mut stdout,
            &mut stderr
        ),
        0
    );
    let records = parse_csv(&stdout);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0][0], "schema_version");
    assert_eq!(records[1][1], "tools");

    let diagnostic: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(
        diagnostic["data"]["coverage"]["source_read_complete"],
        false
    );
    assert_eq!(
        diagnostic["data"]["universe"]["rows_complete_for_selection"],
        "subset"
    );
    assert_eq!(
        diagnostic["data"]["universe"]["partitions_complete"],
        "unknown"
    );
    let losses = diagnostic["data"]["output"]["losses"].as_array().unwrap();
    assert!(
        losses
            .iter()
            .any(|loss| loss == "absence_null_empty_collapse")
    );
    assert!(losses.iter().any(|loss| loss == "structured_as_json_text"));
    assert_eq!(diagnostic["data"]["output"]["csv_safety"], "raw");
    assert_eq!(
        diagnostic["data"]["output"]["formula_interpretation_risk"],
        true
    );
    assert_eq!(
        diagnostic["data"]["output"]["preserve_absent_null_empty_with"],
        serde_json::json!(["json", "jsonl"]),
    );
}

#[test]
fn pij_resolution_and_failures_use_only_the_diagnostic_channel() {
    let session_id = EntityId::derive(EntityKind::Session, [b"resolved-session".as_slice()]);
    let source_id = SourceId::derive([b"resolved-source".as_slice()]);
    let mut stderr = Vec::new();
    emit_pij_resolution(
        "pij-example-seat",
        "claude-code",
        session_id,
        source_id,
        OutputMode::JsonDiagnostic,
        &mut stderr,
    )
    .unwrap();
    let provenance: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(provenance["command"], "pij.resolve");
    assert_eq!(provenance["data"]["pij_id"], "pij-example-seat");
    assert_eq!(provenance["data"]["session_id"], session_id.to_string());
    assert_eq!(provenance["data"]["source_id"], source_id.to_string());
    assert!(provenance["data"].get("native_session_id").is_none());
    assert!(provenance["data"].get("path").is_none());

    stderr.clear();
    emit_pij_resolution(
        "pij\u{202e}seat",
        "claude\u{2066}code",
        session_id,
        source_id,
        OutputMode::Human,
        &mut stderr,
    )
    .unwrap();
    let human = String::from_utf8(stderr).unwrap();
    assert!(!human.contains('\u{202e}') && !human.contains('\u{2066}'));
    assert!(human.contains("\\u{202e}") && human.contains("\\u{2066}"));

    let mut failure = Vec::new();
    assert_eq!(
        emit_pij_failure(
            "UNI-PIJ-NOT-FOUND",
            "The Pij seat was not found in the selected instance.",
            "Check the seat ID or use explicit native selectors.",
            OutputMode::JsonDiagnostic,
            &mut failure,
        ),
        1
    );
    let failure: Value = serde_json::from_slice(&failure).unwrap();
    assert_eq!(failure["error"]["code"], "UNI-PIJ-NOT-FOUND");
    assert!(
        failure["next_action"]
            .as_str()
            .is_some_and(|action| !action.is_empty())
    );
}

#[test]
fn resolved_pij_continuation_uses_pinned_query_ids() {
    let ParsedCommand::PijQuery(mut pending) = parse(
        argv(&[
            "unisphere",
            "tools",
            "list",
            "--pij",
            "pij-example-seat",
            "--format",
            "json",
        ]),
        &context(),
    )
    .unwrap() else {
        panic!("pending Pij query");
    };
    let source = SourceId::derive([b"pij-source".as_slice()]);
    let session = EntityId::derive(EntityKind::Session, [b"pij-session".as_slice()]);
    pending.query.request.scope = QueryScope::Source {
        selector: SourceSelector::Id(source),
    };
    pending.query.request.filters.push(Filter {
        field: FieldId::SessionId,
        predicate: Predicate::In,
        values: vec![FieldValue::Id(session)],
        ignore_case: false,
    });
    let query = FakeQueryApi::new(Ok(response(
        Dataset::Tools,
        OperationKind::List,
        QueryAction::Continue {
            cursor: "q1:cursor:opaque".to_owned(),
            reason: ActionReason::MoreRows,
        },
    )));
    let mut stdout = Vec::new();
    assert_eq!(
        run_query(
            &pending.query,
            &context(),
            &query,
            &ProjectedQueryWriter,
            &mut stdout,
            &mut Vec::new(),
        ),
        0
    );
    let envelope: Value = serde_json::from_slice(&stdout).unwrap();
    let argv = envelope["next_action"]["argv"].as_array().unwrap();
    let source = source.to_string();
    let session = session.to_string();
    assert!(
        argv.iter()
            .any(|value| value.as_str() == Some(source.as_str()))
    );
    assert!(
        argv.iter()
            .any(|value| value.as_str() == Some(session.as_str()))
    );
    assert!(!argv.iter().any(|value| value == "--pij"));
    assert!(
        envelope["next_action"]["required_inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "original_query_options")
    );
}

#[test]
fn generated_actions_withhold_private_paths_and_parse_after_binding() {
    let command = query_command(&[
        "unisphere",
        "sessions",
        "list",
        "--repo",
        "/SENSITIVE-REPOSITORY",
        "--format",
        "json",
    ]);
    let query = FakeQueryApi::new(Ok(response(
        Dataset::Sessions,
        OperationKind::List,
        QueryAction::InspectCoverage {
            reason: ActionReason::EmptySelection,
        },
    )));
    let mut stdout = Vec::new();
    assert_eq!(
        run_query(
            &command,
            &context(),
            &query,
            &ProjectedQueryWriter,
            &mut stdout,
            &mut Vec::new()
        ),
        0
    );
    let text = String::from_utf8(stdout).unwrap();
    assert!(!text.contains("SENSITIVE-REPOSITORY"));
    let envelope: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        envelope["next_action"]["required_inputs"],
        serde_json::json!(["repository_path"])
    );
    let mut action = envelope["next_action"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| OsString::from(value.as_str().unwrap()))
        .collect::<Vec<_>>();
    action.push("/bound/repository".into());
    assert!(matches!(
        parse(action, &context()),
        Ok(ParsedCommand::Query(_))
    ));
}

#[test]
fn stdin_framing_is_explicit_scope_bound_and_preserved_in_actions() {
    let command = query_command(&[
        "unisphere",
        "sessions",
        "list",
        "--input",
        "-",
        "--stdin-format",
        "jsonl",
        "--format",
        "json",
    ]);
    assert_eq!(command.stdin_format, SavedFormat::QueryJsonlV1);
    assert!(
        parse(
            argv(&[
                "unisphere",
                "sessions",
                "list",
                "--input",
                "saved.json",
                "--stdin-format",
                "jsonl",
                "--format",
                "json",
            ]),
            &context(),
        )
        .is_err()
    );

    let query = FakeQueryApi::new(Ok(response(
        Dataset::Sessions,
        OperationKind::List,
        QueryAction::InspectCoverage {
            reason: ActionReason::EmptySelection,
        },
    )));
    let mut stdout = Vec::new();
    assert_eq!(
        run_query(
            &command,
            &context(),
            &query,
            &ProjectedQueryWriter,
            &mut stdout,
            &mut Vec::new(),
        ),
        0
    );
    let action = emitted_action(&stdout);
    assert!(action.iter().any(|value| value == "--stdin-format"));
    assert!(action.iter().any(|value| value == "jsonl"));
    assert!(parse(action, &context()).is_ok());
}

#[test]
fn typed_query_initialization_failures_keep_stdout_clean() {
    let command = query_command(&[
        "unisphere",
        "sessions",
        "list",
        "--repo",
        ".",
        "--format",
        "jsonl",
    ]);
    let failure = QueryFailure::new(
        QueryFailureCode::InvalidData,
        RecoveryAction::UseCompleteInput,
    );
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        emit_query_failure(&command, &failure, &mut stdout, &mut stderr),
        2
    );
    assert!(stdout.is_empty());
    let envelope: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(envelope["error"]["code"], "UNI-QUERY-DATA");
    assert!(
        envelope["next_action"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );

    stderr.clear();
    let failure = QueryFailure::new(
        QueryFailureCode::OutputFailure,
        RecoveryAction::ChooseNewOutput {
            discard_partial: true,
        },
    );
    assert_eq!(
        emit_query_failure(&command, &failure, &mut stdout, &mut stderr),
        1
    );
}

struct PartialFailureWriter;
impl QueryWriter for PartialFailureWriter {
    fn write(
        &self,
        _: &QueryResponse,
        _: &unisphere_core::query::QueryOutputOptions,
        destination: &mut dyn Write,
    ) -> Result<(), QueryFailure> {
        destination.write_all(b"partial").unwrap();
        Err(QueryFailure::new(
            QueryFailureCode::OutputFailure,
            RecoveryAction::ChooseNewOutput {
                discard_partial: true,
            },
        ))
    }
}

#[test]
fn failed_file_serialization_never_publishes_partial_destination() {
    let temporary = tempfile::tempdir().unwrap();
    let destination = temporary.path().join("result.json");
    let command = query_command(&[
        "unisphere",
        "sessions",
        "list",
        "--repo",
        ".",
        "--format",
        "json",
        "--output",
        destination.to_str().unwrap(),
    ]);
    let query = FakeQueryApi::new(Ok(response(
        Dataset::Sessions,
        OperationKind::List,
        QueryAction::ReadSchema {
            dataset: Dataset::Sessions,
            reason: ActionReason::EmptySelection,
        },
    )));
    assert_eq!(
        run_query(
            &command,
            &context(),
            &query,
            &PartialFailureWriter,
            &mut Vec::new(),
            &mut Vec::new()
        ),
        1
    );
    assert!(!destination.exists());
    assert!(temporary.path().read_dir().unwrap().next().is_none());
}

#[test]
fn existing_query_destination_fails_before_query_execution() {
    let temporary = tempfile::tempdir().unwrap();
    let destination = temporary.path().join("result.json");
    std::fs::write(&destination, b"existing").unwrap();
    let command = query_command(&[
        "unisphere",
        "sessions",
        "list",
        "--repo",
        ".",
        "--format",
        "json",
        "--output",
        destination.to_str().unwrap(),
    ]);
    let query = FakeQueryApi::new(Ok(response(
        Dataset::Sessions,
        OperationKind::List,
        QueryAction::ReadSchema {
            dataset: Dataset::Sessions,
            reason: ActionReason::EmptySelection,
        },
    )));
    let mut stderr = Vec::new();
    assert_eq!(
        run_query(
            &command,
            &context(),
            &query,
            &ProjectedQueryWriter,
            &mut Vec::new(),
            &mut stderr,
        ),
        1
    );
    assert!(query.requests().is_empty());
    assert_eq!(std::fs::read(destination).unwrap(), b"existing");
    assert_eq!(
        serde_json::from_slice::<Value>(&stderr).unwrap()["error"]["code"],
        "UNI-QUERY-OUTPUT"
    );
    let error: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(error["error"]["recovery"]["discard_partial"], false);
    assert_eq!(error["error"]["retryable"], false);
}
