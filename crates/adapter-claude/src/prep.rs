//! Pure Claude Code fold into canonical prep tables.
//!
//! Supplied native records in, metadata facts out; no filesystem, clock or
//! environment. Call, turn and trigger rules port the reference RCA parser
//! (`extract.py`) rule for rule:
//!
//! - Calls are deduplicated per source generation on (`message.id`,
//!   `requestId`), keeping the per-field maximum of input, 1 h / 5 m cache
//!   writes, cache reads and output. A repeat whose first sighting was folded by
//!   an earlier batch or run is an `update` sighting; canonical readers merge it
//!   by maximum and keep the last non-null `stop_reason`.
//! - `usage.cache_creation` splits cache writes into 1 h and 5 m. Without it,
//!   the aggregate `cache_creation_input_tokens` is attributed to 1 h in main
//!   sessions and to 5 m in subagent sources; `cache_write_basis` says which.
//! - `<synthetic>` assistant records are `limit_notice` events, never calls.
//! - User records are classified in the reference order with its sender
//!   precedence and manual-compact exception; the next new call opens the turn.
//!
//! Differences from the reference exist only because the reference windows its
//! input and prep does not:
//!
//! - The reference resets per-file state at its window start, so its first
//!   in-window call has `gap_s = -1` and its turns restart at 0. Prep carries
//!   state across the whole source generation, so rows at a window boundary can
//!   differ; the counters of every call are unaffected.
//! - `gap_ms` is exact integer milliseconds. The reference rounds seconds to
//!   0.1 with Python's binary `round`, which disagrees with decimal half-up at
//!   exact 50 ms ties; `gap_ms >= ttl_ms + 50` reproduces its `gap_s > ttl_s`.
//! - A token field a record does not carry is null, not zero, and a recap
//!   before any call has a null `last_context`.
//!
//! Message text is read only to classify openers and derive hashes. It leaves
//! the fold only as `triggers.content_head`, under explicit opt-in.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Map, Value, json};
use time::{Date, Month, OffsetDateTime, Time, UtcOffset, format_description::well_known::Rfc3339};
use unisphere_core::{
    NativeRecord, PipelineError, PipelineErrorKind,
    prep::{
        CacheWriteBasis, CallSighting, CompactionCounts, CompactionSample, ContextSample,
        ModelSwitch, PREP_CHECKPOINT_FORMAT, PrepCallRow, PrepCheckpoint, PrepEventKind,
        PrepEventRow, PrepFold, PrepFoldSession, PrepInput, PrepOptions, PrepRows, PrepSourceKind,
        PrepSourceMeta, PrepToolUseRow, PrepTriggerRow, PrepTurnRow, SessionFacts, ToolOutcome,
        ToolSighting, TurnOrigin,
    },
};

/// Interpretation policy; bump on any rule change so every source re-emits.
pub const PREP_POLICY_VERSION: &str = "claude-code/prep-v3";

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

/// input, cw_1h, cw_5m, cache_read, output; `None` = not recorded.
type Tokens = [Option<i64>; 5];

fn max_opt(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn merge_tokens(into: &mut Tokens, from: &Tokens) {
    for (slot, value) in into.iter_mut().zip(from) {
        *slot = max_opt(*slot, *value);
    }
}

fn cache_write(tokens: &Tokens) -> Option<i64> {
    Some(tokens[1]?.saturating_add(tokens[2]?))
}

/// `input + cw_1h + cw_5m + cache_read`, only when all are recorded.
fn context(tokens: &Tokens) -> Option<i64> {
    Some(
        tokens[0]?
            .saturating_add(cache_write(tokens)?)
            .saturating_add(tokens[3]?),
    )
}

fn sample_tokens(sample: &mut ContextSample, tokens: &Tokens) {
    sample.input = tokens[0];
    sample.cache_read = tokens[3];
    sample.cache_write = cache_write(tokens);
    sample.total = context(tokens);
}

/// Opening record of a turn not yet opened by a call.
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

impl Pending {
    fn save(&self) -> Value {
        json!({
            "kind": self.kind, "sender": self.sender, "pij_msg_id": self.pij_msg_id,
            "offset": self.offset, "ts_ms": self.ts_ms, "chars": self.chars,
            "body_key": self.body_key,
        })
    }

    fn load(value: &Value) -> Option<Self> {
        let p = value.as_object()?;
        Some(Self {
            kind: p.get("kind")?.as_str()?.to_owned(),
            sender: opt_text(p.get("sender")?)?,
            pij_msg_id: opt_text(p.get("pij_msg_id")?)?,
            offset: opt(p.get("offset")?, Value::as_u64)?,
            ts_ms: opt(p.get("ts_ms")?, Value::as_i64)?,
            chars: opt(p.get("chars")?, Value::as_i64)?,
            body_key: opt_text(p.get("body_key")?)?,
        })
    }
}

/// A new call whose later repeats still change a derived fact.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tracked {
    key: u64,
    ts_ms: i64,
    tokens: Tokens,
}

