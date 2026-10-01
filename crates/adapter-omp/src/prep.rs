//! Pure Oh My Pi fold into canonical prep tables.
//!
//! Supplied v3 session-tree records in, metadata facts out; no filesystem,
//! clock or environment. Rules:
//!
//! - A call is an assistant message with a usage block, deduplicated per source
//!   generation on its native entry id (`msg_id`). A repeat whose first
//!   sighting was folded by an earlier batch or run is an `update` sighting.
//!   `request_id` is null: the dialect records no request id (`responseId`
//!   names the response). The model is `provider/model`, the id the harness's
//!   own `model_change` entries use, because one model's window differs by
//!   provider.
//! - Context is `input + cacheRead + cacheWrite` (the harness's
//!   `contextSnapshot.promptTokens`). `usage.cttl` splits cache writes into 5 m
//!   and 1 h; a half it omits is the native `cacheWrite` aggregate minus the
//!   recorded half. Without `cttl` the harness does not distinguish the TTL, so
//!   both halves are null with basis `none`; the aggregate still counts toward
//!   the context.
//! - An `error` response whose usage records no tokens consumed nothing: it is
//!   an `api_error` event, not a call. Every `error` response is such an event.
//!   An `aborted` response that records no tokens is not a call either.
//! - Branch membership comes from parent ids. The fold keeps a bounded window of
//!   the most recent tree entries, each with the call nearest on its path, so a
//!   retry or navigation that re-parents onto a recent entry restores that
//!   branch's context. A parent outside the window leaves the branch context
//!   unknown (null), never guessed. Calls are sidechain only in subagent
//!   sources.
//! - User messages open `human` turns (`subagent-task` in subagent sources).
//!   A `[pij-rs from …]` envelope, in user text or a `pij` custom message, opens
//!   a `peer` turn with its sender and Pij message id; `pij-fyi` and
//!   `irc:incoming` custom messages are `peer` too, the latter with its native
//!   sender. `async-result` and `launch-completion` open `task-notification`
//!   turns and other user-attributed custom messages (skill prompts) `human`
//!   turns. Agent-attributed injections (nudges, reminders, diagnostics) open
//!   none.
//! - `compaction` and `branch_summary` entries are compaction events (`subkind`
//!   is the entry type). Only `compaction` entries count in
//!   `SessionFacts::compactions`; the dialect records no manual/auto trigger.
//! - Tool calls and results are `tool_uses`; `details.wallTimeMs` is the only
//!   native duration.
//! - The mutable title slot is session metadata, not an event, and is not folded.
//! - The dialect records no context window, so `context_window` stays null.
//!
//! Message text is read only to classify openers and derive hashes. It leaves
//! the fold only as `triggers.content_head`, under explicit opt-in.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

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

use crate::DESCRIPTOR;

/// Interpretation policy; bump on any rule change so every source re-emits.
pub const PREP_POLICY_VERSION: &str = "oh-my-pi/prep-v2";

/// Tree entries remembered for re-parenting (retries, navigation).
const WINDOW: usize = 256;

#[derive(Debug, Clone, Copy, Default)]
pub struct OmpPrepFold;

