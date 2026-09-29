//! Target-directory store for `unisphere prep`.
//!
//! Layout under the target directory:
//! - `state.json`: the publication. Committed per-source cursors, fold
//!   checkpoints and the list of committed parts, replaced by atomic rename;
//! - `tables/{calls,turns,triggers,events,tool_uses}/*.parquet`: append-only
//!   parts (`run-NNNNNN.parquet` per run, `compact-NNNNNN-K.parquet` per
//!   compaction); every fact table always has at least one (possibly empty) part;
//! - `tables/sources.parquet`, `tables/sessions.parquet`: snapshots of the
//!   published state's sources;
//! - `views.sql`: the canonical DuckDB views over exactly the committed parts.
//!
//! Every Parquet file carries the key-value metadata
//! `unisphere.table_schema_version` and `unisphere.table`; snapshots also carry
//! `unisphere.state_fingerprint` of the state they project.
//!
//! A commit writes synced parts, then the snapshots, then `state.json`, then
//! `views.sql`. [`PrepStore::load`] restores snapshots and `views.sql` from the
//! published state and removes every part it does not reference, so a failed or
//! crashed run leaves the previous publication and never a row twice.
//! Superseded generations stay in older parts until [`PrepStore::compact`];
//! views keep rows whose `generation` equals the source's generation in
//! `sources`. The store holds an exclusive lock on `TARGET/.prep.lock` for its
//! lifetime.
#![forbid(unsafe_code)]

mod compact;
mod schema;
mod views;

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use parquet::{
    arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder},
    basic::Compression,
    file::{metadata::KeyValue, properties::WriterProperties},
};
use serde::{Serialize, de::DeserializeOwned};
use unisphere_core::{
    PipelineError, PipelineErrorKind,
    prep::{
        PREP_TABLE_SCHEMA_VERSION, PrepCallRow, PrepCommit, PrepCompactReport, PrepEventRow,
        PrepLoaded, PrepRows, PrepState, PrepTableCounts, PrepToolUseRow, PrepTriggerRow,
        PrepTurnRow,
    },
};

use compact::{Current, Pos};
pub use views::VIEWS;

pub const FACT_TABLES: [&str; 5] = ["calls", "turns", "triggers", "events", "tool_uses"];
/// Parquet key-value metadata key holding [`PREP_TABLE_SCHEMA_VERSION`].
pub const META_SCHEMA_VERSION: &str = "unisphere.table_schema_version";
/// Parquet key-value metadata key naming the table a file belongs to.
pub const META_TABLE: &str = "unisphere.table";
/// Snapshot metadata key: fingerprint of the state the snapshot projects.
pub const META_STATE_FINGERPRINT: &str = "unisphere.state_fingerprint";

const STATE: &str = "state.json";
const VIEWS_SQL: &str = "views.sql";
const SNAPSHOTS: [&str; 2] = ["sources", "sessions"];

pub struct ParquetPrepStore {
    target: PathBuf,
    /// Held for the store's lifetime; dropping it releases the target.
    /// `None` for a read-only store, which refuses every write.
    lock: Option<File>,
}

impl ParquetPrepStore {
    /// Open (creating if needed) `target` and take its exclusive writer lock.
    /// A second concurrent writer is refused with `Write`.
    pub fn open(target: PathBuf) -> Result<Self, PipelineError> {
        fs::create_dir_all(&target).map_err(write_error)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(target.join(".prep.lock"))
            .map_err(write_error)?;
        lock.try_lock().map_err(write_error)?;
        Ok(Self {
            target,
            lock: Some(lock),
        })
    }

