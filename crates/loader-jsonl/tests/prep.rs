//! `FileSessionLoader` as a `PrepLoader` on real temporary files.
#![cfg(unix)]

use std::{
    cell::RefCell,
    fs::{self, File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    sync::mpsc,
    thread,
    time::Duration,
};

use tempfile::tempdir;
use unisphere_core::{
    PipelineErrorKind, ReadLimits, SnapshotLimits,
    prep::{NativeAddress, PrepDirIndex, PrepInput, PrepLoader, PrepReadLimits, PrepSourceKind},
};
use unisphere_loader_jsonl::FileSessionLoader;

fn limits(records: usize, batch_bytes: usize) -> PrepReadLimits {
    PrepReadLimits {
        read: ReadLimits {
            max_records: records,
            max_record_bytes: batch_bytes,
            max_batch_bytes: batch_bytes,
        },
        snapshot: SnapshotLimits::default(),
    }
}

fn at(offset: u64) -> NativeAddress {
    NativeAddress {
        offset: Some(offset),
        key: None,
    }
}

fn texts(input: &PrepInput) -> Vec<String> {
    match input {
        PrepInput::Records(records) => records
            .iter()
            .map(|r| String::from_utf8(r.bytes.clone()).unwrap())
            .collect(),
        PrepInput::Snapshot(_) => panic!("append loader returned a snapshot"),
    }
}

#[test]
fn discovery_honours_accept_and_counts_every_skipped_entry() {
    let root = tempdir().unwrap();
    let base = root.path();
    fs::create_dir_all(base.join("p/sub")).unwrap();
    fs::create_dir_all(base.join(".hidden-dir")).unwrap();
    fs::write(base.join("p/b.jsonl"), b"{}\n").unwrap();
    fs::write(base.join("p/sub/a.jsonl"), b"{}\n").unwrap();
    fs::write(base.join("p/notes.txt"), b"x").unwrap();
    fs::write(base.join("p/.dot.jsonl"), b"{}\n").unwrap();
    fs::write(base.join(".hidden-dir/c.jsonl"), b"{}\n").unwrap();
    symlink(base.join("p/b.jsonl"), base.join("p/link.jsonl")).unwrap();
    symlink(base.join("p/sub"), base.join("linked-dir")).unwrap();
    let locked = base.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("d.jsonl"), b"{}\n").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let unreadable_dir_counts = fs::read_dir(&locked).is_err(); // false when running as root

    let offered = RefCell::new(Vec::new());
    let accept = |file: &str| {
        offered.borrow_mut().push(file.to_owned());
        file.ends_with(".jsonl")
    };
    let discovery = FileSessionLoader.discover(base, &accept, &PrepDirIndex::new());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let discovery = discovery.unwrap();

    let mut files: Vec<&str> = discovery.sources.iter().map(|s| s.file.as_str()).collect();
    if !unreadable_dir_counts {
        files.retain(|f| *f != "locked/d.jsonl");
    }
    assert_eq!(
        files,
        ["p/b.jsonl", "p/sub/a.jsonl"],
        "sorted relative paths"
    );
    assert_eq!(discovery.skipped.symlinks, 2);
    assert_eq!(
        discovery.skipped.hidden, 2,
        "a hidden file and a hidden directory"
    );
    assert_eq!(
        discovery.skipped.unreadable_entries,
        u64::from(unreadable_dir_counts)
    );
    let mut offered = offered.into_inner();
    offered.sort();
    assert!(
        offered.contains(&"p/notes.txt".to_owned()),
        "accept sees relative paths"
    );
    assert!(
        !offered
            .iter()
            .any(|f| f.contains("link") || f.starts_with('.'))
    );

    let stat = &discovery.sources[0];
    assert_eq!(stat.kind, PrepSourceKind::Append);
    assert_eq!(stat.path, base.join("p/b.jsonl"));
    assert_eq!(stat.size, 3);
    assert_eq!(
        FileSessionLoader
            .stat(base, &base.join("p/b.jsonl"))
            .unwrap(),
        *stat
    );
}

