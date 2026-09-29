//! Session status for one explicit target over the Plan 028 fold.
//!
//! [`StatusService`] binds harness folds and loaders ([`PrepBinding`]) and the
//! configured discovery roots, finds the target's transcript, folds it with
//! [`fold_source`] and derives every [`SessionStatus`] fact purely from the
//! fold's [`SessionFacts`], the accumulated call/turn/event rows and the
//! injected `now_ms`. It never calls Pij, tmux or any process, and never reads
//! a clock.
//!
//! [`StatusService::status_incremental`] returns an opaque [`StatusCursor`]:
//! handing it back reads only appended complete records. A shrink, identity
//! change, prefix rewrite or a cursor for another target refolds cold and names
//! the reason in [`SourceStatus::reset`]. Apart from `source`, an incremental
//! result equals the cold result for the same bytes and `now_ms`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use globset::{Glob, GlobMatcher};
use unisphere_core::{PipelineError, ReadCursor, ReadLimits, SnapshotLimits};

use crate::prep::{
    CallSighting, PrepBinding, PrepCallRow, PrepEventKind, PrepOptions, PrepReadLimits, PrepResume,
    PrepRows, PrepSourceKind, PrepSourceSet, PrepSourceStat, SessionFacts, derived_root_label,
    fold_source,
};

pub use unisphere_core::status::*;

const HOUR_MS: i64 = 3_600_000;
const FIVE_MINUTES_MS: i64 = 300_000;
/// Oldest model spans are dropped beyond this many.
const MAX_MODEL_SPANS: usize = 64;
/// Only the latest usage-limit notices are kept.
const MAX_LIMITS: usize = 16;
/// Client-generated notices carry this model; they are never API calls.
const SYNTHETIC_MODEL: &str = "<synthetic>";

/// `model-windows@1` ([`MODEL_WINDOWS_TABLE`]): context window by model id. The
/// longest entry that equals the id, or is followed in it by `-`, wins.
/// 1M entries are backed by observed main-chain contexts above 200k, except
/// `claude-sonnet-5`, which the github-copilot model catalog lists at 1M.
const MODEL_WINDOWS: &[(&str, u64)] = &[
    ("claude-opus-5", 1_000_000),
    ("claude-fable-5", 1_000_000),
    ("claude-sonnet-5", 1_000_000),
    ("claude-opus-4-8", 1_000_000),
    ("claude-haiku-4-5", 200_000),
    ("claude-3", 200_000),
];

fn table_window(model: &str) -> Option<u64> {
    MODEL_WINDOWS
        .iter()
        .filter(|(prefix, _)| {
            model
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
        })
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, window)| *window)
}

// ---------------------------------------------------------------------------
// Accumulators
// ---------------------------------------------------------------------------

/// The latest deduplicated main-chain call; update sightings raise its counters.
#[derive(Debug, Clone, PartialEq)]
struct TrackedCall {
    msg_id: Option<String>,
    request_id: Option<String>,
    at_ms: Option<i64>,
    input: Option<i64>,
    output: Option<i64>,
    cache_read: Option<i64>,
    cw_1h: Option<i64>,
    cw_5m: Option<i64>,
    stop_reason: Option<String>,
}

impl TrackedCall {
    fn new(row: &PrepCallRow) -> Self {
        Self {
            msg_id: row.msg_id.clone(),
            request_id: row.request_id.clone(),
            at_ms: row.ts_ms,
            input: row.input,
            output: row.output,
            cache_read: row.cache_read,
            cw_1h: row.cw_1h,
            cw_5m: row.cw_5m,
            stop_reason: row.stop_reason.clone(),
        }
    }

    fn same_call(&self, row: &PrepCallRow) -> bool {
        (self.msg_id.is_some() || self.request_id.is_some())
            && self.msg_id == row.msg_id
            && self.request_id == row.request_id
    }

