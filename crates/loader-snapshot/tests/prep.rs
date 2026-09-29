#![cfg(unix)]
//! Snapshot prep loader: discovery, sidecar-aware stat, bounded revisions and
//! record_at by key. Fixtures are synthetic; no real prompts or identifiers.

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    thread::sleep,
    time::Duration,
};

use rusqlite::Connection;
use tempfile::TempDir;
use unisphere_core::{
    PipelineErrorKind, ReadCursor, ReadLimits, SnapshotFormat, SnapshotLimits, SnapshotRecord,
    SourceIdentity,
    prep::{NativeAddress, PrepInput, PrepLoader, PrepReadLimits, PrepSkipCounts, PrepSourceKind},
};
use unisphere_loader_snapshot::SnapshotPrepLoader;

fn limits() -> PrepReadLimits {
    PrepReadLimits {
        read: ReadLimits::default(),
        snapshot: SnapshotLimits::default(),
    }
}

fn sqlite_loader() -> SnapshotPrepLoader {
    SnapshotPrepLoader::new(SnapshotFormat::SqliteKeyValue {
        table: "ItemTable".into(),
    })
}

fn snapshot(loader: &SnapshotPrepLoader, root: &Path, file: &str) -> (String, Vec<SnapshotRecord>) {
    let stat = loader.stat(root, &root.join(file)).unwrap();
    let batch = loader.read(&stat, None, limits()).unwrap();
    assert!(batch.next_cursor.is_none() && !batch.more && !batch.incomplete_tail);
    let PrepInput::Snapshot(snapshot) = batch.input else {
        panic!("snapshot loaders return snapshots");
    };
    let bytes: usize = snapshot
        .records
        .iter()
        .map(|r| r.key.len() + r.bytes.len())
        .sum();
    assert_eq!(batch.bytes_read, bytes as u64);
    (snapshot.revision, snapshot.records)
}

fn wal_database(path: &Path) -> Connection {
    let writer = Connection::open(path).unwrap();
    writer
        .execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
             CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value BLOB);
             INSERT INTO ItemTable VALUES('alpha', X'7b7d');",
        )
        .unwrap();
    writer
}

#[test]
fn discovery_honours_accept_counts_skips_and_hides_sqlite_sidecars() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("workspace/b")).unwrap();
    fs::create_dir(root.join(".hidden-dir")).unwrap();
    fs::create_dir(root.join("locked")).unwrap();
    for file in [
        "global.vscdb",
        "global.vscdb-wal",
        "global.vscdb-shm",
        "global.vscdb-journal",
        "workspace/b/state.vscdb",
        "workspace/notes.txt",
        ".hidden.vscdb",
        ".hidden-dir/state.vscdb",
        "locked/state.vscdb",
    ] {
        fs::write(root.join(file), b"").unwrap();
    }
    symlink(root.join("global.vscdb"), root.join("link.vscdb")).unwrap();
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();

    let accept = |file: &str| file.ends_with(".vscdb");
    let found = sqlite_loader().discover(root, &accept);
    fs::set_permissions(root.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
    let found = found.unwrap();
    let files: Vec<_> = found.sources.iter().map(|s| s.file.as_str()).collect();
    assert_eq!(files, ["global.vscdb", "workspace/b/state.vscdb"]);
    assert!(
        found
            .sources
            .iter()
            .all(|s| s.kind == PrepSourceKind::Snapshot)
    );
    assert_eq!(
        found.skipped,
        PrepSkipCounts {
            symlinks: 1,
            hidden: 2,
            unreadable_entries: 1,
        }
    );

    // Non-SQLite formats treat sidecar-looking names as ordinary candidates.
    let journal = SnapshotPrepLoader::new(SnapshotFormat::JsonJournal);
    let found = journal
        .discover(root, &|file| file.starts_with("global"))
        .unwrap();
    assert_eq!(found.sources.len(), 4);

    assert_eq!(
        sqlite_loader()
            .discover(Path::new("relative"), &accept)
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidInput
    );
}

