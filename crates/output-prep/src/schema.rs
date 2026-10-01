//! Physical column layout of every published table and the snapshot rows
//! derived from [`PrepState`].

use std::sync::Arc;

use arrow_schema::{DataType, Field, Schema};
use serde::Serialize;
use unisphere_core::{
    SourceIdentity,
    prep::{PrepReplaceReason, PrepSourceKind, PrepSourceStatus, PrepState},
};

fn columns(spec: &[(&str, DataType, bool)]) -> Arc<Schema> {
    Arc::new(Schema::new(
        spec.iter()
            .map(|(name, kind, nullable)| Field::new(*name, kind.clone(), *nullable))
            .collect::<Vec<_>>(),
    ))
}

use DataType::{Boolean as B, Int64 as I, Utf8 as S};

pub(crate) fn schema(table: &str) -> Arc<Schema> {
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
pub(crate) struct SourceRow<'a> {
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
pub(crate) struct SessionRow<'a> {
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

/// `sources` and `sessions` snapshot rows for every source in `state`. Project
/// and agent columns come from each source's [`PrepSourceMeta`], so every
/// harness binding describes its own paths.
///
/// [`PrepSourceMeta`]: unisphere_core::prep::PrepSourceMeta
pub(crate) fn snapshot_rows(state: &PrepState) -> (Vec<SourceRow<'_>>, Vec<SessionRow<'_>>) {
    let as_i64 = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
    let mut sources = Vec::with_capacity(state.sources.len());
    let mut sessions = Vec::with_capacity(state.sources.len());
    for (key, source) in &state.sources {
        let set = state.sets.get(&source.set);
        let (device, inode) = match source.identity {
            SourceIdentity::Unix { device, inode } => (Some(as_i64(device)), Some(as_i64(inode))),
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
    (sources, sessions)
}