impl Tracked {
    fn save(&self) -> Value {
        json!({ "key": hex(self.key), "ts_ms": self.ts_ms, "tokens": self.tokens })
    }

    fn load(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let array = object.get("tokens")?.as_array()?;
        let mut tokens: Tokens = [None; 5];
        if array.len() != tokens.len() {
            return None;
        }
        for (slot, value) in tokens.iter_mut().zip(array) {
            *slot = opt(value, Value::as_i64)?;
        }
        Some(Self {
            key: unhex(object.get("key")?)?,
            ts_ms: object.get("ts_ms")?.as_i64()?,
            tokens,
        })
    }
}

/// A tool use still waiting for its result.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenUse {
    name: Option<String>,
    msg_id: Option<String>,
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
    /// Latest new call, any chain: gap origin and recap/compaction context.
    last_call: Option<Tracked>,
    /// Latest new main-chain call behind `session.latest_context`.
    latest_main: Option<Tracked>,
    /// First new main-chain call after the latest compaction boundary.
    first_after: Option<Tracked>,
    awaiting_first_after: bool,
    /// Every call key folded in this generation (64-bit hashes).
    committed: BTreeSet<u64>,
    /// Every tool-use id folded in this generation (64-bit hashes).
    tool_ids: BTreeSet<u64>,
    open_uses: BTreeMap<String, OpenUse>,
    session: SessionFacts,
}

/// JSON null → `Some(None)`; a value `read` accepts → `Some(Some(v))`; else `None`.
fn opt<T>(value: &Value, read: impl Fn(&Value) -> Option<T>) -> Option<Option<T>> {
    match value {
        Value::Null => Some(None),
        value => read(value).map(Some),
    }
}

fn opt_text(value: &Value) -> Option<Option<String>> {
    opt(value, |v| v.as_str().map(str::to_owned))
}

fn opt_tracked(value: &Value) -> Option<Option<Tracked>> {
    opt(value, Tracked::load)
}

fn hex(key: u64) -> String {
    format!("{key:016x}")
}

fn unhex(value: &Value) -> Option<u64> {
    u64::from_str_radix(value.as_str()?, 16).ok()
}

