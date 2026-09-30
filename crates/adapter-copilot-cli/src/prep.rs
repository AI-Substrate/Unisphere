//! Pure Copilot CLI folds into canonical prep tables.
//!
//! Supplied native input in, metadata facts out; no filesystem, clock or
//! environment. Two representations share one set of row rules:
//!
//! - [`CopilotCliPrepFold`] reads the append-only `events.jsonl` dialect,
//!   resumed from a byte cursor.
//! - [`CopilotCliLegacyPrepFold`] reads one legacy monolithic JSON document per
//!   snapshot revision; every row is addressed by a structural `native_key`
//!   (`document`, `document#/chatMessages/<i>`, `document#/timeline/<i>`).
//!
//! Events rules:
//!
//! - One API call is one native `apiCallId` (else the `messageId` of an
//!   id-less message). Its `assistant.message` records and its
//!   `assistant.usage` record merge by per-field maximum; a sighting whose call
//!   was first folded by an earlier batch or run is an `update`. `msg_id` is
//!   the call id; `request_id` stays null because the usage record carries no
//!   provider request id and every sighting of one call must share both keys.
//! - `model.model_call_success` records are the harness's utility model calls
//!   (their ids never match a conversation call). They are calls with
//!   `is_sidechain = true` and native `responseUsage`: `input` is the recorded
//!   `prompt_tokens`, `cache_read` is `cached_tokens`, and a cache write is
//!   split by its native `cache_ttl_seconds` (3600 → 1 h, 300 → 5 m).
//! - Copilot records only an aggregate `cacheWriteTokens`, so every
//!   conversation call names the documented fallback (main → 1 h, subagent →
//!   5 m) in `cache_write_basis`, whichever of its records is sighted first;
//!   the cache-write columns stay null until a usage record is folded. Every
//!   other field is the first sighting's, as the canonical reader keeps it. No
//!   stop reason is recorded: `stop_reason` is null.
//! - Records carrying a top-level `agentId` belong to a subagent: their calls
//!   are sidechain calls and never open, or feed the context of, main turns.
//! - `user.message` records are triggers classified by native `source`, `agentId`
//!   and the pij envelope; injected skill and instruction records are not
//!   triggers. The next new main-chain call opens the pending turn.
//! - `session.model_change` → `model_switch`; `session.compaction_complete` →
//!   `compaction` (a failed one has subkind `failed` and is not counted;
//!   `threshold`, `memory_pressure` and `context_limit_retry` count as `auto`);
//!   `session.error` → `api_error`; `abort`, `session.truncation` and
//!   `session.context_cleared` → `system_other`.
//! - `context_window` is the harness's own `tokenLimit` (its prompt-token
//!   limit for the active model) from the latest main-chain
//!   `session.compaction_start`, `session.compaction_complete`,
//!   `session.truncation` or `session.usage_info` record that carries one. A
//!   later `session.model_change` clears it, because the limit belongs to the
//!   model it was recorded for; until the next such record it is `None`.
//! - `cache_ttl_seconds` / `cache_expires_ms` are the native prompt-cache
//!   lifetime (`cacheTtlSeconds`, and `cacheExpiresAt` as epoch ms) of the
//!   current model's entry in the latest main-chain `session.usage_checkpoint`
//!   `modelCacheState`. The current model is the one the latest main-chain
//!   call named. With no such call, checkpoint or entry, both are `None`.
//! - Tool uses come from `toolRequests` (with the requesting call id) or, when
//!   unrequested, `tool.execution_start`; results from `tool.execution_complete`
//!   with its native `success`. Durations are not recorded natively: null.
//!
//! Legacy rules: chat messages are the conversation (assistant → call, user →
//! trigger, `tool_calls`/`tool` → tool uses); they carry no timestamp, model or
//! usage, so those columns are null and every folded chat message is counted in
//! `skipped.untimed`. The timeline, which duplicates the conversation with
//! timestamps but no joinable ids, only bounds the session's event times. The
//! dialect has no compaction marker: `compactions` is `None`.
//!
//! Timestamps: a record without one (or with one that does not parse) is still
//! folded with null time columns and counted in `skipped.untimed` (or
//! `skipped.bad_timestamp`); a line that is not a JSON object is `malformed`.
//! Message text is read only to classify openers and derive hashes; it leaves
//! the fold only as `triggers.content_head`, under explicit opt-in.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineError, PipelineErrorKind, SnapshotFormat,
    prep::{
        CacheWriteBasis, CallSighting, CompactionCounts, CompactionSample, ContextSample,
        ModelSwitch, PREP_CHECKPOINT_FORMAT, PrepCallRow, PrepCheckpoint, PrepEventKind,
        PrepEventRow, PrepFold, PrepFoldSession, PrepInput, PrepOptions, PrepRows, PrepSourceKind,
        PrepSourceMeta, PrepToolUseRow, PrepTriggerRow, PrepTurnRow, SessionFacts, ToolOutcome,
        ToolSighting, TurnOrigin,
    },
};

