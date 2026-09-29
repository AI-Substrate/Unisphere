//! Target-directory store for `unisphere prep`.
//!
//! Layout under the target directory:
//! - `state.json`: committed per-source cursors and fold checkpoints (published
//!   last, by atomic rename);
//! - `tables/{calls,turns,triggers,events,tool_uses}/run-NNNNNN.parquet`:
//!   append-only parts, one per table per run that produced rows;
//! - `tables/sources.parquet`, `tables/sessions.parquet`: current snapshots.
//!
//! Parts not referenced by `state.json` belong to an uncommitted run and are
//! removed on load, so readers never see a row twice. Superseded generations
//! stay in older parts; readers keep rows whose `generation` equals the
//! source's current generation in `sources`. The store holds an exclusive lock
//! on `TARGET/.prep.lock` for its lifetime.
#![forbid(unsafe_code)]

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_schema::{DataType, Field, Schema};
use parquet::{arrow::ArrowWriter, basic::Compression, file::properties::WriterProperties};
use serde::Serialize;
use unisphere_core::{
    PipelineError, PipelineErrorKind, SourceIdentity,
    prep::{
        PrepCommit, PrepCompactReport, PrepLoaded, PrepReplaceReason, PrepRows, PrepSourceKind,
        PrepSourceStatus, PrepState,
    },
};

pub const FACT_TABLES: [&str; 5] = ["calls", "turns", "triggers", "events", "tool_uses"];

pub struct ParquetPrepStore {
    target: PathBuf,
    /// Held for the store's lifetime; dropping it releases the target.
    _lock: File,
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
            _lock: lock,
        })
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
    let key = [
        ("source", S, false),
        ("generation", I, false),
        ("native_offset", I, true),
        ("native_key", S, true),
    ];
    let spec: Vec<(&str, DataType, bool)> = match table {
        "calls" => vec![
            ("sighting", S, false),
            ("msg_id", S, true),
            ("request_id", S, true),
            ("ts", S, true),
            ("ts_ms", I, true),
            ("model", S, true),
            ("stop_reason", S, true),
            ("input", I, true),
            ("cw_1h", I, true),
            ("cw_5m", I, true),
            ("cache_read", I, true),
            ("output", I, true),
            ("cache_write_basis", S, false),
            ("is_sidechain", B, false),
            ("gap_ms", I, true),
            ("turn_no", I, true),
            ("call_in_turn", I, true),
            ("records", I, false),
        ],
        "turns" => vec![
            ("turn_no", I, false),
            ("started_ts", S, true),
            ("started_ts_ms", I, true),
            ("first_call_offset", I, true),
            ("origin", S, false),
            ("sender", S, true),
            ("pij_msg_id", S, true),
            ("opener_offset", I, true),
            ("opener_ts_ms", I, true),
            ("opener_chars", I, true),
            ("body_key", S, true),
        ],
        "triggers" => vec![
            ("ts", S, true),
            ("ts_ms", I, true),
            ("kind", S, false),
            ("sender", S, true),
            ("pij_msg_id", S, true),
            ("chars", I, false),
            ("body_key", S, true),
            ("next_turn_no", I, false),
            ("content_head", S, true),
        ],
        "events" => vec![
            ("ts", S, true),
            ("ts_ms", I, true),
            ("kind", S, false),
            ("subkind", S, true),
            ("trigger", S, true),
            ("model", S, true),
            ("pre_tokens", I, true),
            ("post_tokens", I, true),
            ("duration_ms", I, true),
            ("last_context", I, true),
            ("gap_ms", I, true),
            ("resets_at", S, true),
            ("resets_at_ms", I, true),
            ("turn_no", I, false),
            ("body_key", S, true),
        ],
        "tool_uses" => vec![
            ("sighting", S, false),
            ("tool_use_id", S, true),
            ("call_msg_id", S, true),
            ("ts", S, true),
            ("ts_ms", I, true),
            ("name", S, true),
            ("family", S, true),
            ("input_hash", S, true),
            ("input_bytes", I, true),
            ("result_offset", I, true),
            ("result_bytes", I, true),
            ("outcome", S, true),
            ("duration_ms", I, true),
            ("turn_no", I, true),
        ],
        "sources" => {
            return columns(&[
                ("source", S, false),
                ("generation", I, false),
                ("harness", S, false),
                ("label", S, false),
                ("path", S, false),
                ("file", S, false),
                ("kind", S, false),
                ("project", S, true),
                ("is_sub", B, false),
                ("agent_id", S, true),
                ("device", I, true),
                ("inode", I, true),
                ("size", I, false),
                ("mtime_ns", I, false),
                ("committed_offset", I, false),
                ("pending_tail_bytes", I, false),
                ("revision", S, true),
                ("status", S, false),
                ("replace_reason", S, true),
                ("policy", S, false),
                ("table_schema_version", I, false),
            ]);
        }
        "sessions" => {
            return columns(&[
                ("source", S, false),
                ("generation", I, false),
                ("session_id", S, true),
                ("parent_session_id", S, true),
                ("is_sidechain", B, false),
                ("agent_id", S, true),
                ("project", S, true),
                ("cwd", S, true),
                ("first_event_ts", S, true),
                ("first_event_ms", I, true),
                ("last_event_ts", S, true),
                ("last_event_ms", I, true),
                ("seat_hint", S, true),
                ("records", I, false),
                ("calls", I, false),
                ("turns", I, false),
                ("compactions_manual", I, true),
                ("compactions_auto", I, true),
                ("compactions_unknown", I, true),
                ("latest_context_total", I, true),
                ("latest_context_ms", I, true),
                ("latest_model", S, true),
                ("skipped_malformed", I, false),
                ("skipped_untimed", I, false),
                ("skipped_bad_timestamp", I, false),
            ]);
        }
        _ => unreachable!("unknown prep table"),
    };
    columns(&key.into_iter().chain(spec).collect::<Vec<_>>())
}

