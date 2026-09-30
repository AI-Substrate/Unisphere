//! Session-status fakes: a scripted fold that emits exactly the rows and
//! facts a test describes, plus fake status and resolver ports for frontends.
//! Deterministic; no clock, no filesystem.

use std::{collections::BTreeMap, sync::Mutex};

use serde_json::{Value, json};
use unisphere_core::{
    PipelineError, PipelineErrorKind,
    prep::{
        CacheWriteBasis, CallSighting, PREP_CHECKPOINT_FORMAT, PrepCallRow, PrepCheckpoint,
        PrepEventKind, PrepEventRow, PrepFold, PrepFoldSession, PrepInput, PrepOptions, PrepRows,
        PrepSourceKind, PrepSourceMeta, PrepTurnRow, SessionFacts, TurnOrigin,
    },
    status::{
        Resolved, SessionStatus, SessionStatusApi, StatusFailure, StatusQuery, StatusTarget,
        TargetResolver,
    },
};

fn error(kind: PipelineErrorKind) -> PipelineError {
    PipelineError::new(kind, None)
}

// ---------------------------------------------------------------------------
// Scripted fold
// ---------------------------------------------------------------------------

/// Append fold driven by one JSON step per record, so SDK status tests control
/// every row and fact without a real dialect:
///
/// - `{"call": {"ts_ms", "model", "input", "cw_1h", "cw_5m", "cache_read",
///   "output", "stop_reason", "sidechain", "turn_no", "sighting", "msg_id", "basis"}}`
///   → one call row; `sighting` is `first` (default) or `update`, and `msg_id`
///   defaults to `msg-<offset>` (repeat it with `update` to model a re-sighting)
/// - `{"turn": {"turn_no", "ts_ms", "origin"}}` → one turn row (origin in the
///   kebab-case `TurnOrigin` vocabulary)
/// - `{"event": {"ts_ms", "kind", "subkind", "trigger", "model", "pre_tokens",
///   "post_tokens", "resets_at"}}` → one event row (`kind` in `PrepEventKind`)
/// - `{"facts": <partial SessionFacts>}` → replaces the session facts with the
///   given fields laid over the defaults
/// - `FAIL` → the batch fails with `InvalidData`
///
/// Missing fields are null. The checkpoint carries the current facts, so a
/// resumed session continues exactly where a cold one would be.
pub struct ScriptedFold {
    harness: &'static str,
    policy: &'static str,
}

impl ScriptedFold {
    pub const fn new(harness: &'static str) -> Self {
        Self {
            harness,
            policy: "scripted/status-v1",
        }
    }
}

impl PrepFold for ScriptedFold {
    fn harness(&self) -> &'static str {
        self.harness
    }
    fn policy(&self) -> &'static str {
        self.policy
    }
    fn kind(&self) -> PrepSourceKind {
        PrepSourceKind::Append
    }
    fn pattern(&self) -> &'static str {
        "**/*.jsonl"
    }
    fn describe(&self, _file: &str) -> PrepSourceMeta {
        PrepSourceMeta {
            is_sub: false,
            agent_id: None,
            project: None,
        }
    }
    fn open(
        &self,
        _meta: &PrepSourceMeta,
        source: &str,
        generation: u32,
        saved: Option<&PrepCheckpoint>,
    ) -> Result<Box<dyn PrepFoldSession>, PipelineError> {
        let facts = match saved {
            None => SessionFacts::default(),
            Some(checkpoint)
                if checkpoint.format == PREP_CHECKPOINT_FORMAT
                    && checkpoint.policy == self.policy =>
            {
                serde_json::from_value(checkpoint.fold["facts"].clone())
                    .map_err(|_| error(PipelineErrorKind::InvalidData))?
            }
            Some(_) => return Err(error(PipelineErrorKind::InvalidData)),
        };
        Ok(Box::new(ScriptedSession {
            policy: self.policy,
            source: source.to_owned(),
            generation,
            facts,
        }))
    }
}

struct ScriptedSession {
    policy: &'static str,
    source: String,
    generation: u32,
    facts: SessionFacts,
}

fn int(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64)
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn field(v: &Value, key: &str) -> Value {
    v.get(key).cloned().unwrap_or(Value::Null)
}