#[test]
fn stat_folds_wal_growth_that_leaves_the_database_file_untouched() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let path = root.join("state.vscdb");
    let writer = wal_database(&path);
    let loader = sqlite_loader();
    let before = loader.stat(root, &path).unwrap();
    let (revision, records) = snapshot(&loader, root, "state.vscdb");
    assert_eq!(records[0].bytes, b"{}");

    let database_before = fs::metadata(&path).unwrap();
    sleep(Duration::from_millis(20));
    writer
        .execute("INSERT INTO ItemTable VALUES('beta', X'5b5d')", [])
        .unwrap();
    let database_after = fs::metadata(&path).unwrap();
    // The committed write lives only in the WAL: the main file did not move.
    assert_eq!(database_before.len(), database_after.len());
    assert_eq!(
        database_before.modified().unwrap(),
        database_after.modified().unwrap()
    );

    let after = loader.stat(root, &path).unwrap();
    assert_eq!(after.identity, before.identity);
    assert!(
        after.size > before.size,
        "WAL growth must change the folded size"
    );
    assert!(after.mtime_ns > before.mtime_ns);
    let (changed, records) = snapshot(&loader, root, "state.vscdb");
    assert_ne!(changed, revision);
    assert_eq!(
        records.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
        ["alpha", "beta"]
    );

    // Non-SQLite formats report the file alone.
    let plain = SnapshotPrepLoader::new(SnapshotFormat::JsonDocument)
        .stat(root, &path)
        .unwrap();
    assert_eq!(plain.size, database_after.len());

    assert_eq!(
        loader
            .stat(root, Path::new("/elsewhere/state.vscdb"))
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidInput
    );
    symlink(&path, root.join("link.vscdb")).unwrap();
    assert_eq!(
        loader
            .stat(root, &root.join("link.vscdb"))
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidInput
    );
}

#[test]
fn sqlite_is_read_without_writing_or_locking_the_writer() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let path = root.join("state.vscdb");
    let writer = wal_database(&path);
    let wal = root.join("state.vscdb-wal");
    let database_bytes = fs::read(&path).unwrap();
    let wal_bytes = fs::read(&wal).unwrap();
    let loader = sqlite_loader();

    // An open write transaction does not block the read, which sees the
    // last committed state only.
    writer
        .execute_batch("BEGIN IMMEDIATE; INSERT INTO ItemTable VALUES('pending', X'00');")
        .unwrap();
    let (revision, records) = snapshot(&loader, root, "state.vscdb");
    assert_eq!(
        records.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
        ["alpha"]
    );
    writer.execute_batch("ROLLBACK").unwrap();

    // No lock outlives the read: the writer can take an exclusive transaction.
    writer.execute_batch("BEGIN EXCLUSIVE; COMMIT;").unwrap();
    assert_eq!(fs::read(&path).unwrap(), database_bytes);
    assert_eq!(fs::read(&wal).unwrap(), wal_bytes);
    // Same content, same revision.
    assert_eq!(snapshot(&loader, root, "state.vscdb").0, revision);

    // Rollback-journal databases: a reserved writer does not block the read,
    // and nothing is left locked afterwards.
    let journal_path = root.join("journal.vscdb");
    let journal_writer = Connection::open(&journal_path).unwrap();
    journal_writer
        .execute_batch(
            "CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value BLOB);
             INSERT INTO ItemTable VALUES('alpha', X'7b7d');
             BEGIN IMMEDIATE; INSERT INTO ItemTable VALUES('pending', X'00');",
        )
        .unwrap();
    let (_, records) = snapshot(&loader, root, "journal.vscdb");
    assert_eq!(records.len(), 1);
    journal_writer
        .execute_batch("COMMIT; BEGIN EXCLUSIVE; COMMIT;")
        .unwrap();
}