    /// Per-field maximum of the counters and the last recorded stop reason,
    /// exactly as one merged sighting; the time stays the first sighting's.
    fn merge(&mut self, row: &PrepCallRow) {
        let max = |a: Option<i64>, b: Option<i64>| a.max(b);
        self.input = max(self.input, row.input);
        self.output = max(self.output, row.output);
        self.cache_read = max(self.cache_read, row.cache_read);
        self.cw_1h = max(self.cw_1h, row.cw_1h);
        self.cw_5m = max(self.cw_5m, row.cw_5m);
        if row.stop_reason.is_some() {
            self.stop_reason.clone_from(&row.stop_reason);
        }
    }
}

/// Bounded facts accumulated from rows; independent of how input was batched.
#[derive(Debug, Clone, Default, PartialEq)]
struct Accumulated {
    model_spans: Vec<ModelSpan>,
    /// Time of the latest main-chain, non-synthetic call that named a model.
    current_at_ms: Option<i64>,
    last_call: Option<TrackedCall>,
    calls: CallCounts,
    turns_total: u64,
    turns_by_origin: BTreeMap<String, u64>,
    /// Timed turns newer than an hour before `newest_turn_ms`.
    recent_turns: Vec<(i64, String)>,
    newest_turn_ms: Option<i64>,
    limits: Vec<LimitSeen>,
}

impl Accumulated {
    fn apply(&mut self, rows: &PrepRows) {
        for call in &rows.calls {
            self.call(call);
        }
        for turn in &rows.turns {
            let origin = origin_label(turn.origin);
            self.turns_total += 1;
            *self.turns_by_origin.entry(origin.clone()).or_default() += 1;
            if let Some(ts) = turn.started_ts_ms {
                self.newest_turn_ms = self.newest_turn_ms.max(Some(ts));
                self.recent_turns.push((ts, origin));
            }
        }
        for event in &rows.events {
            if event.kind == PrepEventKind::LimitNotice {
                if self.limits.len() == MAX_LIMITS {
                    self.limits.remove(0);
                }
                self.limits.push(LimitSeen {
                    kind: event
                        .subkind
                        .clone()
                        .unwrap_or_else(|| "limit_notice".to_owned()),
                    at_ms: event.ts_ms,
                    resets_at: event.resets_at.clone(),
                });
            }
        }
        // Monotonic in `newest_turn_ms`, so any batch split prunes the same set.
        if let Some(newest) = self.newest_turn_ms {
            self.recent_turns.retain(|(ts, _)| *ts > newest - HOUR_MS);
        }
    }

    fn call(&mut self, row: &PrepCallRow) {
        if row.model.as_deref() == Some(SYNTHETIC_MODEL) {
            return;
        }
        match row.sighting {
            CallSighting::Update => {
                if let Some(last) = self.last_call.as_mut().filter(|l| l.same_call(row)) {
                    last.merge(row);
                }
            }
            CallSighting::First => {
                self.calls.total += 1;
                if row.is_sidechain {
                    self.calls.sidechain += 1;
                    return;
                }
                self.last_call = Some(TrackedCall::new(row));
                if let Some(model) = &row.model {
                    self.model(model, row.ts_ms);
                }
            }
        }
    }

    fn model(&mut self, model: &str, ts: Option<i64>) {
        match self.model_spans.last_mut() {
            Some(span) if span.model == model => {
                span.calls += 1;
                span.first_ms = span.first_ms.or(ts);
                span.last_ms = ts.or(span.last_ms);
            }
            _ => {
                if self.model_spans.len() == MAX_MODEL_SPANS {
                    self.model_spans.remove(0);
                }
                self.model_spans.push(ModelSpan {
                    model: model.to_owned(),
                    first_ms: ts,
                    last_ms: ts,
                    calls: 1,
                });
            }
        }
        self.current_at_ms = ts;
    }
}

fn origin_label(origin: crate::prep::TurnOrigin) -> String {
    serde_json::to_value(origin)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "other".to_owned())
}

// ---------------------------------------------------------------------------
// Pure derivation
// ---------------------------------------------------------------------------

fn non_negative(value: Option<i64>) -> Option<u64> {
    value.and_then(|v| u64::try_from(v).ok())
}

