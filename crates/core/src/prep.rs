//! Incremental "prep" contracts: canonical metadata tables derived from native
//! sessions and the caller-owned state that makes re-runs idempotent.
//!
//! Pure contracts only. A [`PrepLoader`] discovers, stats and reads native
//! sources read-only; a pure [`PrepFold`] interprets the supplied input into rows
//! and [`SessionFacts`]; a [`PrepStore`] durably commits rows before the state
//! that references them. The SDK owns every decision between those ports. No
//! content is emitted unless the caller explicitly opts in.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
    NativeRecord, NativeSnapshot, PipelineError, ReadCursor, ReadLimits, SnapshotLimits,
    SourceIdentity,
};

/// Physical layout/column contract of the published tables.
pub const PREP_TABLE_SCHEMA_VERSION: u32 = 2;
/// Envelope format of [`PrepCheckpoint`]; independent of any fold's policy.
pub const PREP_CHECKPOINT_FORMAT: u32 = 1;
/// Label of the adapter catalogue's default root for a harness.
pub const DEFAULT_ROOT_LABEL: &str = "default";

// ---------------------------------------------------------------------------
// Sources and discovery
// ---------------------------------------------------------------------------

/// Native representation, which decides the change-detection strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrepSourceKind {
    /// LF-framed records appended over time; resumed from a byte cursor.
    Append,
    /// A whole document/journal/database; any change is a new generation.
    Snapshot,
}

/// One explicit discovery root for one harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepSourceSet {
    pub harness: String,
    /// [`DEFAULT_ROOT_LABEL`] for the catalogue root, otherwise an explicit or
    /// [`derived_root_label`] label. Part of every source key.
    pub label: String,
    pub root: PathBuf,
}

impl PrepSourceSet {
    /// `<harness>/<label>`: the key of this set in [`PrepState::sets`].
    pub fn key(&self) -> String {
        format!("{}/{}", self.harness, self.label)
    }

    /// `<harness>/<label>/<file>`: the stable source key used by every table.
    pub fn source_key(&self, file: &str) -> String {
        format!("{}/{}/{}", self.harness, self.label, file)
    }
}

/// Deterministic label for an explicit root without a caller-chosen label:
/// `root-` followed by the first 8 hex digits of SHA-256 over the path bytes.
pub fn derived_root_label(root: &Path) -> String {
    let digest = Sha256::digest(root.to_string_lossy().as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("root-{hex}")
}

/// One discovered native source, observed by `stat` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepSourceStat {
    pub path: PathBuf,
    /// Path relative to the discovery root, `/`-separated.
    pub file: String,
    pub kind: PrepSourceKind,
    pub identity: SourceIdentity,
    /// Snapshot loaders fold sidecars (SQLite `-wal`/`-shm`) into size and mtime.
    pub size: u64,
    pub mtime_ns: i128,
}

/// Entries discovery saw but did not return as sources. Nothing is silent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepSkipCounts {
    pub symlinks: u64,
    pub hidden: u64,
    pub unreadable_entries: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepDiscovery {
    /// Sorted by `file` (byte order).
    pub sources: Vec<PrepSourceStat>,
    pub skipped: PrepSkipCounts,
}

/// Where one native record lives inside its source: a byte offset for append
/// sources, a snapshot key for snapshot sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeAddress {
    pub offset: Option<u64>,
    pub key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrepReadLimits {
    pub read: ReadLimits,
    pub snapshot: SnapshotLimits,
}

/// Native input supplied to a fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepInput {
    Records(Vec<NativeRecord>),
    Snapshot(NativeSnapshot),
}

/// One bounded read. Append reads end at the last complete LF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepBatch {
    pub input: PrepInput,
    /// Append: the cursor after this batch. Snapshot: `None`.
    pub next_cursor: Option<ReadCursor>,
    pub more: bool,
    /// Append: bytes after the last complete LF were left for a later run.
    pub incomplete_tail: bool,
    pub bytes_read: u64,
}