impl PrepFold for OmpPrepFold {
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
    /// `<project>/<session>.jsonl` is a session; a subagent session lives in
    /// `<project>/<parent session stem>/<agent>.jsonl`.
    fn describe(&self, file: &str) -> PrepSourceMeta {
        let segments: Vec<&str> = file.split('/').collect();
        let is_sub = segments.len() >= 3;
        let name = segments.last().copied().unwrap_or(file);
        PrepSourceMeta {
            is_sub,
            agent_id: is_sub.then(|| name.strip_suffix(".jsonl").unwrap_or(name).to_owned()),
            project: Some(project_of(segments[0])),
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

/// `--Users-<name>-rest--` → `rest`; the session directory encodes the cwd
/// with `/` as `-`.
fn project_of(dir: &str) -> String {
    let trimmed = dir.trim_matches('-');
    let mut parts = trimmed.splitn(3, '-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("Users"), Some(_), Some(rest)) if !rest.is_empty() => rest.to_owned(),
        _ if !trimmed.is_empty() => trimmed.to_owned(),
        _ => dir.to_owned(),
    }
}

/// Parent session id of a subagent source: its directory is the parent
/// session's file stem, `<timestamp>_<session id>`.
fn parent_of_sub(source: &str) -> Option<String> {
    let mut segments = source.rsplit('/');
    segments.next()?;
    session_id_of_stem(segments.next()?)
}

fn session_id_of_stem(stem: &str) -> Option<String> {
    let (_, id) = stem.rsplit_once('_')?;
    (!id.is_empty()).then(|| id.to_owned())
}

/// Native `parentSession`: a session id, or the path of the parent's file.
fn parent_session_id(value: &str) -> Option<String> {
    match value.rsplit_once('/') {
        None if !value.contains('.') && !value.is_empty() => Some(value.to_owned()),
        None => session_id_of_stem(value.strip_suffix(".jsonl")?),
        Some((_, name)) => session_id_of_stem(name.strip_suffix(".jsonl")?),
    }
}

/// input, cw_1h, cw_5m, cache_read, output, cache-write aggregate;
/// `None` = not recorded.
type Tokens = [Option<i64>; 6];

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

/// The native aggregate, else the sum of the recorded halves.
fn cache_write(tokens: &Tokens) -> Option<i64> {
    tokens[5].or_else(|| Some(tokens[1]?.saturating_add(tokens[2]?)))
}

/// `input + cache writes + cache_read`, only when all are recorded.
fn context(tokens: &Tokens) -> Option<i64> {
    Some(
        tokens[0]?
            .saturating_add(cache_write(tokens)?)
            .saturating_add(tokens[3]?),
    )
}

/// Opening record of a turn not yet opened by a call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    kind: TurnOrigin,
    sender: Option<String>,
    pij_msg_id: Option<String>,
    offset: u64,
    key: Option<String>,
    ts_ms: i64,
    chars: i64,
    body_key: String,
}

impl Pending {
    fn save(&self) -> Value {
        json!({
            "kind": self.kind, "sender": self.sender, "pij_msg_id": self.pij_msg_id,
            "offset": self.offset, "key": self.key, "ts_ms": self.ts_ms,
            "chars": self.chars, "body_key": self.body_key,
        })
    }

    fn load(value: &Value) -> Option<Self> {
        let p = value.as_object()?;
        Some(Self {
            kind: serde_json::from_value(p.get("kind")?.clone()).ok()?,
            sender: opt_text(p.get("sender")?)?,
            pij_msg_id: opt_text(p.get("pij_msg_id")?)?,
            offset: p.get("offset")?.as_u64()?,
            key: opt_text(p.get("key")?)?,
            ts_ms: p.get("ts_ms")?.as_i64()?,
            chars: p.get("chars")?.as_i64()?,
            body_key: p.get("body_key")?.as_str()?.to_owned(),
        })
    }
}

/// A new call whose later repeats still change a derived fact.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tracked {
    ts_ms: i64,
    tokens: Tokens,
    model: Option<String>,
    stop_reason: Option<String>,
}

impl Tracked {
    fn sample(&self) -> ContextSample {
        ContextSample {
            ts_ms: Some(self.ts_ms),
            model: self.model.clone(),
            stop_reason: self.stop_reason.clone(),
            input: self.tokens[0],
            cache_read: self.tokens[3],
            cache_write: cache_write(&self.tokens),
            total: context(&self.tokens),
        }
    }

    fn save(&self) -> Value {
        json!({
            "ts_ms": self.ts_ms, "tokens": self.tokens,
            "model": self.model, "stop_reason": self.stop_reason,
        })
    }

    fn load(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let array = object.get("tokens")?.as_array()?;
        let mut tokens: Tokens = [None; 6];
        if array.len() != tokens.len() {
            return None;
        }
        for (slot, value) in tokens.iter_mut().zip(array) {
            *slot = opt(value, Value::as_i64)?;
        }
        Some(Self {
            ts_ms: object.get("ts_ms")?.as_i64()?,
            tokens,
            model: opt_text(object.get("model")?)?,
            stop_reason: opt_text(object.get("stop_reason")?)?,
        })
    }
}

/// The call nearest on a tree entry's path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ctx {
    /// The parent fell outside the window: not determinable.
    Unknown,
    NoCall,
    Call(u64),
}

