use std::time::Duration;

use rusqlite::{Connection, OpenFlags, Transaction, config::DbConfig, limits::Limit, types::ValueRef};

use super::*;

fn database_error(failure: rusqlite::Error) -> PipelineError {
    let kind = match failure.sqlite_error_code() {
        Some(rusqlite::ErrorCode::TooBig) => PipelineErrorKind::BatchLimit,
        Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase) => {
            PipelineErrorKind::InvalidData
        }
        _ => PipelineErrorKind::Read,
    };
    error(kind)
}

fn open(source: &SnapshotRef, limits: SnapshotLimits) -> Result<Connection, PipelineError> {
    // SQLite NOFOLLOW rejects symlinks in every path component, unlike the
    // descriptor guard's leaf-only O_NOFOLLOW. Resolve caller-trusted ancestors
    // but retain the final filename so SQLite still rejects a symlink there.
    let parent = source.path.parent().ok_or_else(|| error(PipelineErrorKind::Read))?;
    let filename = source.path.file_name().ok_or_else(|| error(PipelineErrorKind::Read))?;
    let path = fs::canonicalize(parent)
        .map_err(|_| error(PipelineErrorKind::Read))?
        .join(filename);
    // No CREATE, READ_WRITE or URI interpretation. SQLite must retain its normal
    // locking/WAL protocol: immutable=1 would silently discard live WAL updates.
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::from_bits_retain(rusqlite::ffi::SQLITE_OPEN_NOFOLLOW);
    let connection = Connection::open_with_flags(&path, flags).map_err(database_error)?;
    connection.busy_timeout(Duration::ZERO).map_err(database_error)?;
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false).map_err(database_error)?;
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_VIEW, false).map_err(database_error)?;
    // Bound individual SQLite allocations before loading an untrusted schema or
    // payload. The fixed floor permits ordinary schema rows with tiny data limits;
    // data budgets are enforced independently by octet_length before extraction.
    let engine_limit = limits.max_snapshot_bytes.saturating_add(1024).max(64 * 1024);
    connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, i32::try_from(engine_limit).unwrap_or(i32::MAX)).map_err(database_error)?;
    Ok(connection)
}

pub(super) fn read(
    source: &SnapshotRef,
    table: &str,
    limits: SnapshotLimits,
) -> Result<Vec<SnapshotRecord>, PipelineError> {
    let mut connection = open(source, limits)?;
    let transaction = connection.transaction().map_err(database_error)?;
    inspect_table(&transaction, table)?;
    preflight(&transaction, table, limits)?;
    let records = values(&transaction, table, limits)?;
    transaction.commit().map_err(database_error)?;
    Ok(records)
}

fn inspect_table(transaction: &Transaction<'_>, table: &str) -> Result<(), PipelineError> {
    let utf8 = transaction.pragma_query_value(Some("main"), "encoding", |row| {
        Ok(row.get_ref(0)? == ValueRef::Text(b"UTF-8"))
    }).map_err(database_error)?;
    if !utf8 {
        // octet_length reports the DB encoding; our raw TEXT byte contract is
        // UTF-8. Refuse rather than under-budget an implicit UTF-16 conversion.
        return Err(error(PipelineErrorKind::Unsupported));
    }
    let ordinary_table = transaction.query_row(
        "SELECT type FROM pragma_table_list WHERE schema = 'main' AND name = ?1 COLLATE NOCASE",
        [table],
        |row| Ok(row.get_ref(0)? == ValueRef::Text(b"table")),
    ).map_err(|failure| match failure {
        rusqlite::Error::QueryReturnedNoRows => error(PipelineErrorKind::InvalidData),
        failure => database_error(failure),
    })?;
    if !ordinary_table {
        return Err(error(PipelineErrorKind::InvalidData));
    }
    // Only real stored key/value columns, never view/virtual/generated results.
    let mut statement = transaction.prepare(&format!("PRAGMA main.table_xinfo(\"{table}\")")).map_err(database_error)?;
    let mut rows = statement.query([]).map_err(database_error)?;
    let mut key = false;
    let mut value = false;
    while let Some(row) = rows.next().map_err(database_error)? {
        let name = row.get_ref(1).map_err(database_error)?;
        let hidden: i64 = row.get(6).map_err(database_error)?;
        if let ValueRef::Text(name) = name {
            if name.eq_ignore_ascii_case(b"key") {
                key = hidden == 0;
            }
            if name.eq_ignore_ascii_case(b"value") {
                value = hidden == 0;
            }
        }
    }
    if !key || !value {
        return Err(error(PipelineErrorKind::InvalidData));
    }
    Ok(())
}

fn preflight(
    transaction: &Transaction<'_>,
    table: &str,
    limits: SnapshotLimits,
) -> Result<(), PipelineError> {
    // octet_length on a stored column reads its size from the record header. In
    // particular, length(TEXT) or CAST(TEXT AS BLOB) would load an oversized value.
    // Do not ORDER BY here: that could materialize an unbounded SQLite sorter.
    let mut statement = transaction.prepare(&format!(
        "SELECT typeof(\"key\"), octet_length(\"key\"), typeof(\"value\"), octet_length(\"value\") FROM main.\"{table}\" NOT INDEXED"
    )).map_err(database_error)?;
    let mut rows = statement.query([]).map_err(database_error)?;
    let mut budget = Budget::new(limits);
    while let Some(row) = rows.next().map_err(database_error)? {
        if row.get_ref(0).map_err(database_error)? != ValueRef::Text(b"text")
            || !matches!(row.get_ref(2).map_err(database_error)?, ValueRef::Text(b"text" | b"blob"))
        {
            return Err(error(PipelineErrorKind::InvalidData));
        }
        let key_bytes = usize::try_from(row.get::<_, i64>(1).map_err(database_error)?)
            .map_err(|_| error(PipelineErrorKind::BatchLimit))?;
        let value_bytes = usize::try_from(row.get::<_, i64>(3).map_err(database_error)?)
            .map_err(|_| error(PipelineErrorKind::RecordLimit))?;
        budget.charge(key_bytes, value_bytes)?;
    }
    Ok(())
}

