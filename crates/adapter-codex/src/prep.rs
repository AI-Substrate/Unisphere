//! Pure Codex rollout fold into canonical prep tables.
//!
//! Supplied native records in, metadata facts out; no filesystem, clock or
//! environment. Rules:
//!
//! - **Calls.** One call per model response. Per-call counters come from the
//!   `last_token_usage` of `event_msg/token_count` or the `usage` of a
//!   `token_usage_record`; cumulative totals are never summed. A token_count
//!   whose cumulative `total_token_usage` equals the previous token_count's is a
//!   re-emission of that call, and a token_count/token_usage_record pair with
//!   equal per-call usage is one call sighted twice. A `token_usage_record` call
//!   is keyed by its native `response_id` (`msg_id`), so later sightings are
//!   merged in the batch or emitted as `update` sightings; a token_count-only
//!   call has no native id, so its re-emissions are dropped rather than merged.
//! - **Token classes.** Codex `input_tokens` includes cache reads and cache
//!   writes, so `input` is `input_tokens − cached_input_tokens −
//!   cache_write_input_tokens` (native `non_cached_input` minus native writes),
//!   `cache_read` is `cached_input_tokens`. Cache writes exist only where the
//!   record carries `cache_write_input_tokens`: an aggregate with no TTL split,
//!   attributed like other aggregate-only dialects (1 h main, 5 m sidechain).
//!   Without it `cw_1h`/`cw_5m` are null, basis `none`, and any unrecorded writes
//!   stay inside `input`, so `input + cache_read` is still the native context.
//! - **Context window.** The latest `token_count.info.model_context_window`
//!   (the harness's own window for the model in use) is
//!   `SessionFacts.context_window`; without one it stays null.
//! - **Model** is the latest `turn_context.model` or
//!   `thread_settings_applied.thread_settings.model`; a change is a
//!   `model_switch` event. Codex records no stop reason: `stop_reason` is null.
//! - **Turns.** `task_started` opens a pending `other` turn; a later
//!   `event_msg/user_message` replaces it as `human` (`peer` with a pij envelope,
//!   `subagent-task` in a subagent thread) and an inter-agent `agent_message`
//!   as `subagent-task` (parent → child), `task-notification` (child → parent) or
//!   `peer`, unless its metadata says it does not trigger a turn. The next new
//!   call opens the turn. Developer and injected user response items are not
//!   triggers. Codex writes a response's items before its usage, so tool uses
//!   and events seen while an opener is pending carry the turn it will open.
//! - **Events.** `compacted` → `compaction` (Codex records no trigger or
//!   pre-compaction count for it; the paired `context_compacted` marker is not
//!   counted again); `turn_aborted` → `system_other`; `error`/`stream_error` →
//!   `api_error`. The first `token_count` after a compaction whose
//!   `last_token_usage` counts no input or output carries Codex's own
//!   post-compaction context in `total_tokens`: that is
//!   `last_compaction.post_tokens`.
//! - **Tool uses.** `function_call`/`custom_tool_call` → use; their outputs →
//!   result. The outcome and duration come from the native end event
//!   (`exec_command_end` exit code, `patch_apply_end` success,
//!   `mcp_tool_call_end` result), else the output's structured
//!   `metadata.exit_code`/`duration_seconds`, else the harness-written header
//!   (`Exit code: N` / `Process exited with code N`, `Wall time: S seconds`)
//!   before its `Output:` line; otherwise null. An end event after its output
//!   is a second result sighting.
//! - **Session.** `session_meta` (or the legacy header) gives the thread id,
//!   cwd, `parent_thread_id` (subagent: sidechain) or `forked_from_id` as
//!   `parent_session_id`. Legacy records without a timestamp are folded with
//!   null timestamps.
//!
//! Message text is read only to classify openers and derive hashes. It leaves
//! the fold only as `triggers.content_head`, under explicit opt-in.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
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

use super::{DESCRIPTOR, codex_tool_family};

/// Interpretation policy; bump on any rule change so every source re-emits.
pub const PREP_POLICY_VERSION: &str = "codex/prep-v2";

#[derive(Debug, Clone, Copy, Default)]
pub struct CodexPrepFold;

impl PrepFold for CodexPrepFold {
    fn harness(&self) -> &'static str {
        DESCRIPTOR.id
    }
    fn policy(&self) -> &'static str {
        PREP_POLICY_VERSION
    }
    fn kind(&self) -> PrepSourceKind {
        PrepSourceKind::Append
    }
    fn pattern(&self) -> &'static str {
        DESCRIPTOR.locations[0].session_glob
    }
    /// A rollout path carries only its date and thread id; whether it is a
    /// subagent thread and its cwd are native facts read from `session_meta`.
    fn describe(&self, _file: &str) -> PrepSourceMeta {
        PrepSourceMeta::default()
    }
    fn open(
        &self,
        _meta: &PrepSourceMeta,
        source: &str,
        generation: u32,
        saved: Option<&PrepCheckpoint>,
    ) -> Result<Box<dyn PrepFoldSession>, PipelineError> {
        let invalid = || PipelineError::new(PipelineErrorKind::InvalidData, None);
        let state = match saved {
            None => State::new(source, generation),
            Some(checkpoint)
                if checkpoint.format == PREP_CHECKPOINT_FORMAT
                    && checkpoint.policy == PREP_POLICY_VERSION =>
            {
                State::load(source, generation, &checkpoint.fold).ok_or_else(invalid)?
            }
            Some(_) => return Err(invalid()),
        };
        Ok(Box::new(state))
    }
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

