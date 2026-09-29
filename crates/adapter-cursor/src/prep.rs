//! Pure Cursor folds into canonical prep tables.
//!
//! Supplied native input in, metadata facts out; no filesystem, clock or
//! environment. Neither fold estimates a fact its dialect does not record:
//!
//! - **Transcript JSONL** (`cursor-transcript`, append). The writer records no
//!   ids, timestamps, models or usage. Every row is keyed by `native_offset`;
//!   ids, timestamps, model, stop reason, token counters and gaps are null. One
//!   call row per assistant record (a native message, not a proven API call);
//!   a user record is a trigger whose turn the next call opens; `tool_use`
//!   parts are use sightings (the dialect records no results); `turn_ended`
//!   and `metadata` records are `system_other` events. No compaction marker
//!   exists, so `compactions` is null. The session id is the transcript file
//!   stem, which the writer names after the agent session.
//! - **IDE `cursorDiskKV`** (`cursor-ide`, snapshot). One `state.vscdb` holds
//!   many composers. Every row's `native_key` is the native key it was read
//!   from — `composerData:<composer>` or `bubbleId:<composer>:<bubble>` — so the
//!   composer id is always the second `:` field and every row stays fetchable
//!   by key. Composers are folded in key order and their bubbles in the
//!   composer-declared header order; orphan bubbles are not part of that spine
//!   and are ignored. Per composer: a `system_other`/`composer` event carries
//!   `createdAt` and `modelConfig.modelName`; a user bubble is a trigger and,
//!   when its `modelInfo.modelName` differs from the composer's previous
//!   request, a `model_switch`; an assistant bubble is a call (bubble id as
//!   `msg_id`, its own `requestId`, `modelInfo` model and `tokenCount`
//!   input/output, cache columns null) and its `toolFormerData` a use plus,
//!   once terminal, a result with the native outcome. The writer stores 0/0 on
//!   bubbles it did not meter, so a `tokenCount` without a non-zero counter is
//!   not recorded (null), never a free call. `usageData`
//!   has no proven token scope and is not mapped. Turns are numbered across
//!   the whole database; a composer's first call without an opener opens a
//!   `start` turn. `gap_ms` is measured within one composer. Composers listed
//!   in another's `subagentComposerIds`/`subComposerIds` are sidechains. The
//!   per-source [`SessionFacts`] describe the database, not one composer.
//!
//! Message text is read only to count characters and derive a body hash; it
//! leaves a fold only as `triggers.content_head`, under explicit opt-in.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineError, PipelineErrorKind,
    prep::{
        CacheWriteBasis, CallSighting, ContextSample, ModelSwitch, PREP_CHECKPOINT_FORMAT,
        PrepCallRow, PrepCheckpoint, PrepEventKind, PrepEventRow, PrepFold, PrepFoldSession,
        PrepInput, PrepOptions, PrepRows, PrepSourceKind, PrepSourceMeta, PrepToolUseRow,
        PrepTriggerRow, PrepTurnRow, SessionFacts, ToolOutcome, ToolSighting, TurnOrigin,
    },
};

use crate::{DESCRIPTOR, IDE_DESCRIPTOR, ide};

/// Transcript interpretation policy; bump on any rule change so every source re-emits.
pub const TRANSCRIPT_PREP_POLICY: &str = "cursor-transcript/prep-v1";
/// IDE interpretation policy; bump on any rule change so every source re-emits.
pub const IDE_PREP_POLICY: &str = "cursor-ide/prep-v1";

fn invalid_data() -> PipelineError {
    PipelineError::new(PipelineErrorKind::InvalidData, None)
}

/// The saved fold state when `saved` was written by `policy` in this format.
fn saved_fold<'a>(
    saved: Option<&'a PrepCheckpoint>,
    policy: &str,
) -> Result<Option<&'a Value>, PipelineError> {
    match saved {
        None => Ok(None),
        Some(checkpoint)
            if checkpoint.format == PREP_CHECKPOINT_FORMAT && checkpoint.policy == policy =>
        {
            Ok(Some(&checkpoint.fold))
        }
        Some(_) => Err(invalid_data()),
    }
}

