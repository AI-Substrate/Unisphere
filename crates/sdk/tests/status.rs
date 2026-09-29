//! StatusService on the scripted fold and in-memory loader: every fact
//! definition, cursor reuse and visible resets. No clock, no filesystem.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use serde_json::{Value, json};
use unisphere_sdk::{
    PipelineError, StatusCursor, StatusService,
    prep::{
        PrepBinding, PrepCheckpoint, PrepFold, PrepFoldSession, PrepInput, PrepOptions, PrepRows,
        PrepSourceKind, PrepSourceMeta, PrepSourceSet, SessionFacts,
    },
    status::{
        Basis, MODEL_WINDOWS_TABLE, SessionStatus, SessionStatusApi, SourceStatus,
        StatusFailureKind, StatusTarget, UNKNOWN_FACTS,
    },
};
use unisphere_testkit::{prep::MemoryLoader, status::ScriptedFold};

const HARNESS: &str = "claude-code";
const ROOT: &str = "/roots/claude";
const FILE: &str = "proj/s1.jsonl";
const T0: i64 = 1_700_000_000_000;
const MIN: i64 = 60_000;

struct Fixture {
    loader: Arc<MemoryLoader>,
    service: StatusService,
}

fn fixture_with(roots: Vec<PrepSourceSet>) -> Fixture {
    let loader = Arc::new(MemoryLoader::new(PrepSourceKind::Append, ROOT));
    let binding = PrepBinding {
        fold: Arc::new(ScriptedFold::new(HARNESS)),
        loader: loader.clone(),
    };
    Fixture {
        loader,
        service: StatusService::new(vec![binding], roots),
    }
}

fn fixture() -> Fixture {
    fixture_with(vec![PrepSourceSet {
        harness: HARNESS.into(),
        label: "default".into(),
        root: PathBuf::from(ROOT),
    }])
}

fn target(session: &str) -> StatusTarget {
    StatusTarget {
        harness: HARNESS.into(),
        session_id: session.into(),
        transcript: None,
    }
}

fn lines(steps: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for step in steps {
        bytes.extend(serde_json::to_vec(step).unwrap());
        bytes.push(b'\n');
    }
    bytes
}

impl Fixture {
    fn append(&self, file: &str, steps: &[Value]) {
        self.loader.append(file, &lines(steps));
    }

    fn cold(&self, session: &str, now: i64) -> SessionStatus {
        self.service.status(&target(session), now).unwrap()
    }

    fn next(
        &self,
        session: &str,
        cursor: Option<&StatusCursor>,
        now: i64,
    ) -> (SessionStatus, StatusCursor) {
        self.service
            .status_incremental(&target(session), cursor, now)
            .unwrap()
    }
}

/// Everything but `source`, which is the only field allowed to differ.
fn facts_only(mut status: SessionStatus) -> SessionStatus {
    status.source = SourceStatus::default();
    status
}

fn call(ts: i64, model: &str) -> Value {
    json!({"call": {"ts_ms": ts, "model": model, "input": 10, "cw_1h": 5, "cache_read": 100, "output": 7}})
}

fn unknown(status: &SessionStatus, name: &str) -> bool {
    status.unknown.iter().any(|n| n == name)
}

