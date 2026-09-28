//! Target-directory store for `unisphere prep`.
//!
//! Layout under the target directory:
//! - `state.json`: committed per-source cursors and fold state (published last,
//!   by atomic rename);
//! - `tables/{calls,turns,triggers,events}/run-NNNNNN.parquet`: append-only parts,
//!   one per table per run that produced rows;
//! - `tables/sources.parquet`, `tables/sessions.parquet`: current snapshots.
//!
//! Parts not referenced by `state.json` belong to an uncommitted run and are
//! removed on load, so readers never see a row twice. Superseded generations
//! stay in older parts; readers keep rows whose `generation` equals the source's
//! current generation in `sources`.
#![forbid(unsafe_code)]

use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_schema::{DataType, Field, Schema};
use parquet::{arrow::ArrowWriter, basic::Compression, file::properties::WriterProperties};
use serde::Serialize;
use unisphere_core::{
    PipelineError, PipelineErrorKind, SourceIdentity,
    prep::{PrepCommit, PrepRows, PrepState},
};

pub const FACT_TABLES: [&str; 4] = ["calls", "turns", "triggers", "events"];

pub struct ParquetPrepStore {
    target: PathBuf,
}

impl ParquetPrepStore {
    pub fn new(target: PathBuf) -> Self {
        Self { target }
    }
}

fn write_error(_: impl Sized) -> PipelineError {
    PipelineError::new(PipelineErrorKind::Write, None)
}

fn columns(spec: &[(&str, DataType, bool)]) -> Arc<Schema> {
    Arc::new(Schema::new(
        spec.iter()
            .map(|(name, kind, nullable)| Field::new(*name, kind.clone(), *nullable))
            .collect::<Vec<_>>(),
    ))
}

use DataType::{Boolean as B, Int64 as I, Utf8 as S};

fn schema(table: &str) -> Arc<Schema> {
    let key = [("file", S, false), ("generation", I, false)];
    let spec: Vec<(&str, DataType, bool)> = match table {
        "calls" => vec![
            ("native_offset", I, false),
            ("sighting", S, false),
            ("msg_id", S, true),
            ("request_id", S, true),
            ("ts", S, false),
            ("ts_ms", I, false),
            ("model", S, true),
            ("input", I, false),
            ("cw_1h", I, false),
            ("cw_5m", I, false),
            ("cache_read", I, false),
            ("output", I, false),
            ("gap_ms", I, true),
            ("turn_no", I, true),
            ("call_in_turn", I, true),
            ("records", I, false),
        ],
        "turns" => vec![
            ("turn_no", I, false),
            ("started_ts", S, false),
            ("started_ts_ms", I, false),
            ("first_call_offset", I, false),
            ("trigger", S, false),
            ("sender", S, true),
            ("pij_msg_id", S, true),
            ("opener_offset", I, true),
            ("opener_ts_ms", I, true),
            ("opener_chars", I, true),
            ("body_key", S, true),
        ],
        "triggers" => vec![
            ("native_offset", I, false),
            ("ts", S, false),
            ("ts_ms", I, false),
            ("kind", S, false),
            ("sender", S, true),
            ("pij_msg_id", S, true),
            ("chars", I, false),
            ("body_key", S, true),
            ("next_turn_no", I, false),
            ("content_head", S, true),
        ],
        "events" => vec![
            ("native_offset", I, false),
            ("ts", S, false),
            ("ts_ms", I, false),
            ("kind", S, false),
            ("subkind", S, true),
            ("pre_tokens", I, true),
            ("post_tokens", I, true),
            ("duration_ms", I, true),
            ("last_context", I, true),
            ("gap_ms", I, true),
            ("resets_at", S, true),
            ("turn_no", I, false),
            ("body_key", S, true),
        ],
        "sources" => vec![
            ("path", S, false),
            ("project_dir", S, false),
            ("project", S, false),
            ("is_sub", B, false),
            ("agent_id", S, true),
            ("device", I, true),
            ("inode", I, true),
            ("size", I, false),
            ("mtime_ns", I, false),
            ("committed_offset", I, false),
            ("pending_tail_bytes", I, false),
            ("last_status", S, false),
            ("policy", S, false),
            ("table_schema_version", I, false),
        ],
        "sessions" => vec![
            ("session_id", S, true),
            ("is_sub", B, false),
            ("agent_id", S, true),
            ("project", S, false),
            ("cwd", S, true),
            ("first_ts", S, true),
            ("last_ts", S, true),
            ("first_ts_ms", I, true),
            ("last_ts_ms", I, true),
            ("seat_hint", S, true),
            ("records", I, false),
            ("calls", I, false),
            ("skipped_malformed", I, false),
            ("skipped_untimed", I, false),
            ("skipped_bad_timestamp", I, false),
        ],
        _ => unreachable!("unknown prep table"),
    };
    columns(&key.into_iter().chain(spec).collect::<Vec<_>>())
}