fn checkpoint(policy: &str, fold: Value) -> PrepCheckpoint {
    PrepCheckpoint {
        format: PREP_CHECKPOINT_FORMAT,
        policy: policy.to_owned(),
        fold,
    }
}

// ---------------------------------------------------------------------------
// Transcript JSONL
// ---------------------------------------------------------------------------

/// Cursor agent-transcript JSONL fold (append sources).
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorTranscriptPrepFold;

impl PrepFold for CursorTranscriptPrepFold {
    fn harness(&self) -> &'static str {
        DESCRIPTOR.id
    }
    fn policy(&self) -> &'static str {
        TRANSCRIPT_PREP_POLICY
    }
    fn kind(&self) -> PrepSourceKind {
        PrepSourceKind::Append
    }
    fn pattern(&self) -> &'static str {
        DESCRIPTOR.locations[0].session_glob
    }
    fn describe(&self, file: &str) -> PrepSourceMeta {
        PrepSourceMeta {
            is_sub: false,
            agent_id: None,
            project: file
                .split('/')
                .next()
                .filter(|project| !project.is_empty())
                .map(str::to_owned),
        }
    }
    fn open(
        &self,
        _meta: &PrepSourceMeta,
        source: &str,
        generation: u32,
        saved: Option<&PrepCheckpoint>,
    ) -> Result<Box<dyn PrepFoldSession>, PipelineError> {
        let state = match saved_fold(saved, TRANSCRIPT_PREP_POLICY)? {
            None => TranscriptState::new(source, generation),
            Some(fold) => {
                TranscriptState::load(source, generation, fold).ok_or_else(invalid_data)?
            }
        };
        Ok(Box::new(state))
    }
}

/// The trigger a turn has not yet been opened for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Opener {
    origin: TurnOrigin,
    offset: Option<u64>,
    ts_ms: Option<i64>,
    chars: i64,
    body_key: Option<String>,
}

impl Opener {
    fn save(&self) -> Value {
        json!({
            "origin": self.origin, "offset": self.offset, "ts_ms": self.ts_ms,
            "chars": self.chars, "body_key": self.body_key,
        })
    }

    fn load(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        Some(Self {
            origin: serde_json::from_value(object.get("origin")?.clone()).ok()?,
            offset: opt(object.get("offset")?, Value::as_u64)?,
            ts_ms: opt(object.get("ts_ms")?, Value::as_i64)?,
            chars: object.get("chars")?.as_i64()?,
            body_key: opt(object.get("body_key")?, |v| v.as_str().map(str::to_owned))?,
        })
    }
}

/// JSON null → `Some(None)`; a value `read` accepts → `Some(Some(v))`; else `None`.
fn opt<T>(value: &Value, read: impl Fn(&Value) -> Option<T>) -> Option<Option<T>> {
    match value {
        Value::Null => Some(None),
        value => read(value).map(Some),
    }
}

#[derive(Debug, Clone)]
struct TranscriptState {
    source: String,
    generation: u32,
    turn_no: i64,
    call_in_turn: i64,
    turn_zero_emitted: bool,
    pending: Option<Opener>,
    session: SessionFacts,
}

impl TranscriptState {
    fn new(source: &str, generation: u32) -> Self {
        // `<harness>/<label>/<project>/agent-transcripts/<id>/<id>.jsonl`.
        let session_id = source
            .rsplit('/')
            .next()
            .and_then(|name| name.strip_suffix(".jsonl"))
            .filter(|stem| !stem.is_empty())
            .map(str::to_owned);
        Self {
            source: source.to_owned(),
            generation,
            turn_no: 0,
            call_in_turn: 0,
            turn_zero_emitted: false,
            pending: None,
            session: SessionFacts {
                session_id,
                ..SessionFacts::default()
            },
        }
    }

    fn save(&self) -> Value {
        json!({
            "turn_no": self.turn_no,
            "call_in_turn": self.call_in_turn,
            "turn_zero_emitted": self.turn_zero_emitted,
            "pending": self.pending.as_ref().map(Opener::save),
            "session": serde_json::to_value(&self.session).unwrap_or(Value::Null),
        })
    }