/// Recorded cache writes; `None` when the record carries none.
fn cache_write(tokens: &Tokens) -> Option<i64> {
    Some(tokens[1]?.saturating_add(tokens[2]?))
}

/// Context of a call: `input + cache_read + cache writes`. Unrecorded writes
/// are already inside `input` (Codex `input_tokens` includes them), so the sum
/// is the native `input_tokens` either way.
fn context(tokens: &Tokens) -> Option<i64> {
    Some(
        tokens[0]?
            .saturating_add(tokens[3]?)
            .saturating_add(cache_write(tokens).unwrap_or(0)),
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

/// A new call whose later sightings still change a derived fact.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tracked {
    /// Native call key; `None` for a call without a native id.
    key: Option<u64>,
    ts_ms: Option<i64>,
    tokens: Tokens,
}

impl Tracked {
    fn save(&self) -> Value {
        json!({ "key": self.key.map(hex), "ts_ms": self.ts_ms, "tokens": self.tokens })
    }

    fn load(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        Some(Self {
            key: opt(object.get("key")?, unhex)?,
            ts_ms: opt(object.get("ts_ms")?, Value::as_i64)?,
            tokens: load_tokens(object.get("tokens")?)?,
        })
    }
}

fn load_tokens(value: &Value) -> Option<Tokens> {
    let array = value.as_array()?;
    let mut tokens: Tokens = [None; 5];
    if array.len() != tokens.len() {
        return None;
    }
    for (slot, value) in tokens.iter_mut().zip(array) {
        *slot = opt(value, Value::as_i64)?;
    }
    Some(tokens)
}

/// The latest per-call usage sighting, which decides whether the next one is
/// the same call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct UsageMark {
    /// Sighted by a `token_usage_record` (else a `token_count`).
    from_record: bool,
    /// Fingerprint of the per-call counters.
    usage: u64,
    /// token_count: fingerprint of the cumulative total.
    total: Option<u64>,
    /// Native response id of the call, when known.
    msg_id: Option<String>,
    /// Already matched with a sighting of the other record type.
    paired: bool,
}

impl UsageMark {
    fn save(&self) -> Value {
        json!({
            "from_record": self.from_record, "usage": hex(self.usage),
            "total": self.total.map(hex), "msg_id": self.msg_id, "paired": self.paired,
        })
    }

    fn load(value: &Value) -> Option<Self> {
        let m = value.as_object()?;
        Some(Self {
            from_record: m.get("from_record")?.as_bool()?,
            usage: unhex(m.get("usage")?)?,
            total: opt(m.get("total")?, unhex)?,
            msg_id: opt_text(m.get("msg_id")?)?,
            paired: m.get("paired")?.as_bool()?,
        })
    }
}

/// Native outcome of a tool call reported by an end event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct End {
    outcome: Option<ToolOutcome>,
    duration_ms: Option<i64>,
}

impl End {
    fn save(&self) -> Value {
        json!({ "outcome": self.outcome, "duration_ms": self.duration_ms })
    }

    fn load(value: &Value) -> Option<Self> {
        let e = value.as_object()?;
        Some(Self {
            outcome: serde_json::from_value(e.get("outcome")?.clone()).ok()?,
            duration_ms: opt(e.get("duration_ms")?, Value::as_i64)?,
        })
    }
}

#[derive(Debug, Clone)]
struct State {
    source: String,
    generation: u32,
    /// Model of the latest `turn_context`.
    model: Option<String>,
    turn_no: i64,
    call_in_turn: i64,
    turn_zero_emitted: bool,
    pending: Option<Pending>,
    /// `trigger_turn` of the latest inter-agent metadata, for the next message.
    agent_trigger: Option<bool>,
    usage: Option<UsageMark>,
    /// Latest new call: gap origin and compaction context.
    last_call: Option<Tracked>,
    /// Latest new main-chain call behind `session.latest_context`.
    latest_main: Option<Tracked>,
    /// First new main-chain call after the latest compaction boundary.
    first_after: Option<Tracked>,
    awaiting_first_after: bool,
    /// Every keyed call folded in this generation (64-bit hashes).
    committed: BTreeSet<u64>,
    /// Every tool call id with a use sighting (64-bit hashes).
    tool_ids: BTreeSet<u64>,
    /// Every tool call id with a result sighting (64-bit hashes).
    answered: BTreeSet<u64>,
    /// Tool name by call id, until its output.
    open_uses: BTreeMap<String, Option<String>>,
    /// End events that arrived before their output, by call id.
    ends: BTreeMap<String, End>,
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

fn keys(set: &BTreeSet<u64>) -> Vec<String> {
    set.iter().copied().map(hex).collect()
}

impl State {
    fn new(source: &str, generation: u32) -> Self {
        Self {
            source: source.to_owned(),
            generation,
            model: None,
            turn_no: 0,
            call_in_turn: 0,
            turn_zero_emitted: false,
            pending: None,
            agent_trigger: None,
            usage: None,
            last_call: None,
            latest_main: None,
            first_after: None,
            awaiting_first_after: false,
            committed: BTreeSet::new(),
            tool_ids: BTreeSet::new(),
            answered: BTreeSet::new(),
            open_uses: BTreeMap::new(),
            ends: BTreeMap::new(),
            session: SessionFacts {
                compactions: Some(CompactionCounts::default()),
                ..SessionFacts::default()
            },
        }
    }

