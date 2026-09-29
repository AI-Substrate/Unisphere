//! Pure Claude Code fold into canonical prep tables.
//!
//! Supplied native records in, metadata facts out. The rules deliberately match
//! the reference RCA parser: calls deduplicated per file on
//! (`message.id`, `requestId`) with the per-field maximum, `<synthetic>` assistant
//! records as limit notices rather than calls, and turns opened by the first new
//! call after a classified user record. Message text is inspected to classify a
//! turn opener and derive a body hash, but never emitted unless content is
//! explicitly requested.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
#[cfg(test)]
use unisphere_core::NativeRecord;
use unisphere_core::{
    PipelineError, PipelineErrorKind,
    prep::{
        CacheWriteBasis, CallSighting, PREP_CHECKPOINT_FORMAT, PrepCallRow, PrepCheckpoint,
        PrepEventKind, PrepEventRow, PrepFold, PrepFoldSession, PrepInput, PrepOptions, PrepRows,
        PrepSourceKind, PrepSourceMeta, PrepTriggerRow, PrepTurnRow, SessionFacts, TurnOrigin,
    },
};

/// Interpretation policy; bump on any rule change so every source re-emits.
pub const PREP_POLICY_VERSION: &str = "claude-code/prep-v2";

#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudePrepFold;

impl PrepFold for ClaudePrepFold {
    fn harness(&self) -> &'static str {
        "claude-code"
    }
    fn policy(&self) -> &'static str {
        PREP_POLICY_VERSION
    }
    fn kind(&self) -> PrepSourceKind {
        PrepSourceKind::Append
    }
    fn pattern(&self) -> &'static str {
        "**/*.jsonl"
    }
    fn describe(&self, file: &str) -> PrepSourceMeta {
        let is_sub = file.contains("/subagents/");
        let project_dir = file.split('/').next().unwrap_or(file);
        PrepSourceMeta {
            is_sub,
            agent_id: agent_of(file, is_sub).map(str::to_owned),
            project: Some(project_of(project_dir)),
        }
    }
    fn open(
        &self,
        meta: &PrepSourceMeta,
        source: &str,
        generation: u32,
        saved: Option<&PrepCheckpoint>,
    ) -> Result<Box<dyn PrepFoldSession>, PipelineError> {
        let invalid = || PipelineError::new(PipelineErrorKind::InvalidData, None);
        let state = match saved {
            None => State::new(meta, source, generation),
            Some(checkpoint)
                if checkpoint.format == PREP_CHECKPOINT_FORMAT
                    && checkpoint.policy == PREP_POLICY_VERSION =>
            {
                State::load(meta, source, generation, &checkpoint.fold).ok_or_else(invalid)?
            }
            Some(_) => return Err(invalid()),
        };
        Ok(Box::new(state))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    kind: String,
    sender: Option<String>,
    pij_msg_id: Option<String>,
    offset: Option<u64>,
    ts_ms: Option<i64>,
    chars: Option<i64>,
    body_key: Option<String>,
}

/// `-Users-<name>-rest` → `rest`; Claude encodes the cwd with `/` as `-`.
fn project_of(project_dir: &str) -> String {
    let mut parts = project_dir.trim_start_matches('-').splitn(3, '-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("Users"), Some(_), Some(rest)) => rest.to_owned(),
        _ => project_dir.to_owned(),
    }
}

fn agent_of(file: &str, is_sub: bool) -> Option<&str> {
    if !is_sub {
        return None;
    }
    let name = file.rsplit('/').next()?;
    name.strip_suffix(".jsonl")
}

#[derive(Debug, Clone)]
struct State {
    source: String,
    is_sub: bool,
    generation: u32,
    turn_no: i64,
    call_in_turn: i64,
    turn_zero_emitted: bool,
    pending: Option<Pending>,
    last_call_ts_ms: Option<i64>,
    last_call_key: Option<u64>,
    /// input, cw_1h, cw_5m, cache_read of the latest new call, max-updated.
    last_call_ctx: [i64; 4],
    committed: BTreeSet<u64>,
    session: SessionFacts,
}