    /// Open an existing `target` for reading committed state only: nothing is
    /// created, no lock is taken, and `load`/`commit`/`compact` are refused.
    pub fn open_read_only(target: PathBuf) -> Result<Self, PipelineError> {
        if !target.join(STATE).is_file() {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        Ok(Self { target, lock: None })
    }

    fn writable(&self) -> Result<(), PipelineError> {
        self.lock
            .as_ref()
            .map(|_| ())
            .ok_or_else(|| write_error(()))
    }
}

fn write_error(_: impl Sized) -> PipelineError {
    PipelineError::new(PipelineErrorKind::Write, None)
}

fn read_error(_: impl Sized) -> PipelineError {
    PipelineError::new(PipelineErrorKind::Read, None)
}

fn invalid_data(_: impl Sized) -> PipelineError {
    PipelineError::new(PipelineErrorKind::InvalidData, None)
}

fn fsync_dir(dir: &Path) -> Result<(), PipelineError> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(write_error)
}

/// FNV-1a 64: a stable, dependency-free fingerprint for staleness checks.
fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// Write `bytes` to `path` via a synced temporary file and an atomic rename.
/// The caller syncs the parent directory.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PipelineError> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut file = File::create(&tmp).map_err(write_error)?;
    file.write_all(bytes).map_err(write_error)?;
    file.sync_all().map_err(write_error)?;
    drop(file);
    fs::rename(&tmp, path).map_err(write_error)
}

/// Write rows to `path` via a synced temporary file and an atomic rename.
/// The caller syncs the parent directory.
fn write_parquet<T: Serialize>(
    path: &Path,
    table: &str,
    rows: &[T],
    extra: Option<KeyValue>,
) -> Result<u64, PipelineError> {
    let schema = schema::schema(table);
    let tmp = path.with_extension("parquet.tmp");
    let file = File::create(&tmp).map_err(write_error)?;
    let mut metadata = vec![
        KeyValue::new(
            META_SCHEMA_VERSION.to_owned(),
            PREP_TABLE_SCHEMA_VERSION.to_string(),
        ),
        KeyValue::new(META_TABLE.to_owned(), table.to_owned()),
    ];
    metadata.extend(extra);
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .set_key_value_metadata(Some(metadata))
        .build();
    let mut writer =
        ArrowWriter::try_new(file, schema.clone(), Some(props)).map_err(write_error)?;
    for chunk in rows.chunks(65_536) {
        let mut decoder = arrow_json::ReaderBuilder::new(schema.clone())
            .with_batch_size(65_536)
            .build_decoder()
            .map_err(write_error)?;
        decoder.serialize(chunk).map_err(invalid_data)?;
        if let Some(batch) = decoder.flush().map_err(write_error)? {
            writer.write(&batch).map_err(write_error)?;
        }
    }
    let file = writer.into_inner().map_err(write_error)?;
    file.sync_all().map_err(write_error)?;
    let bytes = file.metadata().map_err(write_error)?.len();
    drop(file);
    fs::rename(&tmp, path).map_err(write_error)?;
    Ok(bytes)
}

/// Every row of one Parquet file, decoded through the Arrow JSON writer.
fn read_parquet<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>, PipelineError> {
    let file = File::open(path).map_err(read_error)?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .and_then(|builder| builder.build())
        .map_err(invalid_data)?;
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch.map_err(invalid_data)?;
        let mut json = arrow_json::LineDelimitedWriter::new(Vec::new());
        json.write(&batch).map_err(invalid_data)?;
        json.finish().map_err(invalid_data)?;
        for row in serde_json::Deserializer::from_slice(&json.into_inner()).into_iter::<T>() {
            rows.push(row.map_err(invalid_data)?);
        }
    }
    Ok(rows)
}

/// One key-value metadata entry of a Parquet file; `None` when the file is
/// missing, unreadable or lacks the key.
fn parquet_metadata(path: &Path, key: &str) -> Option<String> {
    let builder = ParquetRecordBatchReaderBuilder::try_new(File::open(path).ok()?).ok()?;
    builder
        .metadata()
        .file_metadata()
        .key_value_metadata()?
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.clone())
}

fn table_prefix(table: &str) -> String {
    format!("tables/{table}/")
}

