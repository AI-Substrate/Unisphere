use serde_json::Value;
use std::{
    ffi::OsString,
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use unisphere_cli::{CliContext, ParsedCommand, parse, run_git_notes, run_native_git_notes};
use unisphere_core::{
    GitNoteRef, GitNoteSelection, GitNotesApi, GitNotesCollection, GitNotesError, GitNotesLimits,
    GitNotesListing, GitNotesRequest, GitNotesScope, PipelineError, PipelineErrorKind,
};

#[derive(Clone)]
struct FakeGitNotes {
    listing: Result<GitNotesListing, GitNotesError>,
    collection: Result<GitNotesCollection, GitNotesError>,
    output: Vec<u8>,
    list_calls: Arc<AtomicUsize>,
    collect_calls: Arc<AtomicUsize>,
}

impl GitNotesApi for FakeGitNotes {
    fn list_notes(
        &self,
        _scope: &GitNotesScope,
        _limits: GitNotesLimits,
    ) -> Result<GitNotesListing, GitNotesError> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        self.listing.clone()
    }

    fn collect_notes(
        &self,
        _request: &GitNotesRequest,
        destination: &mut dyn Write,
    ) -> Result<GitNotesCollection, GitNotesError> {
        self.collect_calls.fetch_add(1, Ordering::SeqCst);
        destination
            .write_all(&self.output)
            .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
        self.collection.clone()
    }
}

fn context(cwd: &Path) -> CliContext {
    CliContext {
        cwd: cwd.to_owned(),
        stdout_is_terminal: false,
        version: "test".into(),
    }
}

fn argv(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn listing(repository: &Path, with_note: bool) -> GitNotesListing {
    let canonical = std::fs::canonicalize(repository).unwrap_or_else(|_| repository.to_owned());
    let repository = canonical.as_path();
    let notes_tip = "a".repeat(40);
    let notes = if with_note {
        vec![GitNoteRef {
            repository: repository.to_owned(),
            repository_id: repository.to_owned(),
            notes_ref: "refs/notes/ai".into(),
            notes_tip: notes_tip.clone(),
            target_commit: "b".repeat(40),
            note_blob: "c".repeat(40),
        }]
    } else {
        Vec::new()
    };
    GitNotesListing {
        repository: repository.to_owned(),
        repository_id: repository.to_owned(),
        git_dir: repository.join(".git"),
        worktree_root: Some(repository.to_owned()),
        notes_ref: "refs/notes/ai".into(),
        notes_tip: with_note.then_some(notes_tip),
        selection: GitNoteSelection::All,
        notes,
    }
}

fn fake(repository: &Path, with_note: bool) -> FakeGitNotes {
    let listing = listing(repository, with_note);
    FakeGitNotes {
        collection: Ok(GitNotesCollection {
            listing: listing.clone(),
            records_written: if with_note { 1 } else { 0 },
        }),
        listing: Ok(listing),
        output: b"{\"resourceLogs\":[]}\n".to_vec(),
        list_calls: Arc::new(AtomicUsize::new(0)),
        collect_calls: Arc::new(AtomicUsize::new(0)),
    }
}

fn parse_export(cwd: &Path, output: Option<&Path>) -> ParsedCommand {
    let mut args = argv(&[
        "unisphere",
        "sessions",
        "export",
        "--adapter",
        "git-ai",
        "--repo",
    ]);
    args.push(cwd.as_os_str().to_owned());
    if let Some(output) = output {
        args.push("--output".into());
        args.push(output.as_os_str().to_owned());
    }
    parse(args, &context(cwd)).unwrap()
}

#[test]
fn typed_runner_lists_with_actionable_empty_and_nonempty_outcomes() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repo");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    let command = parse(
        {
            let mut args = argv(&[
                "unisphere",
                "sessions",
                "list",
                "--adapter",
                "git-ai",
                "--repo",
            ]);
            args.push(repository.as_os_str().to_owned());
            args
        },
        &context(temporary.path()),
    )
    .unwrap();

    let empty = fake(&repository, false);
    let mut empty_stdout = Vec::new();
    assert_eq!(
        run_native_git_notes(&command, |_| Ok(empty), &mut empty_stdout, &mut Vec::new(),),
        0
    );
    let empty_response: Value = serde_json::from_slice(&empty_stdout).unwrap();
    assert_eq!(
        empty_response["data"]["listing"]["notes"],
        Value::Array(Vec::new())
    );
    assert!(
        !empty_response["next_action"]["summary"]
            .as_str()
            .unwrap()
            .is_empty()
    );

    let nonempty = fake(&repository, true);
    let mut nonempty_stdout = Vec::new();
    assert_eq!(
        run_native_git_notes(
            &command,
            |_| Ok(nonempty),
            &mut nonempty_stdout,
            &mut Vec::new(),
        ),
        0
    );
    let nonempty_response: Value = serde_json::from_slice(&nonempty_stdout).unwrap();
    assert_eq!(nonempty_response["next_action"]["argv"][2], "export");
    assert_ne!(
        empty_response["next_action"]["summary"],
        nonempty_response["next_action"]["summary"]
    );
}

#[test]
fn compatibility_wrapper_uses_root_help_without_constructing_port() {
    let temporary = tempfile::tempdir().unwrap();
    let constructions = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&constructions);
    let mut stdout = Vec::new();
    assert_eq!(
        run_git_notes::<FakeGitNotes>(
            argv(&["unisphere", "sessions", "list", "--help"]),
            &context(temporary.path()),
            move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                Err(GitNotesError::GitUnavailable)
            },
            &mut stdout,
            &mut Vec::new(),
        ),
        0
    );
    assert_eq!(constructions.load(Ordering::SeqCst), 0);
    let response: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(response["command"], "help");
}