#[test]
fn json_document_and_journal_revisions_track_content_not_metadata() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    fs::write(root.join("chat.json"), br#"{"requests":[]}"#).unwrap();
    fs::write(root.join("chat.jsonl"), b"{\"kind\":0}\n{\"kind\":1}\n").unwrap();

    let document = SnapshotPrepLoader::new(SnapshotFormat::JsonDocument);
    let (first, records) = snapshot(&document, root, "chat.json");
    assert_eq!(
        records,
        [SnapshotRecord {
            key: "document".into(),
            bytes: br#"{"requests":[]}"#.to_vec()
        }]
    );
    // Rewriting identical bytes changes mtime but not the revision.
    sleep(Duration::from_millis(20));
    fs::write(root.join("chat.json"), br#"{"requests":[]}"#).unwrap();
    assert_eq!(snapshot(&document, root, "chat.json").0, first);
    fs::write(root.join("chat.json"), br#"{"requests":[{}]}"#).unwrap();
    assert_ne!(snapshot(&document, root, "chat.json").0, first);

    let journal = SnapshotPrepLoader::new(SnapshotFormat::JsonJournal);
    let (first, records) = snapshot(&journal, root, "chat.jsonl");
    assert_eq!(
        records.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
        ["journal:0", "journal:1"]
    );
    fs::write(
        root.join("chat.jsonl"),
        b"{\"kind\":0}\n{\"kind\":1}\n{\"kind\":2}\n",
    )
    .unwrap();
    let (grown, records) = snapshot(&journal, root, "chat.jsonl");
    assert_ne!(grown, first);
    assert_eq!(records.len(), 3);

    // Snapshots are bounded: an over-limit source fails instead of truncating.
    let stat = journal.stat(root, &root.join("chat.jsonl")).unwrap();
    let tight = PrepReadLimits {
        read: ReadLimits::default(),
        snapshot: SnapshotLimits {
            max_records: 2,
            ..SnapshotLimits::default()
        },
    };
    assert_eq!(
        journal.read(&stat, None, tight).unwrap_err().kind(),
        PipelineErrorKind::BatchLimit
    );
}

#[test]
fn snapshot_reads_refuse_cursors_anchors_and_replaced_sources() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let path = root.join("chat.json");
    fs::write(&path, b"{}").unwrap();
    let loader = SnapshotPrepLoader::new(SnapshotFormat::JsonDocument);
    assert_eq!(loader.kind(), PrepSourceKind::Snapshot);
    let stat = loader.stat(root, &path).unwrap();

    let cursor = ReadCursor {
        source: path.clone(),
        identity: stat.identity.clone(),
        offset: 0,
    };
    assert_eq!(
        loader
            .read(&stat, Some(&cursor), limits())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidInput
    );
    assert_eq!(
        loader.anchor(&stat, 0).unwrap_err().kind(),
        PipelineErrorKind::Unsupported
    );

    let replacement = root.join("replacement");
    fs::write(&replacement, b"{}").unwrap();
    fs::rename(&replacement, &path).unwrap();
    assert_ne!(loader.stat(root, &path).unwrap().identity, stat.identity);
    assert_eq!(
        loader.read(&stat, None, limits()).unwrap_err().kind(),
        PipelineErrorKind::SourceChanged
    );
    let mut foreign = loader.stat(root, &path).unwrap();
    foreign.identity = SourceIdentity::Unavailable;
    assert_eq!(
        loader.read(&foreign, None, limits()).unwrap_err().kind(),
        PipelineErrorKind::SourceChanged
    );
}

#[test]
fn record_at_returns_exactly_one_value_by_snapshot_key() {
    let directory = TempDir::new().unwrap();
    let root = directory.path();
    let path = root.join("state.vscdb");
    let writer = wal_database(&path);
    writer
        .execute(
            "INSERT INTO ItemTable VALUES('beta', 'synthetic-value')",
            [],
        )
        .unwrap();
    let loader = sqlite_loader();
    let at = |key: Option<&str>, offset: Option<u64>| NativeAddress {
        offset,
        key: key.map(str::to_owned),
    };

    assert_eq!(
        loader
            .record_at(&path, &at(Some("beta"), None), 64)
            .unwrap(),
        b"synthetic-value"
    );
    assert_eq!(
        loader
            .record_at(&path, &at(Some("alpha"), None), 2)
            .unwrap(),
        b"{}"
    );
    assert_eq!(
        loader
            .record_at(&path, &at(Some("beta"), None), 4)
            .unwrap_err()
            .kind(),
        PipelineErrorKind::RecordLimit
    );
    for address in [
        at(Some("missing"), None),
        at(None, Some(0)),
        at(Some(""), None),
    ] {
        assert_eq!(
            loader.record_at(&path, &address, 64).unwrap_err().kind(),
            PipelineErrorKind::InvalidInput
        );
    }

    let journal_path = root.join("chat.jsonl");
    fs::write(&journal_path, b"{\"kind\":0}\n{\"kind\":1}\n").unwrap();
    let journal = SnapshotPrepLoader::new(SnapshotFormat::JsonJournal);
    assert_eq!(
        journal
            .record_at(&journal_path, &at(Some("journal:1"), None), 64)
            .unwrap(),
        b"{\"kind\":1}"
    );

    // Cursor IDE's key/value table, read through the same port.
    let cursor_path = root.join("cursor.vscdb");
    Connection::open(&cursor_path)
        .unwrap()
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE cursorDiskKV(key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);
             INSERT INTO cursorDiskKV VALUES('bubbleId:c1:b1', '{\"type\":1}');",
        )
        .unwrap();
    let cursor = SnapshotPrepLoader::new(SnapshotFormat::SqliteKeyValue {
        table: "cursorDiskKV".into(),
    });
    let (revision, records) = snapshot(&cursor, root, "cursor.vscdb");
    assert_eq!(records.len(), 1);
    assert_eq!(snapshot(&cursor, root, "cursor.vscdb").0, revision);
    assert_eq!(
        cursor
            .record_at(&cursor_path, &at(Some("bubbleId:c1:b1"), None), 64)
            .unwrap(),
        b"{\"type\":1}"
    );
}