impl State {
    fn new(meta: &PrepSourceMeta, source: &str, generation: u32) -> Self {
        Self {
            source: source.to_owned(),
            is_sub: meta.is_sub,
            generation,
            turn_no: 0,
            call_in_turn: 0,
            turn_zero_emitted: false,
            pending: None,
            last_call_ts_ms: None,
            last_call_key: None,
            last_call_ctx: [0; 4],
            committed: BTreeSet::new(),
            session: SessionFacts {
                is_sidechain: meta.is_sub,
                ..SessionFacts::default()
            },
        }
    }

    fn load(meta: &PrepSourceMeta, source: &str, generation: u32, value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let int = |key: &str| object.get(key).and_then(Value::as_i64);
        let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_owned);
        let pending = match object.get("pending") {
            Some(Value::Object(p)) => Some(Pending {
                kind: text(p.get("kind"))?,
                sender: text(p.get("sender")),
                pij_msg_id: text(p.get("pij_msg_id")),
                offset: p.get("offset").and_then(Value::as_u64),
                ts_ms: p.get("ts_ms").and_then(Value::as_i64),
                chars: p.get("chars").and_then(Value::as_i64),
                body_key: text(p.get("body_key")),
            }),
            _ => None,
        };
        let ctx = object.get("last_call_ctx")?.as_array()?;
        let mut last_call_ctx = [0; 4];
        for (slot, value) in last_call_ctx.iter_mut().zip(ctx) {
            *slot = value.as_i64()?;
        }
        let committed = object
            .get("committed")?
            .as_array()?
            .iter()
            .map(|key| key.as_str().and_then(|k| u64::from_str_radix(k, 16).ok()))
            .collect::<Option<BTreeSet<_>>>()?;
        let session = serde_json::from_value(object.get("session")?.clone()).ok()?;
        Some(Self {
            source: source.to_owned(),
            is_sub: meta.is_sub,
            generation,
            turn_no: int("turn_no")?,
            call_in_turn: int("call_in_turn")?,
            turn_zero_emitted: object.get("turn_zero_emitted")?.as_bool()?,
            pending,
            last_call_ts_ms: int("last_call_ts_ms"),
            last_call_key: object
                .get("last_call_key")
                .and_then(Value::as_str)
                .and_then(|k| u64::from_str_radix(k, 16).ok()),
            last_call_ctx,
            committed,
            session,
        })
    }

    fn row_base(&self) -> (String, u32) {
        (self.source.clone(), self.generation)
    }
}

impl PrepFoldSession for State {
    fn checkpoint(&self) -> PrepCheckpoint {
        PrepCheckpoint {
            format: PREP_CHECKPOINT_FORMAT,
            policy: PREP_POLICY_VERSION.to_owned(),
            fold: self.save(),
        }
    }

    fn facts(&self) -> SessionFacts {
        self.session.clone()
    }

    fn fold(&mut self, input: &PrepInput, options: PrepOptions) -> Result<PrepRows, PipelineError> {
        match input {
            PrepInput::Records(records) => self.fold_records(records, options),
            PrepInput::Snapshot(_) => {
                Err(PipelineError::new(PipelineErrorKind::InvalidInput, None))
            }
        }
    }
}

impl State {
    fn save(&self) -> Value {
        json!({
            "turn_no": self.turn_no,
            "call_in_turn": self.call_in_turn,
            "turn_zero_emitted": self.turn_zero_emitted,
            "pending": self.pending.as_ref().map(|p| json!({
                "kind": p.kind, "sender": p.sender, "pij_msg_id": p.pij_msg_id,
                "offset": p.offset, "ts_ms": p.ts_ms, "chars": p.chars, "body_key": p.body_key,
            })),
            "last_call_ts_ms": self.last_call_ts_ms,
            "last_call_key": self.last_call_key.map(|k| format!("{k:016x}")),
            "last_call_ctx": self.last_call_ctx,
            "committed": self.committed.iter().map(|k| format!("{k:016x}")).collect::<Vec<_>>(),
            "session": serde_json::to_value(&self.session).unwrap_or(Value::Null),
        })
    }

