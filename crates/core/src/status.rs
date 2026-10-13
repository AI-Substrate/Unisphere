//! Session status contracts: one harness-neutral shape of general facts about
//! one agent session.
//!
//! Pure contracts only. A [`SessionStatusApi`] answers for an explicit
//! [`StatusTarget`]; a [`TargetResolver`] maps a Pij id or tmux pane to a
//! target. Every fact names its [`Basis`]; a fact the harness does not record is
//! `None` and listed in [`SessionStatus::unknown`], never reported as zero.
//! Time is injected (`now_ms`), never read from a clock here.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

pub use crate::prep::{CompactionCounts, CompactionSample};

pub const STATUS_SCHEMA_VERSION: u32 = 1;
/// Version label of the model → context-window table used for `Basis::Table`.
pub const MODEL_WINDOWS_TABLE: &str = "model-windows@1";

/// The closed vocabulary of [`SessionStatus::unknown`] names. An empty status
/// lists all of them; the SDK removes each name as it fills that fact.
pub const UNKNOWN_FACTS: &[&str] = &[
    "model.current",
    "model.pending_switch",
    "context.used_tokens",
    "context.window_tokens",
    "context.percent",
    "last_call",
    "last_call.ttl_bucket",
    "last_call.cache_warm",
    "last_call.stop_reason",
    "last_call.cache_write_other",
    "last_call.cache_write_ttl_s",
    "last_call.selected_model",
    "last_call.native_cost",
    "last_call.native_cost_unit",
    "timeline.created_ms",
    "timeline.last_updated_ms",
    "compaction.counts",
    "compaction.last",
];

/// Where a fact came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    /// Recorded by the harness.
    Native,
    /// Computed from native facts (and injected `now_ms`).
    Derived,
    /// Looked up in a versioned table (see `ContextStatus::window_table`).
    Table,
    /// File modification time, used only when no native timestamp exists.
    MtimeFallback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact<T> {
    pub value: T,
    pub basis: Basis,
}

impl<T> Fact<T> {
    pub fn new(value: T, basis: Basis) -> Self {
        Self { value, basis }
    }
}

// ---------------------------------------------------------------------------
// Targets and resolution
// ---------------------------------------------------------------------------

/// An explicit session: adapter id (e.g. `claude-code`), native session id and
/// optionally the transcript path. The SDK needs nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusTarget {
    pub harness: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<PathBuf>,
}

/// What a caller asked for. Only the CLI layer resolves `Pij`/`Pane`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusQuery {
    Target(StatusTarget),
    Pij(String),
    Pane(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveBasis {
    Explicit,
    PijRegistry,
    NativePane,
}

/// A second answer that disagreed with the chosen one; returned, never hidden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveConflict {
    pub basis: ResolveBasis,
    pub target: StatusTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    pub query: StatusQuery,
    pub target: StatusTarget,
    pub pij_id: Option<String>,
    pub pane: Option<String>,
    pub basis: ResolveBasis,
    #[serde(default)]
    pub conflicts: Vec<ResolveConflict>,
}

// ---------------------------------------------------------------------------
// Facts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSwitchStatus {
    /// Requested model as the harness recorded it (may be a display name).
    pub requested: String,
    pub at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSpan {
    pub model: String,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    pub calls: u64,
}

/// Most served models [`SessionStatus::models`] lists (latest first).
pub const MAX_STATUS_MODELS: usize = 64;