/// Committed parts of `table`, in file-name order (the order `views.sql` uses).
fn parts_of<'a>(parts: &'a [String], table: &str) -> Vec<&'a String> {
    let prefix = table_prefix(table);
    let mut out: Vec<&String> = parts.iter().filter(|p| p.starts_with(&prefix)).collect();
    out.sort_unstable();
    out
}

fn set_count(counts: &mut PrepTableCounts, table: &str, value: u64) {
    match table {
        "calls" => counts.calls = value,
        "turns" => counts.turns = value,
        "triggers" => counts.triggers = value,
        "events" => counts.events = value,
        "tool_uses" => counts.tool_uses = value,
        _ => unreachable!("unknown fact table"),
    }
}

impl ParquetPrepStore {
    fn read_state(&self) -> Result<Option<PrepState>, PipelineError> {
        match fs::read(self.target.join(STATE)) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(invalid_data),
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(PipelineError::new(PipelineErrorKind::Read, None)),
        }
    }

    fn tables(&self) -> PathBuf {
        self.target.join("tables")
    }

    /// Write one part of `table` for this publication and record it in `parts`.
    /// Refuses to overwrite a committed part.
    fn write_part<T: Serialize>(
        &self,
        table: &str,
        name: &str,
        rows: &[T],
        parts: &mut Vec<String>,
    ) -> Result<(String, u64), PipelineError> {
        let relative = format!("{}{name}", table_prefix(table));
        if parts.contains(&relative) {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        let dir = self.tables().join(table);
        fs::create_dir_all(&dir).map_err(write_error)?;
        let bytes = write_parquet(&dir.join(name), table, rows, None)?;
        fsync_dir(&dir)?;
        parts.push(relative.clone());
        Ok((relative, bytes))
    }

    /// Rewrite both snapshots from `state`; returns (bytes, rows).
    fn write_snapshots(&self, state: &PrepState) -> Result<(u64, u64), PipelineError> {
        let (sources, sessions) = schema::snapshot_rows(state);
        let print = snapshot_fingerprint(&sources, &sessions)?;
        let tables = self.tables();
        fs::create_dir_all(&tables).map_err(write_error)?;
        let stamp = || {
            Some(KeyValue::new(
                META_STATE_FINGERPRINT.to_owned(),
                print.clone(),
            ))
        };
        let mut bytes = write_parquet(
            &tables.join("sources.parquet"),
            "sources",
            &sources,
            stamp(),
        )?;
        bytes += write_parquet(
            &tables.join("sessions.parquet"),
            "sessions",
            &sessions,
            stamp(),
        )?;
        fsync_dir(&tables)?;
        Ok((bytes, (sources.len() + sessions.len()) as u64))
    }

    /// Replace `state.json` atomically; this is the publication point.
    fn publish_state(&self, state: &PrepState) -> Result<u64, PipelineError> {
        let bytes = serde_json::to_vec(state).map_err(write_error)?;
        write_atomic(&self.target.join(STATE), &bytes)?;
        fsync_dir(&self.target)?;
        Ok(bytes.len() as u64)
    }

    /// Rewrite `views.sql` for `parts` unless it already matches.
    fn publish_views(&self, parts: &[String]) -> Result<u64, PipelineError> {
        let sql = views::views_sql(parts);
        let path = self.target.join(VIEWS_SQL);
        if fs::read(&path).is_ok_and(|current| current == sql.as_bytes()) {
            return Ok(0);
        }
        write_atomic(&path, sql.as_bytes())?;
        fsync_dir(&self.target)?;
        Ok(sql.len() as u64)
    }

    /// Bring snapshots and `views.sql` back to the published `state` (a run
    /// may have failed after writing them); `None` removes them.
    fn restore_projections(&self, state: Option<&PrepState>) -> Result<u64, PipelineError> {
        let Some(state) = state else {
            let mut removed = 0;
            let tables = self.tables();
            for path in SNAPSHOTS
                .iter()
                .map(|s| tables.join(format!("{s}.parquet")))
                .chain([self.target.join(VIEWS_SQL)])
            {
                if path.is_file() {
                    fs::remove_file(&path).map_err(write_error)?;
                    removed += 1;
                }
            }
            return Ok(removed);
        };
        let (sources, sessions) = schema::snapshot_rows(state);
        let print = snapshot_fingerprint(&sources, &sessions)?;
        let fresh = SNAPSHOTS.iter().all(|s| {
            parquet_metadata(
                &self.tables().join(format!("{s}.parquet")),
                META_STATE_FINGERPRINT,
            )
            .is_some_and(|stamp| stamp == print)
        });
        if !fresh {
            self.write_snapshots(state)?;
        }
        self.publish_views(&state.parts)?;
        Ok(0)
    }

    /// Remove part files and temporary files the published `parts` do not
    /// reference. Only regular files are touched.
    fn remove_unreferenced(&self, parts: &[String]) -> Result<u64, PipelineError> {
        let mut removed = 0;
        let mut sweep = |dir: &Path, keep: &dyn Fn(&str) -> bool| -> Result<(), PipelineError> {
            let Ok(entries) = fs::read_dir(dir) else {
                return Ok(());
            };
            let mut changed = false;
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().is_ok_and(|t| t.is_file()) && !keep(&name) {
                    fs::remove_file(entry.path()).map_err(write_error)?;
                    removed += 1;
                    changed = true;
                }
            }
            if changed {
                fsync_dir(dir)?;
            }
            Ok(())
        };
        for table in FACT_TABLES {
            let prefix = table_prefix(table);
            sweep(&self.tables().join(table), &|name| {
                parts.contains(&format!("{prefix}{name}"))
            })?;
        }
        sweep(&self.tables(), &|name| !name.ends_with(".tmp"))?;
        sweep(&self.target, &|name| !name.ends_with(".tmp"))?;
        Ok(removed)
    }

    /// Positioned rows of `table` from `parts` (already in file-name order).
    fn read_table<T: DeserializeOwned>(
        &self,
        parts: &[&String],
    ) -> Result<(Vec<(T, Pos)>, u64), PipelineError> {
        let mut rows = Vec::new();
        let mut bytes = 0;
        for (part, relative) in parts.iter().enumerate() {
            let path = self.target.join(relative);
            bytes += fs::metadata(&path).map_err(read_error)?.len();
            rows.extend(
                read_parquet::<T>(&path)?
                    .into_iter()
                    .enumerate()
                    .map(|(row, value)| (value, Pos { part, row })),
            );
        }
        Ok((rows, bytes))
    }

    /// Rewrite one fact table's current generation into `name`; returns
    /// (rows before, rows after, bytes before, bytes after).
    fn compact_table(
        &self,
        table: &str,
        old: &[&String],
        current: &Current<'_>,
        name: &str,
        parts: &mut Vec<String>,
    ) -> Result<(u64, u64, u64, u64), PipelineError> {
        macro_rules! rewrite {
            ($row:ty, $merge:expr) => {{
                let (rows, bytes_before) = self.read_table::<$row>(old)?;
                let before = rows.len() as u64;
                let merged: Vec<$row> = $merge(rows);
                let (_, bytes_after) = self.write_part(table, name, &merged, parts)?;
                (before, merged.len() as u64, bytes_before, bytes_after)
            }};
        }
        Ok(match table {
            "calls" => rewrite!(PrepCallRow, |rows| compact::calls(rows, current)),
            "turns" => rewrite!(PrepTurnRow, |rows| compact::current_rows(
                rows,
                current,
                |r: &PrepTurnRow| (&r.source, r.generation)
            )),
            "triggers" => rewrite!(PrepTriggerRow, |rows| compact::current_rows(
                rows,
                current,
                |r: &PrepTriggerRow| (&r.source, r.generation)
            )),
            "events" => rewrite!(PrepEventRow, |rows| compact::current_rows(
                rows,
                current,
                |r: &PrepEventRow| (&r.source, r.generation)
            )),
            "tool_uses" => rewrite!(PrepToolUseRow, |rows| compact::tool_uses(rows, current)),
            _ => unreachable!("unknown fact table"),
        })
    }
}