    fn fold_records(
        &mut self,
        records: &[unisphere_core::NativeRecord],
        options: PrepOptions,
    ) -> Result<PrepRows, PipelineError> {
        let mut rows = PrepRows::default();
        // Calls first seen in this batch, by key, so repeats merge in place.
        let mut batch_calls: HashMap<u64, usize> = HashMap::new();
        for native in records {
            if !contains(&native.bytes, b"\"timestamp\"") {
                continue;
            }
            let Ok(Value::Object(record)) = serde_json::from_slice::<Value>(&native.bytes) else {
                self.session.skipped.malformed += 1;
                continue;
            };
            let Some(ts) = record
                .get("timestamp")
                .and_then(Value::as_str)
                .filter(|ts| !ts.is_empty())
            else {
                self.session.skipped.untimed += 1;
                continue;
            };
            let Some(ts_ms) = parse_ms(ts) else {
                self.session.skipped.bad_timestamp += 1;
                continue;
            };
            self.observe_session(&record, ts, ts_ms);
            let at = At {
                offset: native.offset,
                ts,
                ts_ms,
            };
            match record.get("type").and_then(Value::as_str) {
                Some("user") => self.user(&record, &at, options, &mut rows),
                Some("system") => self.system(&record, &at, &mut rows),
                Some("assistant") => self.assistant(&record, &at, &mut rows, &mut batch_calls),
                Some("queue-operation") => {
                    let body_key = record
                        .get("content")
                        .and_then(Value::as_str)
                        .map(|text| fnv_hex(normalised_body(text).as_bytes()));
                    let subkind = record
                        .get("operation")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    rows.events
                        .push(self.event(&at, PrepEventKind::QueueOp, subkind, body_key));
                }
                Some("attachment") => {
                    let attachment = record.get("attachment").and_then(Value::as_object);
                    if attachment
                        .and_then(|a| a.get("type"))
                        .and_then(Value::as_str)
                        == Some("queued_command")
                    {
                        let body_key = attachment
                            .and_then(|a| a.get("prompt"))
                            .and_then(Value::as_str)
                            .map(|text| fnv_hex(normalised_body(text).as_bytes()));
                        rows.events.push(self.event(
                            &at,
                            PrepEventKind::QueueOp,
                            Some("queued_command".into()),
                            body_key,
                        ));
                    }
                }
                _ => {}
            }
        }
        Ok(rows)
    }
}

struct At<'a> {
    offset: u64,
    ts: &'a str,
    ts_ms: i64,
}

impl State {
    fn observe_session(&mut self, record: &Map<String, Value>, ts: &str, ts_ms: i64) {
        let session = &mut self.session;
        session.records += 1;
        if session.session_id.as_deref().unwrap_or("").is_empty() {
            session.session_id = string(record, "sessionId").filter(|s| !s.is_empty());
        }
        if session.cwd.as_deref().unwrap_or("").is_empty() {
            session.cwd = string(record, "cwd").filter(|s| !s.is_empty());
        }
        if session.first_event_ts.is_none() {
            session.first_event_ts = Some(ts.to_owned());
            session.first_event_ms = Some(ts_ms);
        }
        session.last_event_ts = Some(ts.to_owned());
        session.last_event_ms = Some(ts_ms);
    }