    /// Strict inverse of [`TranscriptState::save`]; anything else is refused.
    fn load(source: &str, generation: u32, value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let field = |key: &str| object.get(key);
        Some(Self {
            source: source.to_owned(),
            generation,
            turn_no: field("turn_no")?.as_i64()?,
            call_in_turn: field("call_in_turn")?.as_i64()?,
            turn_zero_emitted: field("turn_zero_emitted")?.as_bool()?,
            pending: opt(field("pending")?, Opener::load)?,
            session: serde_json::from_value(field("session")?.clone()).ok()?,
        })
    }

    fn fold_records(&mut self, records: &[NativeRecord], options: PrepOptions) -> PrepRows {
        let mut rows = PrepRows::default();
        for native in records {
            let Ok(Value::Object(record)) = serde_json::from_slice::<Value>(&native.bytes) else {
                self.session.skipped.malformed += 1;
                continue;
            };
            self.session.records += 1;
            let offset = native.offset;
            // Control records carry `type`; Cursor messages carry only `role`.
            if let Some(kind) = record.get("type") {
                let subkind = match kind.as_str() {
                    Some("turn_ended") => match record.get("status").and_then(Value::as_str) {
                        Some(status @ ("success" | "error" | "aborted")) => {
                            format!("turn_ended:{status}")
                        }
                        _ => "turn_ended".to_owned(),
                    },
                    Some("metadata") => "metadata".to_owned(),
                    _ => continue,
                };
                rows.events.push(self.event(offset, subkind));
                continue;
            }
            let message = record.get("message").and_then(Value::as_object);
            match record.get("role").and_then(Value::as_str) {
                Some("user") => self.user(message, offset, options, &mut rows),
                Some("assistant") => self.assistant(message, offset, &mut rows),
                _ => {}
            }
        }
        rows
    }

    fn event(&self, offset: u64, subkind: String) -> PrepEventRow {
        PrepEventRow {
            native_offset: Some(offset),
            subkind: Some(subkind),
            turn_no: self.turn_no,
            ..untimed_event(&self.source, self.generation, PrepEventKind::SystemOther)
        }
    }

    fn user(
        &mut self,
        message: Option<&Map<String, Value>>,
        offset: u64,
        options: PrepOptions,
        rows: &mut PrepRows,
    ) {
        let text = message.map(text_parts).unwrap_or_default();
        let (origin, chars, body_key, content_head) = opener_text(&text, options);
        rows.triggers.push(PrepTriggerRow {
            source: self.source.clone(),
            generation: self.generation,
            native_offset: Some(offset),
            native_key: None,
            ts: None,
            ts_ms: None,
            kind: origin,
            sender: None,
            pij_msg_id: None,
            chars,
            body_key: body_key.clone(),
            next_turn_no: self.turn_no + 1,
            content_head,
        });
        self.pending = Some(Opener {
            origin,
            offset: Some(offset),
            ts_ms: None,
            chars,
            body_key,
        });
    }

    fn assistant(
        &mut self,
        message: Option<&Map<String, Value>>,
        offset: u64,
        rows: &mut PrepRows,
    ) {
        self.session.calls += 1;
        if let Some(opener) = self.pending.take() {
            self.turn_no += 1;
            self.call_in_turn = 0;
            self.push_turn(offset, opener.origin, Some(&opener), rows);
        } else if self.turn_no == 0 && !self.turn_zero_emitted {
            self.push_turn(offset, TurnOrigin::Start, None, rows);
        }
        self.turn_zero_emitted = true;
        self.call_in_turn += 1;
        rows.calls.push(PrepCallRow {
            native_offset: Some(offset),
            turn_no: Some(self.turn_no),
            call_in_turn: Some(self.call_in_turn),
            ..untimed_call(&self.source, self.generation)
        });
        let Some(parts) = message
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for part in parts.iter().filter_map(Value::as_object) {
            if part.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let name = non_empty(part, "name");
            let (input_hash, input_bytes) = hashed(part.get("input"));
            rows.tool_uses.push(PrepToolUseRow {
                source: self.source.clone(),
                generation: self.generation,
                native_offset: Some(offset),
                native_key: None,
                sighting: ToolSighting::Use,
                tool_use_id: non_empty(part, "id"),
                call_msg_id: None,
                ts: None,
                ts_ms: None,
                family: family_of(name.as_deref()),
                name,
                input_hash,
                input_bytes,
                result_offset: None,
                result_bytes: None,
                outcome: None,
                duration_ms: None,
                turn_no: Some(self.turn_no),
            });
        }
    }