#[test]
fn model_switch_mid_session_reports_new_model_and_pending_switch() {
    let f = fixture();
    f.append(
        FILE,
        &[
            call(T0 + 1_000, "claude-opus-5"),
            call(T0 + 2_000, "claude-opus-5"),
            json!({"call": {"ts_ms": T0 + 2_500, "model": "claude-haiku-4-5", "sidechain": true}}),
            json!({"facts": {"last_model_switch": {"ts_ms": T0 + 3_000, "requested_model": "fable"}}}),
        ],
    );
    let (before, cursor) = f.next("s1", None, T0 + 3_500);
    let current = before.model.current.as_ref().unwrap();
    assert_eq!(
        (current.value.as_str(), current.basis),
        ("claude-opus-5", Basis::Native)
    );
    assert_eq!(before.model.current_at_ms, Some(T0 + 2_000));
    let pending = before.model.pending_switch.as_ref().unwrap();
    assert_eq!(
        (pending.requested.as_str(), pending.at_ms),
        ("fable", Some(T0 + 3_000))
    );
    assert!(!unknown(&before, "model.pending_switch"));
    assert_eq!((before.calls.total, before.calls.sidechain), (3, 1));

    f.append(
        FILE,
        &[
            call(T0 + 4_000, "claude-fable-5-1"),
            json!({"call": {"ts_ms": T0 + 4_500, "model": "<synthetic>"}}),
        ],
    );
    let (after, _) = f.next("s1", Some(&cursor), T0 + 5_000);
    assert_eq!(
        after.model.current.as_ref().unwrap().value,
        "claude-fable-5-1"
    );
    assert_eq!(after.model.current_at_ms, Some(T0 + 4_000));
    assert_eq!(after.model.pending_switch, None);
    let history: Vec<_> = after
        .model
        .history
        .iter()
        .map(|span| (span.model.as_str(), span.first_ms, span.last_ms, span.calls))
        .collect();
    assert_eq!(
        history,
        [
            ("claude-opus-5", Some(T0 + 1_000), Some(T0 + 2_000), 2),
            ("claude-fable-5-1", Some(T0 + 4_000), Some(T0 + 4_000), 1),
        ]
    );
    assert_eq!(after.calls.total, 4, "a <synthetic> record is never a call");
    assert_eq!(facts_only(after), facts_only(f.cold("s1", T0 + 5_000)));
}

#[test]
fn switch_with_no_order_is_unknown_and_before_any_call_is_pending() {
    let f = fixture();
    f.append(
        FILE,
        &[json!({"facts": {"last_model_switch": {"requested_model": "opus"}}})],
    );
    let status = f.cold("s1", T0);
    assert_eq!(status.model.pending_switch.unwrap().requested, "opus");
    assert!(!unknown(&f.cold("s1", T0), "model.pending_switch"));

    // A switch without a time cannot be ordered against a known call.
    f.append(
        FILE,
        &[json!({"call": {"ts_ms": T0, "model": "claude-opus-5"}})],
    );
    let status = f.cold("s1", T0);
    assert_eq!(status.model.pending_switch, None);
    assert!(unknown(&status, "model.pending_switch"));
}

fn context_status(model: &str, total: i64) -> SessionStatus {
    let f = fixture();
    f.append(
        FILE,
        &[
            json!({"call": {"ts_ms": T0, "model": model}}),
            json!({"facts": {"latest_context": {"model": model, "input": 1, "cache_read": total - 1, "cache_write": 0, "total": total}}}),
        ],
    );
    f.cold("s1", T0)
}

#[test]
fn context_used_of_table_window_with_percent() {
    let status = context_status("claude-opus-5-5", 250_000);
    let context = &status.context;
    let used = context.used_tokens.as_ref().unwrap();
    assert_eq!((used.value, used.basis), (250_000, Basis::Derived));
    let window = context.window_tokens.as_ref().unwrap();
    assert_eq!((window.value, window.basis), (1_000_000, Basis::Table));
    assert_eq!(context.window_table.as_deref(), Some(MODEL_WINDOWS_TABLE));
    assert_eq!(context.window_table.as_deref(), Some("model-windows@1"));
    assert_eq!(context.percent, Some(25.0));
    assert_eq!(context.display.as_deref(), Some("250k of 1M (25%)"));
    for name in [
        "context.used_tokens",
        "context.window_tokens",
        "context.percent",
    ] {
        assert!(!unknown(&status, name), "{name}");
    }

    let older = context_status("claude-3-5-sonnet-20241022", 150_000);
    assert_eq!(older.context.window_tokens.unwrap().value, 200_000);
    assert_eq!(older.context.percent, Some(75.0));

    let sonnet = context_status("claude-sonnet-5-5", 90_000);
    let window = sonnet.context.window_tokens.unwrap();
    assert_eq!((window.value, window.basis), (1_000_000, Basis::Table));
}