    fn event(
        &self,
        at: &At<'_>,
        kind: PrepEventKind,
        subkind: Option<String>,
        body_key: Option<String>,
    ) -> PrepEventRow {
        let (source, generation) = self.row_base();
        PrepEventRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            ts: Some(at.ts.to_owned()),
            ts_ms: Some(at.ts_ms),
            kind,
            subkind,
            trigger: None,
            model: None,
            pre_tokens: None,
            post_tokens: None,
            duration_ms: None,
            last_context: None,
            gap_ms: None,
            resets_at: None,
            resets_at_ms: None,
            turn_no: self.turn_no,
            body_key,
        }
    }

    fn gap_since_last_call(&self, ts_ms: i64) -> Option<i64> {
        self.last_call_ts_ms.map(|last| ts_ms - last)
    }

    fn user(
        &mut self,
        record: &Map<String, Value>,
        at: &At<'_>,
        options: PrepOptions,
        rows: &mut PrepRows,
    ) {
        let empty = Map::new();
        let message = record
            .get("message")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        if is_tool_result(message) {
            return;
        }
        let text = text_of(message);
        if self.session.seat_hint.is_none() && truthy(record.get("isCompactSummary")) {
            self.session.seat_hint = seat_hint(&text);
        }
        let Some((kind, sender)) = classify(record, &text, self.is_sub) else {
            return;
        };
        let pij_msg_id = pij_message_id(&text);
        let body = normalised_body(&text);
        let body_key = fnv_hex(body.as_bytes());
        let chars = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
        let (source, generation) = self.row_base();
        rows.triggers.push(PrepTriggerRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            ts: Some(at.ts.to_owned()),
            ts_ms: Some(at.ts_ms),
            kind: origin(&kind),
            sender: sender.clone(),
            pij_msg_id: pij_msg_id.clone(),
            chars,
            body_key: Some(body_key.clone()),
            next_turn_no: self.turn_no + 1,
            content_head: options
                .include_content
                .then(|| body.chars().take(200).collect()),
        });
        let keeps_manual = kind == "compact-summary"
            && self
                .pending
                .as_ref()
                .is_some_and(|p| p.kind == "manual-compact");
        if !keeps_manual {
            self.pending = Some(Pending {
                kind,
                sender,
                pij_msg_id,
                offset: Some(at.offset),
                ts_ms: Some(at.ts_ms),
                chars: Some(chars),
                body_key: Some(body_key),
            });
        }
    }

    fn system(&mut self, record: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        match record.get("subtype").and_then(Value::as_str) {
            Some("compact_boundary") => {
                let empty = Map::new();
                let meta = record
                    .get("compactMetadata")
                    .and_then(Value::as_object)
                    .unwrap_or(&empty);
                let mut event = self.event(at, PrepEventKind::Compaction, None, None);
                event.trigger = string(meta, "trigger");
                event.pre_tokens = meta.get("preTokens").and_then(Value::as_i64);
                event.post_tokens = meta.get("postTokens").and_then(Value::as_i64);
                event.duration_ms = meta.get("durationMs").and_then(Value::as_i64);
                event.gap_ms = self.gap_since_last_call(at.ts_ms);
                rows.events.push(event);
            }
            Some("away_summary") => {
                let mut event = self.event(at, PrepEventKind::Recap, None, None);
                event.last_context = Some(self.last_call_ctx.iter().sum());
                event.gap_ms = self.gap_since_last_call(at.ts_ms);
                rows.events.push(event);
            }
            Some("scheduled_task_fire") => {
                rows.events
                    .push(self.event(at, PrepEventKind::ScheduledFire, None, None));
                let (source, generation) = self.row_base();
                rows.triggers.push(PrepTriggerRow {
                    source,
                    generation,
                    native_offset: Some(at.offset),
                    native_key: None,
                    ts: Some(at.ts.to_owned()),
                    ts_ms: Some(at.ts_ms),
                    kind: TurnOrigin::Loop,
                    sender: None,
                    pij_msg_id: None,
                    chars: 0,
                    body_key: None,
                    next_turn_no: self.turn_no + 1,
                    content_head: None,
                });
                self.pending = Some(Pending {
                    kind: "loop".into(),
                    sender: None,
                    pij_msg_id: None,
                    offset: Some(at.offset),
                    ts_ms: Some(at.ts_ms),
                    chars: None,
                    body_key: None,
                });
            }
            _ => {}
        }
    }

    fn assistant(
        &mut self,
        record: &Map<String, Value>,
        at: &At<'_>,
        rows: &mut PrepRows,
        batch_calls: &mut HashMap<u64, usize>,
    ) {
        let empty = Map::new();
        let message = record
            .get("message")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        if message.get("model").and_then(Value::as_str) == Some("<synthetic>") {
            let head: String = text_of(message).chars().take(160).collect();
            let subkind = if head.contains("session limit") {
                "session_limit"
            } else if head.contains("weekly limit") {
                "weekly_limit"
            } else {
                "other"
            };
            let mut event = self.event(at, PrepEventKind::LimitNotice, Some(subkind.into()), None);
            event.resets_at = head.find("resets ").map(|at| {
                head[at + "resets ".len()..]
                    .trim()
                    .chars()
                    .take(80)
                    .collect()
            });
            rows.events.push(event);
            return;
        }
        let Some(usage) = message.get("usage").filter(|u| truthy(Some(u))) else {
            return;
        };
        let Some(usage) = usage.as_object() else {
            return;
        };
        let msg_id = string(message, "id");
        let request_id = string(record, "requestId");
        let key = call_key(msg_id.as_deref(), request_id.as_deref());
        let tokens = |object: &Map<String, Value>, field: &str| {
            object.get(field).and_then(Value::as_i64).unwrap_or(0)
        };
        let cache_creation = usage
            .get("cache_creation")
            .filter(|c| truthy(Some(c)))
            .and_then(Value::as_object);
        let mut values = [
            tokens(usage, "input_tokens"),
            cache_creation.map_or(0, |c| tokens(c, "ephemeral_1h_input_tokens")),
            cache_creation.map_or(0, |c| tokens(c, "ephemeral_5m_input_tokens")),
            tokens(usage, "cache_read_input_tokens"),
            tokens(usage, "output_tokens"),
        ];
        let mut basis = if cache_creation.is_some() {
            CacheWriteBasis::Split
        } else {
            CacheWriteBasis::None
        };
        if cache_creation.is_none() {
            let total = tokens(usage, "cache_creation_input_tokens");
            if total != 0 {
                values[if self.is_sub { 2 } else { 1 }] = total;
                basis = if self.is_sub {
                    CacheWriteBasis::Fallback5m
                } else {
                    CacheWriteBasis::Fallback1h
                };
            }
        }
        if self.last_call_key == Some(key) {
            for (slot, value) in self.last_call_ctx.iter_mut().zip(values) {
                *slot = (*slot).max(value);
            }
        }
        if let Some(&index) = batch_calls.get(&key) {
            let row = &mut rows.calls[index];
            merge_max(row, values);
            row.records += 1;
            return;
        }
        let (source, generation) = self.row_base();
        let mut row = PrepCallRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            sighting: CallSighting::First,
            msg_id,
            request_id,
            ts: Some(at.ts.to_owned()),
            ts_ms: Some(at.ts_ms),
            model: string(message, "model"),
            stop_reason: string(message, "stop_reason"),
            input: Some(values[0]),
            cw_1h: Some(values[1]),
            cw_5m: Some(values[2]),
            cache_read: Some(values[3]),
            output: Some(values[4]),
            cache_write_basis: basis,
            is_sidechain: truthy(record.get("isSidechain")),
            gap_ms: None,
            turn_no: None,
            call_in_turn: None,
            records: 1,
        };
        if self.committed.contains(&key) {
            row.sighting = CallSighting::Update;
        } else {
            self.committed.insert(key);
            self.session.calls += 1;
            if let Some(pending) = self.pending.take() {
                self.turn_no += 1;
                self.call_in_turn = 0;
                let (source, generation) = self.row_base();
                rows.turns.push(PrepTurnRow {
                    source,
                    generation,
                    native_offset: Some(at.offset),
                    native_key: None,
                    turn_no: self.turn_no,
                    started_ts: Some(at.ts.to_owned()),
                    started_ts_ms: Some(at.ts_ms),
                    first_call_offset: Some(at.offset),
                    origin: origin(&pending.kind),
                    sender: pending.sender,
                    pij_msg_id: pending.pij_msg_id,
                    opener_offset: pending.offset,
                    opener_ts_ms: pending.ts_ms,
                    opener_chars: pending.chars,
                    body_key: pending.body_key,
                });
                self.session.turns += 1;
            } else if self.turn_no == 0 && !self.turn_zero_emitted {
                let (source, generation) = self.row_base();
                rows.turns.push(PrepTurnRow {
                    source,
                    generation,
                    native_offset: Some(at.offset),
                    native_key: None,
                    turn_no: 0,
                    started_ts: Some(at.ts.to_owned()),
                    started_ts_ms: Some(at.ts_ms),
                    first_call_offset: Some(at.offset),
                    origin: TurnOrigin::Start,
                    sender: None,
                    pij_msg_id: None,
                    opener_offset: None,
                    opener_ts_ms: None,
                    opener_chars: None,
                    body_key: None,
                });
                self.session.turns += 1;
            }
            self.turn_zero_emitted = true;
            self.call_in_turn += 1;
            row.gap_ms = Some(self.gap_since_last_call(at.ts_ms).unwrap_or(-1));
            row.turn_no = Some(self.turn_no);
            row.call_in_turn = Some(self.call_in_turn);
            self.last_call_ts_ms = Some(at.ts_ms);
            self.last_call_key = Some(key);
            self.last_call_ctx = [values[0], values[1], values[2], values[3]];
        }
        batch_calls.insert(key, rows.calls.len());
        rows.calls.push(row);
    }
}

