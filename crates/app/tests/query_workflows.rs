#![cfg(unix)]

use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn fixture(root: &Path) {
    let root = fs::canonicalize(root).unwrap();
    let directory = root.join(".claude/projects/fixture");
    fs::create_dir_all(&directory).unwrap();
    let records = [
        json!({"type":"user","sessionId":"fixture-session","uuid":"u","cwd":root,"timestamp":"2026-09-01T12:00:00Z","message":{"role":"user","content":"PRIVATE-PROMPT"}}),
        json!({"type":"assistant","sessionId":"fixture-session","uuid":"a","parentUuid":"u","cwd":root,"timestamp":"2026-09-01T12:00:01Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call","name":"Bash","input":{"command":"PRIVATE-COMMAND"}}]}}),
        json!({"type":"user","sessionId":"fixture-session","uuid":"r","parentUuid":"a","cwd":root,"timestamp":"2026-09-01T12:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call","content":"PRIVATE-RESULT","is_error":false}]}}),
    ];
    fs::write(
        directory.join("session.jsonl"),
        records
            .iter()
            .map(|record| serde_json::to_string(record).unwrap() + "\n")
            .collect::<String>(),
    )
    .unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_unisphere"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("PATH", "")
        .output()
        .unwrap()
}

#[test]
fn finite_duration_thresholds_compare_to_native_observed_milliseconds() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    for (threshold, expected) in [("500.5", 1), ("1000", 1), ("1000.1", 0)] {
        let output = run(
            root.path(),
            &[
                "tools",
                "list",
                "--repo",
                ".",
                "--harness",
                "claude-code",
                "--min-duration",
                threshold,
                "--json",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["data"]["matched"], expected);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-"));
    }
}

#[test]
fn partial_csv_reports_coverage_and_format_losses_outside_row_stream() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    fs::write(
        root.path()
            .join(".claude/projects/fixture/incomplete.jsonl"),
        b"{\"type\":",
    )
    .unwrap();
    let output = run(
        root.path(),
        &[
            "tools",
            "list",
            "--repo",
            ".",
            "--harness",
            "claude-code",
            "--allow-partial",
            "--format",
            "csv",
            "--csv-safety",
            "raw",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("schema_version,dataset,id,source_refs"));
    assert!(!stdout.contains("next_action") && !stdout.contains("PRIVATE-"));
    let summary: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(summary["data"]["emitted"], 1);
    assert_eq!(summary["data"]["coverage"]["source_read_complete"], false);
    assert!(summary["data"]["universe"].is_object());
    assert!(summary.to_string().contains("absence_null_empty_collapse"));
    assert!(summary.to_string().contains("raw"));
}