#[derive(Serialize)]
struct SourceRow<'a> {
    source: &'a str,
    generation: u32,
    harness: &'a str,
    label: &'a str,
    path: &'a str,
    file: &'a str,
    kind: PrepSourceKind,
    project: Option<&'a str>,
    is_sub: bool,
    agent_id: Option<&'a str>,
    device: Option<i64>,
    inode: Option<i64>,
    size: i64,
    mtime_ns: i64,
    committed_offset: i64,
    pending_tail_bytes: i64,
    revision: Option<&'a str>,
    status: &'static str,
    replace_reason: Option<PrepReplaceReason>,
    policy: &'a str,
    table_schema_version: u32,
}

#[derive(Serialize)]
struct SessionRow<'a> {
    source: &'a str,
    generation: u32,
    session_id: Option<&'a str>,
    parent_session_id: Option<&'a str>,
    is_sidechain: bool,
    agent_id: Option<&'a str>,
    project: Option<&'a str>,
    cwd: Option<&'a str>,
    first_event_ts: Option<&'a str>,
    first_event_ms: Option<i64>,
    last_event_ts: Option<&'a str>,
    last_event_ms: Option<i64>,
    seat_hint: Option<&'a str>,
    records: u64,
    calls: u64,
    turns: u64,
    compactions_manual: Option<u64>,
    compactions_auto: Option<u64>,
    compactions_unknown: Option<u64>,
    latest_context_total: Option<i64>,
    latest_context_ms: Option<i64>,
    latest_model: Option<&'a str>,
    skipped_malformed: u64,
    skipped_untimed: u64,
    skipped_bad_timestamp: u64,
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

impl ParquetPrepStore {
    fn read_state(&self) -> Result<Option<PrepState>, PipelineError> {
        match fs::read(self.target.join("state.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| PipelineError::new(PipelineErrorKind::InvalidData, None)),
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(PipelineError::new(PipelineErrorKind::Read, None)),
        }
    }
}

impl unisphere_core::prep::PrepStore for ParquetPrepStore {
    fn state(&self) -> Result<Option<PrepState>, PipelineError> {
        self.read_state()
    }

    fn load(&self) -> Result<PrepLoaded, PipelineError> {
        let state = self.read_state()?;
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
        Ok(PrepLoaded {
            state,
            orphans_removed: removed,
        })
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
        part!("tool_uses", &rows.tool_uses);
        fs::create_dir_all(&tables).map_err(write_error)?;
        let as_i64 = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
        let mut sources = Vec::with_capacity(state.sources.len());
        let mut sessions = Vec::with_capacity(state.sources.len());
        for (key, source) in &state.sources {
            let set = state.sets.get(&source.set);
            let (device, inode) = match source.identity {
                SourceIdentity::Unix { device, inode } => {
                    (Some(as_i64(device)), Some(as_i64(inode)))
                }
                SourceIdentity::Unavailable => (None, None),
            };
            let replace_reason = match source.status {
                PrepSourceStatus::Replaced { reason } => Some(reason),
                _ => None,
            };
            sources.push(SourceRow {
                source: key,
                generation: source.generation,
                harness: set.map_or("", |s| s.harness.as_str()),
                label: set.map_or("", |s| s.label.as_str()),
                path: source.path.to_str().unwrap_or_default(),
                file: &source.file,
                kind: source.kind,
                project: source.meta.project.as_deref(),
                is_sub: source.meta.is_sub,
                agent_id: source.meta.agent_id.as_deref(),
                device,
                inode,
                size: as_i64(source.size),
                mtime_ns: i64::try_from(source.mtime_ns).unwrap_or(i64::MAX),
                committed_offset: as_i64(source.offset),
                pending_tail_bytes: as_i64(source.size.saturating_sub(source.offset)),
                revision: source.revision.as_deref(),
                status: source.status.label(),
                replace_reason,
                policy: &source.checkpoint.policy,
                table_schema_version: state.table_schema_version,
            });
            let facts = &source.facts;
            let latest = facts.latest_context.as_ref();
            sessions.push(SessionRow {
                source: key,
                generation: source.generation,
                session_id: facts.session_id.as_deref(),
                parent_session_id: facts.parent_session_id.as_deref(),
                is_sidechain: facts.is_sidechain,
                agent_id: source.meta.agent_id.as_deref(),
                project: source.meta.project.as_deref(),
                cwd: facts.cwd.as_deref(),
                first_event_ts: facts.first_event_ts.as_deref(),
                first_event_ms: facts.first_event_ms,
                last_event_ts: facts.last_event_ts.as_deref(),
                last_event_ms: facts.last_event_ms,
                seat_hint: facts.seat_hint.as_deref(),
                records: facts.records,
                calls: facts.calls,
                turns: facts.turns,
                compactions_manual: facts.compactions.map(|c| c.manual),
                compactions_auto: facts.compactions.map(|c| c.auto),
                compactions_unknown: facts.compactions.map(|c| c.unknown_trigger),
                latest_context_total: latest.and_then(|c| c.total),
                latest_context_ms: latest.and_then(|c| c.ts_ms),
                latest_model: latest.and_then(|c| c.model.as_deref()),
                skipped_malformed: facts.skipped.malformed,
                skipped_untimed: facts.skipped.untimed,
                skipped_bad_timestamp: facts.skipped.bad_timestamp,
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

    fn compact(&self) -> Result<PrepCompactReport, PipelineError> {
        // Compaction is delivered by the store lane (guide unit tk-0004).
        Err(PipelineError::new(PipelineErrorKind::Unsupported, None))
    }
}