/// Storage port for one representation. Native stores are opened read-only and
/// are never locked, renamed or written.
pub trait PrepLoader: Send + Sync {
    fn kind(&self) -> PrepSourceKind;
    /// Recursive discovery under `root`; `accept` receives the relative path.
    /// Symlinks are never followed and hidden entries are skipped, both counted.
    fn discover(
        &self,
        root: &Path,
        accept: &dyn Fn(&str) -> bool,
    ) -> Result<PrepDiscovery, PipelineError>;
    /// Stat one explicit source below `root` (single-source callers).
    fn stat(&self, root: &Path, path: &Path) -> Result<PrepSourceStat, PipelineError>;
    /// Append: read from `from` (or the start) to the last complete LF, bounded.
    /// Snapshot: `from` must be `None`; returns the whole bounded snapshot.
    fn read(
        &self,
        stat: &PrepSourceStat,
        from: Option<&ReadCursor>,
        limits: PrepReadLimits,
    ) -> Result<PrepBatch, PipelineError>;
    /// Append only: digest of the committed prefix `[0, offset)` that detects
    /// same-identity rewrites. Snapshot loaders return `Unsupported`.
    fn anchor(&self, stat: &PrepSourceStat, offset: u64) -> Result<String, PipelineError>;
    /// Exactly one native record at `address`, at most `max_bytes`. Content:
    /// callers must hold an explicit opt-in before calling.
    fn record_at(
        &self,
        path: &Path,
        address: &NativeAddress,
        max_bytes: usize,
    ) -> Result<Vec<u8>, PipelineError>;
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallSighting {
    /// Carries the ordering facts (timestamp, turn placement, gap).
    First,
    /// Raises counters of a call first committed by an earlier batch or run;
    /// canonical readers take the per-field maximum.
    Update,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheWriteBasis {
    /// Native 1 h / 5 m split.
    Split,
    /// Aggregate only, attributed to 1 h (main sessions).
    #[serde(rename = "fallback_1h")]
    Fallback1h,
    /// Aggregate only, attributed to 5 m (subagents).
    #[serde(rename = "fallback_5m")]
    Fallback5m,
    /// No cache-write field recorded.
    None,
}

/// Opening-record origin of a turn (and kind of a trigger). Values match the
/// reference RCA parser's vocabulary; other dialects map into it or use `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TurnOrigin {
    Start,
    Human,
    Peer,
    TaskNotification,
    Coordinator,
    AutoContinuation,
    CompactSummary,
    ManualCompact,
    Scheduled,
    Loop,
    SubagentTask,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrepEventKind {
    Compaction,
    Recap,
    ScheduledFire,
    LimitNotice,
    QueueOp,
    ModelSwitch,
    ApiError,
    SystemOther,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSighting {
    Use,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutcome {
    Ok,
    Error,
    Unknown,
}

/// One deduplicated API call sighting. Every row starts with
/// `(source, generation, native_offset, native_key)`. Every field a dialect may
/// not record is nullable; null means "not recorded", never zero or an estimate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepCallRow {
    pub source: String,
    pub generation: u32,
    pub native_offset: Option<u64>,
    pub native_key: Option<String>,
    pub sighting: CallSighting,
    pub msg_id: Option<String>,
    pub request_id: Option<String>,
    /// Native timestamp; null when the dialect records none.
    pub ts: Option<String>,
    pub ts_ms: Option<i64>,
    pub model: Option<String>,
    /// Native stop reason; the last non-null value across sightings.
    pub stop_reason: Option<String>,
    pub input: Option<i64>,
    pub cw_1h: Option<i64>,
    pub cw_5m: Option<i64>,
    pub cache_read: Option<i64>,
    pub output: Option<i64>,
    pub cache_write_basis: CacheWriteBasis,
    pub is_sidechain: bool,
    /// Milliseconds since the previous new call in the same source; -1 for the first.
    pub gap_ms: Option<i64>,
    pub turn_no: Option<i64>,
    pub call_in_turn: Option<i64>,
    /// Native records merged into this sighting.
    pub records: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepTurnRow {
    pub source: String,
    pub generation: u32,
    pub native_offset: Option<u64>,
    pub native_key: Option<String>,
    pub turn_no: i64,
    pub started_ts: Option<String>,
    pub started_ts_ms: Option<i64>,
    pub first_call_offset: Option<u64>,
    pub origin: TurnOrigin,
    pub sender: Option<String>,
    pub pij_msg_id: Option<String>,
    pub opener_offset: Option<u64>,
    pub opener_ts_ms: Option<i64>,
    pub opener_chars: Option<i64>,
    pub body_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepTriggerRow {
    pub source: String,
    pub generation: u32,
    pub native_offset: Option<u64>,
    pub native_key: Option<String>,
    pub ts: Option<String>,
    pub ts_ms: Option<i64>,
    pub kind: TurnOrigin,
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
    pub source: String,
    pub generation: u32,
    pub native_offset: Option<u64>,
    pub native_key: Option<String>,
    pub ts: Option<String>,
    pub ts_ms: Option<i64>,
    pub kind: PrepEventKind,
    pub subkind: Option<String>,
    /// Compaction trigger (`manual`/`auto`) where native.
    pub trigger: Option<String>,
    /// `model_switch`: the requested model.
    pub model: Option<String>,
    pub pre_tokens: Option<i64>,
    /// Native post-compaction tokens only; never estimated.
    pub post_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
    pub last_context: Option<i64>,
    pub gap_ms: Option<i64>,
    /// Native reset phrase of a limit notice.
    pub resets_at: Option<String>,
    /// Parsed reset instant, only when the source allows it.
    pub resets_at_ms: Option<i64>,
    pub turn_no: i64,
    pub body_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepToolUseRow {
    pub source: String,
    pub generation: u32,
    pub native_offset: Option<u64>,
    pub native_key: Option<String>,
    pub sighting: ToolSighting,
    pub tool_use_id: Option<String>,
    pub call_msg_id: Option<String>,
    pub ts: Option<String>,
    pub ts_ms: Option<i64>,
    /// Native tool name.
    pub name: Option<String>,
    pub family: Option<String>,
    pub input_hash: Option<String>,
    pub input_bytes: Option<i64>,
    pub result_offset: Option<u64>,
    pub result_bytes: Option<i64>,
    pub outcome: Option<ToolOutcome>,
    /// Native duration only.
    pub duration_ms: Option<i64>,
    pub turn_no: Option<i64>,
}

/// Rows produced by one fold call, in native order within each table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrepRows {
    pub calls: Vec<PrepCallRow>,
    pub turns: Vec<PrepTurnRow>,
    pub triggers: Vec<PrepTriggerRow>,
    pub events: Vec<PrepEventRow>,
    pub tool_uses: Vec<PrepToolUseRow>,
}

impl PrepRows {
    pub fn len(&self) -> usize {
        self.calls.len()
            + self.turns.len()
            + self.triggers.len()
            + self.events.len()
            + self.tool_uses.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn extend(&mut self, other: PrepRows) {
        self.calls.extend(other.calls);
        self.turns.extend(other.turns);
        self.triggers.extend(other.triggers);
        self.events.extend(other.events);
        self.tool_uses.extend(other.tool_uses);
    }
    pub fn counts(&self) -> PrepTableCounts {
        PrepTableCounts {
            calls: self.calls.len() as u64,
            turns: self.turns.len() as u64,
            triggers: self.triggers.len() as u64,
            events: self.events.len() as u64,
            tool_uses: self.tool_uses.len() as u64,
        }
    }
}

// ---------------------------------------------------------------------------
// Session facts (reused without a target directory)
// ---------------------------------------------------------------------------

/// Latest deduplicated main-chain call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSample {
    pub ts_ms: Option<i64>,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub input: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
    /// `input + cache_read + cache_write` when all are known.
    pub total: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionCounts {
    pub manual: u64,
    pub auto: u64,
    pub unknown_trigger: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionSample {
    pub ts_ms: Option<i64>,
    pub trigger: Option<String>,
    pub pre_tokens: Option<i64>,
    /// Native post-compaction tokens only.
    pub post_tokens: Option<i64>,
    /// Context of the first new main-chain call after the boundary; `None`
    /// until one exists. Observed, never estimated.
    pub first_context_after: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSwitch {
    pub ts_ms: Option<i64>,
    pub requested_model: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSkips {
    pub malformed: u64,
    pub untimed: u64,
    pub bad_timestamp: u64,
}

/// Session-level facts accumulated by a fold over one source generation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFacts {
    pub session_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub is_sidechain: bool,
    pub cwd: Option<String>,
    /// Latest/earliest native event timestamps; never file mtime.
    pub first_event_ts: Option<String>,
    pub first_event_ms: Option<i64>,
    pub last_event_ts: Option<String>,
    pub last_event_ms: Option<i64>,
    pub records: u64,
    pub calls: u64,
    pub turns: u64,
    pub latest_context: Option<ContextSample>,
    /// `None` only when the dialect has no native compaction marker.
    pub compactions: Option<CompactionCounts>,
    pub last_compaction: Option<CompactionSample>,
    pub last_model_switch: Option<ModelSwitch>,
    pub seat_hint: Option<String>,
    pub skipped: SessionSkips,
}

// ---------------------------------------------------------------------------
// Pure fold
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepOptions {
    pub include_content: bool,
}

/// Path-derived facts a fold knows before reading any byte.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepSourceMeta {
    pub is_sub: bool,
    pub agent_id: Option<String>,
    pub project: Option<String>,
}

/// Serialisable, versioned fold state owned by the caller, committed with its cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepCheckpoint {
    /// [`PREP_CHECKPOINT_FORMAT`] at write time.
    pub format: u32,
    /// The writing fold's [`PrepFold::policy`].
    pub policy: String,
    pub fold: Value,
}

/// Pure, deterministic interpretation of supplied native input into rows.
/// Equal (checkpoint, input, options) give equal output; any batch split or a
/// resume from a checkpoint equals one uninterrupted fold. No I/O, clock or
/// environment.
pub trait PrepFold: Send + Sync {
    fn harness(&self) -> &'static str;
    /// Versioned interpretation policy; a change re-emits every source of the harness.
    fn policy(&self) -> &'static str;
    fn kind(&self) -> PrepSourceKind;
    /// Glob, relative to a root, selecting this dialect's sources.
    fn pattern(&self) -> &'static str;
    fn describe(&self, file: &str) -> PrepSourceMeta;
    /// Start a generation (`saved = None`) or resume one. A checkpoint with a
    /// different format or policy is refused with `InvalidData`.
    fn open(
        &self,
        meta: &PrepSourceMeta,
        source: &str,
        generation: u32,
        saved: Option<&PrepCheckpoint>,
    ) -> Result<Box<dyn PrepFoldSession>, PipelineError>;
}

/// In-memory fold over one source generation.
pub trait PrepFoldSession: Send {
    fn fold(&mut self, input: &PrepInput, options: PrepOptions) -> Result<PrepRows, PipelineError>;
    fn checkpoint(&self) -> PrepCheckpoint;
    fn facts(&self) -> SessionFacts;
}

// ---------------------------------------------------------------------------
// State, outcomes, store
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrepReplaceReason {
    /// Identity (device/inode) changed.
    Rotated,
    /// Shorter than the committed cursor.
    Truncated,
    /// Same identity but the committed prefix changed (anchor mismatch).
    Rewritten,
    /// Snapshot revision changed.
    Revision,
    /// The harness fold policy changed.
    Policy,
    /// The table schema or checkpoint format changed.
    Schema,
    /// The set's root path changed.
    RootMoved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PrepSourceStatus {
    New,
    Unchanged,
    Appended,
    Replaced {
        reason: PrepReplaceReason,
    },
    /// Discovered but excluded by an explicit scope (`modified_since`); not read,
    /// committed state retained.
    Skipped,
    Unreadable,
    Unsupported,
    Missing,
}

impl PrepSourceStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Unchanged => "unchanged",
            Self::Appended => "appended",
            Self::Replaced { .. } => "replaced",
            Self::Skipped => "skipped",
            Self::Unreadable => "unreadable",
            Self::Unsupported => "unsupported",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepSetState {
    pub harness: String,
    pub label: String,
    pub root: PathBuf,
    pub policy: String,
}

/// Durable per-source state committed with (after) its rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepSourceState {
    /// Key of the owning set in [`PrepState::sets`].
    pub set: String,
    pub path: PathBuf,
    pub file: String,
    pub kind: PrepSourceKind,
    pub identity: SourceIdentity,
    pub size: u64,
    pub mtime_ns: i128,
    /// Append: committed byte offset after the last complete LF.
    pub offset: u64,
    /// Append: [`PrepLoader::anchor`] at `offset`.
    pub anchor: Option<String>,
    /// Snapshot: the committed `NativeSnapshot::revision`.
    pub revision: Option<String>,
    pub generation: u32,
    pub meta: PrepSourceMeta,
    pub checkpoint: PrepCheckpoint,
    pub facts: SessionFacts,
    pub status: PrepSourceStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepState {
    pub table_schema_version: u32,
    pub checkpoint_format: u32,
    pub runs: u64,
    /// Part files (relative to the target) that belong to committed runs.
    pub parts: Vec<String>,
    pub sets: BTreeMap<String, PrepSetState>,
    /// Keyed by source key.
    pub sources: BTreeMap<String, PrepSourceState>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepTableCounts {
    pub calls: u64,
    pub turns: u64,
    pub triggers: u64,
    pub events: u64,
    pub tool_uses: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrepCommit {
    pub parts_written: Vec<String>,
    pub bytes_written: u64,
    pub snapshot_rows: u64,
    pub orphans_removed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepLoaded {
    pub state: Option<PrepState>,
    pub orphans_removed: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrepCompactReport {
    pub target: PathBuf,
    pub parts_before: u64,
    pub parts_after: u64,
    pub rows_before: PrepTableCounts,
    pub rows_after: PrepTableCounts,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

/// Durable target. An implementation excludes concurrent writers for its lifetime.
pub trait PrepStore: Send + Sync {
    /// Read the committed state without recovering or writing anything.
    fn state(&self) -> Result<Option<PrepState>, PipelineError>;
    /// Load committed state, discarding parts a crashed run left unreferenced.
    fn load(&self) -> Result<PrepLoaded, PipelineError>;
    /// Write the run's rows durably, then atomically publish the new state.
    fn commit(&self, rows: &PrepRows, state: &PrepState) -> Result<PrepCommit, PipelineError>;
    /// Rewrite current-generation rows and drop superseded ones without changing
    /// any canonical view result.
    fn compact(&self) -> Result<PrepCompactReport, PipelineError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrepSourceOutcome {
    pub source: String,
    pub status: PrepSourceStatus,
    pub generation: u32,
    pub bytes_read: u64,
    pub rows: u64,
    pub committed_offset: u64,
    pub pending_tail_bytes: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrepSetReport {
    pub harness: String,
    pub label: String,
    pub root: PathBuf,
    pub policy: Option<String>,
    /// Whether a fold/loader binding exists for this harness.
    pub supported: bool,
    pub discovered: u64,
    pub skipped: PrepSkipCounts,
    /// Keyed by [`PrepSourceStatus::label`].
    pub by_status: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrepReport {
    pub target: PathBuf,
    pub table_schema_version: u32,
    pub run: u64,
    pub sets: Vec<PrepSetReport>,
    pub bytes_read: u64,
    pub pending_tail_bytes: u64,
    pub rows_written: PrepTableCounts,
    pub commit: PrepCommit,
    /// Every source whose status is neither `unchanged` nor `skipped`
    /// (those are counted per set in `by_status`).
    pub sources: Vec<PrepSourceOutcome>,
}

// ---------------------------------------------------------------------------
// Application port
// ---------------------------------------------------------------------------

/// One explicit prep invocation; the shell resolves every path and bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepRequest {
    pub target: PathBuf,
    pub roots: Vec<PrepSourceSet>,
    pub options: PrepOptions,
    pub limits: PrepReadLimits,
    pub threads: usize,
    /// Discovery scope: ignore sources whose mtime is older (Unix nanoseconds).
    pub modified_since_ns: Option<i128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepCompactRequest {
    pub target: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepRecordRequest {
    pub target: PathBuf,
    /// Source key.
    pub source: String,
    pub address: NativeAddress,
    pub include_content: bool,
    pub max_bytes: usize,
}

/// One native record fetched by address. Content: returned only on explicit opt-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepRecord {
    pub source: String,
    pub path: PathBuf,
    pub address: NativeAddress,
    pub bytes: Vec<u8>,
}

/// Core-owned application port used by the CLI frontend.
pub trait PrepApi: Send + Sync {
    fn prep(&self, request: &PrepRequest) -> Result<PrepReport, PipelineError>;
    fn compact(&self, request: &PrepCompactRequest) -> Result<PrepCompactReport, PipelineError>;
    /// Refused with `InvalidInput` before any native read unless `include_content`.
    fn record(&self, request: &PrepRecordRequest) -> Result<PrepRecord, PipelineError>;
}