    fn save(&self) -> Value {
        let open_uses: Map<String, Value> = self
            .open_uses
            .iter()
            .map(|(id, name)| (id.clone(), json!(name)))
            .collect();
        let ends: Map<String, Value> = self
            .ends
            .iter()
            .map(|(id, end)| (id.clone(), end.save()))
            .collect();
        json!({
            "model": self.model,
            "turn_no": self.turn_no,
            "call_in_turn": self.call_in_turn,
            "turn_zero_emitted": self.turn_zero_emitted,
            "pending": self.pending.as_ref().map(Pending::save),
            "agent_trigger": self.agent_trigger,
            "usage": self.usage.as_ref().map(UsageMark::save),
            "last_call": self.last_call.as_ref().map(Tracked::save),
            "latest_main": self.latest_main.as_ref().map(Tracked::save),
            "first_after": self.first_after.as_ref().map(Tracked::save),
            "awaiting_first_after": self.awaiting_first_after,
            "committed": keys(&self.committed),
            "tool_ids": keys(&self.tool_ids),
            "answered": keys(&self.answered),
            "open_uses": open_uses,
            "ends": ends,
            "session": serde_json::to_value(&self.session).unwrap_or(Value::Null),
        })
    }

    /// Strict inverse of [`State::save`]; anything else is refused.
    fn load(source: &str, generation: u32, value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let field = |key: &str| object.get(key);
        let open_uses = field("open_uses")?
            .as_object()?
            .iter()
            .map(|(id, name)| Some((id.clone(), opt_text(name)?)))
            .collect::<Option<BTreeMap<_, _>>>()?;
        let ends = field("ends")?
            .as_object()?
            .iter()
            .map(|(id, end)| Some((id.clone(), End::load(end)?)))
            .collect::<Option<BTreeMap<_, _>>>()?;
        Some(Self {
            source: source.to_owned(),
            generation,
            model: opt_text(field("model")?)?,
            turn_no: field("turn_no")?.as_i64()?,
            call_in_turn: field("call_in_turn")?.as_i64()?,
            turn_zero_emitted: field("turn_zero_emitted")?.as_bool()?,
            pending: opt(field("pending")?, Pending::load)?,
            agent_trigger: opt(field("agent_trigger")?, Value::as_bool)?,
            usage: opt(field("usage")?, UsageMark::load)?,
            last_call: opt_tracked(field("last_call")?)?,
            latest_main: opt_tracked(field("latest_main")?)?,
            first_after: opt_tracked(field("first_after")?)?,
            awaiting_first_after: field("awaiting_first_after")?.as_bool()?,
            committed: key_set(field("committed")?)?,
            tool_ids: key_set(field("tool_ids")?)?,
            answered: key_set(field("answered")?)?,
            open_uses,
            ends,
            session: serde_json::from_value(field("session")?.clone()).ok()?,
        })
    }

    fn row_base(&self) -> (String, u32) {
        (self.source.clone(), self.generation)
    }

    /// Turn a non-call record belongs to. Codex writes a response's items
    /// before its usage record, so with a pending opener they belong to the
    /// turn that the next new call opens.
    fn current_turn(&self) -> i64 {
        self.turn_no + i64::from(self.pending.is_some())
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

/// Where a native record sits and when it happened (if recorded).
struct At<'a> {
    offset: u64,
    ts: Option<&'a str>,
    ts_ms: Option<i64>,
}

impl At<'_> {
    fn ts(&self) -> Option<String> {
        self.ts.map(str::to_owned)
    }
}

/// Response item types a legacy rollout writes at the top level, unwrapped.
const LEGACY_ITEMS: [&str; 6] = [
    "message",
    "reasoning",
    "function_call",
    "function_call_output",
    "custom_tool_call",
    "custom_tool_call_output",
];

impl State {
    fn fold_records(&mut self, records: &[NativeRecord], options: PrepOptions) -> PrepRows {
        let mut rows = PrepRows::default();
        // Keyed calls sighted in this batch, by key, so repeats merge in place.
        let mut batch_calls: HashMap<u64, usize> = HashMap::new();
        for native in records {
            let Ok(Value::Object(record)) = serde_json::from_slice::<Value>(&native.bytes) else {
                self.session.skipped.malformed += 1;
                continue;
            };
            let (ts, ts_ms) = match record.get("timestamp") {
                None | Some(Value::Null) => (None, None),
                Some(Value::String(ts)) => match parse_ms(ts) {
                    Some(ts_ms) => (Some(ts.as_str()), Some(ts_ms)),
                    None => {
                        self.session.skipped.bad_timestamp += 1;
                        continue;
                    }
                },
                Some(_) => {
                    self.session.skipped.bad_timestamp += 1;
                    continue;
                }
            };
            self.observe(ts, ts_ms);
            let at = At {
                offset: native.offset,
                ts,
                ts_ms,
            };
            let payload = record.get("payload").and_then(Value::as_object);
            match (record.get("type").and_then(Value::as_str), payload) {
                (Some("session_meta"), Some(payload)) => self.session_meta(payload),
                (Some("turn_context"), Some(payload)) => self.turn_context(payload, &at, &mut rows),
                (Some("response_item"), Some(payload)) => {
                    self.response_item(payload, &at, options, &mut rows);
                }
                (Some("event_msg"), Some(payload)) => {
                    self.event_msg(payload, &at, options, &mut rows, &mut batch_calls);
                }
                (Some("token_usage_record"), Some(payload)) => {
                    self.usage_record(payload, &at, &mut rows, &mut batch_calls);
                }
                (Some("compacted"), Some(payload)) => self.compacted(payload, &at, &mut rows),
                (Some("inter_agent_communication_metadata"), Some(payload)) => {
                    self.agent_trigger = payload.get("trigger_turn").and_then(Value::as_bool);
                }
                (Some(kind), None) if LEGACY_ITEMS.contains(&kind) => {
                    self.response_item(&record, &at, options, &mut rows);
                }
                // Legacy header: `{id, timestamp, instructions, git}`.
                (None, None) if record.get("id").is_some_and(Value::is_string) => {
                    self.session_meta(&record);
                }
                _ => {}
            }
        }
        rows
    }