#[test]
fn discovery_and_stat_refuse_invalid_roots_and_paths() {
    let root = tempdir().unwrap();
    let base = root.path();
    fs::create_dir(base.join("real")).unwrap();
    fs::write(base.join("real/a.jsonl"), b"{}\n").unwrap();
    symlink(base.join("real"), base.join("alias")).unwrap();
    symlink(base.join("real/a.jsonl"), base.join("real/link.jsonl")).unwrap();
    let accept = |_: &str| true;
    for bad in [Path::new("relative"), &base.join("alias")] {
        let error = FileSessionLoader
            .discover(bad, &accept, &PrepDirIndex::new())
            .unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidInput, "{bad:?}");
    }
    let real = base.join("real");
    let other = tempdir().unwrap();
    for bad in [real.join("link.jsonl"), other.path().join("a.jsonl")] {
        let error = FileSessionLoader.stat(&real, &bad).unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidInput, "{bad:?}");
    }
}

#[test]
fn reads_end_at_the_last_complete_lf_and_resume_from_the_cursor() {
    let root = tempdir().unwrap();
    let path = root.path().join("a.jsonl");
    fs::write(&path, b"one\ntwo\n\nthree\npart").unwrap();
    let stat = FileSessionLoader.stat(root.path(), &path).unwrap();

    let first = FileSessionLoader.read(&stat, None, limits(2, 64)).unwrap();
    assert_eq!(texts(&first.input), ["one", "two"]);
    assert!(first.more);
    let cursor = first.next_cursor.unwrap();
    assert_eq!((cursor.offset, first.bytes_read), (8, 8));

    let second = FileSessionLoader
        .read(&stat, Some(&cursor), limits(10, 64))
        .unwrap();
    assert_eq!(
        texts(&second.input),
        ["three"],
        "blank lines are framing only"
    );
    assert!(!second.more && second.incomplete_tail);
    let cursor = second.next_cursor.unwrap();
    assert_eq!(cursor.offset, 15, "stops after the last complete LF");
    assert_eq!(second.bytes_read, 7);

    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"ial\n")
        .unwrap();
    let stat = FileSessionLoader.stat(root.path(), &path).unwrap();
    let third = FileSessionLoader
        .read(&stat, Some(&cursor), limits(10, 64))
        .unwrap();
    assert_eq!(texts(&third.input), ["partial"]);
    assert!(!third.incomplete_tail);
    match &third.input {
        PrepInput::Records(records) => assert_eq!(records[0].offset, 15),
        PrepInput::Snapshot(_) => unreachable!(),
    }
}