#[test]
fn unlisted_model_or_negative_total_is_unknown_never_zero() {
    let status = context_status("claude-mythic-9", 90_000);
    assert_eq!(status.context.used_tokens.as_ref().unwrap().value, 90_000);
    assert_eq!(status.context.window_tokens, None);
    assert_eq!(status.context.window_table, None);
    assert_eq!(status.context.percent, None);
    assert_eq!(status.context.display, None);
    assert!(unknown(&status, "context.window_tokens"));
    assert!(unknown(&status, "context.percent"));

    // A prefix only matches on a `-` boundary.
    let status = context_status("claude-opus-50", 90_000);
    assert_eq!(status.context.window_tokens, None);

    let status = context_status("claude-opus-5", -1);
    assert_eq!(status.context.used_tokens, None);
    assert_eq!(status.context.percent, None);
    assert!(unknown(&status, "context.used_tokens"));
}

#[test]
fn compaction_counts_and_last_compaction() {
    let f = fixture();
    f.append(
        FILE,
        &[json!({"facts": {
            "compactions": {"manual": 1, "auto": 2, "unknown_trigger": 0},
            "last_compaction": {"ts_ms": T0, "trigger": "auto", "pre_tokens": 900_000, "post_tokens": 40_000, "first_context_after": 52_000}
        }})],
    );
    let status = f.cold("s1", T0);
    let counts = status.compaction.counts.unwrap();
    assert_eq!(
        (counts.manual, counts.auto, counts.unknown_trigger),
        (1, 2, 0)
    );
    let last = status.compaction.last.as_ref().unwrap();
    assert_eq!(last.ts_ms, Some(T0));
    assert_eq!(last.trigger.as_deref(), Some("auto"));
    assert_eq!(
        (last.pre_tokens, last.post_tokens),
        (Some(900_000), Some(40_000))
    );
    assert!(!unknown(&status, "compaction.counts"));
    assert!(!unknown(&status, "compaction.last"));
}

#[test]
fn zero_compactions_are_known_and_missing_markers_are_unknown() {
    let f = fixture();
    f.append(
        FILE,
        &[json!({"facts": {"compactions": {"manual": 0, "auto": 0, "unknown_trigger": 0}}})],
    );
    let status = f.cold("s1", T0);
    assert_eq!(status.compaction.last, None);
    assert!(!unknown(&status, "compaction.counts"));
    assert!(!unknown(&status, "compaction.last"));

    let f = fixture();
    f.append(FILE, &[json!({"facts": {}})]);
    let status = f.cold("s1", T0);
    assert_eq!(status.compaction.counts, None);
    assert!(unknown(&status, "compaction.counts"));
    assert!(unknown(&status, "compaction.last"));
}

fn turn(no: i64, ts: Option<i64>, origin: &str) -> Value {
    json!({"turn": {"turn_no": no, "ts_ms": ts, "origin": origin}})
}

#[test]
fn last_hour_turns_by_origin_follow_injected_now() {
    let now = T0 + 300 * MIN;
    let f = fixture();
    f.append(
        FILE,
        &[
            turn(0, None, "start"),
            turn(1, Some(now - 120 * MIN), "human"),
            turn(2, Some(now - 30 * MIN), "peer"),
        ],
    );
    let (_, cursor) = f.next("s1", None, now);
    f.append(
        FILE,
        &[
            turn(3, Some(now - 10 * MIN), "human"),
            turn(4, Some(now - 5 * MIN), "task-notification"),
        ],
    );
    let (status, cursor) = f.next("s1", Some(&cursor), now);
    let turns = &status.turns;
    assert_eq!(turns.total, 5);
    let counts = |map: &std::collections::BTreeMap<String, u64>| {
        map.iter().map(|(k, v)| (k.clone(), *v)).collect::<Vec<_>>()
    };
    assert_eq!(
        counts(&turns.by_origin),
        [
            ("human".into(), 2),
            ("peer".into(), 1),
            ("start".into(), 1),
            ("task-notification".into(), 1)
        ]
    );
    assert_eq!(turns.last_hour_total, 3);
    assert_eq!(
        counts(&turns.last_hour_by_origin),
        [
            ("human".into(), 1),
            ("peer".into(), 1),
            ("task-notification".into(), 1)
        ]
    );
    assert_eq!(facts_only(status), facts_only(f.cold("s1", now)));

    // Forty minutes later the peer turn is older than an hour; nothing re-read.
    let reads = f.loader.reads();
    let (later, _) = f.next("s1", Some(&cursor), now + 40 * MIN);
    assert_eq!(f.loader.reads(), reads);
    assert_eq!(later.turns.total, 5);
    assert_eq!(later.turns.last_hour_total, 2);
    assert_eq!(later.turns.last_hour_by_origin.get("peer"), None);
    assert_eq!(facts_only(later), facts_only(f.cold("s1", now + 40 * MIN)));
}