impl Ctx {
    fn save(self) -> Value {
        match self {
            Self::Unknown => json!("?"),
            Self::NoCall => Value::Null,
            Self::Call(key) => json!(hex(key)),
        }
    }

    fn load(value: &Value) -> Option<Self> {
        match value {
            Value::Null => Some(Self::NoCall),
            Value::String(text) if text == "?" => Some(Self::Unknown),
            value => unhex(value).map(Self::Call),
        }
    }
}

/// A recent tree entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    id: String,
    ctx: Ctx,
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
    /// Most recent tree entries in native order; the last is the leaf.
    window: VecDeque<Node>,
    /// Calls referenced by the window, `last_call` or `first_after`.
    calls: BTreeMap<u64, Tracked>,
    /// Latest new call in native order: gap origin.
    last_call: Option<u64>,
    /// First new main-chain call after the latest compaction.
    first_after: Option<u64>,
    awaiting_first_after: bool,
    /// Every call key folded in this generation (64-bit hashes).
    committed: BTreeSet<u64>,
    /// Every tool-call id folded in this generation (64-bit hashes).
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

fn opt_key(value: &Value) -> Option<Option<u64>> {
    opt(value, unhex)
}

fn hex(key: u64) -> String {
    format!("{key:016x}")
}

fn unhex(value: &Value) -> Option<u64> {
    unhex_str(value.as_str()?)
}

fn unhex_str(text: &str) -> Option<u64> {
    (text.len() == 16)
        .then(|| u64::from_str_radix(text, 16).ok())
        .flatten()
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
            window: VecDeque::new(),
            calls: BTreeMap::new(),
            last_call: None,
            first_after: None,
            awaiting_first_after: false,
            committed: BTreeSet::new(),
            tool_ids: BTreeSet::new(),
            open_uses: BTreeMap::new(),
            session: SessionFacts {
                is_sidechain: meta.is_sub,
                parent_session_id: if meta.is_sub {
                    parent_of_sub(source)
                } else {
                    None
                },
                compactions: Some(CompactionCounts::default()),
                ..SessionFacts::default()
            },
        }
    }

    fn save(&self) -> Value {
        let window: Vec<Value> = self
            .window
            .iter()
            .map(|node| json!([node.id, node.ctx.save()]))
            .collect();
        let calls: Map<String, Value> = self
            .calls
            .iter()
            .map(|(key, tracked)| (hex(*key), tracked.save()))
            .collect();
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
            "window": window,
            "calls": calls,
            "last_call": self.last_call.map(hex),
            "first_after": self.first_after.map(hex),
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
        let window = field("window")?
            .as_array()?
            .iter()
            .map(|node| match node.as_array()?.as_slice() {
                [id, ctx] => Some(Node {
                    id: id.as_str()?.to_owned(),
                    ctx: Ctx::load(ctx)?,
                }),
                _ => None,
            })
            .collect::<Option<VecDeque<_>>>()?;
        let calls = field("calls")?
            .as_object()?
            .iter()
            .map(|(key, tracked)| Some((unhex_str(key)?, Tracked::load(tracked)?)))
            .collect::<Option<BTreeMap<_, _>>>()?;
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
        let state = Self {
            source: source.to_owned(),
            is_sub: meta.is_sub,
            generation,
            turn_no: field("turn_no")?.as_i64()?,
            call_in_turn: field("call_in_turn")?.as_i64()?,
            turn_zero_emitted: field("turn_zero_emitted")?.as_bool()?,
            pending: opt(field("pending")?, Pending::load)?,
            window,
            calls,
            last_call: opt_key(field("last_call")?)?,
            first_after: opt_key(field("first_after")?)?,
            awaiting_first_after: field("awaiting_first_after")?.as_bool()?,
            committed: key_set(field("committed")?)?,
            tool_ids: key_set(field("tool_ids")?)?,
            open_uses,
            session: serde_json::from_value(field("session")?.clone()).ok()?,
        };
        // Every referenced call must be present.
        let complete = state
            .window
            .iter()
            .filter_map(|node| match node.ctx {
                Ctx::Call(key) => Some(key),
                _ => None,
            })
            .chain(state.last_call)
            .chain(state.first_after)
            .all(|key| state.calls.contains_key(&key));
        complete.then_some(state)
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
    id: Option<&'a str>,
    ts: &'a str,
    ts_ms: i64,
}

