//! Incremental "prep" contracts: canonical metadata tables derived from native
//! sessions and the caller-owned state that makes re-runs idempotent.
//!
//! Pure contracts only. Loaders stat and read, a pure [`PrepFold`] interprets the
//! supplied records into facts, and a [`PrepStore`] durably commits rows before
//! the matching cursor. No content is emitted unless the fold is asked for it.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{NativeRecord, PipelineError, ReadCursor, ReadLimits, SessionRef, SourceIdentity};

/// Physical layout/column contract of the target directory tables.
pub const PREP_TABLE_SCHEMA_VERSION: u32 = 1;

/// One discovered native source, observed by `stat` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepSourceStat {
    pub path: PathBuf,
    /// Path relative to the discovery root; the stable source key in every table.
    pub file: String,
    pub identity: SourceIdentity,
    pub size: u64,
    pub mtime_ns: i128,
}

/// Structural facts the shell knows about a source before any byte is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepSourceMeta {
    pub file: String,
    pub is_sub: bool,
}

/// Session-level facts accumulated by a fold over a whole source generation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepSessionFacts {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    pub first_ts_ms: Option<i64>,
    pub last_ts_ms: Option<i64>,
    pub seat_hint: Option<String>,
    pub records: u64,
    pub calls: u64,
    pub skipped_malformed: u64,
    pub skipped_untimed: u64,
    pub skipped_bad_timestamp: u64,
}