    fn push_turn(
        &mut self,
        offset: u64,
        origin: TurnOrigin,
        opener: Option<&Opener>,
        rows: &mut PrepRows,
    ) {
        rows.turns.push(PrepTurnRow {
            source: self.source.clone(),
            generation: self.generation,
            native_offset: Some(offset),
            native_key: None,
            turn_no: self.turn_no,
            started_ts: None,
            started_ts_ms: None,
            first_call_offset: Some(offset),
            origin,
            sender: None,
            pij_msg_id: None,
            opener_offset: opener.and_then(|o| o.offset),
            opener_ts_ms: None,
            opener_chars: opener.map(|o| o.chars),
            body_key: opener.and_then(|o| o.body_key.clone()),
        });
        self.session.turns += 1;
    }
}

impl PrepFoldSession for TranscriptState {
    fn fold(&mut self, input: &PrepInput, options: PrepOptions) -> Result<PrepRows, PipelineError> {
        match input {
            PrepInput::Records(records) => Ok(self.fold_records(records, options)),
            PrepInput::Snapshot(_) => {
                Err(PipelineError::new(PipelineErrorKind::InvalidInput, None))
            }
        }
    }

    fn checkpoint(&self) -> PrepCheckpoint {
        checkpoint(TRANSCRIPT_PREP_POLICY, self.save())
    }

    fn facts(&self) -> SessionFacts {
        self.session.clone()
    }
}

// ---------------------------------------------------------------------------
// IDE cursorDiskKV snapshot
// ---------------------------------------------------------------------------

/// Cursor IDE `state.vscdb` `cursorDiskKV` fold (snapshot sources).
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorIdePrepFold;

impl PrepFold for CursorIdePrepFold {
    fn harness(&self) -> &'static str {
        IDE_DESCRIPTOR.id
    }
    fn policy(&self) -> &'static str {
        IDE_PREP_POLICY
    }
    fn kind(&self) -> PrepSourceKind {
        PrepSourceKind::Snapshot
    }
    fn pattern(&self) -> &'static str {
        IDE_DESCRIPTOR.locations[0].session_glob
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
        let mut state = IdeState {
            source: source.to_owned(),
            generation,
            revision: None,
            session: SessionFacts::default(),
        };
        if let Some(fold) = saved_fold(saved, IDE_PREP_POLICY)? {
            let object = fold.as_object().ok_or_else(invalid_data)?;
            state.revision = object
                .get("revision")
                .and_then(|v| opt(v, |v| v.as_str().map(str::to_owned)))
                .ok_or_else(invalid_data)?;
            state.session = object
                .get("session")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .ok_or_else(invalid_data)?;
        }
        Ok(Box::new(state))
    }
}

#[derive(Debug, Clone)]
struct IdeState {
    source: String,
    generation: u32,
    /// Revision of the snapshot the facts describe.
    revision: Option<String>,
    session: SessionFacts,
}

impl PrepFoldSession for IdeState {
    /// Each snapshot is a whole generation: rows and facts describe it alone.
    fn fold(&mut self, input: &PrepInput, options: PrepOptions) -> Result<PrepRows, PipelineError> {
        let PrepInput::Snapshot(snapshot) = input else {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        };
        let mut walk = IdeWalk {
            source: &self.source,
            generation: self.generation,
            options,
            rows: PrepRows::default(),
            session: SessionFacts::default(),
            turn_no: 0,
        };
        walk.snapshot(snapshot)?;
        self.revision = Some(snapshot.revision.clone());
        self.session = walk.session;
        Ok(walk.rows)
    }