fn tokens_label(tokens: u64) -> String {
    let scaled = |value: f64, unit: &str| {
        let text = format!("{value:.1}");
        format!("{}{unit}", text.strip_suffix(".0").unwrap_or(&text))
    };
    if tokens >= 1_000_000 {
        scaled(tokens as f64 / 1_000_000.0, "M")
    } else if tokens >= 1_000 {
        format!("{}k", (tokens as f64 / 1_000.0).round())
    } else {
        tokens.to_string()
    }
}

/// Every fact from the fold's facts, the accumulated rows, the source's mtime
/// and `now_ms`. `source` is left for the caller.
fn derive(
    target: StatusTarget,
    facts: &SessionFacts,
    acc: &Accumulated,
    mtime_ns: i128,
    now_ms: i64,
) -> SessionStatus {
    let mut status = SessionStatus::empty(target);

    // Model.
    let current = acc.model_spans.last().map(|span| span.model.clone());
    let (pending_known, pending_switch) = match &facts.last_model_switch {
        None => (true, None),
        Some(switch) => {
            let pending = || ModelSwitchStatus {
                requested: switch.requested_model.clone(),
                at_ms: switch.ts_ms,
            };
            match (&current, switch.ts_ms, acc.current_at_ms) {
                (None, _, _) => (true, Some(pending())),
                (Some(_), Some(at), Some(call)) => (true, (at > call).then(pending)),
                // No time on one side: whether it follows the call is unknown.
                _ => (false, None),
            }
        }
    };
    status.model = ModelStatus {
        current: current.clone().map(|model| Fact::new(model, Basis::Native)),
        current_at_ms: current.as_ref().and(acc.current_at_ms),
        pending_switch,
        history: acc.model_spans.clone(),
    };

    // Context.
    let used = facts
        .latest_context
        .as_ref()
        .and_then(|sample| non_negative(sample.total));
    let window = current.as_deref().and_then(table_window);
    let percent = used
        .zip(window)
        .filter(|(_, window)| *window > 0)
        .map(|(used, window)| (used as f64 / window as f64 * 1000.0).round() / 10.0);
    status.context = ContextStatus {
        used_tokens: used.map(|used| Fact::new(used, Basis::Native)),
        window_tokens: window.map(|window| Fact::new(window, Basis::Table)),
        window_table: window.map(|_| MODEL_WINDOWS_TABLE.to_owned()),
        percent,
        display: used
            .zip(window)
            .zip(percent)
            .map(|((used, window), percent)| {
                format!(
                    "{} of {} ({percent:.0}%)",
                    tokens_label(used),
                    tokens_label(window)
                )
            }),
    };

    // Last call.
    status.last_call = acc.last_call.as_ref().map(|call| {
        let ttl = if call.cw_1h.is_some_and(|v| v > 0) {
            Some(("1h", HOUR_MS))
        } else if call.cw_5m.is_some_and(|v| v > 0) {
            Some(("5m", FIVE_MINUTES_MS))
        } else {
            None
        };
        LastCall {
            at_ms: call.at_ms,
            input: non_negative(call.input),
            output: non_negative(call.output),
            cache_read: non_negative(call.cache_read),
            cache_write_1h: non_negative(call.cw_1h),
            cache_write_5m: non_negative(call.cw_5m),
            ttl_bucket: ttl.map(|(bucket, _)| Fact::new(bucket.to_owned(), Basis::Derived)),
            cache_warm: ttl
                .zip(call.at_ms)
                .map(|((_, ttl), at)| Fact::new(now_ms - at < ttl, Basis::Derived)),
            stop_reason: call.stop_reason.clone(),
        }
    });

    // Timeline.
    let last_updated = match facts.last_event_ms {
        Some(ms) => Some(Fact::new(ms, Basis::Native)),
        None => i64::try_from(mtime_ns.div_euclid(1_000_000))
            .ok()
            .map(|ms| Fact::new(ms, Basis::MtimeFallback)),
    };
    status.timeline = Timeline {
        created_ms: facts.first_event_ms.map(|ms| Fact::new(ms, Basis::Native)),
        idle_seconds: last_updated
            .as_ref()
            .map(|updated| (now_ms.saturating_sub(updated.value).max(0) / 1000) as u64),
        last_updated_ms: last_updated,
    };

    // Turns.
    let mut last_hour_by_origin = BTreeMap::new();
    for (_, origin) in acc
        .recent_turns
        .iter()
        .filter(|(ts, _)| *ts > now_ms - HOUR_MS)
    {
        *last_hour_by_origin.entry(origin.clone()).or_default() += 1;
    }
    status.turns = TurnStatus {
        total: acc.turns_total,
        last_hour_total: last_hour_by_origin.values().sum(),
        by_origin: acc.turns_by_origin.clone(),
        last_hour_by_origin,
    };

    // Compaction.
    status.compaction = CompactionStatus {
        counts: facts.compactions,
        last: facts.last_compaction.clone(),
    };
    let no_compactions = facts
        .compactions
        .is_some_and(|c| c.manual + c.auto + c.unknown_trigger == 0);

    status.calls = acc.calls;
    status.limits_seen = acc.limits.clone();

    let last_call = status.last_call.as_ref();
    let known = [
        ("model.current", status.model.current.is_some()),
        ("model.pending_switch", pending_known),
        ("context.used_tokens", status.context.used_tokens.is_some()),
        (
            "context.window_tokens",
            status.context.window_tokens.is_some(),
        ),
        ("context.percent", status.context.percent.is_some()),
        ("last_call", last_call.is_some()),
        (
            "last_call.ttl_bucket",
            last_call.is_some_and(|c| c.ttl_bucket.is_some()),
        ),
        (
            "last_call.cache_warm",
            last_call.is_some_and(|c| c.cache_warm.is_some()),
        ),
        (
            "last_call.stop_reason",
            last_call.is_some_and(|c| c.stop_reason.is_some()),
        ),
        ("timeline.created_ms", status.timeline.created_ms.is_some()),
        (
            "timeline.last_updated_ms",
            status.timeline.last_updated_ms.is_some(),
        ),
        ("compaction.counts", status.compaction.counts.is_some()),
        (
            "compaction.last",
            status.compaction.last.is_some() || no_compactions,
        ),
    ];
    status
        .unknown
        .retain(|name| !known.iter().any(|(fact, known)| *known && fact == name));
    status
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// Opaque, caller-held position of one target's transcript: the fold's resume
/// point, the source's observed stat and anchor, the fold's facts and the
/// bounded row accumulators. Hand it back to read only appended bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusCursor {
    target: StatusTarget,
    policy: String,
    root: PathBuf,
    label: String,
    stat: PrepSourceStat,
    /// Append: cursor after the last complete record and the checkpoint there.
    resume: Option<PrepResume>,
    anchor: Option<String>,
    facts: SessionFacts,
    acc: Accumulated,
}