#[test]
fn ttl_bucket_and_cache_warmth_from_the_last_call_write_split() {
    let f = fixture();
    f.append(
        FILE,
        &[json!({"call": {"ts_ms": T0, "model": "claude-opus-5", "input": 3, "cw_1h": 500, "cw_5m": 0, "cache_read": 9_000, "output": 40, "stop_reason": "tool_use"}})],
    );
    let status = f.cold("s1", T0 + 59 * MIN);
    let last = status.last_call.as_ref().unwrap();
    assert_eq!(last.at_ms, Some(T0));
    assert_eq!(
        (last.input, last.output, last.cache_read),
        (Some(3), Some(40), Some(9_000))
    );
    assert_eq!(
        (last.cache_write_1h, last.cache_write_5m),
        (Some(500), Some(0))
    );
    let bucket = last.ttl_bucket.as_ref().unwrap();
    assert_eq!(
        (bucket.value.as_str(), bucket.basis),
        ("1h", Basis::Derived)
    );
    let warm = last.cache_warm.as_ref().unwrap();
    assert_eq!((warm.value, warm.basis), (true, Basis::Derived));
    assert_eq!(last.stop_reason.as_deref(), Some("tool_use"));
    let expired = f.cold("s1", T0 + 61 * MIN);
    assert!(!expired.last_call.unwrap().cache_warm.unwrap().value);

    f.append(
        FILE,
        &[json!({"call": {"ts_ms": T0 + 70 * MIN, "model": "claude-opus-5", "cw_5m": 800}})],
    );
    let five = f.cold("s1", T0 + 74 * MIN).last_call.unwrap();
    assert_eq!(five.ttl_bucket.unwrap().value, "5m");
    assert!(five.cache_warm.unwrap().value);
    let five = f.cold("s1", T0 + 76 * MIN);
    let warm = five.last_call.as_ref().and_then(|c| c.cache_warm.as_ref());
    assert!(!warm.unwrap().value);
    assert!(unknown(&five, "last_call.stop_reason"));

    f.append(
        FILE,
        &[json!({"call": {"ts_ms": T0 + 80 * MIN, "model": "claude-opus-5", "cache_read": 10}})],
    );
    let none = f.cold("s1", T0 + 81 * MIN);
    let last = none.last_call.as_ref().unwrap();
    assert_eq!(
        (last.ttl_bucket.as_ref(), last.cache_warm.as_ref()),
        (None, None)
    );
    assert_eq!(last.cache_write_1h, None, "unrecorded writes are not zero");
    assert!(unknown(&none, "last_call.ttl_bucket"));
    assert!(unknown(&none, "last_call.cache_warm"));
    assert!(!unknown(&none, "last_call"));
}