    fn checkpoint(&self) -> PrepCheckpoint {
        checkpoint(
            IDE_PREP_POLICY,
            json!({
                "revision": self.revision,
                "session": serde_json::to_value(&self.session).unwrap_or(Value::Null),
            }),
        )
    }

    fn facts(&self) -> SessionFacts {
        self.session.clone()
    }
}

/// A native timestamp: the text as recorded (or RFC 3339 for integer
/// milliseconds) and Unix milliseconds.
#[derive(Debug, Clone)]
struct Stamp {
    ts: String,
    ms: i64,
}

/// Per-composer walk state.
struct Composer {
    sidechain: bool,
    pending: Option<Opener>,
    opened: bool,
    call_in_turn: i64,
    /// `None` before the first call; then the previous call's timestamp.
    last_call: Option<Option<i64>>,
    requested_model: Option<String>,
}

struct IdeWalk<'a> {
    source: &'a str,
    generation: u32,
    options: PrepOptions,
    rows: PrepRows,
    session: SessionFacts,
    turn_no: i64,
}

impl IdeWalk<'_> {
    fn snapshot(&mut self, snapshot: &NativeSnapshot) -> Result<(), PipelineError> {
        let index = ide::index_snapshot(snapshot)?;
        let mut composers = Vec::new();
        let mut sidechains: BTreeSet<String> = BTreeSet::new();
        for (&key, &row) in &index {
            let Some(id) = key.strip_prefix("composerData:") else {
                continue;
            };
            match ide::classify_composer(row, id) {
                Ok((object, headers)) => {
                    for field in ["subagentComposerIds", "subComposerIds"] {
                        let ids = object.get(field).and_then(Value::as_array);
                        sidechains.extend(
                            ids.into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .map(str::to_owned),
                        );
                    }
                    composers.push((key, id, object, headers));
                }
                Err(_) => self.session.skipped.malformed += 1,
            }
        }
        let mut referenced = BTreeSet::new();
        for (key, id, object, headers) in composers {
            let mut composer = Composer {
                sidechain: sidechains.contains(id),
                pending: None,
                opened: false,
                call_in_turn: 0,
                last_call: None,
                requested_model: None,
            };
            self.session.records += 1;
            let stamp = match ide::composer_timestamp(&object) {
                Ok(ns) => ns.and_then(stamp_of_ns),
                Err(_) => {
                    self.session.skipped.bad_timestamp += 1;
                    None
                }
            };
            self.observe(stamp.as_ref());
            let mut event = self.event(key, stamp.as_ref(), PrepEventKind::SystemOther);
            event.subkind = Some("composer".into());
            event.model = model_name(object.get("modelConfig"));
            self.rows.events.push(event);
            for header in &headers {
                let Some(bubble_id) = header
                    .get("bubbleId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                else {
                    self.session.skipped.malformed += 1;
                    continue;
                };
                let bubble_key = format!("bubbleId:{id}:{bubble_id}");
                let Some(row) = index.get(bubble_key.as_str()) else {
                    self.session.skipped.malformed += 1;
                    continue;
                };
                if !referenced.insert(bubble_key.clone()) {
                    self.session.skipped.malformed += 1;
                    continue;
                }
                let header_type = header.get("type").and_then(Value::as_i64);
                match ide::classify_bubble(row, bubble_id, header_type) {
                    Ok((bubble, kind)) => {
                        self.bubble(&mut composer, &bubble_key, bubble_id, &bubble, kind);
                    }
                    Err(_) => self.session.skipped.malformed += 1,
                }
            }
        }
        Ok(())
    }

    fn observe(&mut self, stamp: Option<&Stamp>) {
        let Some(stamp) = stamp else { return };
        let session = &mut self.session;
        if session.first_event_ms.is_none_or(|first| stamp.ms < first) {
            session.first_event_ts = Some(stamp.ts.clone());
            session.first_event_ms = Some(stamp.ms);
        }
        if session.last_event_ms.is_none_or(|last| stamp.ms >= last) {
            session.last_event_ts = Some(stamp.ts.clone());
            session.last_event_ms = Some(stamp.ms);
        }
    }

    fn event(&self, key: &str, stamp: Option<&Stamp>, kind: PrepEventKind) -> PrepEventRow {
        PrepEventRow {
            native_key: Some(key.to_owned()),
            ts: stamp.map(|s| s.ts.clone()),
            ts_ms: stamp.map(|s| s.ms),
            turn_no: self.turn_no,
            ..untimed_event(self.source, self.generation, kind)
        }
    }

    fn bubble(
        &mut self,
        composer: &mut Composer,
        key: &str,
        bubble_id: &str,
        bubble: &Map<String, Value>,
        kind: i64,
    ) {
        self.session.records += 1;
        let stamp = match ide::bubble_timestamp(bubble) {
            Ok(ns) => ns.and_then(stamp_of_ns),
            Err(_) => {
                self.session.skipped.bad_timestamp += 1;
                None
            }
        };
        // `createdAt` is kept verbatim as the native text.
        let stamp = stamp.map(|s| Stamp {
            ts: bubble
                .get("createdAt")
                .and_then(Value::as_str)
                .map_or(s.ts, str::to_owned),
            ms: s.ms,
        });
        self.observe(stamp.as_ref());
        let control = bubble.get("capabilityType").and_then(Value::as_i64) == Some(22)
            || bubble.get("isSimulatedMsg").and_then(Value::as_bool) == Some(true);
        match kind {
            _ if control => {
                let mut event = self.event(key, stamp.as_ref(), PrepEventKind::SystemOther);
                event.subkind = Some("control".into());
                self.rows.events.push(event);
            }
            1 => self.user(composer, key, bubble, stamp.as_ref()),
            2 => self.assistant(composer, key, bubble_id, bubble, stamp.as_ref()),
            _ => {}
        }
    }

    fn user(
        &mut self,
        composer: &mut Composer,
        key: &str,
        bubble: &Map<String, Value>,
        stamp: Option<&Stamp>,
    ) {
        if let Some(model) = model_name(bubble.get("modelInfo"))
            && composer.requested_model.as_ref() != Some(&model)
        {
            let mut event = self.event(key, stamp, PrepEventKind::ModelSwitch);
            event.model = Some(model.clone());
            self.rows.events.push(event);
            let switch = ModelSwitch {
                ts_ms: stamp.map(|s| s.ms),
                requested_model: model.clone(),
            };
            if later(
                switch.ts_ms,
                self.session.last_model_switch.as_ref().map(|s| s.ts_ms),
            ) {
                self.session.last_model_switch = Some(switch);
            }
            composer.requested_model = Some(model);
        }
        let text = bubble
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (origin, chars, body_key, content_head) = opener_text(text, self.options);
        self.rows.triggers.push(PrepTriggerRow {
            source: self.source.to_owned(),
            generation: self.generation,
            native_offset: None,
            native_key: Some(key.to_owned()),
            ts: stamp.map(|s| s.ts.clone()),
            ts_ms: stamp.map(|s| s.ms),
            kind: origin,
            sender: None,
            pij_msg_id: None,
            chars,
            body_key: body_key.clone(),
            next_turn_no: self.turn_no + 1,
            content_head,
        });
        composer.pending = Some(Opener {
            origin,
            offset: None,
            ts_ms: stamp.map(|s| s.ms),
            chars,
            body_key,
        });
    }

    fn assistant(
        &mut self,
        composer: &mut Composer,
        key: &str,
        bubble_id: &str,
        bubble: &Map<String, Value>,
        stamp: Option<&Stamp>,
    ) {
        let ts_ms = stamp.map(|s| s.ms);
        let opener = composer.pending.take();
        if opener.is_some() || !composer.opened {
            self.turn_no += 1;
            composer.call_in_turn = 0;
            composer.opened = true;
            self.turn(key, stamp, opener.as_ref());
        }
        composer.call_in_turn += 1;
        let gap_ms = match (composer.last_call, ts_ms) {
            (None, Some(_)) => Some(-1),
            (Some(Some(previous)), Some(now)) => Some(now - previous),
            _ => None,
        };
        composer.last_call = Some(ts_ms);
        let tokens = bubble.get("tokenCount").and_then(Value::as_object);
        let count = |field: &str| {
            tokens
                .and_then(|t| t.get(field))
                .and_then(Value::as_i64)
                .filter(|n| *n >= 0)
        };
        // The writer stores 0/0 on bubbles it did not meter.
        let (input, output) = match (count("inputTokens"), count("outputTokens")) {
            (None | Some(0), None | Some(0)) => (None, None),
            recorded => recorded,
        };
        let call = PrepCallRow {
            native_key: Some(key.to_owned()),
            msg_id: Some(bubble_id.to_owned()),
            request_id: non_empty(bubble, "requestId"),
            ts: stamp.map(|s| s.ts.clone()),
            ts_ms,
            model: model_name(bubble.get("modelInfo")),
            input,
            output,
            is_sidechain: composer.sidechain,
            gap_ms,
            turn_no: Some(self.turn_no),
            call_in_turn: Some(composer.call_in_turn),
            ..untimed_call(self.source, self.generation)
        };
        self.session.calls += 1;
        if !call.is_sidechain && later(ts_ms, self.session.latest_context.as_ref().map(|c| c.ts_ms))
        {
            self.session.latest_context = Some(ContextSample {
                ts_ms,
                model: call.model.clone(),
                stop_reason: None,
                input: call.input,
                cache_read: None,
                cache_write: None,
                total: None,
            });
        }
        self.rows.calls.push(call);
        if let Some(tool) = bubble.get("toolFormerData").and_then(Value::as_object) {
            self.tool(key, bubble_id, tool, stamp);
        }
    }

    fn turn(&mut self, key: &str, stamp: Option<&Stamp>, opener: Option<&Opener>) {
        self.rows.turns.push(PrepTurnRow {
            source: self.source.to_owned(),
            generation: self.generation,
            native_offset: None,
            native_key: Some(key.to_owned()),
            turn_no: self.turn_no,
            started_ts: stamp.map(|s| s.ts.clone()),
            started_ts_ms: stamp.map(|s| s.ms),
            first_call_offset: None,
            origin: opener.map_or(TurnOrigin::Start, |o| o.origin),
            sender: None,
            pij_msg_id: None,
            opener_offset: None,
            opener_ts_ms: opener.and_then(|o| o.ts_ms),
            opener_chars: opener.map(|o| o.chars),
            body_key: opener.and_then(|o| o.body_key.clone()),
        });
        self.session.turns += 1;
    }

    fn tool(
        &mut self,
        key: &str,
        bubble_id: &str,
        tool: &Map<String, Value>,
        stamp: Option<&Stamp>,
    ) {
        let name = non_empty(tool, "name");
        let tool_use_id = non_empty(tool, "toolCallId");
        let (input_hash, input_bytes) = hashed(tool.get("params").or_else(|| tool.get("rawArgs")));
        let use_row = PrepToolUseRow {
            source: self.source.to_owned(),
            generation: self.generation,
            native_offset: None,
            native_key: Some(key.to_owned()),
            sighting: ToolSighting::Use,
            tool_use_id,
            call_msg_id: Some(bubble_id.to_owned()),
            ts: stamp.map(|s| s.ts.clone()),
            ts_ms: stamp.map(|s| s.ms),
            family: family_of(name.as_deref()),
            name,
            input_hash,
            input_bytes,
            result_offset: None,
            result_bytes: None,
            outcome: None,
            duration_ms: None,
            turn_no: Some(self.turn_no),
        };
        let status = tool.get("status").and_then(Value::as_str);
        let error = tool.get("error").filter(|e| !e.is_null());
        let result = tool.get("result").filter(|r| !r.is_null());
        let outcome = match (status, error, result) {
            (Some("error"), _, _) | (_, Some(_), _) => Some(ToolOutcome::Error),
            (Some("completed"), None, _) => Some(ToolOutcome::Ok),
            (Some("cancelled"), None, _) | (_, None, Some(_)) => Some(ToolOutcome::Unknown),
            _ => None,
        };
        let result_row = outcome.map(|outcome| PrepToolUseRow {
            sighting: ToolSighting::Result,
            input_hash: None,
            input_bytes: None,
            result_bytes: error.or(result).map(result_bytes),
            outcome: Some(outcome),
            ..use_row.clone()
        });
        self.rows.tool_uses.push(use_row);
        self.rows.tool_uses.extend(result_row);
    }
}

