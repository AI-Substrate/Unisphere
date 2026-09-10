#![cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use unisphere_core::{
    GitNoteLoader, GitNoteSelection, GitNotesError, GitNotesLimits, GitNotesScope,
};
use unisphere_loader_git::GitObjectLoader;
use unisphere_testkit::git_notes::{git_ok, initialize, standard_git};

fn scope(repo: &Path) -> GitNotesScope {
    GitNotesScope {
        repository: fs::canonicalize(repo).unwrap(),
        notes_ref: "refs/notes/ai".into(),
        selection: GitNoteSelection::All,
    }
}
fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap()
        .trim_end_matches('\n')
        .into()
}
fn tree_bytes(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(path: &Path, values: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, values);
            } else {
                values.push((path.clone(), fs::read(&path).unwrap()));
            }
        }
    }
    let mut values = Vec::new();
    visit(path, &mut values);
    values.sort();
    values
}
#[test]
fn pinned_reads_work_for_bare_and_linked_repositories_without_source_writes() {
    let root = tempfile::tempdir().unwrap();
    let git = standard_git().unwrap();
    let loader = GitObjectLoader::new(git.clone());
    for bare in [false, true] {
        let repo = root.path().join(if bare { "bare" } else { "work" });
        let commit = initialize(&repo, &git, bare, b"original note").unwrap();
        let selected = scope(&repo);
        let before = tree_bytes(&repo);
        let listing = loader
            .list_notes(&selected, GitNotesLimits::default())
            .unwrap();
        assert_eq!(listing.notes[0].target_commit, commit);
        assert_eq!(
            loader
                .read_note(&listing.notes[0], GitNotesLimits::default())
                .unwrap()
                .bytes,
            b"original note\n"
        );
        assert_eq!(tree_bytes(&repo), before);
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
            Some(b"changed note"),
        )
        .unwrap();
        assert_eq!(
            loader
                .read_note(&listing.notes[0], GitNotesLimits::default())
                .unwrap()
                .bytes,
            b"original note\n"
        );
        assert_ne!(
            loader
                .list_notes(&selected, GitNotesLimits::default())
                .unwrap()
                .notes_tip,
            listing.notes_tip
        );
        if !bare {
            let linked = root.path().join("linked");
            git_ok(
                &git,
                &repo,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    linked.to_str().unwrap(),
                    "HEAD",
                ],
                None,
            )
            .unwrap();
            fs::create_dir(linked.join("nested")).unwrap();
            let linked_listing = loader
                .list_notes(&scope(&linked.join("nested")), GitNotesLimits::default())
                .unwrap();
            assert_eq!(linked_listing.repository_id, listing.repository_id);
            assert_eq!(
                linked_listing.worktree_root,
                Some(fs::canonicalize(&linked).unwrap())
            );
            assert_eq!(
                loader
                    .read_note(&linked_listing.notes[0], GitNotesLimits::default())
                    .unwrap()
                    .bytes,
                b"changed note\n"
            );
        }
    }
}
#[test]
fn direct_fanout_selection_does_not_enumerate_unrelated_notes() {
    let root = tempfile::tempdir().unwrap();
    let git = standard_git().unwrap();
    let commit = initialize(root.path(), &git, true, b"selected").unwrap();
    let loader = GitObjectLoader::new(git.clone());
    let original = loader
        .list_notes(&scope(root.path()), GitNotesLimits::default())
        .unwrap();
    let note_blob = &original.notes[0].note_blob;
    let leaf = text(
        git_ok(
            &git,
            root.path(),
            &["mktree"],
            Some(format!("100644 blob {note_blob}\t{}\n", &commit[2..]).as_bytes()),
        )
        .unwrap(),
    );
    let tree = text(git_ok(&git, root.path(), &["mktree"], Some(format!("040000 tree {leaf}\t{}\n100644 blob {note_blob}\t{}\n100644 blob {note_blob}\t{}\n", &commit[..2], "1".repeat(40), "2".repeat(40)).as_bytes())).unwrap());
    let tip = text(
        git_ok(
            &git,
            root.path(),
            &["commit-tree", &tree],
            Some(b"fanout\n"),
        )
        .unwrap(),
    );
    git_ok(
        &git,
        root.path(),
        &["update-ref", "refs/notes/ai", &tip],
        None,
    )
    .unwrap();
    let limits = GitNotesLimits {
        max_listing_bytes: 180,
        ..GitNotesLimits::default()
    };
    assert_eq!(
        loader.list_notes(&scope(root.path()), limits),
        Err(GitNotesError::ListingLimit)
    );
    let mut selected = scope(root.path());
    selected.selection = GitNoteSelection::Commits(vec![commit.clone(), commit.clone()]);
    let listing = loader.list_notes(&selected, limits).unwrap();
    assert_eq!(
        listing
            .notes
            .iter()
            .map(|n| &n.target_commit)
            .collect::<Vec<_>>(),
        [&commit]
    );
    assert_eq!(
        loader.read_note(&listing.notes[0], limits).unwrap().bytes,
        b"selected\n"
    );
}
#[test]
fn absence_errors_and_hostile_repository_settings_are_distinct() {
    let root = tempfile::tempdir().unwrap();
    let git = standard_git().unwrap();
    let commit = initialize(root.path(), &git, true, b"oversized note").unwrap();
    let loader = GitObjectLoader::new(git.clone());
    let mut selected = scope(root.path());
    selected.notes_ref = "refs/notes/missing".into();
    let missing = loader
        .list_notes(&selected, GitNotesLimits::default())
        .unwrap();
    assert!(missing.notes_tip.is_none() && missing.notes.is_empty());
    selected.notes_ref = "refs/notes/ai".into();
    selected.selection = GitNoteSelection::Commits(vec![]);
    let empty = loader
        .list_notes(&selected, GitNotesLimits::default())
        .unwrap();
    assert!(empty.notes_tip.is_some() && empty.notes.is_empty());
    selected.selection = GitNoteSelection::Commits(vec![commit]);
    let listing = loader
        .list_notes(&selected, GitNotesLimits::default())
        .unwrap();
    assert_eq!(
        loader.read_note(
            &listing.notes[0],
            GitNotesLimits {
                max_note_bytes: 2,
                ..GitNotesLimits::default()
            }
        ),
        Err(GitNotesError::NoteLimit)
    );
    assert_eq!(
        GitObjectLoader::new(root.path().join("no-git"))
            .list_notes(&selected, GitNotesLimits::default()),
        Err(GitNotesError::GitUnavailable)
    );
    git_ok(
        &git,
        root.path(),
        &["config", "remote.fixture.promisor", "true"],
        None,
    )
    .unwrap();
    assert_eq!(
        loader.list_notes(&selected, GitNotesLimits::default()),
        Err(GitNotesError::UnsupportedRepository)
    );
}
#[test]
fn deadline_and_oversized_subprocess_output_are_not_success() {
    let root = tempfile::tempdir().unwrap();
    for (name, script, timeout_ms, expected) in [
        (
            "slow",
            "#!/bin/sh\n/bin/sleep 10\n",
            100,
            GitNotesError::Timeout,
        ),
        (
            "large",
            "#!/bin/sh\n/usr/bin/yes overflow\n",
            5000,
            GitNotesError::ListingLimit,
        ),
    ] {
        let executable = root.path().join(name);
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let result = GitObjectLoader::new(executable).list_notes(
            &scope(root.path()),
            GitNotesLimits {
                command_timeout_ms: timeout_ms,
                ..GitNotesLimits::default()
            },
        );
        assert_eq!(result, Err(expected));
    }
}