/// One located transcript and the binding that reads it.
struct Located<'a> {
    binding: &'a PrepBinding,
    root: PathBuf,
    label: String,
    stat: PrepSourceStat,
}

/// Implements [`SessionStatusApi`] over Plan 028 bindings and discovery roots.
pub struct StatusService {
    bindings: Vec<PrepBinding>,
    roots: Vec<PrepSourceSet>,
    limits: PrepReadLimits,
}

impl StatusService {
    pub fn new(bindings: Vec<PrepBinding>, roots: Vec<PrepSourceSet>) -> Self {
        Self {
            bindings,
            roots,
            limits: PrepReadLimits {
                read: ReadLimits::default(),
                snapshot: SnapshotLimits::default(),
            },
        }
    }

    /// Status for `target`, resuming from `cursor` when it still describes the
    /// same target and source. On error the caller keeps its previous cursor.
    pub fn status_incremental(
        &self,
        target: &StatusTarget,
        cursor: Option<&StatusCursor>,
        now_ms: i64,
    ) -> Result<(SessionStatus, StatusCursor), StatusFailure> {
        let bindings: Vec<&PrepBinding> = self
            .bindings
            .iter()
            .filter(|binding| binding.fold.harness() == target.harness)
            .collect();
        if bindings.is_empty() {
            return Err(StatusFailure::new(
                StatusFailureKind::UnsupportedHarness,
                "no status binding for this harness",
            ));
        }
        let mut reset = None;
        let mut prior = None;
        let located = match cursor {
            Some(cursor) if cursor.target != *target => {
                reset = Some("target-changed");
                self.locate(target, &bindings)?
            }
            Some(cursor) => match self.relocate(cursor, &bindings) {
                Some(located) => {
                    prior = Some(cursor);
                    located
                }
                None => {
                    let located = self.locate(target, &bindings)?;
                    reset = Some(if located.binding.fold.policy() == cursor.policy {
                        "rotated"
                    } else {
                        "policy"
                    });
                    located
                }
            },
            None => self.locate(target, &bindings)?,
        };
        let stat = &located.stat;

        let mut resume = None;
        if let Some(prior) = prior {
            let same_stat = prior.stat.identity == stat.identity
                && prior.stat.size == stat.size
                && prior.stat.mtime_ns == stat.mtime_ns;
            if same_stat {
                return Ok(finish(prior.clone(), target, 0, now_ms, None));
            }
            if stat.kind == PrepSourceKind::Append {
                let offset = prior.resume.as_ref().map_or(0, |r| r.cursor.offset);
                if prior.stat.identity != stat.identity {
                    reset = Some("rotated");
                } else if stat.size < offset {
                    reset = Some("truncated");
                } else if offset > 0 {
                    let anchor = located
                        .binding
                        .loader
                        .anchor(stat, offset)
                        .map_err(read_failure)?;
                    if prior.anchor.as_deref() == Some(anchor.as_str()) {
                        resume = Some(prior);
                    } else {
                        reset = Some("anchor");
                    }
                }
            }
        }

        let mut acc = resume.map(|prior| prior.acc.clone()).unwrap_or_default();
        let from = resume.and_then(|prior| {
            prior.resume.as_ref().map(|r| PrepResume {
                cursor: ReadCursor {
                    source: stat.path.clone(),
                    identity: stat.identity.clone(),
                    offset: r.cursor.offset,
                },
                checkpoint: r.checkpoint.clone(),
            })
        });
        let source = format!("{}/{}/{}", target.harness, located.label, stat.file);
        let folded = fold_source(
            located.binding.loader.as_ref(),
            located.binding.fold.as_ref(),
            stat,
            &source,
            0,
            from.as_ref(),
            PrepOptions::default(),
            self.limits,
            &mut |rows| acc.apply(&rows),
        )
        .map_err(read_failure)?;
        let next = StatusCursor {
            target: target.clone(),
            policy: located.binding.fold.policy().to_owned(),
            root: located.root,
            label: located.label,
            stat: located.stat.clone(),
            resume: folded.cursor.map(|cursor| PrepResume {
                cursor,
                checkpoint: folded.checkpoint,
            }),
            anchor: folded.anchor,
            facts: folded.facts,
            acc,
        };
        Ok(finish(next, target, folded.bytes_read, now_ms, reset))
    }