use crate::{DESCRIPTOR, SNAPSHOT_DESCRIPTOR, query::tool_family};

/// Events interpretation policy; bump on any rule change so every source re-emits.
pub const PREP_POLICY_VERSION: &str = "copilot-cli/prep-v3";
/// Legacy-document interpretation policy.
pub const LEGACY_PREP_POLICY_VERSION: &str = "copilot-cli-snapshot/prep-v1";

/// Append fold over `<session>/events.jsonl`.
#[derive(Debug, Clone, Copy, Default)]
pub struct CopilotCliPrepFold;

/// Snapshot fold over a legacy `<session>.json` document.
#[derive(Debug, Clone, Copy, Default)]
pub struct CopilotCliLegacyPrepFold;

fn invalid_data() -> PipelineError {
    PipelineError::new(PipelineErrorKind::InvalidData, None)
}

fn invalid_input() -> PipelineError {
    PipelineError::new(PipelineErrorKind::InvalidInput, None)
}

/// Fresh state, or the strict inverse of a checkpoint written by `policy`.
fn resume<T>(
    saved: Option<&PrepCheckpoint>,
    policy: &str,
    fresh: impl FnOnce() -> T,
    load: impl FnOnce(&Value) -> Option<T>,
) -> Result<T, PipelineError> {
    match saved {
        None => Ok(fresh()),
        Some(checkpoint)
            if checkpoint.format == PREP_CHECKPOINT_FORMAT && checkpoint.policy == policy =>
        {
            load(&checkpoint.fold).ok_or_else(invalid_data)
        }
        Some(_) => Err(invalid_data()),
    }
}

impl PrepFold for CopilotCliPrepFold {
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
    /// Session directories are opaque ids: no project or agent in the path.
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
        let session = resume(
            saved,
            PREP_POLICY_VERSION,
            || EventsSession {
                core: Core::new(source, generation, Some(CompactionCounts::default())),
                cache: CacheState::default(),
            },
            |value| EventsSession::load(source, generation, value),
        )?;
        Ok(Box::new(session))
    }
}

impl PrepFold for CopilotCliLegacyPrepFold {
    fn harness(&self) -> &'static str {
        SNAPSHOT_DESCRIPTOR.id
    }
    fn policy(&self) -> &'static str {
        LEGACY_PREP_POLICY_VERSION
    }
    fn kind(&self) -> PrepSourceKind {
        PrepSourceKind::Snapshot
    }
    fn pattern(&self) -> &'static str {
        SNAPSHOT_DESCRIPTOR.locations[0].session_glob
    }
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
        let session = resume(
            saved,
            LEGACY_PREP_POLICY_VERSION,
            || LegacySession {
                folded: false,
                core: Core::new(source, generation, None),
            },
            |value| {
                let object = value.as_object()?;
                Some(LegacySession {
                    folded: object.get("folded")?.as_bool()?,
                    core: Core::load(source, generation, object.get("core")?)?,
                })
            },
        )?;
        Ok(Box::new(session))
    }
}

// ---------------------------------------------------------------------------
// Shared row rules
// ---------------------------------------------------------------------------

/// input, cw_1h, cw_5m, cache_read, output; `None` = not recorded.
type Tokens = [Option<i64>; 5];

const NO_TOKENS: Tokens = [None; 5];

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

/// Attribution rule of the dialect's aggregate-only cache writes.
fn aggregate_basis(sidechain: bool) -> CacheWriteBasis {
    if sidechain {
        CacheWriteBasis::Fallback5m
    } else {
        CacheWriteBasis::Fallback1h
    }
}

/// `(cw_1h, cw_5m)` of an aggregate cache write under [`aggregate_basis`].
fn fallback_write(total: Option<i64>, sidechain: bool) -> (Option<i64>, Option<i64>) {
    match total {
        None => (None, None),
        Some(total) if sidechain => (Some(0), Some(total)),
        Some(total) => (Some(total), Some(0)),
    }
}

/// Native address and time of one record.
#[derive(Clone, Copy)]
struct At<'a> {
    offset: Option<u64>,
    key: Option<&'a str>,
    ts: Option<&'a str>,
    ts_ms: Option<i64>,
}

impl At<'_> {
    fn key(&self) -> Option<String> {
        self.key.map(str::to_owned)
    }
    fn ts(&self) -> Option<String> {
        self.ts.map(str::to_owned)
    }
}

/// Opening record of a turn not yet opened by a call.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    origin: TurnOrigin,
    sender: Option<String>,
    pij_msg_id: Option<String>,
    offset: Option<u64>,
    ts_ms: Option<i64>,
    chars: i64,
    body_key: Option<String>,
}

impl Pending {
    fn save(&self) -> Value {
        json!({
            "origin": self.origin, "sender": self.sender, "pij_msg_id": self.pij_msg_id,
            "offset": self.offset, "ts_ms": self.ts_ms, "chars": self.chars,
            "body_key": self.body_key,
        })
    }