fn key_set(value: &Value) -> Option<BTreeSet<u64>> {
    value.as_array()?.iter().map(unhex).collect()
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
            last_call: None,
            latest_main: None,
            first_after: None,
            awaiting_first_after: false,
            committed: BTreeSet::new(),
            tool_ids: BTreeSet::new(),
            open_uses: BTreeMap::new(),
            session: SessionFacts {
                is_sidechain: meta.is_sub,
                compactions: Some(CompactionCounts::default()),
                ..SessionFacts::default()
            },
        }
    }

    fn save(&self) -> Value {
        let open_uses: Map<String, Value> = self
            .open_uses
            .iter()
            .map(|(id, open)| {
                (
                    id.clone(),
                    json!({ "name": open.name, "msg_id": open.msg_id }),
                )
            })
            .collect();
        json!({
            "turn_no": self.turn_no,
            "call_in_turn": self.call_in_turn,
            "turn_zero_emitted": self.turn_zero_emitted,
            "pending": self.pending.as_ref().map(Pending::save),
            "last_call": self.last_call.as_ref().map(Tracked::save),
            "latest_main": self.latest_main.as_ref().map(Tracked::save),
            "first_after": self.first_after.as_ref().map(Tracked::save),
            "awaiting_first_after": self.awaiting_first_after,
            "committed": self.committed.iter().copied().map(hex).collect::<Vec<_>>(),
            "tool_ids": self.tool_ids.iter().copied().map(hex).collect::<Vec<_>>(),
            "open_uses": open_uses,
            "session": serde_json::to_value(&self.session).unwrap_or(Value::Null),
        })
    }

    /// Strict inverse of [`State::save`]; anything else is refused.
    fn load(meta: &PrepSourceMeta, source: &str, generation: u32, value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let field = |key: &str| object.get(key);
        let open_uses = field("open_uses")?
            .as_object()?
            .iter()
            .map(|(id, open)| {
                let open = open.as_object()?;
                Some((
                    id.clone(),
                    OpenUse {
                        name: opt_text(open.get("name")?)?,
                        msg_id: opt_text(open.get("msg_id")?)?,
                    },
                ))
            })
            .collect::<Option<BTreeMap<_, _>>>()?;
        Some(Self {
            source: source.to_owned(),
            is_sub: meta.is_sub,
            generation,
            turn_no: field("turn_no")?.as_i64()?,
            call_in_turn: field("call_in_turn")?.as_i64()?,
            turn_zero_emitted: field("turn_zero_emitted")?.as_bool()?,
            pending: opt(field("pending")?, Pending::load)?,
            last_call: opt_tracked(field("last_call")?)?,
            latest_main: opt_tracked(field("latest_main")?)?,
            first_after: opt_tracked(field("first_after")?)?,
            awaiting_first_after: field("awaiting_first_after")?.as_bool()?,
            committed: key_set(field("committed")?)?,
            tool_ids: key_set(field("tool_ids")?)?,
            open_uses,
            session: serde_json::from_value(field("session")?.clone()).ok()?,
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
            PrepInput::Records(records) => Ok(self.fold_records(records, options)),
            PrepInput::Snapshot(_) => {
                Err(PipelineError::new(PipelineErrorKind::InvalidInput, None))
            }
        }
    }
}

struct At<'a> {
    offset: u64,
    ts: &'a str,
    ts_ms: i64,
}

impl State {
    fn fold_records(&mut self, records: &[NativeRecord], options: PrepOptions) -> PrepRows {
        let mut rows = PrepRows::default();
        // Calls sighted in this batch, by key, so repeats merge in place.
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
                    let subkind = string(&record, "operation");
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
        rows
    }