#[test]
fn update_sightings_of_the_latest_call_merge_per_field() {
    let f = fixture();
    f.append(
        FILE,
        &[
            json!({"call": {"msg_id": "m0", "ts_ms": T0, "model": "claude-opus-5", "output": 1}}),
            json!({"call": {"msg_id": "m1", "ts_ms": T0 + 1_000, "model": "claude-opus-5", "input": 10, "cw_1h": 100, "output": 5}}),
        ],
    );
    let (_, cursor) = f.next("s1", None, T0 + 2_000);
    f.append(
        FILE,
        &[
            json!({"call": {"sighting": "update", "msg_id": "m1", "ts_ms": T0 + 1_500, "input": 3, "output": 50, "cache_read": 700, "stop_reason": "end_turn"}}),
            json!({"call": {"sighting": "update", "msg_id": "m0", "output": 999, "stop_reason": "max_tokens"}}),
        ],
    );
    let (status, _) = f.next("s1", Some(&cursor), T0 + 2_000);
    let last = status.last_call.as_ref().unwrap();
    assert_eq!(last.at_ms, Some(T0 + 1_000), "time of the first sighting");
    assert_eq!(
        (
            last.input,
            last.output,
            last.cache_read,
            last.cache_write_1h
        ),
        (Some(10), Some(50), Some(700), Some(100))
    );
    assert_eq!(last.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(status.calls.total, 2, "updates are not new calls");
    assert_eq!(status.model.history[0].calls, 2);
    assert_eq!(facts_only(status), facts_only(f.cold("s1", T0 + 2_000)));
}

#[test]
fn last_updated_is_native_else_labelled_mtime_fallback() {
    let f = fixture();
    f.append(FILE, &[turn(0, Some(T0), "start")]);
    let status = f.cold("s1", T0);
    let updated = status.timeline.last_updated_ms.as_ref().unwrap();
    assert_eq!(updated.basis, Basis::MtimeFallback);
    // The in-memory loader's mtime is a small nanosecond counter.
    assert_eq!(updated.value, 0);
    assert_eq!(status.timeline.idle_seconds, Some((T0 / 1000) as u64));
    assert_eq!(status.timeline.created_ms, None);
    assert!(unknown(&status, "timeline.created_ms"));
    assert!(!unknown(&status, "timeline.last_updated_ms"));

    f.append(
        FILE,
        &[json!({"facts": {"first_event_ms": T0 - 5_000, "last_event_ms": T0 + 1_000}})],
    );
    let status = f.cold("s1", T0 + 91_000);
    let created = status.timeline.created_ms.as_ref().unwrap();
    assert_eq!((created.value, created.basis), (T0 - 5_000, Basis::Native));
    let updated = status.timeline.last_updated_ms.as_ref().unwrap();
    assert_eq!((updated.value, updated.basis), (T0 + 1_000, Basis::Native));
    assert_eq!(status.timeline.idle_seconds, Some(90));
    // A clock behind the transcript never reports negative idle time.
    assert_eq!(f.cold("s1", T0).timeline.idle_seconds, Some(0));
}

#[test]
fn a_session_with_no_facts_lists_them_as_unknown() {
    let f = fixture();
    f.append(FILE, &[turn(0, Some(T0), "start")]);
    let status = f.cold("s1", T0);
    let expected: Vec<&str> = UNKNOWN_FACTS
        .iter()
        .copied()
        .filter(|name| !matches!(*name, "model.pending_switch" | "timeline.last_updated_ms"))
        .collect();
    assert_eq!(status.unknown, expected);
    assert_eq!(status.last_call, None);
    assert_eq!(status.model.current, None);
    assert_eq!(status.schema_version, 1);
    assert_eq!(status.resolved, None);
    assert_eq!(
        status.target.transcript.as_deref(),
        Some(std::path::Path::new("/roots/claude/proj/s1.jsonl"))
    );
}

fn session_steps() -> [Vec<Value>; 3] {
    [
        vec![
            turn(0, Some(T0), "start"),
            call(T0 + 1_000, "claude-opus-5"),
            json!({"facts": {"first_event_ms": T0, "last_event_ms": T0 + 1_000, "compactions": {"manual": 0, "auto": 0, "unknown_trigger": 0}}}),
        ],
        vec![
            turn(1, Some(T0 + 2_000), "human"),
            json!({"call": {"msg_id": "a", "ts_ms": T0 + 3_000, "model": "claude-opus-5", "input": 2, "cw_1h": 1}}),
            json!({"event": {"kind": "limit_notice", "subkind": "session_limit", "ts_ms": T0 + 3_500, "resets_at": "3pm"}}),
        ],
        vec![
            json!({"call": {"sighting": "update", "msg_id": "a", "output": 90, "stop_reason": "end_turn"}}),
            turn(2, Some(T0 + 4_000), "peer"),
            call(T0 + 5_000, "claude-fable-5"),
            json!({"facts": {"first_event_ms": T0, "last_event_ms": T0 + 5_000, "latest_context": {"total": 400_000}, "compactions": {"manual": 1, "auto": 0, "unknown_trigger": 0}}}),
        ],
    ]
}

#[test]
fn incremental_equals_cold_and_reads_only_appended_bytes() {
    let now = T0 + 10_000;
    let f = fixture();
    let mut cursor = None;
    for steps in session_steps() {
        let bytes = lines(&steps);
        f.loader.append(FILE, &bytes);
        let (status, next) = f.next("s1", cursor.as_ref(), now);
        assert_eq!(status.source.bytes_read, bytes.len() as u64);
        assert_eq!(status.source.reset, None);
        assert_eq!(facts_only(status), facts_only(f.cold("s1", now)));
        cursor = Some(next);
    }
    let (status, cursor) = f.next("s1", cursor.as_ref(), now);
    assert_eq!(status.limits_seen.len(), 1);
    assert_eq!(status.limits_seen[0].kind, "session_limit");
    assert_eq!(status.limits_seen[0].resets_at.as_deref(), Some("3pm"));
    assert_eq!(status.context.display.as_deref(), Some("400k of 1M (40%)"));

    // Unchanged: no read at all, and the same facts.
    let reads = f.loader.reads();
    let (again, _) = f.next("s1", Some(&cursor), now);
    assert_eq!(f.loader.reads(), reads);
    assert_eq!(again.source.bytes_read, 0);
    assert_eq!(again.source.reset, None);
    assert_eq!(facts_only(again), facts_only(status));
}

/// [`ScriptedFold`] that counts opens, checkpoints and records folded.
#[derive(Default)]
struct Counts {
    opens: AtomicU64,
    checkpoints: AtomicU64,
    records: AtomicU64,
}

struct CountingFold(ScriptedFold, Arc<Counts>);
struct CountingSession(Box<dyn PrepFoldSession>, Arc<Counts>);

impl PrepFold for CountingFold {
    fn harness(&self) -> &'static str {
        self.0.harness()
    }
    fn policy(&self) -> &'static str {
        self.0.policy()
    }
    fn kind(&self) -> PrepSourceKind {
        self.0.kind()
    }
    fn pattern(&self) -> &'static str {
        self.0.pattern()
    }
    fn describe(&self, file: &str) -> PrepSourceMeta {
        self.0.describe(file)
    }
    fn open(
        &self,
        meta: &PrepSourceMeta,
        source: &str,
        generation: u32,
        saved: Option<&PrepCheckpoint>,
    ) -> Result<Box<dyn PrepFoldSession>, PipelineError> {
        self.1.opens.fetch_add(1, Ordering::Relaxed);
        let inner = self.0.open(meta, source, generation, saved)?;
        Ok(Box::new(CountingSession(inner, self.1.clone())))
    }
}