#[derive(Serialize)]
struct SourceRow<'a> {
    file: &'a str,
    generation: u32,
    path: &'a str,
    project_dir: &'a str,
    project: String,
    is_sub: bool,
    agent_id: Option<&'a str>,
    device: Option<i64>,
    inode: Option<i64>,
    size: i64,
    mtime_ns: i64,
    committed_offset: i64,
    pending_tail_bytes: i64,
    last_status: &'a str,
    policy: &'a str,
    table_schema_version: u32,
}

#[derive(Serialize)]
struct SessionRow<'a> {
    file: &'a str,
    generation: u32,
    session_id: Option<&'a str>,
    is_sub: bool,
    agent_id: Option<&'a str>,
    project: String,
    cwd: Option<&'a str>,
    first_ts: Option<&'a str>,
    last_ts: Option<&'a str>,
    first_ts_ms: Option<i64>,
    last_ts_ms: Option<i64>,
    seat_hint: Option<&'a str>,
    records: u64,
    calls: u64,
    skipped_malformed: u64,
    skipped_untimed: u64,
    skipped_bad_timestamp: u64,
}

/// `-Users-<name>-rest` → `rest`; Claude encodes the cwd with `/` as `-`.
fn project_of(project_dir: &str) -> String {
    project_dir
        .strip_prefix("-Users-")
        .and_then(|rest| rest.split_once('-'))
        .map_or_else(|| project_dir.to_owned(), |(_, rest)| rest.to_owned())
}

fn agent_of(file: &str, is_sub: bool) -> Option<&str> {
    is_sub
        .then(|| file.rsplit('/').next())
        .flatten()
        .and_then(|name| name.strip_suffix(".jsonl"))
}

fn fsync_dir(dir: &Path) -> Result<(), PipelineError> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(write_error)
}