fn merge_max(row: &mut PrepCallRow, values: [i64; 5]) {
    let max = |slot: &mut Option<i64>, value: i64| *slot = Some(slot.unwrap_or(0).max(value));
    max(&mut row.input, values[0]);
    max(&mut row.cw_1h, values[1]);
    max(&mut row.cw_5m, values[2]);
    max(&mut row.cache_read, values[3]);
    max(&mut row.output, values[4]);
}

/// Reference-parser trigger kind → contract origin.
fn origin(kind: &str) -> TurnOrigin {
    match kind {
        "start" => TurnOrigin::Start,
        "human" => TurnOrigin::Human,
        "peer" => TurnOrigin::Peer,
        "task-notification" => TurnOrigin::TaskNotification,
        "coordinator" => TurnOrigin::Coordinator,
        "auto-continuation" => TurnOrigin::AutoContinuation,
        "compact-summary" => TurnOrigin::CompactSummary,
        "manual-compact" => TurnOrigin::ManualCompact,
        "scheduled" => TurnOrigin::Scheduled,
        "loop" => TurnOrigin::Loop,
        "subagent-task" => TurnOrigin::SubagentTask,
        _ => TurnOrigin::Other,
    }
}

/// Python-style truthiness for JSON values (the reference uses `if not x`).
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