    fn observe_session(&mut self, record: &Map<String, Value>, ts: &str, ts_ms: i64) {
        let session = &mut self.session;
        session.records += 1;
        if session.session_id.as_deref().unwrap_or("").is_empty() {
            session.session_id = string(record, "sessionId").filter(|s| !s.is_empty());
        }
        // A subagent source records its parent's session id: the sidechain link.
        if self.is_sub && session.parent_session_id.is_none() {
            session.parent_session_id.clone_from(&session.session_id);
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
        self.last_call.as_ref().map(|last| ts_ms - last.ts_ms)
    }

    fn last_context(&self) -> Option<i64> {
        self.last_call
            .as_ref()
            .and_then(|last| context(&last.tokens))
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
        self.tool_results(record, message, at, rows);
        if is_tool_result(message) {
            return;
        }
        let text = text_of(message);
        if let Some(model) = model_command(&text) {
            self.model_switch(at, model, rows);
        }
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

    /// `tool_result` parts of a user record: result sightings, paired with the
    /// open use by id. A native duration is taken only when the record carries
    /// exactly one result, so it cannot be misattributed.
    fn tool_results(
        &mut self,
        record: &Map<String, Value>,
        message: &Map<String, Value>,
        at: &At<'_>,
        rows: &mut PrepRows,
    ) {
        let Some(parts) = message.get("content").and_then(Value::as_array) else {
            return;
        };
        let results: Vec<&Map<String, Value>> = parts
            .iter()
            .filter_map(Value::as_object)
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
            .collect();
        let duration_ms = match results.as_slice() {
            [_] => record
                .get("toolUseResult")
                .and_then(Value::as_object)
                .and_then(|result| {
                    result
                        .get("durationMs")
                        .or_else(|| result.get("totalDurationMs"))
                })
                .and_then(Value::as_i64),
            _ => None,
        };
        for part in results {
            let tool_use_id = string(part, "tool_use_id");
            let open = tool_use_id
                .as_ref()
                .and_then(|id| self.open_uses.remove(id));
            let (name, call_msg_id) = open.map_or((None, None), |open| (open.name, open.msg_id));
            let (source, generation) = self.row_base();
            rows.tool_uses.push(PrepToolUseRow {
                source,
                generation,
                native_offset: Some(at.offset),
                native_key: None,
                sighting: ToolSighting::Result,
                tool_use_id,
                call_msg_id,
                ts: Some(at.ts.to_owned()),
                ts_ms: Some(at.ts_ms),
                family: family_of(name.as_deref()),
                name,
                input_hash: None,
                input_bytes: None,
                result_offset: Some(at.offset),
                result_bytes: part.get("content").map(result_bytes),
                outcome: Some(match part.get("is_error") {
                    Some(Value::Bool(true)) => ToolOutcome::Error,
                    Some(Value::Bool(false)) => ToolOutcome::Ok,
                    _ => ToolOutcome::Unknown,
                }),
                duration_ms,
                turn_no: Some(self.turn_no),
            });
        }
    }

    fn model_switch(&mut self, at: &At<'_>, model: String, rows: &mut PrepRows) {
        let mut event = self.event(at, PrepEventKind::ModelSwitch, None, None);
        event.model = Some(model.clone());
        rows.events.push(event);
        self.session.last_model_switch = Some(ModelSwitch {
            ts_ms: Some(at.ts_ms),
            requested_model: model,
        });
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
                event.last_context = self.last_context();
                event.gap_ms = self.gap_since_last_call(at.ts_ms);
                let counts = self
                    .session
                    .compactions
                    .get_or_insert_with(CompactionCounts::default);
                match event.trigger.as_deref() {
                    Some("manual") => counts.manual += 1,
                    Some("auto") => counts.auto += 1,
                    _ => counts.unknown_trigger += 1,
                }
                self.session.last_compaction = Some(CompactionSample {
                    ts_ms: Some(at.ts_ms),
                    trigger: event.trigger.clone(),
                    pre_tokens: event.pre_tokens,
                    post_tokens: event.post_tokens,
                    first_context_after: None,
                });
                self.first_after = None;
                self.awaiting_first_after = true;
                rows.events.push(event);
            }
            Some("away_summary") => {
                let mut event = self.event(at, PrepEventKind::Recap, None, None);
                event.last_context = self.last_context();
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
            Some("local_command") => {
                if let Some(model) = record
                    .get("content")
                    .and_then(Value::as_str)
                    .and_then(model_command)
                {
                    self.model_switch(at, model, rows);
                }
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
            self.limit_notice(message, at, rows);
            return;
        }
        self.call(record, message, at, rows, batch_calls);
        self.tool_uses(message, at, rows);
    }

    /// A client-generated `<synthetic>` notice: never an API call.
    fn limit_notice(&self, message: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        let head: String = text_of(message).chars().take(160).collect();
        let subkind = if head.contains("session limit") {
            "session_limit"
        } else if head.contains("weekly limit") {
            "weekly_limit"
        } else {
            "other"
        };
        let mut event = self.event(at, PrepEventKind::LimitNotice, Some(subkind.into()), None);
        event.resets_at = head.find("resets ").map(|start| {
            let phrase = head[start + "resets ".len()..].trim();
            let end = phrase.find(')').map_or(phrase.len(), |close| close + 1);
            phrase[..end].chars().take(80).collect()
        });
        event.resets_at_ms = event
            .resets_at
            .as_deref()
            .and_then(|phrase| reset_instant(phrase, at.ts_ms));
        rows.events.push(event);
    }

    fn tokens(&self, usage: &Map<String, Value>) -> (Tokens, CacheWriteBasis) {
        let int =
            |object: &Map<String, Value>, field: &str| object.get(field).and_then(Value::as_i64);
        let mut tokens: Tokens = [
            int(usage, "input_tokens"),
            None,
            None,
            int(usage, "cache_read_input_tokens"),
            int(usage, "output_tokens"),
        ];
        let split = usage
            .get("cache_creation")
            .filter(|c| truthy(Some(c)))
            .and_then(Value::as_object);
        let basis = match (split, int(usage, "cache_creation_input_tokens")) {
            (Some(split), _) => {
                tokens[1] = int(split, "ephemeral_1h_input_tokens");
                tokens[2] = int(split, "ephemeral_5m_input_tokens");
                CacheWriteBasis::Split
            }
            (None, Some(total)) if self.is_sub => {
                (tokens[1], tokens[2]) = (Some(0), Some(total));
                CacheWriteBasis::Fallback5m
            }
            (None, Some(total)) => {
                (tokens[1], tokens[2]) = (Some(total), Some(0));
                CacheWriteBasis::Fallback1h
            }
            (None, None) => CacheWriteBasis::None,
        };
        (tokens, basis)
    }

    fn call(
        &mut self,
        record: &Map<String, Value>,
        message: &Map<String, Value>,
        at: &At<'_>,
        rows: &mut PrepRows,
        batch_calls: &mut HashMap<u64, usize>,
    ) {
        let Some(usage) = message
            .get("usage")
            .filter(|u| truthy(Some(u)))
            .and_then(Value::as_object)
        else {
            return;
        };
        let msg_id = string(message, "id");
        let request_id = string(record, "requestId");
        let key = call_key(msg_id.as_deref(), request_id.as_deref());
        let (tokens, basis) = self.tokens(usage);
        let stop_reason = string(message, "stop_reason");
        self.repeat_facts(key, &tokens, stop_reason.as_deref());
        if let Some(&index) = batch_calls.get(&key) {
            let row = &mut rows.calls[index];
            row.input = max_opt(row.input, tokens[0]);
            row.cw_1h = max_opt(row.cw_1h, tokens[1]);
            row.cw_5m = max_opt(row.cw_5m, tokens[2]);
            row.cache_read = max_opt(row.cache_read, tokens[3]);
            row.output = max_opt(row.output, tokens[4]);
            if stop_reason.is_some() {
                row.stop_reason = stop_reason;
            }
            row.records += 1;
            return;
        }
        let is_sidechain = self.is_sub || truthy(record.get("isSidechain"));
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
            stop_reason,
            input: tokens[0],
            cw_1h: tokens[1],
            cw_5m: tokens[2],
            cache_read: tokens[3],
            output: tokens[4],
            cache_write_basis: basis,
            is_sidechain,
            gap_ms: None,
            turn_no: None,
            call_in_turn: None,
            records: 1,
        };
        if self.committed.contains(&key) {
            row.sighting = CallSighting::Update;
        } else {
            self.new_call(key, tokens, at, &mut row, rows);
        }
        batch_calls.insert(key, rows.calls.len());
        rows.calls.push(row);
    }

    /// A repeat raises the facts derived from its first sighting.
    fn repeat_facts(&mut self, key: u64, tokens: &Tokens, stop_reason: Option<&str>) {
        if let Some(last) = self.last_call.as_mut().filter(|t| t.key == key) {
            merge_tokens(&mut last.tokens, tokens);
        }
        if let Some(main) = self.latest_main.as_mut().filter(|t| t.key == key) {
            merge_tokens(&mut main.tokens, tokens);
            if let Some(sample) = self.session.latest_context.as_mut() {
                sample_tokens(sample, &main.tokens);
                if let Some(stop_reason) = stop_reason {
                    sample.stop_reason = Some(stop_reason.to_owned());
                }
            }
        }
        if let Some(first) = self.first_after.as_mut().filter(|t| t.key == key)
            && let Some(compaction) = self.session.last_compaction.as_mut()
        {
            merge_tokens(&mut first.tokens, tokens);
            compaction.first_context_after = context(&first.tokens);
        }
    }

    /// First sighting of a call in this generation: ordering facts, turns and
    /// session facts.
    fn new_call(
        &mut self,
        key: u64,
        tokens: Tokens,
        at: &At<'_>,
        row: &mut PrepCallRow,
        rows: &mut PrepRows,
    ) {
        self.committed.insert(key);
        self.session.calls += 1;
        if let Some(pending) = self.pending.take() {
            self.turn_no += 1;
            self.call_in_turn = 0;
            self.push_turn(at, origin(&pending.kind), Some(pending), rows);
        } else if self.turn_no == 0 && !self.turn_zero_emitted {
            self.push_turn(at, TurnOrigin::Start, None, rows);
        }
        self.turn_zero_emitted = true;
        self.call_in_turn += 1;
        row.gap_ms = Some(self.gap_since_last_call(at.ts_ms).unwrap_or(-1));
        row.turn_no = Some(self.turn_no);
        row.call_in_turn = Some(self.call_in_turn);
        let tracked = Tracked {
            key,
            ts_ms: at.ts_ms,
            tokens,
        };
        if !row.is_sidechain {
            let mut sample = ContextSample {
                ts_ms: Some(at.ts_ms),
                model: row.model.clone(),
                stop_reason: row.stop_reason.clone(),
                input: None,
                cache_read: None,
                cache_write: None,
                total: None,
            };
            sample_tokens(&mut sample, &tokens);
            self.session.latest_context = Some(sample);
            self.latest_main = Some(tracked.clone());
            if self.awaiting_first_after {
                self.awaiting_first_after = false;
                if let Some(compaction) = self.session.last_compaction.as_mut() {
                    compaction.first_context_after = context(&tokens);
                }
                self.first_after = Some(tracked.clone());
            }
        }
        self.last_call = Some(tracked);
    }

    fn push_turn(
        &mut self,
        at: &At<'_>,
        origin: TurnOrigin,
        opener: Option<Pending>,
        rows: &mut PrepRows,
    ) {
        let (source, generation) = self.row_base();
        let opener = opener.as_ref();
        rows.turns.push(PrepTurnRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            turn_no: self.turn_no,
            started_ts: Some(at.ts.to_owned()),
            started_ts_ms: Some(at.ts_ms),
            first_call_offset: Some(at.offset),
            origin,
            sender: opener.and_then(|p| p.sender.clone()),
            pij_msg_id: opener.and_then(|p| p.pij_msg_id.clone()),
            opener_offset: opener.and_then(|p| p.offset),
            opener_ts_ms: opener.and_then(|p| p.ts_ms),
            opener_chars: opener.and_then(|p| p.chars),
            body_key: opener.and_then(|p| p.body_key.clone()),
        });
        self.session.turns += 1;
    }

