#![cfg(unix)]

use std::fs;

use rusqlite::{Connection, params};
use tempfile::TempDir;
use unisphere_core::{
    PipelineErrorKind, SnapshotFormat, SnapshotLimits, SnapshotLoader, SnapshotRecord, SnapshotRef,
};
use unisphere_loader_snapshot::FileSnapshotLoader;

fn database(schema: &str) -> (TempDir, SnapshotRef, Connection) {
    let directory = tempfile::tempdir().unwrap();
    let source = SnapshotRef {
        path: directory.path().join("native.db"),
        format: SnapshotFormat::SqliteKeyValue {
            table: "ItemTable".into(),
        },
        session_id: None,
    };
    let connection = Connection::open(&source.path).unwrap();
    connection.execute_batch(schema).unwrap();
    (directory, source, connection)
}

fn limits(records: usize, record_bytes: usize, snapshot_bytes: usize) -> SnapshotLimits {
    SnapshotLimits {
        max_records: records,
        max_record_bytes: record_bytes,
        max_snapshot_bytes: snapshot_bytes,
    }
}

#[test]
fn sqlite_retains_text_and_opaque_blob_bytes_with_native_keys_in_byte_order() {
    let (_directory, mut source, connection) =
        database("CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value BLOB);");
    connection
        .execute(
            "INSERT INTO ItemTable VALUES(?1,?2)",
            params!["z", b"\xff\0\x01".as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO ItemTable VALUES(?1,?2)",
            params!["a", " { \"native\": 1 }\r\n"],
        )
        .unwrap();
    let snapshot = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap();
    assert_eq!(
        snapshot.records,
        vec![
            SnapshotRecord {
                key: "a".into(),
                bytes: b" { \"native\": 1 }\r\n".to_vec()
            },
            SnapshotRecord {
                key: "z".into(),
                bytes: b"\xff\0\x01".to_vec()
            },
        ]
    );
    source.session_id = Some("a".into());
    let selected = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap();
    assert_eq!(selected.records, snapshot.records);
    assert_eq!(selected.revision, snapshot.revision);
}

#[test]
fn revision_is_independent_of_insertion_order_but_tracks_keys_values_and_deletion() {
    let (_directory, source, connection) = database(
        "CREATE TABLE ItemTable(key TEXT, value BLOB); INSERT INTO ItemTable VALUES('b', '2'), ('a', '1');",
    );
    let original = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap();
    connection
        .execute_batch("DELETE FROM ItemTable; INSERT INTO ItemTable VALUES('a', '1'), ('b', '2');")
        .unwrap();
    assert_eq!(
        FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap()
            .revision,
        original.revision
    );
    connection
        .execute_batch("UPDATE ItemTable SET key='c' WHERE key='b';")
        .unwrap();
    let rekeyed = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap();
    assert_ne!(original.revision, rekeyed.revision);
    connection
        .execute_batch("UPDATE ItemTable SET value='3' WHERE key='c';")
        .unwrap();
    let edited = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap();
    assert_ne!(rekeyed.revision, edited.revision);
    connection.execute_batch("DELETE FROM ItemTable;").unwrap();
    let empty = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap();
    assert!(empty.records.is_empty());
    assert_ne!(edited.revision, empty.revision);
    assert_eq!(
        empty,
        FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap()
    );
}

#[test]
fn revision_frames_key_and_value_boundaries_and_table_namespace() {
    let (_directory, source, connection) = database(
        "CREATE TABLE ItemTable(key TEXT, value BLOB); INSERT INTO ItemTable VALUES('ab', 'c'); CREATE TABLE Other(key TEXT, value BLOB); INSERT INTO Other VALUES('ab', 'c');",
    );
    let original = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap();
    connection
        .execute_batch("UPDATE ItemTable SET key='a', value='bc';")
        .unwrap();
    assert_ne!(
        original.revision,
        FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap()
            .revision
    );
    let other = SnapshotRef {
        format: SnapshotFormat::SqliteKeyValue {
            table: "Other".into(),
        },
        ..source
    };
    let other = FileSnapshotLoader
        .read_snapshot(&other, SnapshotLimits::default())
        .unwrap();
    assert_eq!(original.records, other.records);
    assert_ne!(original.revision, other.revision);
}

#[test]
fn exact_database_limits_count_utf8_keys_and_raw_values_not_sql_character_lengths() {
    let (_directory, source, connection) =
        database("CREATE TABLE ItemTable(key TEXT, value BLOB);");
    connection
        .execute(
            "INSERT INTO ItemTable VALUES(?1,?2)",
            params!["\u{e9}\0k", "\u{e9}\0"],
        )
        .unwrap();
    let snapshot = FileSnapshotLoader
        .read_snapshot(&source, limits(1, 3, 7))
        .unwrap();
    assert_eq!(
        snapshot.records,
        vec![SnapshotRecord {
            key: "\u{e9}\0k".into(),
            bytes: "\u{e9}\0".as_bytes().to_vec()
        }]
    );
    assert_eq!(
        FileSnapshotLoader
            .read_snapshot(&source, limits(1, 2, 7))
            .unwrap_err()
            .kind(),
        PipelineErrorKind::RecordLimit
    );
    assert_eq!(
        FileSnapshotLoader
            .read_snapshot(&source, limits(1, 3, 6))
            .unwrap_err()
            .kind(),
        PipelineErrorKind::BatchLimit
    );
    connection
        .execute_batch("INSERT INTO ItemTable VALUES('x', X'');")
        .unwrap();
    assert_eq!(
        FileSnapshotLoader
            .read_snapshot(&source, limits(1, 3, 8))
            .unwrap_err()
            .kind(),
        PipelineErrorKind::BatchLimit
    );
}

#[test]
fn oversized_native_text_blob_and_key_fail_before_unbounded_extraction() {
    for kind in ["text", "blob", "key"] {
        let (_directory, source, connection) =
            database("CREATE TABLE ItemTable(key TEXT, value BLOB);");
        match kind {
            "text" => connection
                .execute(
                    "INSERT INTO ItemTable VALUES('k', ?1)",
                    ["x".repeat(2 * 1024 * 1024)],
                )
                .unwrap(),
            "blob" => connection
                .execute("INSERT INTO ItemTable VALUES('k', zeroblob(2097152))", [])
                .unwrap(),
            "key" => connection
                .execute(
                    "INSERT INTO ItemTable VALUES(?1, X'')",
                    ["x".repeat(2 * 1024 * 1024)],
                )
                .unwrap(),
            _ => unreachable!(),
        };
        let failure = FileSnapshotLoader
            .read_snapshot(&source, limits(2, 16, 32))
            .unwrap_err();
        assert_eq!(
            failure.kind(),
            if kind == "key" {
                PipelineErrorKind::BatchLimit
            } else {
                PipelineErrorKind::RecordLimit
            },
            "{kind}"
        );
        assert_eq!(failure.offset(), None);
    }
}

#[test]
fn duplicate_empty_nontext_and_invalid_utf8_keys_are_not_silently_overwritten() {
    for rows in [
        "INSERT INTO ItemTable VALUES('same', '1'), ('same', '2');",
        "INSERT INTO ItemTable VALUES('', '1');",
        "INSERT INTO ItemTable VALUES(X'61', '1');",
        "INSERT INTO ItemTable VALUES(NULL, '1');",
        "INSERT INTO ItemTable VALUES(CAST(X'ff' AS TEXT), '1');",
        "INSERT INTO ItemTable VALUES('a', NULL);",
        "INSERT INTO ItemTable VALUES('a', 42);",
    ] {
        let (_directory, source, connection) = database("CREATE TABLE ItemTable(key, value);");
        connection.execute_batch(rows).unwrap();
        assert_eq!(
            FileSnapshotLoader
                .read_snapshot(&source, SnapshotLimits::default())
                .unwrap_err()
                .kind(),
            PipelineErrorKind::InvalidData,
            "{rows}"
        );
    }
}

#[test]
fn sqlite_requires_a_real_stored_key_value_table_and_utf8_database() {
    for schema in [
        "CREATE TABLE Other(key TEXT, value BLOB);",
        "CREATE TABLE ItemTable(other TEXT, value BLOB);",
        "CREATE VIEW ItemTable AS SELECT 'key' AS key, 'value' AS value;",
        "CREATE VIRTUAL TABLE ItemTable USING fts5(key, value);",
        "CREATE TABLE ItemTable(key TEXT, base TEXT, value TEXT GENERATED ALWAYS AS (base) VIRTUAL);",
        "PRAGMA encoding='UTF-16le'; CREATE TABLE ItemTable(key TEXT, value BLOB);",
    ] {
        let (_directory, source, _connection) = database(schema);
        let failure = FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap_err();
        assert_eq!(
            failure.kind(),
            if schema.starts_with("PRAGMA encoding") {
                PipelineErrorKind::Unsupported
            } else {
                PipelineErrorKind::InvalidData
            },
            "{schema}"
        );
    }
}

#[test]
fn malformed_database_returns_safe_errors_without_native_payload() {
    let directory = tempfile::tempdir().unwrap();
    let source = SnapshotRef {
        path: directory.path().join("native.db"),
        format: SnapshotFormat::SqliteKeyValue {
            table: "ItemTable".into(),
        },
        session_id: None,
    };
    fs::write(&source.path, b"private-database-bytes-that-are-not-SQLite").unwrap();
    let failure = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap_err();
    assert_eq!(failure.kind(), PipelineErrorKind::InvalidData);
    assert!(!failure.to_string().contains("private-database"));
    assert_eq!(failure.offset(), None);
}

#[test]
fn read_only_snapshot_keeps_database_bytes_unchanged_and_sees_committed_wal_content() {
    let (_directory, source, connection) = database(
        "CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value BLOB); INSERT INTO ItemTable VALUES('k', 'old');",
    );
    drop(connection);
    let before = fs::read(&source.path).unwrap();
    let original = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap();
    assert_eq!(before, fs::read(&source.path).unwrap());
    let writer = Connection::open(&source.path).unwrap();
    writer
        .execute_batch("PRAGMA journal_mode=WAL; UPDATE ItemTable SET value='new';")
        .unwrap();
    let updated = FileSnapshotLoader
        .read_snapshot(&source, SnapshotLimits::default())
        .unwrap();
    assert_eq!(updated.records[0].bytes, b"new");
    assert_ne!(original.revision, updated.revision);
    writer
        .execute_batch("BEGIN IMMEDIATE; UPDATE ItemTable SET value='uncommitted';")
        .unwrap();
    assert_eq!(
        FileSnapshotLoader
            .read_snapshot(&source, SnapshotLimits::default())
            .unwrap(),
        updated
    );
    writer.execute_batch("ROLLBACK;").unwrap();
}