    /// The cursor's source, if its binding still exists and the path still stats.
    fn relocate<'a>(
        &self,
        cursor: &StatusCursor,
        bindings: &[&'a PrepBinding],
    ) -> Option<Located<'a>> {
        let binding = bindings.iter().copied().find(|binding| {
            binding.fold.policy() == cursor.policy && binding.loader.kind() == cursor.stat.kind
        })?;
        let stat = binding.loader.stat(&cursor.root, &cursor.stat.path).ok()?;
        Some(Located {
            binding,
            root: cursor.root.clone(),
            label: cursor.label.clone(),
            stat,
        })
    }

    /// Transcript rule: an explicit path is read under the configured root that
    /// contains it, else under its parent directory; without a path the file
    /// whose stem is the session id is discovered under the harness's roots.
    fn locate<'a>(
        &self,
        target: &StatusTarget,
        bindings: &[&'a PrepBinding],
    ) -> Result<Located<'a>, StatusFailure> {
        let roots = self
            .roots
            .iter()
            .filter(|set| set.harness == target.harness);
        match &target.transcript {
            Some(path) => {
                let (root, label) = match roots
                    .filter(|set| path.starts_with(&set.root))
                    .max_by_key(|set| set.root.components().count())
                {
                    Some(set) => (set.root.clone(), set.label.clone()),
                    None => {
                        let parent = path.parent().ok_or_else(not_found)?;
                        (parent.to_path_buf(), derived_root_label(parent))
                    }
                };
                let file = path
                    .strip_prefix(&root)
                    .ok()
                    .and_then(Path::to_str)
                    .unwrap_or_default();
                let binding = bindings
                    .iter()
                    .copied()
                    .find(|binding| matcher(binding).is_some_and(|m| m.is_match(file)))
                    .unwrap_or(bindings[0]);
                let stat = binding.loader.stat(&root, path).map_err(|_| not_found())?;
                Ok(Located {
                    binding,
                    root,
                    label,
                    stat,
                })
            }
            None => {
                let mut found: Option<Located<'a>> = None;
                let mut failed = false;
                for set in roots {
                    for binding in bindings.iter().copied() {
                        let Some(pattern) = matcher(binding) else {
                            failed = true;
                            continue;
                        };
                        let accept = |file: &str| {
                            pattern.is_match(file)
                                && Path::new(file).file_stem().and_then(|s| s.to_str())
                                    == Some(target.session_id.as_str())
                        };
                        let Ok(discovery) = binding.loader.discover(&set.root, &accept) else {
                            failed = true;
                            continue;
                        };
                        for stat in discovery.sources {
                            // The most recently modified match wins; ties keep the first.
                            if found
                                .as_ref()
                                .is_none_or(|best| stat.mtime_ns > best.stat.mtime_ns)
                            {
                                found = Some(Located {
                                    binding,
                                    root: set.root.clone(),
                                    label: set.label.clone(),
                                    stat,
                                });
                            }
                        }
                    }
                }
                match found {
                    Some(located) => Ok(located),
                    None if failed => Err(StatusFailure::new(
                        StatusFailureKind::Read,
                        "discovery failed under a configured root",
                    )),
                    None => Err(not_found()),
                }
            }
        }
    }
}