    fn observe(&mut self, ts: Option<&str>, ts_ms: Option<i64>) {
        let session = &mut self.session;
        session.records += 1;
        if let (Some(ts), Some(ts_ms)) = (ts, ts_ms) {
            if session.first_event_ts.is_none() {
                session.first_event_ts = Some(ts.to_owned());
                session.first_event_ms = Some(ts_ms);
            }
            session.last_event_ts = Some(ts.to_owned());
            session.last_event_ms = Some(ts_ms);
        }
    }

    /// The first header names this thread; later headers (a fork's copied
    /// history) do not re-identify it.
    fn session_meta(&mut self, meta: &Map<String, Value>) {
        let session = &mut self.session;
        if session.session_id.is_some() {
            return;
        }
        let Some(id) = nonempty(meta, "id") else {
            return;
        };
        session.session_id = Some(id);
        let parent = nonempty(meta, "parent_thread_id");
        session.is_sidechain = parent.is_some();
        session.parent_session_id = parent.or_else(|| nonempty(meta, "forked_from_id"));
        if session.cwd.is_none() {
            session.cwd = nonempty(meta, "cwd");
        }
    }

    fn turn_context(&mut self, context: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        if self.session.cwd.is_none() {
            self.session.cwd = nonempty(context, "cwd");
        }
        self.set_model(context, at, rows);
    }

    /// The model in effect from `settings.model` on.
    fn set_model(&mut self, settings: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        let Some(model) = nonempty(settings, "model") else {
            return;
        };
        if self.model.as_ref().is_some_and(|current| *current != model) {
            let mut event = self.event(at, PrepEventKind::ModelSwitch, None);
            event.model = Some(model.clone());
            rows.events.push(event);
            self.session.last_model_switch = Some(ModelSwitch {
                ts_ms: at.ts_ms,
                requested_model: model.clone(),
            });
        }
        self.model = Some(model);
    }

    fn event(&self, at: &At<'_>, kind: PrepEventKind, subkind: Option<&str>) -> PrepEventRow {
        let (source, generation) = self.row_base();
        PrepEventRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            ts: at.ts(),
            ts_ms: at.ts_ms,
            kind,
            subkind: subkind.map(str::to_owned),
            trigger: None,
            model: None,
            pre_tokens: None,
            post_tokens: None,
            duration_ms: None,
            last_context: None,
            gap_ms: None,
            resets_at: None,
            resets_at_ms: None,
            turn_no: self.current_turn(),
            body_key: None,
        }
    }

    fn gap_since_last_call(&self, ts_ms: Option<i64>) -> Option<i64> {
        Some(ts_ms? - self.last_call.as_ref()?.ts_ms?)
    }

    fn last_context(&self) -> Option<i64> {
        self.last_call
            .as_ref()
            .and_then(|last| context(&last.tokens))
    }