impl At<'_> {
    fn key(&self) -> Option<String> {
        self.id.map(str::to_owned)
    }
}

impl State {
    fn fold_records(&mut self, records: &[NativeRecord], options: PrepOptions) -> PrepRows {
        let mut rows = PrepRows::default();
        // Calls sighted in this batch, by key, so repeats merge in place.
        let mut batch_calls: HashMap<u64, usize> = HashMap::new();
        for native in records {
            let Ok(Value::Object(record)) = serde_json::from_slice::<Value>(&native.bytes) else {
                self.session.skipped.malformed += 1;
                continue;
            };
            let kind = record.get("type").and_then(Value::as_str);
            if kind == Some("title") {
                continue;
            }
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
            self.observe_session(ts, ts_ms);
            let at = At {
                offset: native.offset,
                id: record.get("id").and_then(Value::as_str),
                ts,
                ts_ms,
            };
            if kind == Some("session") {
                self.header(&record);
                continue;
            }
            self.place(&record, at.id);
            match kind {
                Some("message") => {
                    let empty = Map::new();
                    let message = record
                        .get("message")
                        .and_then(Value::as_object)
                        .unwrap_or(&empty);
                    match message.get("role").and_then(Value::as_str) {
                        Some("assistant") => {
                            self.assistant(message, &at, &mut rows, &mut batch_calls);
                        }
                        Some("user") => {
                            let text = text_of(message.get("content"));
                            let (origin, sender, pij_msg_id) = match pij_rs_from(&text) {
                                Some((sender, _)) => {
                                    (TurnOrigin::Peer, Some(sender), pij_message_id(&text))
                                }
                                None if self.is_sub => (TurnOrigin::SubagentTask, None, None),
                                None => (TurnOrigin::Human, None, None),
                            };
                            self.trigger(
                                &at, origin, sender, pij_msg_id, &text, options, &mut rows,
                            );
                        }
                        Some("toolResult") => self.tool_result(message, &at, &mut rows),
                        _ => {}
                    }
                }
                Some("custom_message") => self.custom_message(&record, &at, options, &mut rows),
                Some("compaction" | "branch_summary") => self.compaction(&record, &at, &mut rows),
                Some("model_change") => self.model_change(&record, &at, &mut rows),
                _ => {}
            }
        }
        self.refresh_latest_context();
        rows
    }

    fn observe_session(&mut self, ts: &str, ts_ms: i64) {
        let session = &mut self.session;
        session.records += 1;
        if session.first_event_ts.is_none() {
            session.first_event_ts = Some(ts.to_owned());
            session.first_event_ms = Some(ts_ms);
        }
        session.last_event_ts = Some(ts.to_owned());
        session.last_event_ms = Some(ts_ms);
    }

    fn header(&mut self, record: &Map<String, Value>) {
        let session = &mut self.session;
        if session.session_id.is_none() {
            session.session_id = string(record, "id").filter(|s| !s.is_empty());
        }
        if session.cwd.is_none() {
            session.cwd = string(record, "cwd").filter(|s| !s.is_empty());
        }
        if session.parent_session_id.is_none() {
            session.parent_session_id = record
                .get("parentSession")
                .and_then(Value::as_str)
                .and_then(parent_session_id);
        }
    }

    /// Append a tree entry to the window with the context its parent carries.
    fn place(&mut self, record: &Map<String, Value>, id: Option<&str>) {
        let ctx = match record.get("parentId") {
            // An absent parent continues the leaf; null starts a new root.
            None => self.leaf_ctx(),
            Some(Value::Null) => Ctx::NoCall,
            Some(parent) => {
                let parent = parent.as_str();
                self.window
                    .iter()
                    .rev()
                    .find(|node| Some(node.id.as_str()) == parent)
                    .map_or(Ctx::Unknown, |node| node.ctx)
            }
        };
        let Some(id) = id else { return };
        self.window.push_back(Node {
            id: id.to_owned(),
            ctx,
        });
        if self.window.len() > WINDOW {
            self.window.pop_front();
            self.prune();
        }
    }

    fn leaf_ctx(&self) -> Ctx {
        self.window.back().map_or(Ctx::NoCall, |node| node.ctx)
    }