impl PrepFoldSession for CountingSession {
    fn fold(&mut self, input: &PrepInput, options: PrepOptions) -> Result<PrepRows, PipelineError> {
        if let PrepInput::Records(records) = input {
            self.1
                .records
                .fetch_add(records.len() as u64, Ordering::Relaxed);
        }
        self.0.fold(input, options)
    }
    fn checkpoint(&self) -> PrepCheckpoint {
        self.1.checkpoints.fetch_add(1, Ordering::Relaxed);
        self.0.checkpoint()
    }
    fn facts(&self) -> SessionFacts {
        self.0.facts()
    }
}

#[test]
fn warm_work_is_the_appended_records_not_the_history() {
    let counts = Arc::new(Counts::default());
    let loader = Arc::new(MemoryLoader::new(PrepSourceKind::Append, ROOT));
    let service = StatusService::new(
        vec![PrepBinding {
            fold: Arc::new(CountingFold(ScriptedFold::new(HARNESS), counts.clone())),
            loader: loader.clone(),
        }],
        vec![PrepSourceSet {
            harness: HARNESS.into(),
            label: "default".into(),
            root: PathBuf::from(ROOT),
        }],
    );
    let history: Vec<Value> = (0..500)
        .map(|i| call(T0 + i * 1_000, "claude-opus-5"))
        .collect();
    loader.append(FILE, &lines(&history));
    let (_, mut cursor) = service.status_incremental(&target("s1"), None, T0).unwrap();
    assert_eq!(counts.records.load(Ordering::Relaxed), 500);

    for i in 0..20 {
        let (opens, records, reads) = (
            counts.opens.load(Ordering::Relaxed),
            counts.records.load(Ordering::Relaxed),
            loader.reads(),
        );
        let appended = lines(&[call(T0 + 600_000 + i * 1_000, "claude-fable-5")]);
        loader.append(FILE, &appended);
        let (status, next) = service
            .status_incremental(&target("s1"), Some(&cursor), T0)
            .unwrap();
        assert_eq!(status.source.reset, None);
        assert_eq!(status.source.bytes_read, appended.len() as u64);
        assert_eq!(status.calls.total, 501 + i as u64);
        // The open fold is resumed: no reopen, no checkpoint round trip, and
        // only the appended record is read and folded.
        assert_eq!(counts.opens.load(Ordering::Relaxed), opens);
        assert_eq!(counts.records.load(Ordering::Relaxed), records + 1);
        assert_eq!(loader.reads(), reads + 1);
        cursor = next;
    }
    assert_eq!(counts.opens.load(Ordering::Relaxed), 1);
    assert_eq!(counts.checkpoints.load(Ordering::Relaxed), 0);
}