fn snapshot_fingerprint<A: Serialize, B: Serialize>(
    sources: &[A],
    sessions: &[B],
) -> Result<String, PipelineError> {
    serde_json::to_vec(&(sources, sessions))
        .map(|bytes| fingerprint(&bytes))
        .map_err(write_error)
}

impl unisphere_core::prep::PrepStore for ParquetPrepStore {
    fn state(&self) -> Result<Option<PrepState>, PipelineError> {
        self.read_state()
    }

    fn load(&self) -> Result<PrepLoaded, PipelineError> {
        self.writable()?;
        let state = self.read_state()?;
        // Restore projections first: until then `views.sql` may still name
        // parts a finished compaction left for deletion.
        let mut removed = self.restore_projections(state.as_ref())?;
        let parts = state.as_ref().map_or(&[][..], |s| s.parts.as_slice());
        removed += self.remove_unreferenced(parts)?;
        Ok(PrepLoaded {
            state,
            orphans_removed: removed,
        })
    }

    fn commit(&self, rows: &PrepRows, state: &PrepState) -> Result<PrepCommit, PipelineError> {
        self.writable()?;
        let mut commit = PrepCommit::default();
        let mut state = state.clone();
        let name = format!("run-{:06}.parquet", state.runs);
        macro_rules! part {
            ($table:literal, $rows:expr) => {
                // A table without any committed part gets an empty one, so every
                // view reads at least one self-describing file.
                if !$rows.is_empty() || parts_of(&state.parts, $table).is_empty() {
                    let (relative, bytes) =
                        self.write_part($table, &name, $rows, &mut state.parts)?;
                    commit.bytes_written += bytes;
                    commit.parts_written.push(relative);
                }
            };
        }
        part!("calls", &rows.calls);
        part!("turns", &rows.turns);
        part!("triggers", &rows.triggers);
        part!("events", &rows.events);
        part!("tool_uses", &rows.tool_uses);
        let (bytes, snapshot_rows) = self.write_snapshots(&state)?;
        commit.bytes_written += bytes;
        commit.snapshot_rows = snapshot_rows;
        commit.bytes_written += self.publish_state(&state)?;
        commit.bytes_written += self.publish_views(&state.parts)?;
        Ok(commit)
    }