    fn load(value: &Value) -> Option<Self> {
        let p = value.as_object()?;
        Some(Self {
            origin: serde_json::from_value(p.get("origin")?.clone()).ok()?,
            sender: opt_text(p.get("sender")?)?,
            pij_msg_id: opt_text(p.get("pij_msg_id")?)?,
            offset: opt(p.get("offset")?, Value::as_u64)?,
            ts_ms: opt(p.get("ts_ms")?, Value::as_i64)?,
            chars: p.get("chars")?.as_i64()?,
            body_key: opt_text(p.get("body_key")?)?,
        })
    }
}

/// A new call whose later sightings still change a derived fact.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tracked {
    key: u64,
    ts_ms: Option<i64>,
    tokens: Tokens,
}

impl Tracked {
    fn save(&self) -> Value {
        json!({ "key": hex(self.key), "ts_ms": self.ts_ms, "tokens": self.tokens })
    }

    fn load(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let array = object.get("tokens")?.as_array()?;
        let mut tokens = NO_TOKENS;
        if array.len() != tokens.len() {
            return None;
        }
        for (slot, value) in tokens.iter_mut().zip(array) {
            *slot = opt(value, Value::as_i64)?;
        }
        Some(Self {
            key: unhex(object.get("key")?)?,
            ts_ms: opt(object.get("ts_ms")?, Value::as_i64)?,
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

/// One call sighting as the dialect records it.
struct Sighting {
    key: u64,
    msg_id: Option<String>,
    request_id: Option<String>,
    model: Option<String>,
    tokens: Tokens,
    basis: CacheWriteBasis,
    sidechain: bool,
}

/// Row state shared by both representations.
#[derive(Debug, Clone)]
struct Core {
    source: String,
    generation: u32,
    turn_no: i64,
    call_in_turn: i64,
    turn_zero_emitted: bool,
    pending: Option<Pending>,
    /// Latest new call, any chain: gap origin and compaction context.
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

impl Core {
    fn new(source: &str, generation: u32, compactions: Option<CompactionCounts>) -> Self {
        Self {
            source: source.to_owned(),
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
                compactions,
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

    /// Strict inverse of [`Core::save`]; anything else is refused.
    fn load(source: &str, generation: u32, value: &Value) -> Option<Self> {
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

    /// Native time of a record: counted when absent or unparseable, and the
    /// session's event bounds when present.
    fn stamp<'a>(&mut self, value: Option<&'a Value>) -> (Option<&'a str>, Option<i64>) {
        let skipped = &mut self.session.skipped;
        let ts = match value {
            None | Some(Value::Null) => None,
            Some(Value::String(ts)) if ts.is_empty() => None,
            Some(value) => match value.as_str().and_then(|ts| Some((ts, parse_ms(ts)?))) {
                Some(stamp) => Some(stamp),
                None => {
                    skipped.bad_timestamp += 1;
                    return (None, None);
                }
            },
        };
        let Some((ts, ts_ms)) = ts else {
            skipped.untimed += 1;
            return (None, None);
        };
        let session = &mut self.session;
        if session.first_event_ts.is_none() {
            session.first_event_ts = Some(ts.to_owned());
            session.first_event_ms = Some(ts_ms);
        }
        session.last_event_ts = Some(ts.to_owned());
        session.last_event_ms = Some(ts_ms);
        (Some(ts), Some(ts_ms))
    }

    fn row_base(&self) -> (String, u32) {
        (self.source.clone(), self.generation)
    }

    fn event(&self, at: &At<'_>, kind: PrepEventKind, subkind: Option<String>) -> PrepEventRow {
        let (source, generation) = self.row_base();
        PrepEventRow {
            source,
            generation,
            native_offset: at.offset,
            native_key: at.key(),
            ts: at.ts(),
            ts_ms: at.ts_ms,
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

    fn gap_since_last_call(&self, ts_ms: Option<i64>) -> Option<i64> {
        Some(ts_ms? - self.last_call.as_ref()?.ts_ms?)
    }

    fn last_context(&self) -> Option<i64> {
        self.last_call
            .as_ref()
            .and_then(|last| context(&last.tokens))
    }

    /// A trigger row; `opens` makes it the opener of the next main-chain turn.
    fn trigger(
        &mut self,
        at: &At<'_>,
        (origin, sender): (TurnOrigin, Option<String>),
        text: &str,
        opens: bool,
        options: PrepOptions,
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
            native_offset: at.offset,
            native_key: at.key(),
            ts: at.ts(),
            ts_ms: at.ts_ms,
            kind: origin,
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
                origin,
                sender,
                pij_msg_id,
                offset: at.offset,
                ts_ms: at.ts_ms,
                chars,
                body_key: Some(body_key),
            });
        }
    }

    fn call(
        &mut self,
        at: &At<'_>,
        sighting: Sighting,
        rows: &mut PrepRows,
        batch_calls: &mut HashMap<u64, usize>,
    ) {
        let Sighting {
            key,
            msg_id,
            request_id,
            model,
            tokens,
            basis,
            sidechain,
        } = sighting;
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
        let (source, generation) = self.row_base();
        let mut row = PrepCallRow {
            source,
            generation,
            native_offset: at.offset,
            native_key: at.key(),
            sighting: CallSighting::First,
            msg_id,
            request_id,
            ts: at.ts(),
            ts_ms: at.ts_ms,
            model,
            stop_reason: None,
            input: tokens[0],
            cw_1h: tokens[1],
            cw_5m: tokens[2],
            cache_read: tokens[3],
            output: tokens[4],
            cache_write_basis: basis,
            is_sidechain: sidechain,
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

    /// A later sighting raises the facts derived from its first sighting.
    fn repeat_facts(&mut self, key: u64, tokens: &Tokens) {
        if let Some(last) = self.last_call.as_mut().filter(|t| t.key == key) {
            merge_tokens(&mut last.tokens, tokens);
        }
        if let Some(main) = self.latest_main.as_mut().filter(|t| t.key == key) {
            merge_tokens(&mut main.tokens, tokens);
            if let Some(sample) = self.session.latest_context.as_mut() {
                sample_tokens(sample, &main.tokens);
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
    /// session facts. Only main-chain calls open turns or feed the context.
    fn new_call(
        &mut self,
        key: u64,
        tokens: Tokens,
        at: &At<'_>,
        row: &mut PrepCallRow,
        rows: &mut PrepRows,
    ) {
        let main = !row.is_sidechain;
        self.committed.insert(key);
        self.session.calls += 1;
        if main {
            if let Some(pending) = self.pending.take() {
                self.turn_no += 1;
                self.call_in_turn = 0;
                self.push_turn(at, pending.origin, Some(pending), rows);
            } else if self.turn_no == 0 && !self.turn_zero_emitted {
                self.push_turn(at, TurnOrigin::Start, None, rows);
            }
            self.turn_zero_emitted = true;
        }
        self.call_in_turn += 1;
        row.gap_ms = match &self.last_call {
            None => Some(-1),
            Some(_) => self.gap_since_last_call(at.ts_ms),
        };
        row.turn_no = Some(self.turn_no);
        row.call_in_turn = Some(self.call_in_turn);
        let tracked = Tracked {
            key,
            ts_ms: at.ts_ms,
            tokens,
        };
        if main {
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
            native_offset: at.offset,
            native_key: at.key(),
            turn_no: self.turn_no,
            started_ts: at.ts(),
            started_ts_ms: at.ts_ms,
            first_call_offset: at.offset,
            origin,
            sender: opener.and_then(|p| p.sender.clone()),
            pij_msg_id: opener.and_then(|p| p.pij_msg_id.clone()),
            opener_offset: opener.and_then(|p| p.offset),
            opener_ts_ms: opener.and_then(|p| p.ts_ms),
            opener_chars: opener.map(|p| p.chars),
            body_key: opener.and_then(|p| p.body_key.clone()),
        });
        self.session.turns += 1;
    }

    /// A use sighting, deduplicated by native id across batches and runs.
    fn tool_use(
        &mut self,
        at: &At<'_>,
        tool_use_id: Option<String>,
        name: Option<String>,
        call_msg_id: Option<String>,
        input: Option<&Value>,
        rows: &mut PrepRows,
    ) {
        if let Some(id) = &tool_use_id {
            if !self.tool_ids.insert(fnv(id.as_bytes())) {
                return;
            }
            self.open_uses.insert(
                id.clone(),
                OpenUse {
                    name: name.clone(),
                    msg_id: call_msg_id.clone(),
                },
            );
        }
        let input = input.map(|input| {
            let mut bytes = Vec::new();
            canonical_json(input, &mut bytes);
            bytes
        });
        let (source, generation) = self.row_base();
        rows.tool_uses.push(PrepToolUseRow {
            source,
            generation,
            native_offset: at.offset,
            native_key: at.key(),
            sighting: ToolSighting::Use,
            tool_use_id,
            call_msg_id,
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
            turn_no: Some(self.turn_no),
        });
    }

    /// A result sighting, paired with its open use by id.
    fn tool_result(
        &mut self,
        at: &At<'_>,
        tool_use_id: Option<String>,
        outcome: ToolOutcome,
        result_bytes: Option<i64>,
        rows: &mut PrepRows,
    ) {
        let open = tool_use_id
            .as_ref()
            .and_then(|id| self.open_uses.remove(id));
        let (name, call_msg_id) = open.map_or((None, None), |open| (open.name, open.msg_id));
        let (source, generation) = self.row_base();
        rows.tool_uses.push(PrepToolUseRow {
            source,
            generation,
            native_offset: at.offset,
            native_key: at.key(),
            sighting: ToolSighting::Result,
            tool_use_id,
            call_msg_id,
            ts: at.ts(),
            ts_ms: at.ts_ms,
            family: family_of(name.as_deref()),
            name,
            input_hash: None,
            input_bytes: None,
            result_offset: at.offset,
            result_bytes,
            outcome: Some(outcome),
            duration_ms: None,
            turn_no: Some(self.turn_no),
        });
    }
}

// ---------------------------------------------------------------------------
// Events (append)
// ---------------------------------------------------------------------------

/// A prompt-cache lifetime as `session.usage_checkpoint` records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lifetime {
    ttl_seconds: Option<i64>,
    expires_ms: Option<i64>,
}

/// What the cache-lifetime facts are derived from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct CacheState {
    /// Model the latest main-chain call named.
    call_model: Option<String>,
    /// The latest main-chain checkpoint's `modelCacheState`, by `modelId`.
    lifetimes: BTreeMap<String, Lifetime>,
}

impl CacheState {
    fn save(&self) -> Value {
        let lifetimes: Map<String, Value> = self
            .lifetimes
            .iter()
            .map(|(model, life)| {
                (
                    model.clone(),
                    json!({ "ttl_seconds": life.ttl_seconds, "expires_ms": life.expires_ms }),
                )
            })
            .collect();
        json!({ "call_model": self.call_model, "lifetimes": lifetimes })
    }

    fn load(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let lifetimes = object
            .get("lifetimes")?
            .as_object()?
            .iter()
            .map(|(model, life)| {
                let life = life.as_object()?;
                let read = |key: &str| opt(life.get(key)?, Value::as_i64);
                Some((
                    model.clone(),
                    Lifetime {
                        ttl_seconds: read("ttl_seconds")?,
                        expires_ms: read("expires_ms")?,
                    },
                ))
            })
            .collect::<Option<BTreeMap<_, _>>>()?;
        Some(Self {
            call_model: opt_text(object.get("call_model")?)?,
            lifetimes,
        })
    }

    /// Replace the lifetimes with a checkpoint's `modelCacheState` entries.
    fn record(&mut self, entries: &[Value]) {
        self.lifetimes = entries
            .iter()
            .filter_map(Value::as_object)
            .filter_map(|entry| {
                let lifetime = Lifetime {
                    ttl_seconds: int(entry, "cacheTtlSeconds"),
                    expires_ms: entry
                        .get("cacheExpiresAt")
                        .and_then(Value::as_str)
                        .and_then(parse_ms),
                };
                Some((text(entry, "modelId")?, lifetime))
            })
            .collect();
    }

    fn apply(&self, session: &mut SessionFacts) {
        let lifetime = self
            .call_model
            .as_ref()
            .and_then(|model| self.lifetimes.get(model));
        session.cache_ttl_seconds = lifetime.and_then(|life| life.ttl_seconds);
        session.cache_expires_ms = lifetime.and_then(|life| life.expires_ms);
    }
}

struct EventsSession {
    core: Core,
    cache: CacheState,
}

impl PrepFoldSession for EventsSession {
    fn checkpoint(&self) -> PrepCheckpoint {
        PrepCheckpoint {
            format: PREP_CHECKPOINT_FORMAT,
            policy: PREP_POLICY_VERSION.to_owned(),
            fold: json!({ "core": self.core.save(), "cache": self.cache.save() }),
        }
    }

    fn facts(&self) -> SessionFacts {
        self.core.session.clone()
    }

    fn fold(&mut self, input: &PrepInput, options: PrepOptions) -> Result<PrepRows, PipelineError> {
        match input {
            PrepInput::Records(records) => Ok(self.fold_records(records, options)),
            PrepInput::Snapshot(_) => Err(invalid_input()),
        }
    }
}

impl EventsSession {
    /// Strict inverse of the events checkpoint; anything else is refused.
    fn load(source: &str, generation: u32, value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        Some(Self {
            core: Core::load(source, generation, object.get("core")?)?,
            cache: CacheState::load(object.get("cache")?)?,
        })
    }

    fn fold_records(&mut self, records: &[NativeRecord], options: PrepOptions) -> PrepRows {
        let mut rows = PrepRows::default();
        // Calls sighted in this batch, by key, so repeats merge in place.
        let mut batch_calls: HashMap<u64, usize> = HashMap::new();
        let empty = Map::new();
        for native in records {
            let core = &mut self.core;
            let Ok(Value::Object(record)) = serde_json::from_slice::<Value>(&native.bytes) else {
                core.session.skipped.malformed += 1;
                continue;
            };
            core.session.records += 1;
            let (ts, ts_ms) = core.stamp(record.get("timestamp"));
            let at = At {
                offset: Some(native.offset),
                key: None,
                ts,
                ts_ms,
            };
            let data = record
                .get("data")
                .and_then(Value::as_object)
                .unwrap_or(&empty);
            let sidechain = text(&record, "agentId").is_some();
            let kind = record.get("type").and_then(Value::as_str).unwrap_or("");
            if !sidechain
                && matches!(
                    kind,
                    "session.compaction_start"
                        | "session.compaction_complete"
                        | "session.truncation"
                        | "session.usage_info"
                )
                && let Some(limit) = int(data, "tokenLimit").filter(|limit| *limit > 0)
            {
                core.session.context_window = Some(limit);
            }
            match kind {
                "session.start" | "session.resume" => session_context(core, kind, data),
                "user.message" => {
                    let body = data.get("content").and_then(Value::as_str).unwrap_or("");
                    if let Some((origin, opens)) = classify_event(&record, data, body) {
                        core.trigger(&at, origin, body, opens, options, &mut rows);
                    }
                }
                "assistant.message" => {
                    let msg_id = text(data, "apiCallId").or_else(|| text(data, "messageId"));
                    let sighting = Sighting {
                        key: call_key(msg_id.as_deref(), native.offset),
                        msg_id: msg_id.clone(),
                        request_id: None,
                        model: text(data, "model"),
                        tokens: [None, None, None, None, int(data, "outputTokens")],
                        basis: aggregate_basis(sidechain),
                        sidechain,
                    };
                    core.call(&at, sighting, &mut rows, &mut batch_calls);
                    for request in data
                        .get("toolRequests")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_object)
                    {
                        core.tool_use(
                            &at,
                            text(request, "toolCallId"),
                            text(request, "name"),
                            msg_id.clone(),
                            request.get("arguments"),
                            &mut rows,
                        );
                    }
                }
                "assistant.usage" => {
                    let msg_id = text(data, "apiCallId");
                    let (cw_1h, cw_5m) = fallback_write(int(data, "cacheWriteTokens"), sidechain);
                    let sighting = Sighting {
                        key: call_key(msg_id.as_deref(), native.offset),
                        msg_id,
                        request_id: None,
                        model: text(data, "model"),
                        tokens: [
                            int(data, "inputTokens"),
                            cw_1h,
                            cw_5m,
                            int(data, "cacheReadTokens"),
                            int(data, "outputTokens"),
                        ],
                        basis: aggregate_basis(sidechain),
                        sidechain,
                    };
                    core.call(&at, sighting, &mut rows, &mut batch_calls);
                }
                "model.model_call_success" => {
                    let sighting = utility_call(data, native.offset);
                    core.call(&at, sighting, &mut rows, &mut batch_calls);
                }
                "tool.execution_start" => core.tool_use(
                    &at,
                    text(data, "toolCallId"),
                    text(data, "toolName"),
                    None,
                    data.get("arguments"),
                    &mut rows,
                ),
                "tool.execution_complete" => {
                    let outcome = match data.get("success") {
                        Some(Value::Bool(true)) => ToolOutcome::Ok,
                        Some(Value::Bool(false)) => ToolOutcome::Error,
                        _ => ToolOutcome::Unknown,
                    };
                    let result_bytes = data
                        .get("result")
                        .and_then(|result| result.get("content"))
                        .and_then(Value::as_str)
                        .map(|content| len_i64(content.len()));
                    core.tool_result(
                        &at,
                        text(data, "toolCallId"),
                        outcome,
                        result_bytes,
                        &mut rows,
                    );
                }
                "session.model_change" => {
                    core.session.context_window = None;
                    if let Some(model) = text(data, "newModel") {
                        let mut event =
                            core.event(&at, PrepEventKind::ModelSwitch, text(data, "source"));
                        event.model = Some(model.clone());
                        rows.events.push(event);
                        core.session.last_model_switch = Some(ModelSwitch {
                            ts_ms: at.ts_ms,
                            requested_model: model,
                        });
                    }
                }
                "session.compaction_complete" => {
                    rows.events.push(compaction(core, &at, data));
                }
                "session.error" => {
                    let subkind = text(data, "errorType");
                    rows.events
                        .push(core.event(&at, PrepEventKind::ApiError, subkind));
                }
                "abort" | "session.truncation" | "session.context_cleared" => {
                    rows.events.push(core.event(
                        &at,
                        PrepEventKind::SystemOther,
                        Some(kind.to_owned()),
                    ));
                }
                "session.usage_checkpoint" if !sidechain => {
                    if let Some(entries) = data.get("modelCacheState").and_then(Value::as_array) {
                        self.cache.record(entries);
                    }
                }
                _ => {}
            }
            if let Some(model) = core
                .session
                .latest_context
                .as_ref()
                .and_then(|sample| sample.model.as_ref())
                && self.cache.call_model.as_ref() != Some(model)
            {
                self.cache.call_model = Some(model.clone());
            }
        }
        self.cache.apply(&mut self.core.session);
        rows
    }
}

/// Session identity, lineage and working directory: first native value wins.
fn session_context(core: &mut Core, kind: &str, data: &Map<String, Value>) {
    let session = &mut core.session;
    if kind == "session.start" && session.session_id.is_none() {
        session.session_id = text(data, "sessionId");
    }
    if session.parent_session_id.is_none() {
        session.parent_session_id = text(data, "detachedFromSpawningParentSessionId");
    }
    if session.cwd.is_none() {
        session.cwd = data
            .get("context")
            .and_then(Value::as_object)
            .and_then(|context| text(context, "cwd"));
    }
}

/// A native compaction boundary. A failed attempt is an event but not a boundary.
fn compaction(core: &mut Core, at: &At<'_>, data: &Map<String, Value>) -> PrepEventRow {
    let failed = data.get("success") == Some(&Value::Bool(false));
    let mut event = core.event(
        at,
        PrepEventKind::Compaction,
        failed.then(|| "failed".to_owned()),
    );
    event.trigger = text(data, "trigger");
    event.pre_tokens = int(data, "preCompactionTokens");
    event.post_tokens = int(data, "postCompactionTokens");
    event.last_context = core.last_context();
    event.gap_ms = core.gap_since_last_call(at.ts_ms);
    if failed {
        return event;
    }
    let counts = core
        .session
        .compactions
        .get_or_insert_with(CompactionCounts::default);
    match event.trigger.as_deref() {
        Some("manual") => counts.manual += 1,
        Some("threshold" | "memory_pressure" | "context_limit_retry") => counts.auto += 1,
        _ => counts.unknown_trigger += 1,
    }
    core.session.last_compaction = Some(CompactionSample {
        ts_ms: at.ts_ms,
        trigger: event.trigger.clone(),
        pre_tokens: event.pre_tokens,
        post_tokens: event.post_tokens,
        first_context_after: None,
    });
    core.first_after = None;
    core.awaiting_first_after = true;
    event
}

/// A utility model call with its native response usage.
fn utility_call(data: &Map<String, Value>, offset: u64) -> Sighting {
    let empty = Map::new();
    let object = |parent: &Map<String, Value>, key: &str| {
        parent.get(key).and_then(Value::as_object).cloned()
    };
    let usage = object(data, "responseUsage").unwrap_or_default();
    let details = object(&usage, "prompt_tokens_details").unwrap_or_default();
    let request_id = text(data, "requestId");
    let msg_id = text(data, "callId").or_else(|| request_id.clone());
    let written = int(&details, "cache_creation_tokens");
    let (cw_1h, cw_5m, basis) = match (written, int(&details, "cache_ttl_seconds")) {
        (Some(total), Some(3600)) => (Some(total), Some(0), CacheWriteBasis::Split),
        (Some(total), Some(300)) => (Some(0), Some(total), CacheWriteBasis::Split),
        (None, _) => (None, None, CacheWriteBasis::None),
        (total, _) => {
            let (cw_1h, cw_5m) = fallback_write(total, true);
            (cw_1h, cw_5m, aggregate_basis(true))
        }
    };
    Sighting {
        key: call_key(msg_id.as_deref(), offset),
        msg_id,
        request_id,
        model: data
            .get("modelCall")
            .and_then(Value::as_object)
            .unwrap_or(&empty)
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        tokens: [
            int(&usage, "prompt_tokens"),
            cw_1h,
            cw_5m,
            int(&details, "cached_tokens"),
            int(&usage, "completion_tokens"),
        ],
        basis,
        sidechain: true,
    }
}

/// Trigger classification of a `user.message`: `(origin, sender)` and whether
/// it opens a main-chain turn; injected skill and instruction records are not
/// triggers.
fn classify_event(
    record: &Map<String, Value>,
    data: &Map<String, Value>,
    body: &str,
) -> Option<((TurnOrigin, Option<String>), bool)> {
    let source = data.get("source").and_then(Value::as_str);
    if source.is_some_and(|s| s.starts_with("skill-") || s == "instruction-discovery") {
        return None;
    }
    if text(record, "agentId").is_some() || source.is_some_and(|s| s.starts_with("agent-")) {
        return Some(((TurnOrigin::SubagentTask, None), false));
    }
    let origin = classify_text(body).unwrap_or_else(|| {
        let origin = match source {
            None | Some("user") => TurnOrigin::Human,
            Some(s) if s.starts_with("schedule-") => TurnOrigin::Scheduled,
            Some(_) => TurnOrigin::Other,
        };
        (origin, None)
    });
    Some((origin, true))
}

/// Origins the text itself declares: a pij peer envelope or `/compact`.
fn classify_text(body: &str) -> Option<(TurnOrigin, Option<String>)> {
    if let Some((sender, _)) = pij_rs_from(body) {
        return Some((TurnOrigin::Peer, Some(sender)));
    }
    body.trim_start()
        .starts_with("/compact")
        .then_some((TurnOrigin::ManualCompact, None))
}

// ---------------------------------------------------------------------------
// Legacy document (snapshot)
// ---------------------------------------------------------------------------

struct LegacySession {
    /// A generation is one snapshot revision, folded exactly once.
    folded: bool,
    core: Core,
}

impl PrepFoldSession for LegacySession {
    fn checkpoint(&self) -> PrepCheckpoint {
        PrepCheckpoint {
            format: PREP_CHECKPOINT_FORMAT,
            policy: LEGACY_PREP_POLICY_VERSION.to_owned(),
            fold: json!({ "folded": self.folded, "core": self.core.save() }),
        }
    }

    fn facts(&self) -> SessionFacts {
        self.core.session.clone()
    }

    fn fold(&mut self, input: &PrepInput, options: PrepOptions) -> Result<PrepRows, PipelineError> {
        match input {
            PrepInput::Snapshot(snapshot) => self.fold_snapshot(snapshot, options),
            PrepInput::Records(_) => Err(invalid_input()),
        }
    }
}

impl LegacySession {
    fn fold_snapshot(
        &mut self,
        snapshot: &NativeSnapshot,
        options: PrepOptions,
    ) -> Result<PrepRows, PipelineError> {
        if self.folded || snapshot.source.format != SnapshotFormat::JsonDocument {
            return Err(invalid_input());
        }
        let [record] = snapshot.records.as_slice() else {
            return Err(invalid_data());
        };
        if record.key != "document" || snapshot.revision.is_empty() {
            return Err(invalid_data());
        }
        let Ok(Value::Object(document)) = serde_json::from_slice::<Value>(&record.bytes) else {
            return Err(invalid_data());
        };
        let session_id = text(&document, "sessionId");
        if let Some(selected) = snapshot.source.session_id.as_deref()
            && session_id.as_deref() != Some(selected)
        {
            return Err(invalid_input());
        }
        self.folded = true;
        let core = &mut self.core;
        core.session.session_id = session_id;
        core.session.records += 1;
        core.stamp(document.get("startTime"));
        let mut rows = PrepRows::default();
        let mut batch_calls = HashMap::new();
        for (index, item) in items(core, &document, "chatMessages") {
            let key = format!("document#/chatMessages/{index}");
            let Some(message) = item.as_object() else {
                core.session.skipped.malformed += 1;
                continue;
            };
            core.session.records += 1;
            // Chat messages carry no native time.
            core.session.skipped.untimed += 1;
            let at = At {
                offset: None,
                key: Some(&key),
                ts: None,
                ts_ms: None,
            };
            match message.get("role").and_then(Value::as_str) {
                Some("user") => {
                    let body = message.get("content").and_then(Value::as_str).unwrap_or("");
                    let origin = classify_text(body).unwrap_or((TurnOrigin::Human, None));
                    core.trigger(&at, origin, body, true, options, &mut rows);
                }
                Some("assistant") => {
                    let sighting = Sighting {
                        key: call_key(None, index as u64),
                        msg_id: None,
                        request_id: None,
                        model: None,
                        tokens: NO_TOKENS,
                        basis: CacheWriteBasis::None,
                        sidechain: false,
                    };
                    core.call(&at, sighting, &mut rows, &mut batch_calls);
                    for call in message
                        .get("tool_calls")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_object)
                    {
                        let function = call.get("function").and_then(Value::as_object);
                        core.tool_use(
                            &at,
                            text(call, "id"),
                            function.and_then(|f| text(f, "name")),
                            None,
                            function.and_then(|f| f.get("arguments")),
                            &mut rows,
                        );
                    }
                }
                Some("tool") => {
                    let result_bytes = message
                        .get("content")
                        .and_then(Value::as_str)
                        .map(|content| len_i64(content.len()));
                    core.tool_result(
                        &at,
                        text(message, "tool_call_id"),
                        ToolOutcome::Unknown,
                        result_bytes,
                        &mut rows,
                    );
                }
                _ => {}
            }
        }
        for (_, item) in items(core, &document, "timeline") {
            let Some(entry) = item.as_object() else {
                core.session.skipped.malformed += 1;
                continue;
            };
            core.session.records += 1;
            core.stamp(entry.get("timestamp"));
        }
        Ok(rows)
    }
}

/// Items of a document array; a present non-array view is one malformed record.
fn items<'a>(
    core: &mut Core,
    document: &'a Map<String, Value>,
    view: &str,
) -> impl Iterator<Item = (usize, &'a Value)> + use<'a> {
    let array = match document.get(view) {
        None => None,
        Some(Value::Array(array)) => Some(array),
        Some(_) => {
            core.session.skipped.malformed += 1;
            None
        }
    };
    array.into_iter().flatten().enumerate()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The query mapping's tool-family vocabulary; unknown names stay null.
fn family_of(name: Option<&str>) -> Option<String> {
    name.and_then(tool_family).map(str::to_owned)
}

fn text(object: &Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn int(object: &Map<String, Value>, key: &str) -> Option<i64> {
    object.get(key).and_then(Value::as_i64)
}

fn len_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

fn parse_ms(ts: &str) -> Option<i64> {
    let parsed = OffsetDateTime::parse(ts, &Rfc3339).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos().div_euclid(1_000_000)).ok()
}

/// A call's dedupe key: its native id, else its position (a call of its own).
fn call_key(id: Option<&str>, position: u64) -> u64 {
    match id {
        Some(id) => fnv(&[b"id:", id.as_bytes()].concat()),
        None => fnv(format!("at:{position}").as_bytes()),
    }
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
/// tags and cross-session wrappers removed and whitespace collapsed: the same
/// normalisation as the Claude fold, so `body_key` joins across harnesses and
/// to pij's `message.pushed` bodies. Hashed; never stored.
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