/// One deduplicated API call sighting group. `sighting = "first"` rows carry
/// the ordering facts; `"update"` rows only raise counters of a call first
/// committed by an earlier run (readers take the per-field maximum).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepCallRow {
    pub file: String,
    pub generation: u32,
    pub native_offset: u64,
    pub sighting: String,
    pub msg_id: Option<String>,
    pub request_id: Option<String>,
    pub ts: String,
    pub ts_ms: i64,
    pub model: Option<String>,
    pub input: i64,
    pub cw_1h: i64,
    pub cw_5m: i64,
    pub cache_read: i64,
    pub output: i64,
    /// Milliseconds since the previous new call in the same source; -1 for the first.
    pub gap_ms: Option<i64>,
    pub turn_no: Option<i64>,
    pub call_in_turn: Option<i64>,
    pub records: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepTurnRow {
    pub file: String,
    pub generation: u32,
    pub turn_no: i64,
    pub started_ts: String,
    pub started_ts_ms: i64,
    pub first_call_offset: u64,
    pub trigger: String,
    pub sender: Option<String>,
    pub pij_msg_id: Option<String>,
    pub opener_offset: Option<u64>,
    pub opener_ts_ms: Option<i64>,
    pub opener_chars: Option<i64>,
    pub body_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepTriggerRow {
    pub file: String,
    pub generation: u32,
    pub native_offset: u64,
    pub ts: String,
    pub ts_ms: i64,
    pub kind: String,
    pub sender: Option<String>,
    pub pij_msg_id: Option<String>,
    pub chars: i64,
    pub body_key: Option<String>,
    /// Turn number this trigger would open; the turns table says whether it did.
    pub next_turn_no: i64,
    /// Opt-in only (`include_content`): the first 200 characters of the normalised body.
    pub content_head: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepEventRow {
    pub file: String,
    pub generation: u32,
    pub native_offset: u64,
    pub ts: String,
    pub ts_ms: i64,
    pub kind: String,
    pub subkind: Option<String>,
    pub pre_tokens: Option<i64>,
    pub post_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub last_context: Option<i64>,
    pub gap_ms: Option<i64>,
    pub resets_at: Option<String>,
    pub turn_no: i64,
    pub body_key: Option<String>,
}

/// Rows produced by one fold call, in native order within each table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrepRows {
    pub calls: Vec<PrepCallRow>,
    pub turns: Vec<PrepTurnRow>,
    pub triggers: Vec<PrepTriggerRow>,
    pub events: Vec<PrepEventRow>,
}

impl PrepRows {
    pub fn len(&self) -> usize {
        self.calls.len() + self.turns.len() + self.triggers.len() + self.events.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn extend(&mut self, other: PrepRows) {
        self.calls.extend(other.calls);
        self.turns.extend(other.turns);
        self.triggers.extend(other.triggers);
        self.events.extend(other.events);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepOptions {
    pub include_content: bool,
}

/// Opaque, serialisable per-source fold state owned by the caller, like a cursor.
pub type PrepFoldState = Value;

/// Pure, deterministic interpretation of supplied native records into table rows.
/// Equal (state, records, options) produce equal output. No I/O, clock or environment.
pub trait PrepFold: Send + Sync {
    fn harness(&self) -> &'static str;
    /// Versioned interpretation policy; a change forces every source to re-emit.
    fn policy(&self) -> &'static str;
    fn is_sub(&self, file: &str) -> bool;
    /// Start a generation (`saved = None`) or resume one from a committed state.
    fn open(
        &self,
        meta: &PrepSourceMeta,
        generation: u32,
        saved: Option<&PrepFoldState>,
    ) -> Result<Box<dyn PrepFoldSession>, PipelineError>;
}

/// In-memory fold over one source generation; pure over the supplied records.
pub trait PrepFoldSession: Send {
    /// Fold one bounded batch. Repeats of a call first seen in an earlier batch or
    /// run become `update` rows; the state carries every committed call key.
    fn fold(
        &mut self,
        records: &[NativeRecord],
        options: PrepOptions,
    ) -> Result<PrepRows, PipelineError>;
    fn save(&self) -> PrepFoldState;
    fn session(&self) -> PrepSessionFacts;
}

/// Storage port for discovery, stat and bounded reads of one harness root.
pub trait PrepLoader: Send + Sync {
    fn discover(&self, root: &std::path::Path) -> Result<Vec<PrepSourceStat>, PipelineError>;
    fn read_batch(
        &self,
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
    ) -> Result<crate::LoadedBatch, PipelineError>;
    /// Digest of the committed prefix's head and the bytes just before `offset`.
    /// Detects same-inode rewrites that keep an LF at the cursor.
    fn anchor(&self, path: &std::path::Path, offset: u64) -> Result<String, PipelineError>;
}

/// Durable per-source state committed with (after) its rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepSourceState {
    pub path: PathBuf,
    pub identity: SourceIdentity,
    pub size: u64,
    pub mtime_ns: i128,
    pub offset: u64,
    pub anchor: Option<String>,
    pub generation: u32,
    pub is_sub: bool,
    pub fold: PrepFoldState,
    pub session: PrepSessionFacts,
    pub last_status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepState {
    pub table_schema_version: u32,
    pub harness: String,
    pub policy: String,
    pub root: PathBuf,
    pub runs: u64,
    /// Part files (relative to the target) that belong to committed runs.
    pub parts: Vec<String>,
    pub sources: BTreeMap<String, PrepSourceState>,
}

/// Row counts written by one committed run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepTableCounts {
    pub calls: u64,
    pub turns: u64,
    pub triggers: u64,
    pub events: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrepCommit {
    pub parts_written: Vec<String>,
    pub bytes_written: u64,
    pub snapshot_rows: u64,
    pub orphans_removed: u64,
}

pub trait PrepStore: Send + Sync {
    /// Load committed state, discarding part files a crashed run left unreferenced.
    fn load(&self) -> Result<(Option<PrepState>, u64), PipelineError>;
    /// Write the run's rows durably, then atomically publish the new state.
    fn commit(&self, rows: &PrepRows, state: &PrepState) -> Result<PrepCommit, PipelineError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepSourceOutcome {
    pub file: String,
    /// new | unchanged | appended | replaced | policy | failed
    pub status: String,
    pub generation: u32,
    pub bytes_read: u64,
    pub rows: u64,
    pub committed_offset: u64,
    pub pending_tail_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrepReport {
    pub target: PathBuf,
    pub root: PathBuf,
    pub harness: String,
    pub policy: String,
    pub table_schema_version: u32,
    pub run: u64,
    pub sources_discovered: u64,
    pub sources_by_status: BTreeMap<String, u64>,
    pub bytes_read: u64,
    pub rows_written: PrepTableCounts,
    pub pending_tail_bytes: u64,
    pub commit: PrepCommit,
    /// Only sources that were read, changed or failed; unchanged ones are counted above.
    pub sources: Vec<PrepSourceOutcome>,
}

/// One explicit prep invocation; the shell resolves every path and bound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepRequest {
    pub target: PathBuf,
    pub root: PathBuf,
    pub options: PrepOptions,
    pub limits: ReadLimits,
    pub threads: usize,
    /// Discovery scope: ignore sources whose mtime is older (Unix nanoseconds).
    pub modified_since_ns: Option<i128>,
}

/// Core-owned application port used by the CLI frontend.
pub trait PrepApi: Send + Sync {
    fn prep(&self, request: &PrepRequest) -> Result<PrepReport, PipelineError>;
}