/// Write rows to `path` via a synced temporary file and an atomic rename.
fn write_parquet<T: Serialize>(path: &Path, table: &str, rows: &[T]) -> Result<u64, PipelineError> {
    let schema = schema(table);
    let tmp = path.with_extension("parquet.tmp");
    let file = File::create(&tmp).map_err(write_error)?;
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer =
        ArrowWriter::try_new(file, schema.clone(), Some(props)).map_err(write_error)?;
    for chunk in rows.chunks(65_536) {
        let mut decoder = arrow_json::ReaderBuilder::new(schema.clone())
            .with_batch_size(65_536)
            .build_decoder()
            .map_err(write_error)?;
        decoder
            .serialize(chunk)
            .map_err(|_| PipelineError::new(PipelineErrorKind::InvalidData, None))?;
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

impl unisphere_core::prep::PrepStore for ParquetPrepStore {
    fn load(&self) -> Result<(Option<PrepState>, u64), PipelineError> {
        let state_path = self.target.join("state.json");
        let state: Option<PrepState> = match fs::read(&state_path) {
            Ok(bytes) => Some(
                serde_json::from_slice(&bytes)
                    .map_err(|_| PipelineError::new(PipelineErrorKind::InvalidData, None))?,
            ),
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(PipelineError::new(PipelineErrorKind::Read, None)),
        };
        let parts = state.as_ref().map(|s| s.parts.as_slice()).unwrap_or(&[]);
        let mut removed = 0;
        for table in FACT_TABLES {
            let dir = self.target.join("tables").join(table);
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let relative = format!("tables/{table}/{}", entry.file_name().to_string_lossy());
                if !parts.contains(&relative) {
                    fs::remove_file(entry.path()).map_err(write_error)?;
                    removed += 1;
                }
            }
        }
        Ok((state, removed))
    }

    fn commit(&self, rows: &PrepRows, state: &PrepState) -> Result<PrepCommit, PipelineError> {
        let tables = self.target.join("tables");
        let mut commit = PrepCommit::default();
        let mut state = state.clone();
        let name = format!("run-{:06}.parquet", state.runs);
        macro_rules! part {
            ($table:literal, $rows:expr) => {
                if !$rows.is_empty() {
                    let dir = tables.join($table);
                    fs::create_dir_all(&dir).map_err(write_error)?;
                    commit.bytes_written += write_parquet(&dir.join(&name), $table, $rows)?;
                    fsync_dir(&dir)?;
                    let relative = format!("tables/{}/{name}", $table);
                    commit.parts_written.push(relative.clone());
                    state.parts.push(relative);
                }
            };
        }
        part!("calls", &rows.calls);
        part!("turns", &rows.turns);
        part!("triggers", &rows.triggers);
        part!("events", &rows.events);
        fs::create_dir_all(&tables).map_err(write_error)?;
        let as_i64 = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
        let mut sources = Vec::with_capacity(state.sources.len());
        let mut sessions = Vec::with_capacity(state.sources.len());
        for (file, source) in &state.sources {
            let project_dir = file.split('/').next().unwrap_or(file);
            let (device, inode) = match source.identity {
                SourceIdentity::Unix { device, inode } => {
                    (Some(as_i64(device)), Some(as_i64(inode)))
                }
                SourceIdentity::Unavailable => (None, None),
            };
            let agent_id = agent_of(file, source.is_sub);
            sources.push(SourceRow {
                file,
                generation: source.generation,
                path: source.path.to_str().unwrap_or_default(),
                project_dir,
                project: project_of(project_dir),
                is_sub: source.is_sub,
                agent_id,
                device,
                inode,
                size: as_i64(source.size),
                mtime_ns: i64::try_from(source.mtime_ns).unwrap_or(i64::MAX),
                committed_offset: as_i64(source.offset),
                pending_tail_bytes: as_i64(source.size.saturating_sub(source.offset)),
                last_status: &source.last_status,
                policy: &state.policy,
                table_schema_version: state.table_schema_version,
            });
            let facts = &source.session;
            sessions.push(SessionRow {
                file,
                generation: source.generation,
                session_id: facts.session_id.as_deref(),
                is_sub: source.is_sub,
                agent_id,
                project: project_of(project_dir),
                cwd: facts.cwd.as_deref(),
                first_ts: facts.first_ts.as_deref(),
                last_ts: facts.last_ts.as_deref(),
                first_ts_ms: facts.first_ts_ms,
                last_ts_ms: facts.last_ts_ms,
                seat_hint: facts.seat_hint.as_deref(),
                records: facts.records,
                calls: facts.calls,
                skipped_malformed: facts.skipped_malformed,
                skipped_untimed: facts.skipped_untimed,
                skipped_bad_timestamp: facts.skipped_bad_timestamp,
            });
        }
        commit.bytes_written +=
            write_parquet(&tables.join("sources.parquet"), "sources", &sources)?;
        commit.bytes_written +=
            write_parquet(&tables.join("sessions.parquet"), "sessions", &sessions)?;
        commit.snapshot_rows = (sources.len() + sessions.len()) as u64;
        fsync_dir(&tables)?;
        let bytes = serde_json::to_vec(&state).map_err(write_error)?;
        let tmp = self.target.join("state.json.tmp");
        let mut file = File::create(&tmp).map_err(write_error)?;
        file.write_all(&bytes).map_err(write_error)?;
        file.sync_all().map_err(write_error)?;
        drop(file);
        fs::rename(&tmp, self.target.join("state.json")).map_err(write_error)?;
        fsync_dir(&self.target)?;
        commit.bytes_written += bytes.len() as u64;
        Ok(commit)
    }
}