#[test]
fn native_files_are_opened_read_only_without_locks() {
    let root = tempdir().unwrap();
    let path = root.path().join("a.jsonl");
    fs::write(&path, b"one\ntwo\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let before = fs::metadata(&path).unwrap();
    // An exclusive advisory lock held by a writer must never block the loader.
    let holder = File::open(&path).unwrap();
    holder.lock().unwrap();

    let (done, finished) = mpsc::channel();
    let base = root.path().to_path_buf();
    let target = path.clone();
    thread::spawn(move || {
        let loader = FileSessionLoader;
        let discovery = loader
            .discover(&base, &|_| true, &PrepDirIndex::new())
            .unwrap();
        let stat = &discovery.sources[0];
        let batch = loader.read(stat, None, limits(10, 64)).unwrap();
        let anchor = loader.anchor(stat, 8).unwrap();
        let record = loader.record_at(&target, &at(4), 64).unwrap();
        done.send((texts(&batch.input), anchor, record)).unwrap();
    });
    let (records, anchor, record) = finished
        .recv_timeout(Duration::from_secs(10))
        .expect("loader blocked on a lock");
    assert_eq!(records, ["one", "two"]);
    assert!(anchor.starts_with("sha256:"));
    assert_eq!(record, b"two");
    holder.unlock().unwrap();

    let after = fs::metadata(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"one\ntwo\n");
    assert_eq!(after.modified().unwrap(), before.modified().unwrap());
    assert_eq!(after.permissions().mode() & 0o777, 0o444);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn anchor_detects_a_same_inode_rewrite_of_the_committed_prefix() {
    let root = tempdir().unwrap();
    let path = root.path().join("a.jsonl");
    // Long enough that the head and tail windows differ.
    let line = |c: char| format!("{}\n", c.to_string().repeat(3000));
    fs::write(&path, format!("{}{}{}", line('a'), line('b'), line('c'))).unwrap();
    let committed = 6002;
    let stat = FileSessionLoader.stat(root.path(), &path).unwrap();
    let anchor = FileSessionLoader.anchor(&stat, committed).unwrap();

    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"more\n")
        .unwrap();
    let grown = FileSessionLoader.stat(root.path(), &path).unwrap();
    assert_eq!(
        FileSessionLoader.anchor(&grown, committed).unwrap(),
        anchor,
        "appends leave the committed prefix intact"
    );

    for (offset, byte) in [(10, b'X'), (5000, b'Y')] {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.write_all(&[byte]).unwrap();
        drop(file);
        let rewritten = FileSessionLoader.stat(root.path(), &path).unwrap();
        assert_eq!(rewritten.identity, stat.identity, "same inode");
        assert_ne!(
            FileSessionLoader.anchor(&rewritten, committed).unwrap(),
            anchor,
            "rewrite at {offset}"
        );
    }

    fs::write(&path, b"short\n").unwrap();
    let truncated = FileSessionLoader.stat(root.path(), &path).unwrap();
    let error = FileSessionLoader.anchor(&truncated, committed).unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::SourceChanged);
}

#[test]
fn record_at_returns_one_complete_line_from_a_line_start_only() {
    let root = tempdir().unwrap();
    let path = root.path().join("a.jsonl");
    fs::write(&path, b"first\nsecond-record\npartial").unwrap();
    let loader = FileSessionLoader;
    assert_eq!(loader.record_at(&path, &at(0), 64).unwrap(), b"first");
    assert_eq!(
        loader.record_at(&path, &at(6), 64).unwrap(),
        b"second-record"
    );
    assert_eq!(
        loader.record_at(&path, &at(6), 13).unwrap(),
        b"second-record",
        "exactly max_bytes"
    );

    let refused = [
        (at(3), 64, PipelineErrorKind::InvalidInput, "mid-line"),
        (
            at(20),
            64,
            PipelineErrorKind::InvalidInput,
            "incomplete tail",
        ),
        (
            at(6),
            12,
            PipelineErrorKind::RecordLimit,
            "longer than max_bytes",
        ),
        (
            NativeAddress {
                offset: None,
                key: Some("k".into()),
            },
            64,
            PipelineErrorKind::InvalidInput,
            "snapshot key on an append source",
        ),
    ];
    for (address, max, kind, why) in refused {
        let error = loader.record_at(&path, &address, max).unwrap_err();
        assert_eq!(error.kind(), kind, "{why}");
    }

    let link = root.path().join("link.jsonl");
    symlink(&path, &link).unwrap();
    let error = loader.record_at(&link, &at(0), 64).unwrap_err();
    assert_eq!(
        error.kind(),
        PipelineErrorKind::Read,
        "symlinks are never followed"
    );
}

/// Set a directory's mtime to `seconds` after the epoch: far past the racy window.
fn age(dir: &Path, seconds: u64) {
    let at = std::time::UNIX_EPOCH + Duration::from_secs(seconds);
    File::open(dir).unwrap().set_modified(at).unwrap();
}

/// [`age`] every directory under (and including) `root`.
fn age_dirs(root: &Path, seconds: u64) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            }
        }
        age(&dir, seconds);
    }
}