#[test]
fn a_cursor_resumed_once_refolds_visibly_when_handed_back_again() {
    let f = fixture();
    f.append(FILE, &[call(T0, "claude-opus-5")]);
    let (_, first) = f.next("s1", None, T0);
    // Unchanged status keeps the fold: the clone still resumes.
    let (_, unchanged) = f.next("s1", Some(&first), T0);
    f.append(FILE, &[call(T0 + 1_000, "claude-fable-5")]);
    let (status, _) = f.next("s1", Some(&unchanged), T0);
    assert_eq!(status.source.reset, None);

    // `first` shares the fold that was just advanced, so it cannot resume.
    f.append(FILE, &[call(T0 + 2_000, "claude-opus-5")]);
    let (status, _) = f.next("s1", Some(&first), T0);
    let cold = f.cold("s1", T0);
    assert_eq!(status.source.reset.as_deref(), Some("spent"));
    assert_eq!(status.source.bytes_read, cold.source.bytes_read);
    assert_eq!(facts_only(status), facts_only(cold));
}

#[test]
fn partial_tail_never_errors_and_is_read_once_complete() {
    let f = fixture();
    f.append(FILE, &[call(T0, "claude-opus-5")]);
    let tail = br#"{"call": {"ts_ms": 1700000009000, "model": "claude-fable-5""#;
    f.loader.append(FILE, tail);
    let (status, cursor) = f.next("s1", None, T0);
    assert_eq!(status.source.pending_tail_bytes, tail.len() as u64);
    assert_eq!(status.calls.total, 1);
    assert_eq!(status.model.current.unwrap().value, "claude-opus-5");

    f.loader.append(FILE, b"}}\n");
    let (status, _) = f.next("s1", Some(&cursor), T0);
    assert_eq!(status.source.pending_tail_bytes, 0);
    assert_eq!(status.source.reset, None);
    assert_eq!(status.source.bytes_read, tail.len() as u64 + 3);
    assert_eq!(status.calls.total, 2);
    assert_eq!(status.model.current.unwrap().value, "claude-fable-5");
}