fn string(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn parse_ms(ts: &str) -> Option<i64> {
    let parsed = OffsetDateTime::parse(ts, &Rfc3339).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos().div_euclid(1_000_000)).ok()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn call_key(msg_id: Option<&str>, request_id: Option<&str>) -> u64 {
    let mut bytes = Vec::with_capacity(64);
    for part in [msg_id, request_id] {
        match part {
            Some(part) => {
                bytes.push(1);
                bytes.extend_from_slice(part.as_bytes());
            }
            None => bytes.push(0),
        }
        bytes.push(0xff);
    }
    fnv(&bytes)
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn fnv_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv(bytes))
}

fn text_of(message: &Map<String, Value>) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(Value::as_object)
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .map(|part| part.get("text").and_then(Value::as_str).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn is_tool_result(message: &Map<String, Value>) -> bool {
    message
        .get("content")
        .and_then(Value::as_array)
        .and_then(|parts| parts.first())
        .and_then(|part| part.get("type"))
        .and_then(Value::as_str)
        == Some("tool_result")
}

/// Opening-record classification, in the reference parser's order.
fn classify(
    record: &Map<String, Value>,
    text: &str,
    is_sub: bool,
) -> Option<(String, Option<String>)> {
    let origin = record.get("origin").and_then(Value::as_object);
    let origin_kind = origin.and_then(|o| o.get("kind")).and_then(Value::as_str);
    if truthy(record.get("isCompactSummary")) {
        return Some(("compact-summary".into(), None));
    }
    if origin_kind == Some("peer") {
        let sender = from_name(text).or_else(|| pij_rs_from(text).map(|(s, _)| s));
        let sender = sender.or_else(|| {
            origin
                .and_then(|o| o.get("from"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
        return Some(("peer".into(), sender.filter(|s| !s.is_empty())));
    }
    if let Some(kind @ ("human" | "task-notification" | "coordinator" | "auto-continuation")) =
        origin_kind
    {
        return Some((kind.into(), None));
    }
    let head = text.trim_start();
    if head.starts_with("/compact") || head.starts_with("<command-name>/compact") {
        return Some(("manual-compact".into(), None));
    }
    if head.starts_with("Base directory for this skill")
        || truthy(record.get("turnCompanion"))
        || head.starts_with("[Image")
        || head.starts_with("<local-command")
        || head.starts_with("<command-name>")
        || head.starts_with("<system-reminder>")
    {
        return None;
    }
    if record.get("promptSource").and_then(Value::as_str) == Some("system") {
        return Some(("scheduled".into(), None));
    }
    if is_sub {
        return Some(("subagent-task".into(), None));
    }
    Some(("other".into(), None))
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `from-name="([^"]+)"`, first match.
fn from_name(text: &str) -> Option<String> {
    const OPEN: &str = "from-name=\"";
    let mut rest = text;
    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at + OPEN.len()..];
        if let Some(end) = after.find('"')
            && end > 0
        {
            return Some(after[..end].to_owned());
        }
        rest = after;
    }
    None
}

/// `\[pij-rs from ([\w-]+)\]`: the sender and the byte end of the match.
fn pij_rs_from(text: &str) -> Option<(String, usize)> {
    const OPEN: &str = "[pij-rs from ";
    let mut base = 0;
    while let Some(at) = text[base..].find(OPEN) {
        let start = base + at + OPEN.len();
        let len: usize = text[start..]
            .chars()
            .take_while(|c| is_word(*c) || *c == '-')
            .map(char::len_utf8)
            .sum();
        if len > 0 && text[start + len..].starts_with(']') {
            return Some((text[start..start + len].to_owned(), start + len + 1));
        }
        base = base + at + 1;
    }
    None
}

/// `\[pijMessageId:([0-9a-f-]+)\]`.
fn pij_message_id(text: &str) -> Option<String> {
    const OPEN: &str = "[pijMessageId:";
    let mut base = 0;
    while let Some(at) = text[base..].find(OPEN) {
        let start = base + at + OPEN.len();
        let len = text[start..]
            .bytes()
            .take_while(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase() || *b == b'-')
            .count();
        if len > 0 && text[start + len..].starts_with(']') {
            return Some(text[start..start + len].to_owned());
        }
        base = base + at + 1;
    }
    None
}

/// `\b(pij-[a-z]+-[a-z]+)\b`.
fn seat_hint(text: &str) -> Option<String> {
    let mut base = 0;
    while let Some(at) = text[base..].find("pij-") {
        let start = base + at;
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word(c));
        if before_ok {
            let rest = &text[start + 4..];
            let a = rest.bytes().take_while(u8::is_ascii_lowercase).count();
            if a > 0 && rest[a..].starts_with('-') {
                let b = rest[a + 1..]
                    .bytes()
                    .take_while(u8::is_ascii_lowercase)
                    .count();
                let end = start + 4 + a + 1 + b;
                if b > 0 && text[end..].chars().next().is_none_or(|c| !is_word(c)) {
                    return Some(text[start..end].to_owned());
                }
            }
        }
        base = start + 1;
    }
    None
}

/// The pij-rs payload (between `[pij-rs from …]` and `[/pij]`, which excludes the
/// harness's fixed delivery footer) with `[pij…]` tags and cross-session wrappers
/// removed and whitespace collapsed. Hashed for fan-out grouping and the join to
/// pij's `message.pushed` bodies; never stored.
fn normalised_body(text: &str) -> String {
    let body = pij_rs_from(text).map_or(text, |(_, end)| {
        let rest = &text[end..];
        rest.find("[/pij]").map_or(rest, |close| &rest[..close])
    });
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    loop {
        let tag = rest
            .find("[pij")
            .and_then(|at| rest[at..].find(']').map(|close| (at, at + close + 1)));
        let wrapper = ["<cross-session-message", "</cross-session-message"]
            .iter()
            .filter_map(|open| {
                rest.find(open)
                    .and_then(|at| rest[at..].find('>').map(|close| (at, at + close + 1)))
            })
            .min();
        let next = match (tag, wrapper) {
            (Some(t), Some(w)) => Some(t.min(w)),
            (t, w) => t.or(w),
        };
        match next {
            Some((start, end)) => {
                out.push_str(&rest[..start]);
                rest = &rest[end..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records(lines: &[&str]) -> Vec<NativeRecord> {
        let mut offset = 0;
        lines
            .iter()
            .map(|line| {
                let record = NativeRecord {
                    offset,
                    bytes: line.as_bytes().to_vec(),
                };
                offset += line.len() as u64 + 1;
                record
            })
            .collect()
    }

    const ASSISTANT: &str = r#"{"type":"assistant","timestamp":"2026-09-26T00:00:0SZ","requestId":"req_R","message":{"id":"msg_M","model":"claude-opus-5-5","usage":{"input_tokens":I,"cache_read_input_tokens":100,"output_tokens":O,"cache_creation":{"ephemeral_1h_input_tokens":7,"ephemeral_5m_input_tokens":0}}}}"#;

    fn assistant(second: u8, msg: &str, input: u32, output: u32) -> String {
        ASSISTANT
            .replace("0S", &format!("{second:02}"))
            .replace("msg_M", msg)
            .replace("\"input_tokens\":I", &format!("\"input_tokens\":{input}"))
            .replace(
                "\"output_tokens\":O",
                &format!("\"output_tokens\":{output}"),
            )
    }

    fn fold_all(state: &mut State, lines: &[String]) -> PrepRows {
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        state
            .fold(&PrepInput::Records(records(&lines)), PrepOptions::default())
            .unwrap()
    }

    fn meta() -> PrepSourceMeta {
        PrepSourceMeta::default()
    }

    const SOURCE: &str = "claude-code/default/p/s.jsonl";

    #[test]
    fn repeats_merge_by_maximum_and_cross_batch_repeats_become_updates() {
        let mut state = State::new(&meta(), SOURCE, 0);
        let rows = fold_all(
            &mut state,
            &[assistant(1, "msg_a", 3, 1), assistant(1, "msg_a", 2, 9)],
        );
        assert_eq!(rows.calls.len(), 1);
        assert_eq!(
            (rows.calls[0].input, rows.calls[0].output),
            (Some(3), Some(9))
        );
        // Resume from the saved state as a later run would.
        let saved = state.save();
        let mut resumed = State::load(&meta(), SOURCE, 0, &saved).unwrap();
        let rows = fold_all(
            &mut resumed,
            &[assistant(2, "msg_a", 1, 11), assistant(3, "msg_b", 1, 1)],
        );
        assert_eq!(rows.calls[0].sighting, CallSighting::Update);
        assert_eq!(rows.calls[0].output, Some(11));
        assert_eq!(rows.calls[0].turn_no, None);
        assert_eq!(rows.calls[1].sighting, CallSighting::First);
        assert_eq!(rows.calls[1].gap_ms, Some(2000));
        assert_eq!(resumed.facts().calls, 2);
    }

    #[test]
    fn peer_message_opens_the_next_turn_with_sender_and_no_content() {
        let mut state = State::new(&meta(), SOURCE, 0);
        let peer = r#"{"type":"user","timestamp":"2026-09-26T00:00:05Z","origin":{"kind":"peer"},"message":{"content":"[pij-rs from pij-quiet-heron] secret words [pijMessageId:ab12-cd]"}}"#;
        let rows = fold_all(
            &mut state,
            &[
                assistant(1, "msg_a", 1, 1),
                peer.to_owned(),
                assistant(9, "msg_b", 1, 1),
            ],
        );
        assert_eq!(rows.turns.len(), 2);
        assert_eq!(rows.turns[0].origin, TurnOrigin::Start);
        let turn = &rows.turns[1];
        assert_eq!((turn.turn_no, turn.origin), (1, TurnOrigin::Peer));
        assert_eq!(turn.sender.as_deref(), Some("pij-quiet-heron"));
        assert_eq!(turn.pij_msg_id.as_deref(), Some("ab12-cd"));
        assert_eq!(rows.triggers[0].content_head, None);
        assert_eq!(rows.calls[1].call_in_turn, Some(1));
    }

    #[test]
    fn synthetic_notices_are_events_not_calls() {
        let mut state = State::new(&meta(), SOURCE, 0);
        let synthetic = r#"{"type":"assistant","timestamp":"2026-09-26T00:00:05Z","message":{"model":"<synthetic>","usage":{"input_tokens":0},"content":[{"type":"text","text":"You've hit your weekly limit · resets 3pm"}]}}"#;
        let rows = fold_all(&mut state, &[synthetic.to_owned()]);
        assert!(rows.calls.is_empty());
        assert_eq!(rows.events[0].subkind.as_deref(), Some("weekly_limit"));
        assert_eq!(rows.events[0].resets_at.as_deref(), Some("3pm"));
    }

    #[test]
    fn body_normalisation_strips_pij_envelope() {
        assert_eq!(
            normalised_body(
                "[pij-rs from pij-a-b]  hello\n<cross-session-message x=\"1\">world</cross-session-message> [pij tag]"
            ),
            "hello world"
        );
        assert_eq!(
            normalised_body("Header [pij-rs from pij-a-b] the body [/pij] fixed harness footer"),
            "the body"
        );
        assert_eq!(
            seat_hint("seat pij-quiet-heron, ok"),
            Some("pij-quiet-heron".into())
        );
        assert_eq!(seat_hint("xpij-quiet-heron"), None);
    }
}