#[test]
fn incremental_discovery_relists_only_changed_directories_and_matches_a_full_walk() {
    let root = tempdir().unwrap();
    let base = root.path();
    for dir in [
        "p1/s1/sub",
        "p2/s2/sub/deep",
        "p3/.cache",
        "p3/unrelated/x/y",
    ] {
        fs::create_dir_all(base.join(dir)).unwrap();
    }
    fs::write(base.join("p1/s1.jsonl"), b"{}\n").unwrap();
    fs::write(base.join("p2/s2/sub/deep/agent.jsonl"), b"{}\n").unwrap();
    fs::write(base.join("p3/unrelated/x/y/notes.txt"), b"x").unwrap();
    fs::write(base.join("p3/.hidden.jsonl"), b"{}\n").unwrap();
    symlink(base.join("p1/s1.jsonl"), base.join("p1/link.jsonl")).unwrap();
    age_dirs(base, 1_000_000_000);
    let accept = |file: &str| file.ends_with(".jsonl");
    let full = |loader: &FileSessionLoader| loader.discover(base, &accept, &PrepDirIndex::new());
    let files = |found: &unisphere_core::prep::PrepDiscovery| -> Vec<(String, u64)> {
        found
            .sources
            .iter()
            .map(|s| (s.file.clone(), s.size))
            .collect()
    };

    let first = full(&FileSessionLoader).unwrap();
    // root, p1, p1/s1, p1/s1/sub, p2, p2/s2, p2/s2/sub, deep, p3, unrelated, x, y
    assert_eq!((first.dirs_listed, first.dirs_reused), (12, 0));
    assert_eq!(first.index.len(), 12, "aged directories are indexed");

    // Unchanged tree: every directory is reused after one stat, nothing listed.
    let unchanged = FileSessionLoader
        .discover(base, &accept, &first.index)
        .unwrap();
    assert_eq!((unchanged.dirs_listed, unchanged.dirs_reused), (0, 12));
    assert_eq!(files(&unchanged), files(&first));
    assert_eq!(unchanged.skipped, first.skipped);

    // Growth of a known source needs no listing: it is stat'ed directly.
    OpenOptions::new()
        .append(true)
        .open(base.join("p1/s1.jsonl"))
        .unwrap()
        .write_all(b"{}\n")
        .unwrap();
    let grown = FileSessionLoader
        .discover(base, &accept, &first.index)
        .unwrap();
    assert_eq!(grown.dirs_listed, 0);
    assert_eq!(grown.sources[0].size, 6);

    // A new file deep in an otherwise unchanged tree: only its directory is listed.
    fs::write(base.join("p2/s2/sub/deep/second.jsonl"), b"{}\n").unwrap();
    let added = FileSessionLoader
        .discover(base, &accept, &first.index)
        .unwrap();
    assert_eq!((added.dirs_listed, added.dirs_reused), (1, 11));
    assert_eq!(files(&added), files(&full(&FileSessionLoader).unwrap()));
    assert!(
        !added.index.contains_key("p2/s2/sub/deep"),
        "a just-modified directory is racy and listed again next time"
    );
    let racy = FileSessionLoader
        .discover(base, &accept, &added.index)
        .unwrap();
    assert_eq!(racy.dirs_listed, 1);

    // A replaced subtree (removed, recreated) is found through its parent:
    // only the parent and the new directories are listed.
    fs::remove_dir_all(base.join("p1/s1")).unwrap();
    fs::create_dir_all(base.join("p1/s1/other")).unwrap();
    fs::write(base.join("p1/s1/other/new.jsonl"), b"{}\n").unwrap();
    age_dirs(&base.join("p1"), 1_000_000_100);
    let replaced = FileSessionLoader
        .discover(base, &accept, &racy.index)
        .unwrap();
    let fresh = full(&FileSessionLoader).unwrap();
    assert_eq!(files(&replaced), files(&fresh));
    assert_eq!(replaced.skipped, fresh.skipped);
    // p1, p1/s1, p1/s1/other; plus the still-racy deep directory.
    assert_eq!(replaced.dirs_listed, 4);
}
