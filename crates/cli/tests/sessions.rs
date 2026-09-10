use serde_json::Value;
use std::{ffi::OsString, io::Write};
use unisphere_cli::{
    CliContext, ParsedCommand, parse, run_native_export, run_native_list, run_sessions,
};
use unisphere_core::{
    CollectionBatch, MappedBatch, PipelineError, PipelineErrorKind, ReadCursor, SourceIdentity,
};
use unisphere_testkit::collection::FakeCollector;

fn context() -> CliContext {
    CliContext {
        cwd: std::env::temp_dir(),
        stdout_is_terminal: false,
        version: "test".into(),
    }
}
fn fake(context: &CliContext) -> FakeCollector {
    FakeCollector::new(
        Ok(Vec::new()),
        Ok(CollectionBatch {
            mapped: MappedBatch::default(),
            next_cursor: ReadCursor {
                source: context.cwd.join("session.jsonl"),
                identity: SourceIdentity::Unix {
                    device: 1,
                    inode: 2,
                },
                offset: 3,
            },
            more: false,
            incomplete_tail: false,
        }),
    )
    .with_output(b"{\"resourceLogs\":[]}\n".to_vec())
}
fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn parsed_native_commands_execute_without_second_parsing() {
    let context = context();
    let collector = fake(&context);
    let ParsedCommand::NativeRootList(list) = parse(
        args(&["unisphere", "sessions", "list", "--root", "."]),
        &context,
    )
    .unwrap() else {
        panic!("native list route");
    };
    let mut list_stdout = Vec::new();
    let mut list_stderr = Vec::new();
    assert_eq!(
        run_native_list(&list, &collector, &mut list_stdout, &mut list_stderr),
        0
    );
    let list_response: Value = serde_json::from_slice(&list_stdout).unwrap();
    let list_action = list_response["next_action"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| OsString::from(value.as_str().unwrap()))
        .collect::<Vec<_>>();
    assert!(matches!(
        parse(list_action, &context),
        Ok(ParsedCommand::Docs(_))
    ));

    let collector = fake(&context);
    let ParsedCommand::NativeExport(export) = parse(
        args(&[
            "unisphere",
            "sessions",
            "export",
            "--adapter",
            "claude-code",
            "--input",
            "session.jsonl",
        ]),
        &context,
    )
    .unwrap() else {
        panic!("native export route");
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_native_export(&export, &collector, &mut stdout, &mut stderr),
        0
    );
    assert_eq!(stdout, b"{\"resourceLogs\":[]}\n");
    let summary: Value = serde_json::from_slice(&stderr).unwrap();
    let action = summary["next_action"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| OsString::from(value.as_str().unwrap()))
        .collect::<Vec<_>>();
    assert!(matches!(
        parse(action, &context),
        Ok(ParsedCommand::Catalog(_))
    ));
}

#[test]
fn export_keeps_diagnostics_off_telemetry_stdout_and_passes_explicit_policy() {
    let context = context();
    let collector = fake(&context);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_sessions(
        args(&[
            "unisphere",
            "sessions",
            "export",
            "--input",
            "session.jsonl",
            "--include-content",
        ]),
        &context,
        &collector,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(exit, 0);
    assert_eq!(stdout, b"{\"resourceLogs\":[]}\n");
    let summary: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(summary["command"], "sessions.export");
    assert_eq!(summary["data"]["offset"], 3);
    assert!(collector.calls()[0].options.include_content);
    assert!(collector.calls()[0].batch.session.path.is_absolute());
}

#[test]
fn zero_limits_fail_before_file_creation_or_collector_calls() {
    let temporary = tempfile::tempdir().unwrap();
    let context = CliContext {
        cwd: temporary.path().into(),
        ..context()
    };
    let collector = fake(&context);
    let exit = run_sessions(
        args(&[
            "unisphere",
            "sessions",
            "export",
            "--input",
            "session.jsonl",
            "--output",
            "new.jsonl",
            "--max-records",
            "0",
        ]),
        &context,
        &collector,
        &mut Vec::new(),
        &mut Vec::new(),
    );
    assert_eq!(exit, 2);
    assert!(!temporary.path().join("new.jsonl").exists());
    assert!(collector.calls().is_empty());
}

#[test]
fn output_file_is_never_overwritten() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::write(temporary.path().join("keep.jsonl"), b"existing output").unwrap();
    let context = CliContext {
        cwd: temporary.path().into(),
        ..context()
    };
    let collector = fake(&context);
    let exit = run_sessions(
        args(&[
            "unisphere",
            "sessions",
            "export",
            "--input",
            "session.jsonl",
            "--output",
            "keep.jsonl",
        ]),
        &context,
        &collector,
        &mut Vec::new(),
        &mut Vec::new(),
    );
    assert_eq!(exit, 1);
    assert_eq!(
        std::fs::read(temporary.path().join("keep.jsonl")).unwrap(),
        b"existing output"
    );
    assert!(collector.calls().is_empty());
}

#[test]
fn empty_listing_says_nonrecursive_and_names_leaf_directory_remedy() {
    let context = context();
    let collector = fake(&context);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let exit = run_sessions(
        args(&["unisphere", "sessions", "list", "--root", "."]),
        &context,
        &collector,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(exit, 0);
    let response: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(response["data"]["recursive"], false);
    assert_eq!(response["data"]["sessions"], serde_json::json!([]));
    assert_eq!(
        serde_json::from_slice::<Value>(&stderr).unwrap()["command"],
        "sessions.list"
    );
}

#[test]
fn loader_failure_is_safe_and_not_a_telemetry_record() {
    let collector = FakeCollector::new(
        Err(PipelineError::new(PipelineErrorKind::Read, None)),
        Err(PipelineError::new(PipelineErrorKind::Read, None)),
    );
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_sessions(
            args(&[
                "unisphere",
                "sessions",
                "export",
                "--input",
                "session.jsonl"
            ]),
            &context(),
            &collector,
            &mut stdout,
            &mut stderr
        ),
        1
    );
    assert!(stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&stderr).unwrap()["error"]["code"],
        "UNI-READ"
    );
}

#[test]
fn rejected_destination_does_not_report_success() {
    struct Reject;
    impl Write for Reject {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let context = context();
    let collector = fake(&context);
    let mut stderr = Vec::new();
    assert_eq!(
        run_sessions(
            args(&[
                "unisphere",
                "sessions",
                "export",
                "--input",
                "session.jsonl"
            ]),
            &context,
            &collector,
            &mut Reject,
            &mut stderr
        ),
        1
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&stderr).unwrap()["error"]["code"],
        "UNI-WRITE"
    );
}

#[test]
fn invalid_invocation_does_not_echo_sensitive_arguments() {
    let context = context();
    let collector = fake(&context);
    let mut stderr = Vec::new();
    assert_eq!(
        run_sessions(
            args(&[
                "unisphere",
                "sessions",
                "export",
                "--unknown=SENSITIVE-MARKER"
            ]),
            &context,
            &collector,
            &mut Vec::new(),
            &mut stderr
        ),
        2
    );
    assert!(
        !String::from_utf8(stderr)
            .unwrap()
            .contains("SENSITIVE-MARKER")
    );
    assert!(collector.calls().is_empty());
}