impl SessionStatusApi for StatusService {
    fn status(&self, target: &StatusTarget, now_ms: i64) -> Result<SessionStatus, StatusFailure> {
        self.status_incremental(target, None, now_ms)
            .map(|(status, _)| status)
    }
}

fn matcher(binding: &PrepBinding) -> Option<GlobMatcher> {
    Glob::new(binding.fold.pattern())
        .ok()
        .map(|glob| glob.compile_matcher())
}

fn not_found() -> StatusFailure {
    StatusFailure::new(
        StatusFailureKind::TranscriptNotFound,
        "no transcript found for the session",
    )
}

fn read_failure(error: PipelineError) -> StatusFailure {
    StatusFailure::new(
        StatusFailureKind::Read,
        format!("reading the transcript failed: {error}"),
    )
}

fn finish(
    cursor: StatusCursor,
    target: &StatusTarget,
    bytes_read: u64,
    now_ms: i64,
    reset: Option<&str>,
) -> (SessionStatus, StatusCursor) {
    let read = StatusTarget {
        transcript: Some(cursor.stat.path.clone()),
        ..target.clone()
    };
    let mut status = derive(
        read,
        &cursor.facts,
        &cursor.acc,
        cursor.stat.mtime_ns,
        now_ms,
    );
    let offset = cursor.resume.as_ref().map_or(0, |r| r.cursor.offset);
    status.source = SourceStatus {
        bytes_read,
        pending_tail_bytes: match cursor.stat.kind {
            PrepSourceKind::Append => cursor.stat.size.saturating_sub(offset),
            PrepSourceKind::Snapshot => 0,
        },
        reset: reset.map(str::to_owned),
    };
    (status, cursor)
}