/// Whether a sample at `candidate` supersedes one at `current` (`None` = no
/// sample yet). Untimed samples sort before timed ones; ties go to the later
/// sample in native order.
fn later(candidate: Option<i64>, current: Option<Option<i64>>) -> bool {
    match current {
        None => true,
        Some(current) => candidate >= current,
    }
}

fn stamp_of_ns(ns: u64) -> Option<Stamp> {
    let ms = i64::try_from(ns / 1_000_000).ok()?;
    let ts = OffsetDateTime::from_unix_timestamp_nanos(i128::from(ns))
        .ok()?
        .format(&Rfc3339)
        .ok()?;
    Some(Stamp { ts, ms })
}

fn model_name(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_object)
        .and_then(|object| non_empty(object, "modelName"))
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// A call row with every field the dialects may not record left null.
fn untimed_call(source: &str, generation: u32) -> PrepCallRow {
    PrepCallRow {
        source: source.to_owned(),
        generation,
        native_offset: None,
        native_key: None,
        sighting: CallSighting::First,
        msg_id: None,
        request_id: None,
        ts: None,
        ts_ms: None,
        model: None,
        stop_reason: None,
        input: None,
        cw_1h: None,
        cw_5m: None,
        cache_read: None,
        output: None,
        cache_write_basis: CacheWriteBasis::None,
        is_sidechain: false,
        gap_ms: None,
        turn_no: None,
        call_in_turn: None,
        records: 1,
    }
}

