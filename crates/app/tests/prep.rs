#![cfg(unix)]
//! Composition-root behaviour of `unisphere prep`: catalogue default roots.

use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn prep(home: &Path, target: &Path, extra: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_unisphere"))
        .args(["prep", "--target"])
        .arg(target)
        .args(extra)
        .arg("--json")
        .env_clear()
        .env("HOME", home)
        .env("PATH", "")
        .output()
        .unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (output.status.code().unwrap(), value)
}

fn sets(report: &Value) -> Vec<(String, bool, u64)> {
    report["data"]["sets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["harness"].as_str().unwrap().to_owned(),
                s["supported"].as_bool().unwrap(),
                s["discovered"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn every_existing_catalogue_root_is_prepped_or_reported_unsupported() {
    let home = tempfile::tempdir().unwrap();
    let claude = home.path().join(".claude/projects/-work-demo");
    fs::create_dir_all(&claude).unwrap();
    fs::write(
        claude.join("s.jsonl"),
        br#"{"type":"assistant","timestamp":"2026-09-26T00:00:01Z","requestId":"r1","message":{"id":"m1","model":"m","usage":{"input_tokens":1,"output_tokens":1}}}
"#,
    )
    .unwrap();
    // A unix-family hint (Oh My Pi) and an OS-specific one must both resolve.
    fs::create_dir_all(home.path().join(".omp/agent/sessions")).unwrap();
    let target = home.path().join("target");

    let (code, report) = prep(home.path(), &target, &[]);
    assert_eq!(code, 0, "{report}");
    let found = sets(&report);
    assert!(
        found.contains(&("claude-code".into(), true, 1)),
        "{found:?}"
    );
    assert!(found.contains(&("oh-my-pi".into(), false, 0)), "{found:?}");
    assert_eq!(
        found.len(),
        2,
        "absent catalogue roots are not invented: {found:?}"
    );

    let (code, report) = prep(home.path(), &target, &["--harness", "oh-my-pi"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(sets(&report), [("oh-my-pi".into(), false, 0)]);
}

#[test]
fn no_existing_or_explicit_root_is_a_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("target");
    let (code, _) = prep(home.path(), &target, &[]);
    assert_eq!(code, 2);
    assert!(!target.join("state.json").exists());
}