#[test]
fn shrink_rotation_and_rewrite_reset_visibly_to_the_cold_result() {
    let now = T0 + 10_000;
    let first = lines(&[json!({"call": {"ts_ms": T0, "model": "claude-opus-5", "input": 111}})]);
    let second = lines(&[call(T0 + 1_000, "claude-fable-5")]);
    type Change = fn(&MemoryLoader, &[u8], &[u8]);
    let cases: [(&str, Change); 3] = [
        ("truncated", |loader, first, _| {
            loader.truncate(FILE, first.len())
        }),
        ("rotated", |loader, first, _| loader.rotate(FILE, first)),
        ("anchor", |loader, first, second| {
            // Same identity and size, different committed prefix.
            let mut bytes = [first, second].concat();
            let at = bytes.windows(3).position(|w| w == b"111").unwrap();
            bytes[at..at + 3].copy_from_slice(b"222");
            loader.rewrite(FILE, &bytes, true);
        }),
    ];
    for (reason, change) in cases {
        let f = fixture();
        f.loader.append(FILE, &first);
        f.loader.append(FILE, &second);
        let (_, cursor) = f.next("s1", None, now);
        change(&f.loader, &first, &second);
        let (status, _) = f.next("s1", Some(&cursor), now);
        let cold = f.cold("s1", now);
        assert_eq!(status.source.reset.as_deref(), Some(reason));
        assert_eq!(status.source.bytes_read, cold.source.bytes_read, "{reason}");
        assert_eq!(facts_only(status), facts_only(cold), "{reason}");
    }
}

#[test]
fn a_cursor_for_another_target_resets_visibly() {
    let f = fixture();
    f.append(FILE, &[call(T0, "claude-opus-5")]);
    f.append("proj/s2.jsonl", &[call(T0, "claude-fable-5")]);
    let (_, cursor) = f.next("s1", None, T0);
    let (status, _) = f.next("s2", Some(&cursor), T0);
    assert_eq!(status.source.reset.as_deref(), Some("target-changed"));
    assert_eq!(status.target.session_id, "s2");
    assert_eq!(
        status.model.current.as_ref().unwrap().value,
        "claude-fable-5"
    );
    assert_eq!(facts_only(status), facts_only(f.cold("s2", T0)));
}

#[test]
fn explicit_transcript_uses_its_configured_root_else_its_parent() {
    let f = fixture();
    f.append(FILE, &[call(T0, "claude-opus-5")]);
    let explicit = StatusTarget {
        transcript: Some(PathBuf::from(ROOT).join(FILE)),
        ..target("any-id")
    };
    let status = f.service.status(&explicit, T0).unwrap();
    assert_eq!(status.target, explicit);
    assert_eq!(status.calls.total, 1);

    // No configured roots: the parent directory is the root.
    let f = fixture_with(Vec::new());
    f.append("s9.jsonl", &[call(T0, "claude-opus-5")]);
    let explicit = StatusTarget {
        transcript: Some(PathBuf::from(ROOT).join("s9.jsonl")),
        ..target("s9")
    };
    assert_eq!(f.service.status(&explicit, T0).unwrap().calls.total, 1);
    let failure = f.service.status(&target("s9"), T0).unwrap_err();
    assert_eq!(failure.kind, StatusFailureKind::TranscriptNotFound);
}

#[test]
fn failures_are_typed() {
    let f = fixture();
    let other = StatusTarget {
        harness: "codex".into(),
        ..target("s1")
    };
    let failure = f.service.status(&other, T0).unwrap_err();
    assert_eq!(failure.kind, StatusFailureKind::UnsupportedHarness);
    assert_eq!(failure.code(), "UNI-STATUS-UNSUPPORTED-HARNESS");

    let failure = f.service.status(&target("missing"), T0).unwrap_err();
    assert_eq!(failure.kind, StatusFailureKind::TranscriptNotFound);

    let missing = StatusTarget {
        transcript: Some(PathBuf::from(ROOT).join("proj/missing.jsonl")),
        ..target("missing")
    };
    let failure = f.service.status(&missing, T0).unwrap_err();
    assert_eq!(failure.kind, StatusFailureKind::TranscriptNotFound);

    f.append(FILE, &[call(T0, "claude-opus-5")]);
    let (_, cursor) = f.next("s1", None, T0);
    f.loader.append(FILE, b"FAIL\n");
    let failure = f
        .service
        .status_incremental(&target("s1"), Some(&cursor), T0)
        .unwrap_err();
    assert_eq!(failure.kind, StatusFailureKind::Read);
    assert!(!failure.message.contains("FAIL"));
}