fn values(
    transaction: &Transaction<'_>,
    table: &str,
    limits: SnapshotLimits,
) -> Result<Vec<SnapshotRecord>, PipelineError> {
    let mut statement = transaction.prepare(&format!(
        "SELECT \"key\", \"value\" FROM main.\"{table}\" NOT INDEXED"
    )).map_err(database_error)?;
    let mut rows = statement.query([]).map_err(database_error)?;
    let mut budget = Budget::new(limits);
    let mut records = Vec::new();
    while let Some(row) = rows.next().map_err(database_error)? {
        let ValueRef::Text(key) = row.get_ref(0).map_err(database_error)? else {
            return Err(error(PipelineErrorKind::InvalidData));
        };
        let (ValueRef::Text(bytes) | ValueRef::Blob(bytes)) = row.get_ref(1).map_err(database_error)? else {
            return Err(error(PipelineErrorKind::InvalidData));
        };
        budget.charge(key.len(), bytes.len())?;
        let key = std::str::from_utf8(key).map_err(|_| error(PipelineErrorKind::InvalidData))?;
        records.push(SnapshotRecord { key: key.to_owned(), bytes: bytes.to_vec() });
    }
    records.sort_unstable_by(|left, right| left.key.as_bytes().cmp(right.key.as_bytes()));
    if records.windows(2).any(|pair| pair[0].key == pair[1].key) {
        return Err(error(PipelineErrorKind::InvalidData));
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use super::*;

    #[test]
    fn sqlite_accepts_trusted_ancestor_links_but_rejects_database_links() {
        let directory = TempDir::new().unwrap();
        let alias = directory.path().join("parent-link");
        std::os::unix::fs::symlink(directory.path(), &alias).unwrap();
        let mut source = SnapshotRef {
            path: alias.join("source.db"),
            format: SnapshotFormat::SqliteKeyValue { table: "ItemTable".into() },
            session_id: None,
        };
        let writer = Connection::open(&source.path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE ItemTable(key TEXT, value BLOB); INSERT INTO ItemTable VALUES('k', X'77616c');").unwrap();
        let limits = SnapshotLimits::default();
        let snapshot = super::super::read_snapshot(&source, limits).unwrap();
        assert_eq!(snapshot.records, vec![SnapshotRecord { key: "k".into(), bytes: b"wal".to_vec() }]);

        std::os::unix::fs::symlink("source.db", alias.join("leaf-link.db")).unwrap();
        source.path = alias.join("leaf-link.db");
        // Test SQLite directly too: the descriptor guard must not mask a
        // regression that accidentally canonicalizes the database leaf.
        assert_eq!(open(&source, limits).err().unwrap().kind(), PipelineErrorKind::Read);
        assert_eq!(super::super::read_snapshot(&source, limits).unwrap_err().kind(), PipelineErrorKind::Read);
    }

    #[test]
    fn wal_commit_between_size_check_and_extraction_cannot_mix_revisions() {
        let directory = TempDir::new().unwrap();
        let source = SnapshotRef {
            path: directory.path().join("source.db"),
            format: SnapshotFormat::SqliteKeyValue { table: "ItemTable".into() },
            session_id: None,
        };
        let writer = Connection::open(&source.path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value BLOB); INSERT INTO ItemTable VALUES('a', X'31'), ('b', X'31');").unwrap();
        let limits = SnapshotLimits { max_records: 2, max_record_bytes: 1, max_snapshot_bytes: 4 };
        let mut reader = open(&source, limits).unwrap();
        let transaction = reader.transaction().unwrap();
        inspect_table(&transaction, "ItemTable").unwrap();
        preflight(&transaction, "ItemTable", limits).unwrap();
        writer.execute_batch("BEGIN; UPDATE ItemTable SET value=X'3232'; INSERT INTO ItemTable VALUES('c', X'32'); COMMIT;").unwrap();
        let records = values(&transaction, "ItemTable", limits).unwrap();
        assert_eq!(records, vec![SnapshotRecord { key: "a".into(), bytes: b"1".to_vec() }, SnapshotRecord { key: "b".into(), bytes: b"1".to_vec() }]);
        assert!(transaction.is_readonly("main").unwrap());
        transaction.commit().unwrap();
        assert_eq!(super::read(&source, "ItemTable", limits).unwrap_err().kind(), PipelineErrorKind::RecordLimit);
    }

    #[test]
    fn loader_connection_rejects_sql_mutations_and_database_creation() {
        let directory = TempDir::new().unwrap();
        let source = SnapshotRef {
            path: directory.path().join("source.db"),
            format: SnapshotFormat::SqliteKeyValue { table: "ItemTable".into() },
            session_id: None,
        };
        let limits = SnapshotLimits::default();
        assert!(open(&source, limits).is_err());
        assert!(!source.path.exists());
        Connection::open(&source.path).unwrap().execute_batch("CREATE TABLE ItemTable(key TEXT, value BLOB);").unwrap();
        let connection = open(&source, limits).unwrap();
        let failure = connection.execute("INSERT INTO ItemTable VALUES('key','value')", []).unwrap_err();
        assert_eq!(failure.sqlite_error_code(), Some(rusqlite::ErrorCode::ReadOnly));
    }
}