    /// `tool_use` parts of an assistant record: use sightings, deduplicated by
    /// native id across batches and runs of this generation.
    fn tool_uses(&mut self, message: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        let Some(parts) = message.get("content").and_then(Value::as_array) else {
            return;
        };
        let msg_id = string(message, "id");
        for part in parts.iter().filter_map(Value::as_object) {
            if part.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let tool_use_id = string(part, "id");
            if let Some(id) = &tool_use_id
                && !self.tool_ids.insert(fnv(id.as_bytes()))
            {
                continue;
            }
            let name = string(part, "name");
            let input = part.get("input").map(|input| {
                let mut bytes = Vec::new();
                canonical_json(input, &mut bytes);
                bytes
            });
            if let Some(id) = &tool_use_id {
                self.open_uses.insert(
                    id.clone(),
                    OpenUse {
                        name: name.clone(),
                        msg_id: msg_id.clone(),
                    },
                );
            }
            let (source, generation) = self.row_base();
            rows.tool_uses.push(PrepToolUseRow {
                source,
                generation,
                native_offset: Some(at.offset),
                native_key: None,
                sighting: ToolSighting::Use,
                tool_use_id,
                call_msg_id: msg_id.clone(),
                ts: Some(at.ts.to_owned()),
                ts_ms: Some(at.ts_ms),
                family: family_of(name.as_deref()),
                name,
                input_hash: input.as_deref().map(fnv_hex),
                input_bytes: input.map(|bytes| len_i64(bytes.len())),
                result_offset: None,
                result_bytes: None,
                outcome: None,
                duration_ms: None,
                turn_no: Some(self.turn_no),
            });
        }
    }
}