    fn response_item(
        &mut self,
        item: &Map<String, Value>,
        at: &At<'_>,
        options: PrepOptions,
        rows: &mut PrepRows,
    ) {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => self.tool_use(item, "arguments", at, rows),
            Some("custom_tool_call") => self.tool_use(item, "input", at, rows),
            Some("function_call_output" | "custom_tool_call_output") => {
                self.tool_result(item, at, rows);
            }
            Some("agent_message") => self.agent_message(item, at, options, rows),
            _ => {}
        }
    }

    fn event_msg(
        &mut self,
        event: &Map<String, Value>,
        at: &At<'_>,
        options: PrepOptions,
        rows: &mut PrepRows,
        batch_calls: &mut HashMap<u64, usize>,
    ) {
        match event.get("type").and_then(Value::as_str) {
            Some("token_count") => self.token_count(event, at, rows, batch_calls),
            Some("user_message") => self.user_message(event, at, options, rows),
            Some("thread_settings_applied") => {
                if let Some(settings) = event.get("thread_settings").and_then(Value::as_object) {
                    self.set_model(settings, at, rows);
                }
            }
            Some("task_started") => {
                self.pending = Some(Pending {
                    kind: "other".into(),
                    sender: None,
                    pij_msg_id: None,
                    offset: Some(at.offset),
                    ts_ms: at.ts_ms,
                    chars: None,
                    body_key: None,
                });
            }
            Some("turn_aborted") => {
                let mut row = self.event(at, PrepEventKind::SystemOther, Some("turn_aborted"));
                row.duration_ms = event.get("duration_ms").and_then(Value::as_i64);
                rows.events.push(row);
            }
            Some("error") => rows
                .events
                .push(self.event(at, PrepEventKind::ApiError, None)),
            Some("stream_error") => {
                rows.events
                    .push(self.event(at, PrepEventKind::ApiError, Some("stream_error")));
            }
            Some("exec_command_end") => {
                let outcome = event.get("exit_code").and_then(Value::as_i64).map(exit);
                self.tool_end(event, outcome, at, rows);
            }
            Some("patch_apply_end") => {
                let outcome = event.get("success").and_then(Value::as_bool).map(|ok| {
                    if ok {
                        ToolOutcome::Ok
                    } else {
                        ToolOutcome::Error
                    }
                });
                self.tool_end(event, outcome, at, rows);
            }
            Some("mcp_tool_call_end") => {
                let result = event.get("result").and_then(Value::as_object);
                let outcome = match (
                    result.and_then(|r| r.get("Ok")),
                    result.and_then(|r| r.get("Err")),
                ) {
                    (Some(ok), _) => Some(
                        if ok.get("isError").and_then(Value::as_bool) == Some(true) {
                            ToolOutcome::Error
                        } else {
                            ToolOutcome::Ok
                        },
                    ),
                    (None, Some(_)) => Some(ToolOutcome::Error),
                    (None, None) => None,
                };
                self.tool_end(event, outcome, at, rows);
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Triggers and turns
    // -----------------------------------------------------------------------

    fn user_message(
        &mut self,
        event: &Map<String, Value>,
        at: &At<'_>,
        options: PrepOptions,
        rows: &mut PrepRows,
    ) {
        let text = event.get("message").and_then(Value::as_str).unwrap_or("");
        let sender = pij_rs_from(text)
            .map(|(sender, _)| sender)
            .or_else(|| from_name(text));
        let kind = if sender.is_some() {
            "peer"
        } else if self.session.is_sidechain {
            "subagent-task"
        } else {
            "human"
        };
        self.trigger(kind, sender, text, at, options, true, rows);
    }

    /// An inter-agent message; paths name the thread tree (`/root/worker`).
    fn agent_message(
        &mut self,
        item: &Map<String, Value>,
        at: &At<'_>,
        options: PrepOptions,
        rows: &mut PrepRows,
    ) {
        let author = nonempty(item, "author");
        let recipient = nonempty(item, "recipient");
        let below = |child: &str, parent: &str| {
            child
                .strip_prefix(parent)
                .is_some_and(|rest| rest.starts_with('/') || parent.ends_with('/'))
        };
        let kind = match (author.as_deref(), recipient.as_deref()) {
            (Some(a), Some(r)) if below(r, a) => "subagent-task",
            (Some(a), Some(r)) if below(a, r) => "task-notification",
            _ => "peer",
        };
        let text = item
            .get("content")
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let opens = self.agent_trigger.take() != Some(false);
        self.trigger(kind, author, &text, at, options, opens, rows);
    }

    #[allow(clippy::too_many_arguments)]
    fn trigger(
        &mut self,
        kind: &str,
        sender: Option<String>,
        text: &str,
        at: &At<'_>,
        options: PrepOptions,
        opens: bool,
        rows: &mut PrepRows,
    ) {
        let pij_msg_id = pij_message_id(text);
        let body = normalised_body(text);
        let body_key = fnv_hex(body.as_bytes());
        let chars = len_i64(text.chars().count());
        let (source, generation) = self.row_base();
        rows.triggers.push(PrepTriggerRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            ts: at.ts(),
            ts_ms: at.ts_ms,
            kind: origin(kind),
            sender: sender.clone(),
            pij_msg_id: pij_msg_id.clone(),
            chars,
            body_key: Some(body_key.clone()),
            next_turn_no: self.turn_no + 1,
            content_head: options
                .include_content
                .then(|| body.chars().take(200).collect()),
        });
        if opens {
            self.pending = Some(Pending {
                kind: kind.to_owned(),
                sender,
                pij_msg_id,
                offset: Some(at.offset),
                ts_ms: at.ts_ms,
                chars: Some(chars),
                body_key: Some(body_key),
            });
        }
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
            started_ts: at.ts(),
            started_ts_ms: at.ts_ms,
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

    // -----------------------------------------------------------------------
    // Compaction
    // -----------------------------------------------------------------------

    fn compacted(&mut self, compacted: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        let mut event = self.event(at, PrepEventKind::Compaction, None);
        event.last_context = self.last_context();
        event.gap_ms = self.gap_since_last_call(at.ts_ms);
        self.session
            .compactions
            .get_or_insert_with(CompactionCounts::default)
            .unknown_trigger += 1;
        self.session.last_compaction = Some(CompactionSample {
            ts_ms: at.ts_ms,
            trigger: None,
            pre_tokens: None,
            post_tokens: None,
            first_context_after: None,
        });
        if self.session.seat_hint.is_none() {
            self.session.seat_hint = compacted
                .get("message")
                .and_then(Value::as_str)
                .and_then(seat_hint);
        }
        self.first_after = None;
        self.awaiting_first_after = true;
        rows.events.push(event);
    }

    // -----------------------------------------------------------------------
    // Calls
    // -----------------------------------------------------------------------

    /// Per-call counters and a fingerprint that identifies the same call's
    /// counters across record types.
    fn tokens(&self, usage: &Map<String, Value>) -> (Tokens, CacheWriteBasis, u64) {
        let int = |field: &str| {
            usage
                .get(field)
                .and_then(Value::as_i64)
                .filter(|value| *value >= 0)
        };
        let input = int("input_tokens");
        let cached = int("cached_input_tokens");
        let written = int("cache_write_input_tokens");
        let output = int("output_tokens");
        let uncached = input.map(|input| {
            input
                .saturating_sub(cached.unwrap_or(0))
                .saturating_sub(written.unwrap_or(0))
                .max(0)
        });
        let (cw_1h, cw_5m, basis) = match written {
            Some(total) if self.session.is_sidechain => {
                (Some(0), Some(total), CacheWriteBasis::Fallback5m)
            }
            Some(total) => (Some(total), Some(0), CacheWriteBasis::Fallback1h),
            None => (None, None, CacheWriteBasis::None),
        };
        let mut print = Vec::with_capacity(64);
        for value in [
            input,
            cached,
            Some(written.unwrap_or(0)),
            output,
            int("reasoning_output_tokens"),
            int("total_tokens"),
        ] {
            print.extend_from_slice(format!("{value:?};").as_bytes());
        }
        ([uncached, cw_1h, cw_5m, cached, output], basis, fnv(&print))
    }

    fn token_count(
        &mut self,
        event: &Map<String, Value>,
        at: &At<'_>,
        rows: &mut PrepRows,
        batch_calls: &mut HashMap<u64, usize>,
    ) {
        let Some(info) = event.get("info").and_then(Value::as_object) else {
            return;
        };
        let Some(last) = info.get("last_token_usage").and_then(Value::as_object) else {
            return;
        };
        let (tokens, basis, usage) = self.tokens(last);
        let total = info
            .get("total_token_usage")
            .and_then(Value::as_object)
            .map(|total| self.tokens(total).2);
        if let Some(window) = info
            .get("model_context_window")
            .and_then(Value::as_i64)
            .filter(|window| *window > 0)
        {
            self.session.context_window = Some(window);
        }
        self.post_compaction(last);
        match self.usage.clone() {
            // Re-emission: the cumulative total did not move.
            Some(mark) if !mark.from_record && total.is_some() && mark.total == total => {
                if let Some(msg_id) = mark.msg_id {
                    self.sight(Some(msg_id), tokens, basis, at, rows, batch_calls);
                }
            }
            // The token_count half of a token_usage_record's call.
            Some(mark) if mark.from_record && !mark.paired && mark.usage == usage => {
                self.sight(mark.msg_id.clone(), tokens, basis, at, rows, batch_calls);
                self.usage = Some(UsageMark {
                    from_record: false,
                    usage,
                    total,
                    msg_id: mark.msg_id,
                    paired: true,
                });
            }
            _ => {
                self.sight(None, tokens, basis, at, rows, batch_calls);
                self.usage = Some(UsageMark {
                    from_record: false,
                    usage,
                    total,
                    msg_id: None,
                    paired: false,
                });
            }
        }
    }

    /// Codex's own count of the compacted context, recorded before any call
    /// follows the compaction.
    fn post_compaction(&mut self, last: &Map<String, Value>) {
        let int = |field: &str| last.get(field).and_then(Value::as_i64);
        if !self.awaiting_first_after
            || int("input_tokens") != Some(0)
            || int("output_tokens").is_some_and(|output| output != 0)
        {
            return;
        }
        if let Some(compaction) = self.session.last_compaction.as_mut()
            && compaction.post_tokens.is_none()
        {
            compaction.post_tokens = int("total_tokens").filter(|total| *total > 0);
        }
    }

    fn usage_record(
        &mut self,
        record: &Map<String, Value>,
        at: &At<'_>,
        rows: &mut PrepRows,
        batch_calls: &mut HashMap<u64, usize>,
    ) {
        let Some(counters) = record.get("usage").and_then(Value::as_object) else {
            return;
        };
        let (tokens, basis, usage) = self.tokens(counters);
        let msg_id = nonempty(record, "response_id");
        if let Some(mark) = self.usage.as_mut()
            && !mark.from_record
            && !mark.paired
            && mark.msg_id.is_none()
            && mark.usage == usage
        {
            // Second half of an id-less token_count call: nothing to merge into.
            mark.paired = true;
            return;
        }
        self.sight(msg_id.clone(), tokens, basis, at, rows, batch_calls);
        self.usage = Some(UsageMark {
            from_record: true,
            usage,
            total: None,
            msg_id,
            paired: false,
        });
    }

    /// One sighting of a call: merged into this batch's row, an `update` of a
    /// call committed earlier, or a new call.
    fn sight(
        &mut self,
        msg_id: Option<String>,
        tokens: Tokens,
        basis: CacheWriteBasis,
        at: &At<'_>,
        rows: &mut PrepRows,
        batch_calls: &mut HashMap<u64, usize>,
    ) {
        let key = msg_id.as_deref().map(|id| fnv(id.as_bytes()));
        if let Some(key) = key {
            self.repeat_facts(key, &tokens);
            if let Some(&index) = batch_calls.get(&key) {
                let row = &mut rows.calls[index];
                row.input = max_opt(row.input, tokens[0]);
                row.cw_1h = max_opt(row.cw_1h, tokens[1]);
                row.cw_5m = max_opt(row.cw_5m, tokens[2]);
                row.cache_read = max_opt(row.cache_read, tokens[3]);
                row.output = max_opt(row.output, tokens[4]);
                row.records += 1;
                return;
            }
        }
        let (source, generation) = self.row_base();
        let mut row = PrepCallRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            sighting: CallSighting::First,
            msg_id,
            request_id: None,
            ts: at.ts(),
            ts_ms: at.ts_ms,
            model: self.model.clone(),
            stop_reason: None,
            input: tokens[0],
            cw_1h: tokens[1],
            cw_5m: tokens[2],
            cache_read: tokens[3],
            output: tokens[4],
            cache_write_basis: basis,
            is_sidechain: self.session.is_sidechain,
            gap_ms: None,
            turn_no: None,
            call_in_turn: None,
            records: 1,
        };
        match key {
            Some(key) if self.committed.contains(&key) => row.sighting = CallSighting::Update,
            _ => self.new_call(key, tokens, at, &mut row, rows),
        }
        if let Some(key) = key {
            batch_calls.insert(key, rows.calls.len());
        }
        rows.calls.push(row);
    }

    /// A repeat raises the facts derived from its first sighting.
    fn repeat_facts(&mut self, key: u64, tokens: &Tokens) {
        let same = |t: &&mut Tracked| t.key == Some(key);
        if let Some(last) = self.last_call.as_mut().filter(same) {
            merge_tokens(&mut last.tokens, tokens);
        }
        if let Some(main) = self.latest_main.as_mut().filter(same) {
            merge_tokens(&mut main.tokens, tokens);
            if let Some(sample) = self.session.latest_context.as_mut() {
                sample_tokens(sample, &main.tokens);
            }
        }
        if let Some(first) = self.first_after.as_mut().filter(same)
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
        key: Option<u64>,
        tokens: Tokens,
        at: &At<'_>,
        row: &mut PrepCallRow,
        rows: &mut PrepRows,
    ) {
        if let Some(key) = key {
            self.committed.insert(key);
        }
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
        row.gap_ms = match (at.ts_ms, &self.last_call) {
            (Some(_), Some(_)) => self.gap_since_last_call(at.ts_ms),
            (Some(_), None) => Some(-1),
            (None, _) => None,
        };
        row.turn_no = Some(self.turn_no);
        row.call_in_turn = Some(self.call_in_turn);
        let tracked = Tracked {
            key,
            ts_ms: at.ts_ms,
            tokens,
        };
        if !row.is_sidechain {
            let mut sample = ContextSample {
                ts_ms: at.ts_ms,
                model: row.model.clone(),
                stop_reason: None,
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

    // -----------------------------------------------------------------------
    // Tool uses
    // -----------------------------------------------------------------------

    fn tool_use(
        &mut self,
        item: &Map<String, Value>,
        input: &str,
        at: &At<'_>,
        rows: &mut PrepRows,
    ) {
        let tool_use_id = nonempty(item, "call_id");
        if let Some(id) = &tool_use_id
            && !self.tool_ids.insert(fnv(id.as_bytes()))
        {
            return;
        }
        let name = nonempty(item, "name");
        let input = item.get(input).map(input_bytes);
        if let Some(id) = &tool_use_id {
            self.open_uses.insert(id.clone(), name.clone());
        }
        let (source, generation) = self.row_base();
        rows.tool_uses.push(PrepToolUseRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            sighting: ToolSighting::Use,
            tool_use_id,
            call_msg_id: None,
            ts: at.ts(),
            ts_ms: at.ts_ms,
            family: family_of(name.as_deref()),
            name,
            input_hash: input.as_deref().map(fnv_hex),
            input_bytes: input.map(|bytes| len_i64(bytes.len())),
            result_offset: None,
            result_bytes: None,
            outcome: None,
            duration_ms: None,
            turn_no: Some(self.current_turn()),
        });
    }

    fn tool_result(&mut self, item: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        let tool_use_id = nonempty(item, "call_id");
        let name = tool_use_id
            .as_ref()
            .and_then(|id| self.open_uses.remove(id))
            .flatten()
            .or_else(|| nonempty(item, "name"));
        let output = item.get("output");
        let end = tool_use_id
            .as_ref()
            .and_then(|id| self.ends.remove(id))
            .or_else(|| output.and_then(output_end))
            .unwrap_or(End {
                outcome: None,
                duration_ms: None,
            });
        if let Some(id) = &tool_use_id {
            self.answered.insert(fnv(id.as_bytes()));
        }
        let (source, generation) = self.row_base();
        rows.tool_uses.push(PrepToolUseRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            sighting: ToolSighting::Result,
            tool_use_id,
            call_msg_id: None,
            ts: at.ts(),
            ts_ms: at.ts_ms,
            family: family_of(name.as_deref()),
            name,
            input_hash: None,
            input_bytes: None,
            result_offset: Some(at.offset),
            result_bytes: output.map(result_bytes),
            outcome: end.outcome,
            duration_ms: end.duration_ms,
            turn_no: Some(self.current_turn()),
        });
    }

    /// A native end event: held for its output, or a second result sighting
    /// when the output came first.
    fn tool_end(
        &mut self,
        event: &Map<String, Value>,
        outcome: Option<ToolOutcome>,
        at: &At<'_>,
        rows: &mut PrepRows,
    ) {
        let Some(id) = nonempty(event, "call_id") else {
            return;
        };
        let end = End {
            outcome,
            duration_ms: event.get("duration").and_then(duration_ms),
        };
        if end.outcome.is_none() && end.duration_ms.is_none() {
            return;
        }
        if !self.answered.contains(&fnv(id.as_bytes())) {
            self.ends.insert(id, end);
            return;
        }
        let (source, generation) = self.row_base();
        rows.tool_uses.push(PrepToolUseRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: None,
            sighting: ToolSighting::Result,
            tool_use_id: Some(id),
            call_msg_id: None,
            ts: at.ts(),
            ts_ms: at.ts_ms,
            name: None,
            family: None,
            input_hash: None,
            input_bytes: None,
            result_offset: None,
            result_bytes: None,
            outcome: end.outcome,
            duration_ms: end.duration_ms,
            turn_no: Some(self.current_turn()),
        });
    }
}

fn exit(code: i64) -> ToolOutcome {
    if code == 0 {
        ToolOutcome::Ok
    } else {
        ToolOutcome::Error
    }
}

/// `{secs, nanos}` → whole milliseconds.
fn duration_ms(value: &Value) -> Option<i64> {
    let secs = value.get("secs")?.as_i64()?;
    let nanos = value.get("nanos").and_then(Value::as_i64).unwrap_or(0);
    secs.checked_mul(1000)?.checked_add(nanos / 1_000_000)
}

/// Seconds as written by the harness (`1.25`) → rounded milliseconds.
fn seconds_ms(seconds: f64) -> Option<i64> {
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1000.0).round() as i64)
}

