#![cfg(unix)]

use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

fn fixture(root: &Path, native: &str) {
    let root = fs::canonicalize(root).unwrap();
    let directory = root.join(".omp/agent/sessions/project");
    fs::create_dir_all(&directory).unwrap();
    let records = [
        json!({"type":"session","version":3,"id":native,"cwd":root,"timestamp":"2026-09-01T12:00:00Z"}),
        json!({"type":"message","id":"request","parentId":null,"timestamp":"2026-09-01T12:00:01Z","message":{"role":"user","content":[{"type":"text","text":"PRIVATE-PIJ-PROMPT"}]}}),
        json!({"type":"message","id":"reply","parentId":"request","timestamp":"2026-09-01T12:00:02Z","message":{"role":"assistant","content":[{"type":"text","text":"PRIVATE-PIJ-ANSWER"}]}}),
    ];
    fs::write(
        directory.join(format!("{native}.jsonl")),
        records
            .iter()
            .map(|record| serde_json::to_string(record).unwrap() + "\n")
            .collect::<String>(),
    )
    .unwrap();
}

fn install_pij(root: &Path, response: Value, exit: u8) {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let path = bin.join("pij");
    // Log every invocation. Any lookup other than the one supported state read fails.
    fs::write(&path, format!("#!/bin/sh\nprintf '%s\\n' called >> '{}'/calls\n[ \"$#\" = 3 ] && [ \"$1\" = state ] && [ \"$2\" = pij-fixture-seat ] && [ \"$3\" = --json ] || exit 99\nprintf '%s\\n' '{}'\nexit {exit}\n", root.display(), response)).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_unisphere"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("PATH", root.join("bin"))
        .output()
        .unwrap()
}

fn mapping(native: Value, retired: bool) -> Value {
    json!({"v":2,"ok":true,"data":{"id":"pij-fixture-seat","harness":"omp","session":native,
        "tombstonedAt": if retired { json!("2026-09-01T00:00:00Z") } else { Value::Null }}})
}

#[test]
fn live_and_retired_aliases_reach_native_sdk_without_changing_identity() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), "native-one");
    let mut identity = None;
    for retired in [false, true] {
        install_pij(root.path(), mapping(json!("native-one"), retired), 0);
        let output = run(
            root.path(),
            &["sessions", "show", "--pij", "pij-fixture-seat", "--json"],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(envelope["data"]["emitted"], 1);
        let id = envelope["data"]["rows"][0]["id"].clone();
        if let Some(previous) = &identity {
            assert_eq!(previous, &id);
        }
        identity = Some(id);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-PIJ"));
        let provenance: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert!(provenance.to_string().contains("pij-fixture-seat"));
        assert!(!provenance.to_string().contains("native-one"));
    }
    assert_eq!(
        fs::read_to_string(root.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn changed_alias_is_resolved_per_query_and_missing_transcript_is_not_unknown_seat() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), "first");
    fixture(root.path(), "second");
    let mut ids = Vec::new();
    for native in ["first", "second"] {
        install_pij(root.path(), mapping(json!(native), false), 0);
        let output = run(
            root.path(),
            &["sessions", "show", "--pij", "pij-fixture-seat", "--json"],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        ids.push(envelope["data"]["rows"][0]["id"].clone());
    }
    assert_ne!(ids[0], ids[1]);
    install_pij(root.path(), mapping(json!("missing"), false), 0);
    let output = run(
        root.path(),
        &[
            "sessions",
            "show",
            "--pij",
            "pij-fixture-seat",
            "--format",
            "jsonl",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let failure: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(failure["error"]["code"], "UNI-PIJ-TRANSCRIPT-MISSING");
}

#[test]
fn optional_lookup_failure_categories_do_not_pollute_row_streams() {
    let root = tempfile::tempdir().unwrap();
    let cases = [
        (
            json!({"v":2,"ok":false,"error":"not_found"}),
            4,
            "UNI-PIJ-UNKNOWN",
        ),
        (mapping(Value::Null, false), 0, "UNI-PIJ-NATIVE-ID"),
        (
            json!({"v":2,"ok":false,"error":"PRIVATE-DAEMON-ERROR"}),
            1,
            "UNI-PIJ-UNAVAILABLE",
        ),
    ];
    for (response, exit, code) in cases {
        install_pij(root.path(), response, exit);
        let output = run(
            root.path(),
            &[
                "messages",
                "list",
                "--pij",
                "pij-fixture-seat",
                "--format",
                "jsonl",
            ],
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let failure: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(failure["error"]["code"], code);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-DAEMON-ERROR"));
    }
    fs::remove_file(root.path().join("bin/pij")).unwrap();
    let output = run(
        root.path(),
        &[
            "messages",
            "list",
            "--pij",
            "pij-fixture-seat",
            "--format",
            "jsonl",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "UNI-PIJ-MISSING"
    );
}

#[test]
fn explicit_native_operation_never_invokes_pij() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), "native-one");
    install_pij(
        root.path(),
        json!({"v":2,"ok":false,"error":"must_not_call"}),
        99,
    );
    let output = run(
        root.path(),
        &[
            "sessions",
            "list",
            "--repo",
            ".",
            "--harness",
            "oh-my-pi",
            "--json",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"]["emitted"],
        1
    );
    assert!(!root.path().join("calls").exists());
}

#[test]
fn continuation_uses_pinned_native_source_after_alias_changes() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), "first");
    fixture(root.path(), "second");
    install_pij(root.path(), mapping(json!("first"), false), 0);
    let first = run(
        root.path(),
        &[
            "messages",
            "list",
            "--pij",
            "pij-fixture-seat",
            "--limit",
            "1",
            "--json",
        ],
    );
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let envelope: Value = serde_json::from_slice(&first.stdout).unwrap();
    let first_id = envelope["data"]["rows"][0]["id"].clone();
    let session = envelope["data"]["rows"][0]["fields"]["session_id"]
        .as_str()
        .unwrap();
    let source = envelope["data"]["rows"][0]["source_refs"][0]["source_id"]
        .as_str()
        .unwrap();
    let cursor = envelope["data"]["next_cursor"]
        .as_str()
        .expect("second native message remains");
    let action = envelope["next_action"]["argv"].as_array().unwrap();
    assert!(action.iter().any(|arg| arg == "--source"));
    assert!(!action.iter().any(|arg| arg == "--pij"));
    install_pij(root.path(), mapping(json!("second"), false), 0);
    let continued = run(
        root.path(),
        &[
            "messages",
            "list",
            "--source",
            source,
            "--harness",
            "oh-my-pi",
            "--session",
            session,
            "--limit",
            "1",
            "--cursor",
            cursor,
            "--json",
        ],
    );
    assert!(
        continued.status.success(),
        "{}",
        String::from_utf8_lossy(&continued.stderr)
    );
    let next: Value = serde_json::from_slice(&continued.stdout).unwrap();
    assert_eq!(next["data"]["emitted"], 1);
    assert_ne!(next["data"]["rows"][0]["id"], first_id);
    assert_eq!(next["data"]["rows"][0]["fields"]["session_id"], session);
    assert_eq!(
        fs::read_to_string(root.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}