/// The query mapping's tool-family vocabulary; unknown names stay null.
fn family_of(name: Option<&str>) -> Option<String> {
    name.and_then(super::tool_family).map(str::to_owned)
}

fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

/// Result size: UTF-8 bytes of a text result, else canonical JSON bytes.
fn result_bytes(content: &Value) -> i64 {
    match content {
        Value::String(text) => len_i64(text.len()),
        other => {
            let mut bytes = Vec::new();
            canonical_json(other, &mut bytes);
            len_i64(bytes.len())
        }
    }
}

/// Compact JSON with object keys sorted, so a hash depends only on the value.
fn canonical_json(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            out.push(b'{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                // Writing into a Vec cannot fail.
                let _ = serde_json::to_writer(&mut *out, key);
                out.push(b':');
                canonical_json(&object[key], out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                canonical_json(item, out);
            }
            out.push(b']');
        }
        scalar => {
            let _ = serde_json::to_writer(&mut *out, scalar);
        }
    }
}

/// Requested model of a native `/model` command record (user text or
/// `local_command` content); `None` without explicit arguments, because the
/// interactive picker's choice is not in the command record.
fn model_command(text: &str) -> Option<String> {
    const ARGS: &str = "<command-args>";
    let rest = text
        .trim_start()
        .strip_prefix("<command-name>/model</command-name>")?;
    let start = rest.find(ARGS)? + ARGS.len();
    let end = start + rest[start..].find("</command-args>")?;
    let model = rest[start..end].trim();
    (!model.is_empty()).then(|| model.to_owned())
}