/// Cache facts of one model that served main-chain calls. A model router may
/// change the served model on every call, and each model keeps its own prompt
/// cache, so a session can be warm on one model and cold on another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCacheStatus {
    pub model: String,
    /// Main-chain calls this model served.
    pub calls: u64,
    pub last_call_at_ms: Option<i64>,
    /// TTL of its latest call's cache write: native when recorded, else
    /// derived from a native 1 h/5 m split.
    pub cache_ttl_s: Option<Fact<u64>>,
    /// Native expiry recorded with its latest call, else derived
    /// `last_call_at_ms + cache_ttl_s`.
    pub cache_expires_ms: Option<Fact<i64>>,
    /// `now < cache_expires_ms`.
    pub cache_warm: Option<Fact<bool>>,
    /// Its latest call's uncached input + cache read + cache writes.
    pub last_context: Option<Fact<u64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelStatus {
    /// Model of the latest main-chain, non-synthetic call.
    pub current: Option<Fact<String>>,
    pub current_at_ms: Option<i64>,
    /// A model switch recorded after the latest call; the next call uses it.
    pub pending_switch: Option<ModelSwitchStatus>,
    /// Main-chain model changes in order.
    pub history: Vec<ModelSpan>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextStatus {
    /// Latest main-chain call: input + cache read + cache write.
    pub used_tokens: Option<Fact<u64>>,
    pub window_tokens: Option<Fact<u64>>,
    /// Set when `window_tokens.basis` is `Table`, e.g. `model-windows@1`.
    pub window_table: Option<String>,
    /// Only when both used and window are known.
    pub percent: Option<f64>,
    /// Human form, e.g. `250k of 1M (25%)`.
    pub display: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LastCall {
    pub at_ms: Option<i64>,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write_1h: Option<u64>,
    pub cache_write_5m: Option<u64>,
    /// `1h` or `5m`, from where the last call wrote cache.
    pub ttl_bucket: Option<Fact<String>>,
    /// `now - at < ttl`.
    pub cache_warm: Option<Fact<bool>>,
    pub stop_reason: Option<String>,
    /// Cache-write tokens whose native TTL is neither 1 h nor 5 m (e.g. a
    /// Copilot GPT model's 1800 s); `Derived` 0 when the write's TTL was not
    /// recorded and the documented 1 h/5 m fallback took it.
    pub cache_write_other: Option<Fact<u64>>,
    /// Native TTL, in seconds, of the last call's cache write.
    pub cache_write_ttl_s: Option<Fact<u64>>,
    /// The model the session selected when a model router chose the model
    /// that ran (`model.current`), e.g. `hydrafusion`.
    pub selected_model: Option<Fact<String>>,
    /// Native cost of the last call in `native_cost_unit`; never money.
    pub native_cost: Option<Fact<u64>>,
    /// Unit of `native_cost`, e.g. `nano_aiu`.
    pub native_cost_unit: Option<Fact<String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Timeline {
    pub created_ms: Option<Fact<i64>>,
    /// Latest native event; file mtime only as a labelled fallback.
    pub last_updated_ms: Option<Fact<i64>>,
    pub idle_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnStatus {
    pub total: u64,
    pub last_hour_total: u64,
    /// Keyed by turn origin (`human`, `peer`, `task-notification`, ...).
    pub by_origin: BTreeMap<String, u64>,
    pub last_hour_by_origin: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionStatus {
    /// `None` only when the harness has no native compaction marker.
    pub counts: Option<CompactionCounts>,
    pub last: Option<CompactionSample>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallCounts {
    pub total: u64,
    pub sidechain: u64,
}

/// A usage-limit notice the transcript recorded (after the limit was hit).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitSeen {
    pub kind: String,
    pub at_ms: Option<i64>,
    /// Native reset phrase, e.g. `3pm`.
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStatus {
    pub bytes_read: u64,
    pub pending_tail_bytes: u64,
    /// Why an incremental cursor was discarded and the source refolded cold.
    pub reset: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionStatus {
    pub schema_version: u32,
    /// The SDK fills this, including the transcript path it actually read.
    pub target: StatusTarget,
    /// Filled by the CLI layer for every query (basis `explicit` for
    /// `--session`); `None` from the SDK.
    pub resolved: Option<Resolved>,
    pub model: ModelStatus,
    pub context: ContextStatus,
    pub last_call: Option<LastCall>,
    pub timeline: Timeline,
    pub turns: TurnStatus,
    pub compaction: CompactionStatus,
    pub calls: CallCounts,
    pub limits_seen: Vec<LimitSeen>,
    pub source: SourceStatus,
    /// Facts this harness/session cannot supply; names from [`UNKNOWN_FACTS`].
    pub unknown: Vec<String>,
    /// Per served model, latest call first, at most [`MAX_STATUS_MODELS`].
    /// `model.current` and `last_call` stay the latest call overall.
    #[serde(default)]
    pub models: Vec<ModelCacheStatus>,
}

impl SessionStatus {
    /// An empty status for `target`: every fact unknown (all of
    /// [`UNKNOWN_FACTS`]) until filled.
    pub fn empty(target: StatusTarget) -> Self {
        Self {
            schema_version: STATUS_SCHEMA_VERSION,
            target,
            resolved: None,
            model: ModelStatus::default(),
            context: ContextStatus::default(),
            last_call: None,
            timeline: Timeline::default(),
            turns: TurnStatus::default(),
            compaction: CompactionStatus::default(),
            calls: CallCounts::default(),
            limits_seen: Vec::new(),
            source: SourceStatus::default(),
            unknown: UNKNOWN_FACTS
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            models: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Failures and ports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusFailureKind {
    UnsupportedHarness,
    TranscriptNotFound,
    Read,
    PijUnavailable,
    PijUnknownSeat,
    PijNoSession,
    DeadBinding,
    PaneNotFound,
    /// The lookup needs tools this platform lacks (e.g. tmux/ps on Windows).
    UnsupportedPlatform,
}

impl StatusFailureKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedHarness => "UNI-STATUS-UNSUPPORTED-HARNESS",
            Self::TranscriptNotFound => "UNI-STATUS-TRANSCRIPT-NOT-FOUND",
            Self::Read => "UNI-STATUS-READ",
            Self::PijUnavailable => "UNI-STATUS-PIJ-UNAVAILABLE",
            Self::PijUnknownSeat => "UNI-STATUS-PIJ-UNKNOWN-SEAT",
            Self::PijNoSession => "UNI-STATUS-PIJ-NO-SESSION",
            Self::DeadBinding => "UNI-STATUS-DEAD-BINDING",
            Self::PaneNotFound => "UNI-STATUS-PANE-NOT-FOUND",
            Self::UnsupportedPlatform => "UNI-STATUS-UNSUPPORTED-PLATFORM",
        }
    }

    pub const fn recovery(self) -> &'static str {
        match self {
            Self::UnsupportedHarness => "Use a supported harness (see `unisphere adapters list`).",
            Self::TranscriptNotFound => "Pass the transcript path explicitly.",
            Self::Read => "Check the transcript is readable, then retry.",
            Self::PijUnavailable => "Start the Pij daemon or query by --session and --harness.",
            Self::PijUnknownSeat => "Check the seat id with `pij list`.",
            Self::PijNoSession => "The seat has no recorded native session; query by --session.",
            Self::DeadBinding => "The seat's recorded process is gone; re-adopt the seat.",
            Self::PaneNotFound => "Check the pane id with `tmux list-panes -a`.",
            Self::UnsupportedPlatform => {
                "Pane lookup is unavailable on this platform; query by --session and --harness."
            }
        }
    }
}

/// A typed status failure; `message` never contains transcript content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusFailure {
    pub kind: StatusFailureKind,
    pub message: String,
}

impl StatusFailure {
    pub fn new(kind: StatusFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    pub const fn recovery(&self) -> &'static str {
        self.kind.recovery()
    }
}

impl std::fmt::Display for StatusFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message)
    }
}

impl std::error::Error for StatusFailure {}

/// Status for one explicit target. Implementations perform no Pij, tmux or
/// process lookups; `now_ms` is supplied by the caller.
pub trait SessionStatusApi: Send + Sync {
    fn status(&self, target: &StatusTarget, now_ms: i64) -> Result<SessionStatus, StatusFailure>;
}

/// Maps a query (Pij id, tmux pane or explicit target) to a target.
pub trait TargetResolver: Send + Sync {
    fn resolve(&self, query: &StatusQuery) -> Result<Resolved, StatusFailure>;
}
