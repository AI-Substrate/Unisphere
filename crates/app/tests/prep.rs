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
fn every_existing_catalogue_root_is_prepped_and_unbound_harnesses_are_reported() {
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
    assert!(found.contains(&("oh-my-pi".into(), true, 0)), "{found:?}");
    assert_eq!(
        found.len(),
        2,
        "absent catalogue roots are not invented: {found:?}"
    );

    let (code, report) = prep(home.path(), &target, &["--harness", "oh-my-pi"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(sets(&report), [("oh-my-pi".into(), true, 0)]);

    // A root for a harness prep has no fold for is reported, not dropped.
    let other = home.path().join("other");
    fs::create_dir_all(&other).unwrap();
    let spec = format!("git-ai={}", other.display());
    let (code, report) = prep(
        home.path(),
        &target,
        &["--no-default-roots", "--root", &spec],
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(sets(&report), [("git-ai".into(), false, 0)]);
}

#[test]
fn no_existing_or_explicit_root_is_a_usage_error() {
    let home = tempfile::tempdir().unwrap();
    let target = home.path().join("target");
    let (code, _) = prep(home.path(), &target, &[]);
    assert_eq!(code, 2);
    assert!(!target.join("state.json").exists());
}

/// Cursor IDE's SQLite key/value store end to end: prepped by revision, not
/// re-emitted when unchanged, a new generation when the database changes, and
/// every row's native key fetchable with explicit content opt-in.
#[test]
fn sqlite_snapshot_sources_are_prepped_by_revision_and_addressable() {
    let home = tempfile::tempdir().unwrap();
    let root = home.path().join("globalStorage");
    fs::create_dir_all(&root).unwrap();
    let db = root.join("state.vscdb");
    let fixture: Vec<Value> = serde_json::from_str(include_str!(
        "../../adapter-cursor/tests/fixtures/prep/ide.json"
    ))
    .unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE cursorDiskKV(key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);",
        )
        .unwrap();
    for row in &fixture {
        connection
            .execute(
                "INSERT INTO cursorDiskKV VALUES(?1, ?2)",
                (row["key"].as_str().unwrap(), row["value"].to_string()),
            )
            .unwrap();
    }
    let target = home.path().join("target");
    let spec = format!("cursor-ide:demo={}", root.display());
    let args = ["--no-default-roots", "--root", spec.as_str()];

    let (code, cold) = prep(home.path(), &target, &args);
    assert_eq!(code, 0, "{cold}");
    assert_eq!(cold["data"]["sets"][0]["by_status"]["new"], 1, "{cold}");
    assert!(
        cold["data"]["rows_written"]["calls"].as_u64() > Some(0),
        "{cold}"
    );

    let (_, unchanged) = prep(home.path(), &target, &args);
    assert_eq!(
        unchanged["data"]["sets"][0]["by_status"]["unchanged"], 1,
        "{unchanged}"
    );
    assert_eq!(unchanged["data"]["bytes_read"], 0, "{unchanged}");

    connection
        .execute(
            "INSERT INTO cursorDiskKV VALUES('agentKv:another', '{}')",
            [],
        )
        .unwrap();
    drop(connection);
    let (_, changed) = prep(home.path(), &target, &args);
    let source = &changed["data"]["sources"][0];
    assert_eq!(source["status"]["reason"], "revision", "{changed}");
    assert_eq!(source["generation"], 1, "{changed}");

    let key = fixture
        .iter()
        .map(|row| row["key"].as_str().unwrap())
        .find(|key| key.starts_with("bubbleId:"))
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_unisphere"))
        .args(["prep", "record", "--target"])
        .arg(&target)
        .args(["--source", "cursor-ide/demo/state.vscdb", "--key", key])
        .args(["--include-content", "--json"])
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: Value = serde_json::from_slice(&output.stdout).unwrap();
    let fetched: Value = serde_json::from_str(record["data"]["record"].as_str().unwrap()).unwrap();
    let expected = &fixture.iter().find(|row| row["key"] == key).unwrap()["value"];
    assert_eq!(&fetched, expected);
}