/// Reset instant of a limit phrase such as `3:10pm (Australia/Brisbane)` or
/// `Oct 9 at 9am (Australia/Brisbane)`: the first instant at or after the
/// notice that matches it. Only fixed-offset zones are resolved; a zone with
/// daylight-saving rules needs a time-zone database this pure fold does not
/// carry, so its instant stays null rather than guessed.
fn reset_instant(phrase: &str, notice_ms: i64) -> Option<i64> {
    let (when, zone) = phrase.split_once(" (")?;
    let offset = UtcOffset::from_whole_seconds(fixed_offset(zone.strip_suffix(')')?)?).ok()?;
    let words: Vec<&str> = when.split_whitespace().collect();
    let (clock, date_words) = words.split_last()?;
    let time = clock_time(clock)?;
    let notice = OffsetDateTime::from_unix_timestamp_nanos(i128::from(notice_ms) * 1_000_000)
        .ok()?
        .to_offset(offset);
    let at = |date: Date| date.with_time(time).assume_offset(offset);
    let reset = match date_words {
        [] => {
            let today = at(notice.date());
            if today >= notice {
                today
            } else {
                at(notice.date().next_day()?)
            }
        }
        [month, day] | [month, day, "at"] => {
            let month = month_of(month)?;
            let day: u8 = day.trim_end_matches(',').parse().ok()?;
            let this_year = at(Date::from_calendar_date(notice.year(), month, day).ok()?);
            if this_year >= notice {
                this_year
            } else {
                at(Date::from_calendar_date(notice.year() + 1, month, day).ok()?)
            }
        }
        _ => return None,
    };
    i64::try_from(reset.unix_timestamp_nanos().div_euclid(1_000_000)).ok()
}

/// `9pm`, `3:10pm`, `12am` or 24-hour `21:30`.
fn clock_time(word: &str) -> Option<Time> {
    let lower = word.to_ascii_lowercase();
    let (digits, meridiem) = match (lower.strip_suffix("am"), lower.strip_suffix("pm")) {
        (Some(digits), _) => (digits, Some(0)),
        (_, Some(digits)) => (digits, Some(12)),
        _ => (lower.as_str(), None),
    };
    let (hour, minute) = match digits.split_once(':') {
        Some((hour, minute)) => (hour.parse::<u8>().ok()?, minute.parse::<u8>().ok()?),
        None => (digits.parse::<u8>().ok()?, 0),
    };
    let hour = match meridiem {
        Some(base) if (1..=12).contains(&hour) => hour % 12 + base,
        Some(_) => return None,
        None if digits.contains(':') => hour,
        None => return None,
    };
    Time::from_hms(hour, minute, 0).ok()
}

fn month_of(word: &str) -> Option<Month> {
    let prefix = word.get(..3)?.to_ascii_lowercase();
    let month = match prefix.as_str() {
        "jan" => Month::January,
        "feb" => Month::February,
        "mar" => Month::March,
        "apr" => Month::April,
        "may" => Month::May,
        "jun" => Month::June,
        "jul" => Month::July,
        "aug" => Month::August,
        "sep" => Month::September,
        "oct" => Month::October,
        "nov" => Month::November,
        "dec" => Month::December,
        _ => return None,
    };
    word.chars()
        .all(|c| c.is_ascii_alphabetic() || c == '.')
        .then_some(month)
}