impl PrepFoldSession for ScriptedSession {
    fn fold(
        &mut self,
        input: &PrepInput,
        _options: PrepOptions,
    ) -> Result<PrepRows, PipelineError> {
        let PrepInput::Records(records) = input else {
            return Err(error(PipelineErrorKind::InvalidData));
        };
        let mut rows = PrepRows::default();
        for record in records {
            if record.bytes == b"FAIL" {
                return Err(error(PipelineErrorKind::InvalidData));
            }
            let step: Value = serde_json::from_slice(&record.bytes)
                .map_err(|_| error(PipelineErrorKind::InvalidData))?;
            let offset = Some(record.offset);
            if let Some(c) = step.get("call") {
                rows.calls.push(PrepCallRow {
                    source: self.source.clone(),
                    generation: self.generation,
                    native_offset: offset,
                    native_key: None,
                    sighting: match c.get("sighting").and_then(Value::as_str) {
                        None | Some("first") => CallSighting::First,
                        Some("update") => CallSighting::Update,
                        Some(_) => return Err(error(PipelineErrorKind::InvalidData)),
                    },
                    msg_id: Some(
                        text(c, "msg_id").unwrap_or_else(|| format!("msg-{}", record.offset)),
                    ),
                    request_id: None,
                    ts: None,
                    ts_ms: int(c, "ts_ms"),
                    model: text(c, "model"),
                    stop_reason: text(c, "stop_reason"),
                    input: int(c, "input"),
                    cw_1h: int(c, "cw_1h"),
                    cw_5m: int(c, "cw_5m"),
                    cache_read: int(c, "cache_read"),
                    output: int(c, "output"),
                    cache_write_basis: match c.get("basis").and_then(Value::as_str) {
                        None | Some("split") => CacheWriteBasis::Split,
                        Some(other) => serde_json::from_value(Value::from(other))
                            .map_err(|_| error(PipelineErrorKind::InvalidData))?,
                    },
                    is_sidechain: c.get("sidechain").and_then(Value::as_bool) == Some(true),
                    gap_ms: None,
                    turn_no: int(c, "turn_no"),
                    call_in_turn: None,
                    records: 1,
                });
            } else if let Some(t) = step.get("turn") {
                rows.turns.push(PrepTurnRow {
                    source: self.source.clone(),
                    generation: self.generation,
                    native_offset: offset,
                    native_key: None,
                    turn_no: int(t, "turn_no").unwrap_or(0),
                    started_ts: None,
                    started_ts_ms: int(t, "ts_ms"),
                    first_call_offset: None,
                    origin: serde_json::from_value::<TurnOrigin>(field(t, "origin"))
                        .map_err(|_| error(PipelineErrorKind::InvalidData))?,
                    sender: None,
                    pij_msg_id: None,
                    opener_offset: None,
                    opener_ts_ms: None,
                    opener_chars: None,
                    body_key: None,
                });
            } else if let Some(e) = step.get("event") {
                rows.events.push(PrepEventRow {
                    source: self.source.clone(),
                    generation: self.generation,
                    native_offset: offset,
                    native_key: None,
                    ts: None,
                    ts_ms: int(e, "ts_ms"),
                    kind: serde_json::from_value::<PrepEventKind>(field(e, "kind"))
                        .map_err(|_| error(PipelineErrorKind::InvalidData))?,
                    subkind: text(e, "subkind"),
                    trigger: text(e, "trigger"),
                    model: text(e, "model"),
                    pre_tokens: int(e, "pre_tokens"),
                    post_tokens: int(e, "post_tokens"),
                    duration_ms: None,
                    last_context: None,
                    gap_ms: None,
                    resets_at: text(e, "resets_at"),
                    resets_at_ms: None,
                    turn_no: 0,
                    body_key: None,
                });
            } else if let Some(f) = step.get("facts") {
                // Partial facts are laid over the defaults.
                let mut merged = serde_json::to_value(SessionFacts::default())
                    .map_err(|_| error(PipelineErrorKind::InvalidData))?;
                let (Some(base), Some(given)) = (merged.as_object_mut(), f.as_object()) else {
                    return Err(error(PipelineErrorKind::InvalidData));
                };
                base.extend(given.clone());
                self.facts = serde_json::from_value(merged)
                    .map_err(|_| error(PipelineErrorKind::InvalidData))?;
            } else {
                return Err(error(PipelineErrorKind::InvalidData));
            }
        }
        Ok(rows)
    }

    fn checkpoint(&self) -> PrepCheckpoint {
        PrepCheckpoint {
            format: PREP_CHECKPOINT_FORMAT,
            policy: self.policy.to_owned(),
            fold: json!({"facts": self.facts}),
        }
    }

    fn facts(&self) -> SessionFacts {
        self.facts.clone()
    }
}