fn untimed_event(source: &str, generation: u32, kind: PrepEventKind) -> PrepEventRow {
    PrepEventRow {
        source: source.to_owned(),
        generation,
        native_offset: None,
        native_key: None,
        ts: None,
        ts_ms: None,
        kind,
        subkind: None,
        trigger: None,
        model: None,
        pre_tokens: None,
        post_tokens: None,
        duration_ms: None,
        last_context: None,
        gap_ms: None,
        resets_at: None,
        resets_at_ms: None,
        turn_no: 0,
        body_key: None,
    }
}

/// Origin, character count, body hash and opt-in head of an opening message.
/// Text makes it `human`; a message without text is `other`.
fn opener_text(
    text: &str,
    options: PrepOptions,
) -> (TurnOrigin, i64, Option<String>, Option<String>) {
    let body = normalised_body(text);
    let origin = if body.is_empty() {
        TurnOrigin::Other
    } else {
        TurnOrigin::Human
    };
    let chars = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
    let body_key = (!body.is_empty()).then(|| fnv_hex(body.as_bytes()));
    let head = options
        .include_content
        .then(|| body.chars().take(200).collect());
    (origin, chars, body_key, head)
}

/// Whitespace runs collapsed to one space, ends trimmed.
fn normalised_body(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text parts of a transcript message joined by a space.
fn text_parts(message: &Map<String, Value>) -> String {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn non_empty(object: &Map<String, Value>, field: &str) -> Option<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// The query mapping's tool-family vocabulary; unknown names stay null.
fn family_of(name: Option<&str>) -> Option<String> {
    name.and_then(crate::query::tool_family).map(str::to_owned)
}

/// Result size: UTF-8 bytes of a text result, else canonical JSON bytes.
fn result_bytes(value: &Value) -> i64 {
    match value {
        Value::String(text) => i64::try_from(text.len()).unwrap_or(i64::MAX),
        other => hashed(Some(other)).1.unwrap_or(0),
    }
}

/// FNV-1a hash and length of the canonical JSON of `value`.
fn hashed(value: Option<&Value>) -> (Option<String>, Option<i64>) {
    let Some(value) = value else {
        return (None, None);
    };
    let mut bytes = Vec::new();
    canonical_json(value, &mut bytes);
    (
        Some(fnv_hex(&bytes)),
        Some(i64::try_from(bytes.len()).unwrap_or(i64::MAX)),
    )
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

fn fnv_hex(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}