/// Outcome recorded with a tool output: structured `metadata`, else the
/// harness header before the `Output:` line.
fn output_end(output: &Value) -> Option<End> {
    let text = output.as_str()?;
    if let Ok(Value::Object(object)) = serde_json::from_str::<Value>(text) {
        let metadata = object.get("metadata")?.as_object()?;
        return Some(End {
            outcome: metadata.get("exit_code").and_then(Value::as_i64).map(exit),
            duration_ms: metadata
                .get("duration_seconds")
                .and_then(Value::as_f64)
                .and_then(seconds_ms),
        });
    }
    let (mut code, mut wall) = (None, None);
    for line in text.lines().take(8) {
        if line == "Output:" {
            return Some(End {
                outcome: code.map(exit),
                duration_ms: wall,
            });
        }
        if let Some(value) = line
            .strip_prefix("Exit code: ")
            .or_else(|| line.strip_prefix("Process exited with code "))
        {
            code = value.trim().parse::<i64>().ok();
        } else if let Some(value) = line
            .strip_prefix("Wall time: ")
            .and_then(|value| value.strip_suffix(" seconds"))
        {
            wall = value.trim().parse::<f64>().ok().and_then(seconds_ms);
        }
    }
    None
}

/// The query mapping's tool-family vocabulary; unknown names stay null.
fn family_of(name: Option<&str>) -> Option<String> {
    name.and_then(codex_tool_family).map(str::to_owned)
}

fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

/// Tool input bytes: a JSON-encoded object/array argument string is
/// canonicalised so the hash depends only on the value; free-form input (a
/// patch) is its UTF-8 bytes.
fn input_bytes(input: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    match input {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(value @ (Value::Object(_) | Value::Array(_))) => canonical_json(&value, &mut bytes),
            _ => bytes.extend_from_slice(text.as_bytes()),
        },
        other => canonical_json(other, &mut bytes),
    }
    bytes
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

/// Trigger kind → contract origin.
fn origin(kind: &str) -> TurnOrigin {
    match kind {
        "human" => TurnOrigin::Human,
        "peer" => TurnOrigin::Peer,
        "task-notification" => TurnOrigin::TaskNotification,
        "subagent-task" => TurnOrigin::SubagentTask,
        _ => TurnOrigin::Other,
    }
}

fn nonempty(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn parse_ms(ts: &str) -> Option<i64> {
    let parsed = OffsetDateTime::parse(ts, &Rfc3339).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos().div_euclid(1_000_000)).ok()
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

/// The pij-rs payload (between `[pij-rs from …]` and `[/pij]`) with `[pij…]`
/// tags and cross-session wrappers removed and whitespace collapsed; hashed
/// for fan-out grouping, never stored.
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

    #[test]
    fn output_headers_yield_outcomes_only_before_the_output_line() {
        let end = |text: &str| output_end(&Value::String(text.into()));
        assert_eq!(
            end("Exit code: 2\nWall time: 1.25 seconds\nOutput:\nx"),
            Some(End {
                outcome: Some(ToolOutcome::Error),
                duration_ms: Some(1250),
            })
        );
        assert_eq!(
            end(
                "Chunk ID: a1\nWall time: 0.0004 seconds\nProcess exited with code 0\nOriginal token count: 3\nOutput:\n"
            ),
            Some(End {
                outcome: Some(ToolOutcome::Ok),
                duration_ms: Some(0),
            })
        );
        // A header-like line inside free text without the delimiter is not native.
        assert_eq!(end("Exit code: 1\nno delimiter"), None);
        assert_eq!(
            end("Output:\nExit code: 1"),
            Some(End {
                outcome: None,
                duration_ms: None
            })
        );
        assert_eq!(
            end(r#"{"output":"x","metadata":{"exit_code":0,"duration_seconds":0.5}}"#),
            Some(End {
                outcome: Some(ToolOutcome::Ok),
                duration_ms: Some(500),
            })
        );
        assert_eq!(end(r#"{"result":"x"}"#), None);
    }
}