#[test]
fn compatibility_wrapper_preserves_native_invalid_input_envelope() {
    let temporary = tempfile::tempdir().unwrap();
    let constructions = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&constructions);
    let mut stderr = Vec::new();
    assert_eq!(
        run_git_notes::<FakeGitNotes>(
            argv(&["unisphere", "sessions", "list", "--adapter", "other"]),
            &context(temporary.path()),
            move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                Err(GitNotesError::GitUnavailable)
            },
            &mut Vec::new(),
            &mut stderr,
        ),
        2
    );
    assert_eq!(constructions.load(Ordering::SeqCst), 0);
    let response: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(response["error"]["kind"], "invalid_input");
    assert_eq!(response["error"]["output_code"], Value::Null);
}

#[test]
fn invalid_typed_variant_fails_before_constructing_port() {
    let temporary = tempfile::tempdir().unwrap();
    let command = parse(
        argv(&["unisphere", "adapters", "list"]),
        &context(temporary.path()),
    )
    .unwrap();
    let constructions = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&constructions);
    let mut stderr = Vec::new();
    assert_eq!(
        run_native_git_notes::<FakeGitNotes>(
            &command,
            move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                Err(GitNotesError::GitUnavailable)
            },
            &mut Vec::new(),
            &mut stderr,
        ),
        2
    );
    assert_eq!(constructions.load(Ordering::SeqCst), 0);
    let response: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(response["error"]["kind"], "invalid_input");
    assert!(
        !response["next_action"]["summary"]
            .as_str()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn export_keeps_otlp_stdout_data_only_and_reports_guidance_on_stderr() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repo");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    let command = parse_export(&repository, None);
    let port = fake(&repository, true);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_native_git_notes(&command, |_| Ok(port), &mut stdout, &mut stderr),
        0
    );
    assert_eq!(stdout, b"{\"resourceLogs\":[]}\n");
    let response: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(response["command"], "sessions.export");
    assert_eq!(response["data"]["semantics"], "replace_projection");
    assert!(
        !response["next_action"]["summary"]
            .as_str()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn export_refuses_source_paths_and_never_collects_into_them() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repo");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    let target = repository.join("output.otlp.jsonl");
    let command = parse_export(&repository, Some(&target));
    let port = fake(&repository, true);
    let collect_calls = Arc::clone(&port.collect_calls);
    let mut stderr = Vec::new();
    assert_eq!(
        run_native_git_notes(&command, |_| Ok(port), &mut Vec::new(), &mut stderr),
        2
    );
    assert_eq!(collect_calls.load(Ordering::SeqCst), 0);
    assert!(!target.exists());
    let response: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(response["error"]["kind"], "invalid_input");
}

#[test]
fn successful_file_export_publishes_new_private_output() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repo");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    let target = temporary.path().join("output.otlp.jsonl");
    let command = parse_export(&repository, Some(&target));
    let port = fake(&repository, true);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_native_git_notes(&command, |_| Ok(port), &mut stdout, &mut stderr),
        0
    );
    assert!(stdout.is_empty());
    assert_eq!(std::fs::read(&target).unwrap(), b"{\"resourceLogs\":[]}\n");
    let response: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(response["command"], "sessions.export");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[test]
fn failed_export_removes_stage_and_never_publishes_partial_bytes() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repo");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    let target = temporary.path().join("output.otlp.jsonl");
    let command = parse_export(&repository, Some(&target));
    let mut port = fake(&repository, true);
    port.output = b"partial".to_vec();
    port.collection = Err(GitNotesError::ObjectRead);
    let mut stderr = Vec::new();
    assert_eq!(
        run_native_git_notes(&command, |_| Ok(port), &mut Vec::new(), &mut stderr),
        1
    );
    assert!(!target.exists());
    assert!(std::fs::read_dir(temporary.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".unisphere-output-")
    }));
    let response: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(response["error"]["kind"], "object_read");
    assert!(
        !response["next_action"]["summary"]
            .as_str()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn existing_export_is_not_overwritten_and_preserves_output_error_code() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repo");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    let target = temporary.path().join("output.otlp.jsonl");
    std::fs::write(&target, b"keep").unwrap();
    let command = parse_export(&repository, Some(&target));
    let port = fake(&repository, true);
    let collect_calls = Arc::clone(&port.collect_calls);
    let mut stderr = Vec::new();
    assert_eq!(
        run_native_git_notes(&command, |_| Ok(port), &mut Vec::new(), &mut stderr),
        1
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
    assert_eq!(collect_calls.load(Ordering::SeqCst), 0);
    let response: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(response["error"]["kind"], "output");
    assert_eq!(response["error"]["output_code"], "UNI-WRITE");
}

#[test]
fn constructor_failures_keep_kind_and_cause_specific_recovery() {
    let temporary = tempfile::tempdir().unwrap();
    let repository = temporary.path().join("repo");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    let command = parse_export(&repository, None);
    let mut stderr = Vec::new();
    assert_eq!(
        run_native_git_notes::<FakeGitNotes>(
            &command,
            |_| Err(GitNotesError::GitUnavailable),
            &mut Vec::new(),
            &mut stderr,
        ),
        1
    );
    let response: Value = serde_json::from_slice(&stderr).unwrap();
    assert_eq!(response["error"]["kind"], "git_unavailable");
    assert_eq!(response["error"]["output_code"], Value::Null);
    assert!(
        response["next_action"]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .any(|part| part == "--git-executable")
    );
}