    fn compact(&self) -> Result<PrepCompactReport, PipelineError> {
        self.writable()?;
        let mut report = PrepCompactReport {
            target: self.target.clone(),
            ..PrepCompactReport::default()
        };
        let Some(mut state) = self.load()?.state else {
            return Ok(report);
        };
        let old = std::mem::take(&mut state.parts);
        let name = (0..)
            .map(|k| format!("compact-{:06}-{k}.parquet", state.runs))
            .find(|name| !old.iter().any(|p| p.ends_with(&format!("/{name}"))))
            .expect("unbounded candidates");
        let current: Current<'_> = state
            .sources
            .iter()
            .map(|(key, source)| (key.as_str(), source.generation))
            .collect();
        let mut parts = Vec::with_capacity(FACT_TABLES.len());
        for table in FACT_TABLES {
            let old_parts = parts_of(&old, table);
            let (before, after, bytes_before, bytes_after) =
                self.compact_table(table, &old_parts, &current, &name, &mut parts)?;
            set_count(&mut report.rows_before, table, before);
            set_count(&mut report.rows_after, table, after);
            report.bytes_before += bytes_before;
            report.bytes_after += bytes_after;
        }
        report.parts_before = old.len() as u64;
        report.parts_after = parts.len() as u64;
        state.parts = parts;
        self.publish_state(&state)?;
        self.publish_views(&state.parts)?;
        self.remove_unreferenced(&state.parts)?;
        Ok(report)
    }
}