// ---------------------------------------------------------------------------
// Port fakes
// ---------------------------------------------------------------------------

/// Answers from a fixed table keyed by (harness, session id); records calls.
#[derive(Default)]
pub struct FakeStatusApi {
    answers: BTreeMap<(String, String), Result<SessionStatus, StatusFailure>>,
    calls: Mutex<Vec<(StatusTarget, i64)>>,
}

impl FakeStatusApi {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(
        mut self,
        answer: Result<SessionStatus, StatusFailure>,
        target: &StatusTarget,
    ) -> Self {
        self.answers
            .insert((target.harness.clone(), target.session_id.clone()), answer);
        self
    }

    pub fn calls(&self) -> Vec<(StatusTarget, i64)> {
        self.calls.lock().unwrap().clone()
    }
}

impl SessionStatusApi for FakeStatusApi {
    fn status(&self, target: &StatusTarget, now_ms: i64) -> Result<SessionStatus, StatusFailure> {
        self.calls.lock().unwrap().push((target.clone(), now_ms));
        self.answers
            .get(&(target.harness.clone(), target.session_id.clone()))
            .cloned()
            .unwrap_or_else(|| {
                Err(StatusFailure::new(
                    unisphere_core::status::StatusFailureKind::TranscriptNotFound,
                    "no fake answer for target",
                ))
            })
    }
}

/// Resolves from a fixed list of (query, answer) pairs; unknown queries fail
/// with `PaneNotFound` for panes and `PijUnknownSeat` otherwise.
#[derive(Default)]
pub struct FakeTargetResolver {
    answers: Vec<(StatusQuery, Result<Resolved, StatusFailure>)>,
}

impl FakeTargetResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, query: StatusQuery, answer: Result<Resolved, StatusFailure>) -> Self {
        self.answers.push((query, answer));
        self
    }
}

impl TargetResolver for FakeTargetResolver {
    fn resolve(&self, query: &StatusQuery) -> Result<Resolved, StatusFailure> {
        use unisphere_core::status::StatusFailureKind::{PaneNotFound, PijUnknownSeat};
        self.answers
            .iter()
            .find(|(q, _)| q == query)
            .map(|(_, a)| a.clone())
            .unwrap_or_else(|| {
                let kind = if matches!(query, StatusQuery::Pane(_)) {
                    PaneNotFound
                } else {
                    PijUnknownSeat
                };
                Err(StatusFailure::new(kind, "no fake resolution"))
            })
    }
}

#[cfg(test)]
mod tests {
    use unisphere_core::NativeRecord;

    use super::*;

    fn records(lines: &[&str]) -> PrepInput {
        let mut offset = 0;
        PrepInput::Records(
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
                .collect(),
        )
    }

    #[test]
    fn scripted_steps_emit_rows_facts_and_resume() {
        let fold = ScriptedFold::new("claude-code");
        let meta = fold.describe("s.jsonl");
        let mut session = fold.open(&meta, "src", 1, None).unwrap();
        let rows = session
            .fold(
                &records(&[
                    r#"{"call": {"ts_ms": 10, "model": "m1", "input": 5, "msg_id": "a"}}"#,
                    r#"{"call": {"ts_ms": 11, "model": "m1", "output": 9, "msg_id": "a", "sighting": "update"}}"#,
                    r#"{"turn": {"turn_no": 1, "ts_ms": 9, "origin": "peer"}}"#,
                    r#"{"event": {"ts_ms": 12, "kind": "compaction", "trigger": "auto", "pre_tokens": 100}}"#,
                    r#"{"facts": {"records": 4, "calls": 1, "turns": 1}}"#,
                ]),
                PrepOptions::default(),
            )
            .unwrap();
        assert_eq!(rows.calls.len(), 2);
        assert_eq!(rows.calls[1].sighting, CallSighting::Update);
        assert_eq!(rows.calls[1].msg_id.as_deref(), Some("a"));
        assert_eq!(rows.turns[0].origin, TurnOrigin::Peer);
        assert_eq!(rows.events[0].kind, PrepEventKind::Compaction);
        assert_eq!(session.facts().calls, 1);

        let resumed = fold
            .open(&meta, "src", 1, Some(&session.checkpoint()))
            .unwrap();
        assert_eq!(resumed.facts(), session.facts());
        let mut failing = fold.open(&meta, "src", 1, None).unwrap();
        assert!(
            failing
                .fold(&records(&["FAIL"]), PrepOptions::default())
                .is_err()
        );
    }
}