    fn prune(&mut self) {
        let keep: BTreeSet<u64> = self
            .window
            .iter()
            .filter_map(|node| match node.ctx {
                Ctx::Call(key) => Some(key),
                _ => None,
            })
            .chain(self.last_call)
            .chain(self.first_after)
            .collect();
        self.calls.retain(|key, _| keep.contains(key));
    }

    /// The context of the current branch: its nearest call, when determinable.
    fn branch_context(&self) -> Option<&Tracked> {
        match self.leaf_ctx() {
            Ctx::Call(key) => self.calls.get(&key),
            Ctx::Unknown | Ctx::NoCall => None,
        }
    }

    fn refresh_latest_context(&mut self) {
        self.session.latest_context = if self.is_sub {
            None
        } else {
            self.branch_context().map(Tracked::sample)
        };
    }

    fn event(&self, at: &At<'_>, kind: PrepEventKind, subkind: Option<String>) -> PrepEventRow {
        let (source, generation) = self.row_base();
        PrepEventRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: at.key(),
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
            body_key: None,
        }
    }

    fn gap_since_last_call(&self, ts_ms: i64) -> Option<i64> {
        self.last_call
            .and_then(|key| self.calls.get(&key))
            .map(|last| ts_ms - last.ts_ms)
    }

    fn custom_message(
        &mut self,
        record: &Map<String, Value>,
        at: &At<'_>,
        options: PrepOptions,
        rows: &mut PrepRows,
    ) {
        let text = text_of(record.get("content"));
        let origin = match record.get("customType").and_then(Value::as_str) {
            Some("pij" | "pij-fyi" | "irc:incoming") => TurnOrigin::Peer,
            Some("async-result" | "launch-completion") => TurnOrigin::TaskNotification,
            _ if record.get("attribution").and_then(Value::as_str) == Some("user") => {
                TurnOrigin::Human
            }
            _ => return,
        };
        let details = record.get("details").and_then(Value::as_object);
        let detail = |key: &str| {
            details
                .and_then(|details| string(details, key))
                .filter(|value| !value.is_empty())
        };
        let (sender, pij_msg_id) = if origin == TurnOrigin::Peer {
            (
                pij_rs_from(&text)
                    .map(|(sender, _)| sender)
                    .or_else(|| detail("from")),
                detail("pijMessageId").or_else(|| pij_message_id(&text)),
            )
        } else {
            (None, None)
        };
        self.trigger(at, origin, sender, pij_msg_id, &text, options, rows);
    }

    #[allow(clippy::too_many_arguments)]
    fn trigger(
        &mut self,
        at: &At<'_>,
        kind: TurnOrigin,
        sender: Option<String>,
        pij_msg_id: Option<String>,
        text: &str,
        options: PrepOptions,
        rows: &mut PrepRows,
    ) {
        let body = normalised_body(text);
        let body_key = fnv_hex(body.as_bytes());
        let chars = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
        let (source, generation) = self.row_base();
        rows.triggers.push(PrepTriggerRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: at.key(),
            ts: Some(at.ts.to_owned()),
            ts_ms: Some(at.ts_ms),
            kind,
            sender: sender.clone(),
            pij_msg_id: pij_msg_id.clone(),
            chars,
            body_key: Some(body_key.clone()),
            next_turn_no: self.turn_no + 1,
            content_head: options
                .include_content
                .then(|| body.chars().take(200).collect()),
        });
        self.pending = Some(Pending {
            kind,
            sender,
            pij_msg_id,
            offset: at.offset,
            key: at.key(),
            ts_ms: at.ts_ms,
            chars,
            body_key,
        });
    }

    fn compaction(&mut self, record: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        let entry = record
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let is_compaction = entry.as_deref() == Some("compaction");
        let mut event = self.event(at, PrepEventKind::Compaction, entry);
        event.pre_tokens = record.get("tokensBefore").and_then(Value::as_i64);
        event.post_tokens = record.get("tokensAfter").and_then(Value::as_i64);
        event.last_context = self.branch_context().and_then(|t| context(&t.tokens));
        event.gap_ms = self.gap_since_last_call(at.ts_ms);
        if is_compaction {
            if let Some(counts) = self.session.compactions.as_mut() {
                counts.unknown_trigger += 1;
            }
            self.session.last_compaction = Some(CompactionSample {
                ts_ms: Some(at.ts_ms),
                trigger: None,
                pre_tokens: event.pre_tokens,
                post_tokens: event.post_tokens,
                first_context_after: None,
            });
            self.first_after = None;
            self.awaiting_first_after = true;
            self.prune();
        }
        rows.events.push(event);
    }

    /// A native model selection; role-specific selections (`role` other than
    /// `default`) are events but do not change the session's main model.
    fn model_change(&mut self, record: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        let Some(model) = string(record, "model") else {
            return;
        };
        let role = string(record, "role");
        let main = role.as_deref().is_none_or(|role| role == "default");
        let mut event = self.event(at, PrepEventKind::ModelSwitch, role);
        event.model = Some(model.clone());
        rows.events.push(event);
        if main {
            self.session.last_model_switch = Some(ModelSwitch {
                ts_ms: Some(at.ts_ms),
                requested_model: model,
            });
        }
    }

    fn tokens(&self, usage: &Map<String, Value>) -> (Tokens, CacheWriteBasis) {
        let int =
            |object: &Map<String, Value>, field: &str| object.get(field).and_then(Value::as_i64);
        let mut tokens: Tokens = [
            int(usage, "input"),
            None,
            None,
            int(usage, "cacheRead"),
            int(usage, "output"),
            int(usage, "cacheWrite"),
        ];
        let total = tokens[5];
        let split = usage
            .get("cttl")
            .and_then(Value::as_object)
            .and_then(|ttl| {
                let (h5, h1) = (int(ttl, "ephemeral5m"), int(ttl, "ephemeral1h"));
                let rest = |half: i64| total.map(|t| t - half).filter(|r| *r >= 0);
                match (h1, h5) {
                    (Some(h1), Some(h5)) => Some((h1, h5)),
                    (Some(h1), None) => Some((h1, rest(h1)?)),
                    (None, Some(h5)) => Some((rest(h5)?, h5)),
                    (None, None) => None,
                }
            });
        let basis = match split {
            Some((h1, h5)) => {
                (tokens[1], tokens[2]) = (Some(h1), Some(h5));
                CacheWriteBasis::Split
            }
            None => CacheWriteBasis::None,
        };
        (tokens, basis)
    }

    fn assistant(
        &mut self,
        message: &Map<String, Value>,
        at: &At<'_>,
        rows: &mut PrepRows,
        batch_calls: &mut HashMap<u64, usize>,
    ) {
        let stop_reason = string(message, "stopReason");
        let usage = message
            .get("usage")
            .and_then(Value::as_object)
            .filter(|usage| !usage.is_empty());
        let tokens = usage.map(|usage| self.tokens(usage));
        let consumed = tokens
            .as_ref()
            .is_some_and(|(t, _)| t.iter().flatten().any(|n| *n != 0));
        if stop_reason.as_deref() == Some("error") {
            let status = message.get("errorStatus").and_then(|status| match status {
                Value::Number(n) => Some(n.to_string()),
                Value::String(s) => Some(s.clone()),
                _ => None,
            });
            rows.events
                .push(self.event(at, PrepEventKind::ApiError, status));
        }
        if !consumed && matches!(stop_reason.as_deref(), Some("error" | "aborted")) {
            self.tool_uses(message, at, rows);
            return;
        }
        if let Some((tokens, basis)) = tokens {
            self.call(message, at, tokens, basis, stop_reason, rows, batch_calls);
        }
        self.tool_uses(message, at, rows);
    }

    #[allow(clippy::too_many_arguments)]
    fn call(
        &mut self,
        message: &Map<String, Value>,
        at: &At<'_>,
        tokens: Tokens,
        basis: CacheWriteBasis,
        stop_reason: Option<String>,
        rows: &mut PrepRows,
        batch_calls: &mut HashMap<u64, usize>,
    ) {
        let key = match at.id {
            Some(id) => fnv(format!("id:{id}").as_bytes()),
            None => fnv(format!("offset:{}", at.offset).as_bytes()),
        };
        if let Some(node) = self
            .window
            .back_mut()
            .filter(|n| Some(n.id.as_str()) == at.id)
        {
            node.ctx = Ctx::Call(key);
        }
        if let Some(tracked) = self.calls.get_mut(&key) {
            merge_tokens(&mut tracked.tokens, &tokens);
            if stop_reason.is_some() {
                tracked.stop_reason.clone_from(&stop_reason);
            }
        }
        self.refresh_first_after(key);
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
        let (source, generation) = self.row_base();
        let mut row = PrepCallRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: at.key(),
            sighting: CallSighting::First,
            msg_id: at.key(),
            request_id: None,
            ts: Some(at.ts.to_owned()),
            ts_ms: Some(at.ts_ms),
            model: qualified_model(message),
            stop_reason,
            input: tokens[0],
            cw_1h: tokens[1],
            cw_5m: tokens[2],
            cache_read: tokens[3],
            output: tokens[4],
            cache_write_basis: basis,
            is_sidechain: self.is_sub,
            gap_ms: None,
            turn_no: None,
            call_in_turn: None,
            records: 1,
        };
        if self.committed.contains(&key) {
            row.sighting = CallSighting::Update;
            // A repeat outside the window: its path context is this record's.
            self.calls
                .entry(key)
                .or_insert_with(|| tracked_of(&row, &tokens, at.ts_ms));
        } else {
            self.new_call(key, tokens, at, &mut row, rows);
        }
        batch_calls.insert(key, rows.calls.len());
        rows.calls.push(row);
    }

    fn refresh_first_after(&mut self, key: u64) {
        if self.first_after == Some(key)
            && let Some(first) = self.calls.get(&key)
            && let Some(compaction) = self.session.last_compaction.as_mut()
        {
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
            self.push_turn(at, pending.kind, Some(pending), rows);
        } else if self.turn_no == 0 && !self.turn_zero_emitted {
            self.push_turn(at, TurnOrigin::Start, None, rows);
        }
        self.turn_zero_emitted = true;
        self.call_in_turn += 1;
        row.gap_ms = Some(self.gap_since_last_call(at.ts_ms).unwrap_or(-1));
        row.turn_no = Some(self.turn_no);
        row.call_in_turn = Some(self.call_in_turn);
        self.calls.insert(key, tracked_of(row, &tokens, at.ts_ms));
        self.last_call = Some(key);
        if !row.is_sidechain && self.awaiting_first_after {
            self.awaiting_first_after = false;
            self.first_after = Some(key);
            self.refresh_first_after(key);
        }
        self.prune();
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
            native_key: at.key(),
            turn_no: self.turn_no,
            started_ts: Some(at.ts.to_owned()),
            started_ts_ms: Some(at.ts_ms),
            first_call_offset: Some(at.offset),
            origin,
            sender: opener.and_then(|p| p.sender.clone()),
            pij_msg_id: opener.and_then(|p| p.pij_msg_id.clone()),
            opener_offset: opener.map(|p| p.offset),
            opener_ts_ms: opener.map(|p| p.ts_ms),
            opener_chars: opener.map(|p| p.chars),
            body_key: opener.map(|p| p.body_key.clone()),
        });
        self.session.turns += 1;
    }

    /// `toolCall` parts of an assistant message: use sightings, deduplicated by
    /// native id across batches and runs of this generation.
    fn tool_uses(&mut self, message: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        let Some(parts) = message.get("content").and_then(Value::as_array) else {
            return;
        };
        for part in parts.iter().filter_map(Value::as_object) {
            if part.get("type").and_then(Value::as_str) != Some("toolCall") {
                continue;
            }
            let tool_use_id = string(part, "id");
            if let Some(id) = &tool_use_id
                && !self.tool_ids.insert(fnv(id.as_bytes()))
            {
                continue;
            }
            let name = string(part, "name");
            let input = part.get("arguments").map(|input| {
                let mut bytes = Vec::new();
                canonical_json(input, &mut bytes);
                bytes
            });
            if let Some(id) = &tool_use_id {
                self.open_uses.insert(
                    id.clone(),
                    OpenUse {
                        name: name.clone(),
                        msg_id: at.key(),
                    },
                );
            }
            let (source, generation) = self.row_base();
            rows.tool_uses.push(PrepToolUseRow {
                source,
                generation,
                native_offset: Some(at.offset),
                native_key: at.key(),
                sighting: ToolSighting::Use,
                tool_use_id,
                call_msg_id: at.key(),
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

    /// A `toolResult` message: one result sighting paired with its open use.
    fn tool_result(&mut self, message: &Map<String, Value>, at: &At<'_>, rows: &mut PrepRows) {
        let tool_use_id = string(message, "toolCallId");
        let open = tool_use_id
            .as_ref()
            .and_then(|id| self.open_uses.remove(id));
        let (open_name, call_msg_id) = open.map_or((None, None), |open| (open.name, open.msg_id));
        let name = string(message, "toolName").or(open_name);
        let (source, generation) = self.row_base();
        rows.tool_uses.push(PrepToolUseRow {
            source,
            generation,
            native_offset: Some(at.offset),
            native_key: at.key(),
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
            result_bytes: message.get("content").map(result_bytes),
            outcome: Some(match message.get("isError") {
                Some(Value::Bool(true)) => ToolOutcome::Error,
                Some(Value::Bool(false)) => ToolOutcome::Ok,
                _ => ToolOutcome::Unknown,
            }),
            duration_ms: message
                .get("details")
                .and_then(|details| details.get("wallTimeMs"))
                .and_then(Value::as_i64),
            turn_no: Some(self.turn_no),
        });
    }
}

fn tracked_of(row: &PrepCallRow, tokens: &Tokens, ts_ms: i64) -> Tracked {
    Tracked {
        ts_ms,
        tokens: *tokens,
        model: row.model.clone(),
        stop_reason: row.stop_reason.clone(),
    }
}

/// `provider/model`; the bare model when the message names no provider.
fn qualified_model(message: &Map<String, Value>) -> Option<String> {
    let model = string(message, "model").filter(|model| !model.is_empty())?;
    Some(
        match string(message, "provider").filter(|provider| !provider.is_empty()) {
            Some(provider) => format!("{provider}/{model}"),
            None => model,
        },
    )
}

/// The query mapping's tool-family vocabulary; unknown names stay null.
fn family_of(name: Option<&str>) -> Option<String> {
    name.and_then(crate::query::tool_family).map(str::to_owned)
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

fn string(object: &Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(Value::as_str).map(str::to_owned)
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

/// Text of a string content or of its `text` parts.
fn text_of(content: Option<&Value>) -> String {
    match content {
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

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
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

/// The pij-rs payload (between `[pij-rs from …]` and `[/pij]`) with `[pij…]`
/// tags removed and whitespace collapsed. Hashed for fan-out grouping and the
/// join to pij's `message.pushed` bodies; never stored.
fn normalised_body(text: &str) -> String {
    let body = pij_rs_from(text).map_or(text, |(_, end)| {
        let rest = &text[end..];
        rest.find("[/pij]").map_or(rest, |close| &rest[..close])
    });
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some((start, end)) = rest
        .find("[pij")
        .and_then(|at| rest[at..].find(']').map(|close| (at, at + close + 1)))
    {
        out.push_str(&rest[..start]);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_paths_name_project_parent_and_agent() {
        assert_eq!(project_of("--Users-dev-demo-app--"), "demo-app");
        assert_eq!(project_of("--private-tmp--"), "private-tmp");
        assert_eq!(project_of("-"), "-");
        assert_eq!(
            parent_of_sub("oh-my-pi/default/--p--/2026-01-02T03-04-05-000Z_sess-p/Agent.jsonl"),
            Some("sess-p".into())
        );
        assert_eq!(parent_session_id("sess-p"), Some("sess-p".into()));
        assert_eq!(
            parent_session_id("/root/--p--/2026-01-02T03-04-05-000Z_sess-p.jsonl"),
            Some("sess-p".into())
        );
        assert_eq!(parent_session_id("/root/unrelated.txt"), None);
    }

    #[test]
    fn body_normalisation_keeps_only_the_pij_payload() {
        assert_eq!(
            normalised_body("[pij-rs from pij-a-b]\n  hello [pijMessageId:ab-12]\nworld\n[/pij]"),
            "hello world"
        );
        assert_eq!(
            pij_rs_from("x [pij-rs from pij-a-b] y"),
            Some(("pij-a-b".into(), 23))
        );
        assert_eq!(pij_message_id("[pijMessageId:ab-12]"), Some("ab-12".into()));
        assert_eq!(pij_message_id("[pijMessageId:AB]"), None);
    }
}