/// Offset in seconds of a zone that observes no daylight saving.
fn fixed_offset(zone: &str) -> Option<i32> {
    const H: i32 = 3600;
    match zone {
        "UTC" | "Etc/UTC" | "GMT" | "Etc/GMT" | "Etc/Universal" | "Universal" | "Etc/Zulu"
        | "Zulu" => Some(0),
        "Australia/Brisbane" | "Australia/Lindeman" => Some(10 * H),
        "Australia/Darwin" => Some(9 * H + H / 2),
        "Asia/Tokyo" | "Asia/Seoul" => Some(9 * H),
        "Australia/Perth" | "Asia/Shanghai" | "Asia/Hong_Kong" | "Asia/Taipei"
        | "Asia/Singapore" | "Asia/Manila" | "Asia/Kuala_Lumpur" => Some(8 * H),
        "Asia/Bangkok" | "Asia/Jakarta" | "Asia/Ho_Chi_Minh" => Some(7 * H),
        "Asia/Kolkata" | "Asia/Calcutta" => Some(5 * H + H / 2),
        "Asia/Dubai" => Some(4 * H),
        "America/Phoenix" => Some(-7 * H),
        "Pacific/Honolulu" => Some(-10 * H),
        // POSIX-style names invert the sign: Etc/GMT-10 is UTC+10.
        _ => zone
            .strip_prefix("Etc/GMT")
            .and_then(|hours| hours.parse::<i32>().ok())
            .filter(|hours| (-14..=12).contains(hours))
            .map(|hours| -hours * H),
    }
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

    fn ms(ts: &str) -> i64 {
        parse_ms(ts).unwrap()
    }

    #[test]
    fn reset_phrases_resolve_to_the_next_matching_instant_in_fixed_zones() {
        // 10:22 in Brisbane (UTC+10, no daylight saving).
        let notice = ms("2026-01-10T00:22:00Z");
        assert_eq!(
            reset_instant("3:10pm (Australia/Brisbane)", notice),
            Some(ms("2026-01-10T05:10:00Z"))
        );
        // Already past 9am local: the next 9am.
        assert_eq!(
            reset_instant("9am (Australia/Brisbane)", notice),
            Some(ms("2026-01-10T23:00:00Z"))
        );
        assert_eq!(
            reset_instant("12am (UTC)", notice),
            Some(ms("2026-01-11T00:00:00Z"))
        );
        assert_eq!(
            reset_instant("Jan 12 at 9am (Australia/Brisbane)", notice),
            Some(ms("2026-01-11T23:00:00Z"))
        );
        // A calendar date already behind the notice is next year's.
        assert_eq!(
            reset_instant("Jan 2 at 9am (Etc/GMT-10)", notice),
            Some(ms("2027-01-01T23:00:00Z"))
        );
    }

    #[test]
    fn reset_phrases_without_a_resolvable_zone_or_clock_stay_null() {
        let notice = ms("2026-01-10T00:22:00Z");
        for phrase in [
            "3pm (Europe/London)",
            "3pm",
            "soon (UTC)",
            "13pm (UTC)",
            "Foo 9 at 9am (UTC)",
        ] {
            assert_eq!(reset_instant(phrase, notice), None, "{phrase}");
        }
    }

    #[test]
    fn model_commands_need_explicit_arguments() {
        let command = |args: &str| {
            format!(
                "<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args>{args}</command-args>"
            )
        };
        assert_eq!(model_command(&command(" opus ")), Some("opus".into()));
        assert_eq!(model_command(&command("")), None);
        assert_eq!(
            model_command("<command-name>/modelx</command-name><command-args>a</command-args>"),
            None
        );
        assert_eq!(model_command("say /model opus"), None);
    }

    #[test]
    fn canonical_json_ignores_key_order() {
        let a: Value = serde_json::from_str(r#"{"b":[1,{"y":2,"x":"s"}],"a":null}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"a":null,"b":[1,{"x":"s","y":2}]}"#).unwrap();
        let (mut left, mut right) = (Vec::new(), Vec::new());
        canonical_json(&a, &mut left);
        canonical_json(&b, &mut right);
        assert_eq!(left, right);
        assert_eq!(left, br#"{"a":null,"b":[1,{"x":"s","y":2}]}"#);
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
