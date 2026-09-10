#![cfg(unix)]
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::symlink,
    path::Path,
    process::{Command, Output},
};
use unisphere_testkit::git_notes::{git_ok, initialize, standard_git};
const NOTE: &[u8] = include_bytes!("../../adapter-git-ai/fixtures/mixed.notes");

fn run(root: &Path, repository: &Path, args: &[&str], with_git: bool) -> Output {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    if with_git && !bin.join("git").exists() {
        symlink(standard_git().unwrap(), bin.join("git")).unwrap();
    }
    Command::new(env!("CARGO_BIN_EXE_unisphere"))
        .args(args)
        .current_dir(repository)
        .env_clear()
        .env("HOME", root.join("empty-home"))
        .env("PATH", if with_git { bin } else { root.join("no-git") })
        .output()
        .unwrap()
}
fn json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn declared_attribution_session_is_queryable_without_inventing_a_transcript() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let commit = initialize(&repo, &standard_git().unwrap(), false, NOTE).unwrap();
    let sessions = run(
        root.path(),
        &repo,
        &[
            "sessions",
            "list",
            "--repo",
            ".",
            "--source-adapter",
            "git-ai",
            "--columns",
            "transcript_available,turn_count,message_count,tool_call_count",
            "--json",
        ],
        true,
    );
    let value = json(&sessions);
    assert_eq!(value["data"]["emitted"], 1);
    let row = &value["data"]["rows"][0];
    assert_eq!(row["fields"]["transcript_available"], false);
    assert!(!String::from_utf8_lossy(&sessions.stdout).contains("SENSITIVE"));
    let source = row["source_refs"][0]["source_id"].as_str().unwrap();
    for dataset in ["turns", "messages", "tools"] {
        let empty = run(
            root.path(),
            &repo,
            &[
                dataset,
                "list",
                "--source",
                source,
                "--source-adapter",
                "git-ai",
                "--json",
            ],
            true,
        );
        assert_eq!(json(&empty)["data"]["emitted"], 0);
    }
    let events = run(
        root.path(),
        &repo,
        &[
            "events",
            "list",
            "--source",
            source,
            "--source-adapter",
            "git-ai",
            "--limit",
            "0",
            "--json",
        ],
        true,
    );
    let events = json(&events);
    assert!(events["data"]["matched"].as_u64().unwrap() > 0);
    for event in events["data"]["rows"].as_array().unwrap() {
        assert!(event["fields"].get("timestamp").is_none());
        assert_eq!(event["source_refs"][0]["locator_kind"], "git_note");
    }
    let unchanged = git_ok(
        &standard_git().unwrap(),
        &repo,
        &["notes", "--ref=refs/notes/ai", "show", &commit],
        None,
    )
    .unwrap();
    assert_eq!(unchanged, NOTE);
}
#[test]
fn note_revision_invalidates_cursor_and_bad_note_is_not_empty_success() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let git = standard_git().unwrap();
    let commit = initialize(&repo, &git, false, NOTE).unwrap();
    let first = json(&run(
        root.path(),
        &repo,
        &[
            "events",
            "list",
            "--repo",
            ".",
            "--source-adapter",
            "git-ai",
            "--limit",
            "1",
            "--json",
        ],
        true,
    ));
    let cursor = first["data"]["next_cursor"].as_str().unwrap();
    let changed = String::from_utf8(NOTE.to_vec())
        .unwrap()
        .replace("synthetic-fixture", "changed-fixture");
    git_ok(
        &git,
        &repo,
        &[
            "notes",
            "--ref=refs/notes/ai",
            "add",
            "-f",
            "-F",
            "-",
            &commit,
        ],
        Some(changed.as_bytes()),
    )
    .unwrap();
    let stale = run(
        root.path(),
        &repo,
        &[
            "events",
            "list",
            "--repo",
            ".",
            "--source-adapter",
            "git-ai",
            "--limit",
            "1",
            "--cursor",
            cursor,
            "--json",
        ],
        true,
    );
    assert_eq!(stale.status.code(), Some(1));
    assert!(stale.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&stale.stderr).unwrap()["error"]["reason"],
        "source_view_changed"
    );
    git_ok(
        &git,
        &repo,
        &[
            "notes",
            "--ref=refs/notes/ai",
            "add",
            "-f",
            "-F",
            "-",
            &commit,
        ],
        Some(b"malformed-private-note"),
    )
    .unwrap();
    let failure = run(
        root.path(),
        &repo,
        &[
            "sessions",
            "list",
            "--repo",
            ".",
            "--source-adapter",
            "git-ai",
            "--json",
        ],
        true,
    );
    assert_eq!(failure.status.code(), Some(1));
    assert!(failure.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&failure.stderr).contains("malformed-private-note"));
    let partial = json(&run(
        root.path(),
        &repo,
        &[
            "sessions",
            "list",
            "--repo",
            ".",
            "--source-adapter",
            "git-ai",
            "--allow-partial",
            "--json",
        ],
        true,
    ));
    assert_eq!(partial["data"]["coverage"]["source_read_complete"], false);
}
#[test]
fn missing_git_is_actionable_and_empty_notes_are_a_complete_selection() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let git = standard_git().unwrap();
    initialize(&repo, &git, false, NOTE).unwrap();
    let missing = run(
        root.path(),
        &repo,
        &[
            "sessions",
            "list",
            "--repo",
            ".",
            "--source-adapter",
            "git-ai",
            "--json",
        ],
        false,
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&missing.stderr).unwrap()["error"]["recovery"]["reason"],
        "git_unavailable"
    );
    git_ok(&git, &repo, &["update-ref", "-d", "refs/notes/ai"], None).unwrap();
    let empty = json(&run(
        root.path(),
        &repo,
        &[
            "sessions",
            "list",
            "--repo",
            ".",
            "--source-adapter",
            "git-ai",
            "--json",
        ],
        true,
    ));
    assert_eq!(empty["data"]["emitted"], 0);
    assert_eq!(empty["data"]["coverage"]["source_read_complete"], true);
}
