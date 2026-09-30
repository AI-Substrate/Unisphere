//! Copilot CLI prep folds on synthetic, content-free fixtures.
//!
//! `fixtures/prep/sess-0001/events.jsonl` exercises the events dialect (calls
//! split across message and usage records, subagent and utility calls, typed
//! events, tool uses, malformed and untimed lines); `fixtures/prep/legacy-0001.json`
//! the legacy document. Every text field says "placeholder". The pre-existing
//! `fixtures/{events.jsonl,legacy.json}` mark every sensitive value "SENSITIVE".

use unisphere_adapter_copilot_cli::{
    CopilotCliLegacyPrepFold, CopilotCliPrepFold, DESCRIPTOR, LEGACY_PREP_POLICY_VERSION,
    PREP_POLICY_VERSION, SNAPSHOT_DESCRIPTOR,
};
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineErrorKind, SnapshotFormat, SnapshotRecord, SnapshotRef,
    prep::{
        CacheWriteBasis, CallSighting, CompactionCounts, CompactionSample, ContextSample,
        ModelSwitch, PREP_CHECKPOINT_FORMAT, PrepCallRow, PrepCheckpoint, PrepEventKind, PrepFold,
        PrepInput, PrepOptions, PrepRows, PrepSourceKind, PrepSourceMeta, SessionFacts,
        SessionSkips, ToolOutcome, ToolSighting, TurnOrigin,
    },
};

const EVENTS_FILE: &str = "sess-0001/events.jsonl";
const EVENTS: &str = include_str!("fixtures/prep/sess-0001/events.jsonl");
const LEGACY: &str = include_str!("fixtures/prep/legacy-0001.json");
const SENSITIVE_EVENTS: &str = include_str!("fixtures/events.jsonl");
const SENSITIVE_LEGACY: &str = include_str!("fixtures/legacy.json");

fn records(text: &str) -> Vec<NativeRecord> {
    let mut offset = 0;
    text.lines()
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

fn line_offset(line: usize) -> u64 {
    EVENTS.lines().take(line).map(|l| l.len() as u64 + 1).sum()
}

/// Milliseconds of `2026-01-10T00:00:<seconds>Z`.
fn ms(seconds: i64) -> i64 {
    1_768_003_200_000 + seconds * 1000
}

struct Run {
    rows: PrepRows,
    facts: SessionFacts,
    checkpoint: PrepCheckpoint,
}

/// Fold `text` in batches ending at `splits`; with `resume`, every batch after
/// the first starts from a checkpoint serialised to JSON text and reopened.
fn fold_with(text: &str, splits: &[usize], resume: bool, options: PrepOptions) -> Run {
    let fold = CopilotCliPrepFold;
    let meta = fold.describe(EVENTS_FILE);
    let source = format!("copilot-cli/default/{EVENTS_FILE}");
    let all = records(text);
    let mut session = fold.open(&meta, &source, 0, None).unwrap();
    let mut rows = PrepRows::default();
    let mut start = 0;
    for end in splits.iter().copied().chain([all.len()]) {
        if resume && start > 0 {
            let json = serde_json::to_string(&session.checkpoint()).unwrap();
            let saved: PrepCheckpoint = serde_json::from_str(&json).unwrap();
            session = fold.open(&meta, &source, 0, Some(&saved)).unwrap();
        }
        let batch = PrepInput::Records(all[start..end].to_vec());
        rows.extend(session.fold(&batch, options).unwrap());
        start = end;
    }
    Run {
        rows,
        facts: session.facts(),
        checkpoint: session.checkpoint(),
    }
}

fn fold(text: &str) -> Run {
    fold_with(text, &[], false, PrepOptions::default())
}

fn snapshot(document: &str) -> PrepInput {
    PrepInput::Snapshot(NativeSnapshot {
        source: SnapshotRef {
            path: "/fixtures/legacy-0001.json".into(),
            format: SnapshotFormat::JsonDocument,
            session_id: None,
        },
        revision: "rev-1".into(),
        records: vec![SnapshotRecord {
            key: "document".into(),
            bytes: document.as_bytes().to_vec(),
        }],
    })
}

fn fold_legacy(document: &str, options: PrepOptions) -> Run {
    let fold = CopilotCliLegacyPrepFold;
    let meta = fold.describe("legacy-0001.json");
    let mut session = fold
        .open(
            &meta,
            "copilot-cli-snapshot/default/legacy-0001.json",
            3,
            None,
        )
        .unwrap();
    let rows = session.fold(&snapshot(document), options).unwrap();
    Run {
        rows,
        facts: session.facts(),
        checkpoint: session.checkpoint(),
    }
}

/// The canonical reader's merge: one row per (msg_id, request_id) keeping the
/// first sighting's ordering facts, per-field maxima and the sum of records.
fn canonical(rows: &PrepRows) -> PrepRows {
    let max = |a: Option<i64>, b: Option<i64>| match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let mut calls: Vec<PrepCallRow> = Vec::new();
    for call in &rows.calls {
        match calls
            .iter_mut()
            .find(|c| c.msg_id == call.msg_id && c.request_id == call.request_id)
        {
            Some(first) => {
                assert_eq!(call.sighting, CallSighting::Update);
                first.input = max(first.input, call.input);
                first.cw_1h = max(first.cw_1h, call.cw_1h);
                first.cw_5m = max(first.cw_5m, call.cw_5m);
                first.cache_read = max(first.cache_read, call.cache_read);
                first.output = max(first.output, call.output);
                first.records += call.records;
            }
            None => {
                assert_eq!(call.sighting, CallSighting::First);
                calls.push(call.clone());
            }
        }
    }
    PrepRows {
        calls,
        ..rows.clone()
    }
}

#[test]
fn fold_identities_match_the_catalogue_descriptors() {
    let events = CopilotCliPrepFold;
    assert_eq!(events.harness(), "copilot-cli");
    assert_eq!(events.harness(), DESCRIPTOR.id);
    assert_eq!(events.policy(), PREP_POLICY_VERSION);
    assert_eq!(PREP_POLICY_VERSION, "copilot-cli/prep-v1");
    assert_eq!(events.kind(), PrepSourceKind::Append);
    assert_eq!(events.pattern(), "*/events.jsonl");
    assert_eq!(events.describe(EVENTS_FILE), PrepSourceMeta::default());

    let legacy = CopilotCliLegacyPrepFold;
    assert_eq!(legacy.harness(), "copilot-cli-snapshot");
    assert_eq!(legacy.harness(), SNAPSHOT_DESCRIPTOR.id);
    assert_eq!(legacy.policy(), LEGACY_PREP_POLICY_VERSION);
    assert_eq!(LEGACY_PREP_POLICY_VERSION, "copilot-cli-snapshot/prep-v1");
    assert_eq!(legacy.kind(), PrepSourceKind::Snapshot);
    assert_eq!(legacy.pattern(), "*.json");
    for descriptor in [DESCRIPTOR, SNAPSHOT_DESCRIPTOR] {
        assert!(
            descriptor.capabilities.cli_persisted_resume,
            "{}",
            descriptor.id
        );
    }
}

#[test]
fn calls_merge_message_and_usage_sightings_by_native_call_id() {
    let run = fold(EVENTS);
    type Expected = (
        &'static str,
        Option<&'static str>,
        &'static str,
        [Option<i64>; 5],
        CacheWriteBasis,
        bool,
        (i64, i64, i64),
        i64,
    );
    // msg_id, request_id, model, [input, cw_1h, cw_5m, cache_read, output],
    // basis, sidechain, (gap_ms, turn_no, call_in_turn), records.
    let expected: [Expected; 6] = [
        (
            "api-1",
            None,
            "model-a",
            [Some(100), Some(20), Some(0), Some(50), Some(12)],
            CacheWriteBasis::Fallback1h,
            false,
            (-1, 1, 1),
            3,
        ),
        (
            "api-2",
            None,
            "model-a",
            // No usage record: the rule is named, the write stays unrecorded.
            [None, None, None, None, Some(5)],
            CacheWriteBasis::Fallback1h,
            false,
            (7000, 1, 2),
            1,
        ),
        (
            "api-3",
            None,
            "model-a",
            [Some(200), Some(10), Some(0), Some(120), Some(4)],
            CacheWriteBasis::Fallback1h,
            false,
            (11_000, 2, 1),
            2,
        ),
        // Subagent call: fallback 5 m, no turn of its own.
        (
            "api-4",
            None,
            "model-a",
            [Some(30), Some(0), Some(5), Some(0), Some(3)],
            CacheWriteBasis::Fallback5m,
            true,
            (4000, 2, 2),
            2,
        ),
        // Utility call: native response usage, cache write split by native TTL.
        (
            "util-1",
            Some("ureq-1"),
            "model-small",
            [Some(40), Some(0), Some(8), Some(10), Some(6)],
            CacheWriteBasis::Split,
            true,
            (8000, 2, 3),
            1,
        ),
        (
            "api-5",
            None,
            "model-b",
            [Some(60), Some(0), Some(0), Some(0), Some(7)],
            CacheWriteBasis::Fallback1h,
            false,
            (8000, 3, 1),
            2,
        ),
    ];
    assert_eq!(run.rows.calls.len(), expected.len());
    for (call, (msg_id, request_id, model, tokens, basis, sidechain, order, records)) in
        run.rows.calls.iter().zip(expected)
    {
        assert_eq!(call.msg_id.as_deref(), Some(msg_id));
        assert_eq!(call.request_id.as_deref(), request_id, "{msg_id}");
        assert_eq!(call.model.as_deref(), Some(model), "{msg_id}");
        assert_eq!(
            [
                call.input,
                call.cw_1h,
                call.cw_5m,
                call.cache_read,
                call.output
            ],
            tokens,
            "{msg_id}"
        );
        assert_eq!(call.cache_write_basis, basis, "{msg_id}");
        assert_eq!(call.is_sidechain, sidechain, "{msg_id}");
        assert_eq!(
            (call.gap_ms, call.turn_no, call.call_in_turn),
            (Some(order.0), Some(order.1), Some(order.2)),
            "{msg_id}"
        );
        assert_eq!(call.records, records, "{msg_id}");
        assert_eq!(call.sighting, CallSighting::First);
        // No stop reason is recorded natively.
        assert_eq!(call.stop_reason, None);
        assert_eq!(call.native_key, None);
    }
    let first = &run.rows.calls[0];
    assert_eq!(first.native_offset, Some(line_offset(3)));
    assert_eq!(
        (first.ts.as_deref(), first.ts_ms),
        (Some("2026-01-10T00:00:03.000Z"), Some(ms(3)))
    );
}

#[test]
fn a_sighting_in_a_later_batch_is_an_update_without_ordering_facts() {
    // Split after the second message of api-1, before its usage record.
    let run = fold_with(EVENTS, &[7], false, PrepOptions::default());
    let sightings: Vec<_> = run
        .rows
        .calls
        .iter()
        .filter(|c| c.msg_id.as_deref() == Some("api-1"))
        .collect();
    assert_eq!(sightings.len(), 2);
    let (first, update) = (sightings[0], sightings[1]);
    assert_eq!(
        (first.sighting, first.input, first.output, first.records),
        (CallSighting::First, None, Some(12), 2)
    );
    assert_eq!(
        (update.sighting, update.input, update.cw_1h, update.records),
        (CallSighting::Update, Some(100), Some(20), 1)
    );
    assert_eq!(
        (update.gap_ms, update.turn_no, update.call_in_turn),
        (None, None, None)
    );
    assert_eq!(update.native_offset, Some(line_offset(7)));
    assert_eq!(run.facts.calls, 6);
}

#[test]
fn every_batch_split_and_checkpoint_resume_equals_one_fold() {
    let whole = fold(EVENTS);
    let expected = canonical(&whole.rows);
    let n = records(EVENTS).len();
    let mut plans: Vec<Vec<usize>> = (1..n).map(|k| vec![k]).collect();
    plans.push((1..n).collect());
    for splits in plans {
        for resume in [false, true] {
            let run = fold_with(EVENTS, &splits, resume, PrepOptions::default());
            let label = format!("splits={splits:?} resume={resume}");
            assert_eq!(canonical(&run.rows), expected, "{label}");
            assert_eq!(run.facts, whole.facts, "{label}");
            assert_eq!(run.checkpoint, whole.checkpoint, "{label}");
        }
    }
}

#[test]
fn user_messages_become_triggers_and_open_main_chain_turns() {
    let rows = fold(EVENTS).rows;
    let triggers: Vec<_> = rows
        .triggers
        .iter()
        .map(|t| {
            (
                t.native_offset,
                t.kind,
                t.sender.as_deref(),
                t.pij_msg_id.as_deref(),
                t.next_turn_no,
            )
        })
        .collect();
    // The skill injection (line 24) is not a trigger.
    assert_eq!(
        triggers,
        vec![
            (Some(line_offset(1)), TurnOrigin::Human, None, None, 1),
            (
                Some(line_offset(10)),
                TurnOrigin::Peer,
                Some("pij-quiet-heron"),
                Some("0a1b-2c3d"),
                2
            ),
            (
                Some(line_offset(14)),
                TurnOrigin::SubagentTask,
                None,
                None,
                3
            ),
            (
                Some(line_offset(21)),
                TurnOrigin::ManualCompact,
                None,
                None,
                3
            ),
            (Some(line_offset(25)), TurnOrigin::Human, None, None, 3),
        ]
    );
    assert_eq!(
        rows.triggers[0].chars,
        "placeholder human prompt".len() as i64
    );
    assert!(rows.triggers.iter().all(|t| t.body_key.is_some()));
    // The Peer body key hashes the envelope-free payload.
    assert_ne!(rows.triggers[1].body_key, rows.triggers[0].body_key);

    let turns: Vec<_> = rows
        .turns
        .iter()
        .map(|t| {
            (
                t.turn_no,
                t.origin,
                t.first_call_offset,
                t.opener_offset,
                t.started_ts_ms,
                t.sender.as_deref(),
            )
        })
        .collect();
    // The subagent task and the later manual compact never open a main turn.
    assert_eq!(
        turns,
        vec![
            (
                1,
                TurnOrigin::Human,
                Some(line_offset(3)),
                Some(line_offset(1)),
                Some(ms(3)),
                None
            ),
            (
                2,
                TurnOrigin::Peer,
                Some(line_offset(11)),
                Some(line_offset(10)),
                Some(ms(21)),
                Some("pij-quiet-heron")
            ),
            (
                3,
                TurnOrigin::Human,
                Some(line_offset(26)),
                Some(line_offset(25)),
                Some(ms(41)),
                None
            ),
        ]
    );
    assert_eq!(rows.turns[0].opener_chars, Some(24));
    assert_eq!(rows.turns[1].pij_msg_id.as_deref(), Some("0a1b-2c3d"));
}

#[test]
fn model_changes_compactions_errors_and_aborts_are_typed_events() {
    let rows = fold(EVENTS).rows;
    let events: Vec<_> = rows
        .events
        .iter()
        .map(|e| {
            (
                e.native_offset,
                e.kind,
                e.subkind.as_deref(),
                e.trigger.as_deref(),
                e.model.as_deref(),
                (e.pre_tokens, e.post_tokens),
                (e.last_context, e.gap_ms),
                e.turn_no,
            )
        })
        .collect();
    assert_eq!(
        events,
        vec![
            (
                Some(line_offset(18)),
                PrepEventKind::Compaction,
                None,
                Some("threshold"),
                None,
                (Some(400), Some(80)),
                // The latest call is the subagent's: 30 + 5 + 0, 7 s before.
                (Some(35), Some(7000)),
                2
            ),
            (
                Some(line_offset(20)),
                PrepEventKind::ModelSwitch,
                Some("model_picker"),
                None,
                Some("model-b"),
                (None, None),
                (None, None),
                2
            ),
            (
                Some(line_offset(22)),
                PrepEventKind::Compaction,
                Some("failed"),
                Some("manual"),
                None,
                (None, None),
                (Some(58), Some(3000)),
                2
            ),
            (
                Some(line_offset(23)),
                PrepEventKind::Compaction,
                None,
                Some("manual"),
                None,
                (Some(300), Some(60)),
                (Some(58), Some(4000)),
                2
            ),
            (
                Some(line_offset(28)),
                PrepEventKind::ApiError,
                Some("query"),
                None,
                None,
                (None, None),
                (None, None),
                3
            ),
            (
                Some(line_offset(29)),
                PrepEventKind::SystemOther,
                Some("abort"),
                None,
                None,
                (None, None),
                (None, None),
                3
            ),
        ]
    );
    // Durations are not recorded natively.
    assert!(rows.events.iter().all(|e| e.duration_ms.is_none()));
}

#[test]
fn tool_uses_pair_requests_with_results_and_native_outcomes() {
    let rows = fold(EVENTS).rows;
    let uses: Vec<_> = rows
        .tool_uses
        .iter()
        .map(|t| {
            (
                t.sighting,
                t.tool_use_id.as_deref(),
                t.call_msg_id.as_deref(),
                t.name.as_deref(),
                t.family.as_deref(),
                t.outcome,
                t.result_bytes,
                t.turn_no,
            )
        })
        .collect();
    // tool.execution_start repeats a requested id and adds no row.
    assert_eq!(
        uses,
        vec![
            (
                ToolSighting::Use,
                Some("tool-1"),
                Some("api-1"),
                Some("view"),
                Some("file-read"),
                None,
                None,
                Some(1)
            ),
            (
                ToolSighting::Result,
                Some("tool-1"),
                Some("api-1"),
                Some("view"),
                Some("file-read"),
                Some(ToolOutcome::Ok),
                Some("placeholder result".len() as i64),
                Some(1)
            ),
            (
                ToolSighting::Use,
                Some("tool-2"),
                Some("api-2"),
                Some("bash"),
                Some("shell"),
                None,
                None,
                Some(1)
            ),
            (
                ToolSighting::Result,
                Some("tool-2"),
                Some("api-2"),
                Some("bash"),
                Some("shell"),
                Some(ToolOutcome::Error),
                None,
                Some(1)
            ),
        ]
    );
    let first = &rows.tool_uses[0];
    assert!(first.input_hash.is_some());
    assert_eq!(
        first.input_bytes,
        Some(r#"{"path":"/work/demo/a","view_range":[1,2]}"#.len() as i64)
    );
    assert_eq!(rows.tool_uses[1].result_offset, Some(line_offset(5)));
    assert!(rows.tool_uses.iter().all(|t| t.duration_ms.is_none()));
}

#[test]
fn session_facts_report_context_compactions_model_and_coverage() {
    let facts = fold(EVENTS).facts;
    assert_eq!(
        facts,
        SessionFacts {
            context_window: None,
            session_id: Some("sess-0001".into()),
            parent_session_id: None,
            is_sidechain: false,
            cwd: Some("/work/demo".into()),
            first_event_ts: Some("2026-01-10T00:00:00.000Z".into()),
            first_event_ms: Some(ms(0)),
            last_event_ts: Some("2026-01-10T00:00:50.000Z".into()),
            last_event_ms: Some(ms(50)),
            records: 33,
            calls: 6,
            turns: 3,
            latest_context: Some(ContextSample {
                ts_ms: Some(ms(41)),
                model: Some("model-b".into()),
                stop_reason: None,
                input: Some(60),
                cache_read: Some(0),
                cache_write: Some(0),
                total: Some(60),
            }),
            compactions: Some(CompactionCounts {
                manual: 1,
                auto: 1,
                unknown_trigger: 0,
            }),
            last_compaction: Some(CompactionSample {
                ts_ms: Some(ms(37)),
                trigger: Some("manual".into()),
                pre_tokens: Some(300),
                post_tokens: Some(60),
                first_context_after: Some(60),
            }),
            last_model_switch: Some(ModelSwitch {
                ts_ms: Some(ms(34)),
                requested_model: "model-b".into(),
            }),
            seat_hint: None,
            skipped: SessionSkips {
                malformed: 1,
                untimed: 1,
                bad_timestamp: 1,
            },
        }
    );
}

#[test]
fn first_context_after_waits_for_the_usage_of_the_first_main_chain_call() {
    // Stop after the first message of api-5: its usage is not folded yet.
    let lines: Vec<&str> = EVENTS.lines().take(27).collect();
    let facts = fold(&(lines.join("\n") + "\n")).facts;
    let compaction = facts.last_compaction.unwrap();
    assert_eq!(compaction.first_context_after, None);
    assert_eq!(facts.latest_context.unwrap().total, None);
}

#[test]
fn untimed_event_records_fold_with_null_time_columns() {
    let text = concat!(
        r#"{"type":"user.message","data":{"content":"placeholder"}}"#,
        "\n",
        r#"{"type":"assistant.message","timestamp":"soon","data":{"apiCallId":"a","outputTokens":2}}"#,
        "\n",
        r#"{"type":"assistant.message","timestamp":"2026-01-10T00:00:05.000Z","data":{"apiCallId":"b"}}"#,
        "\n",
    );
    let run = fold(text);
    let calls: Vec<_> = run
        .rows
        .calls
        .iter()
        .map(|c| (c.msg_id.as_deref(), c.ts_ms, c.gap_ms, c.output))
        .collect();
    assert_eq!(
        calls,
        vec![
            (Some("a"), None, Some(-1), Some(2)),
            // The gap needs both native times.
            (Some("b"), Some(ms(5)), None, None),
        ]
    );
    assert_eq!(run.rows.turns[0].started_ts, None);
    assert_eq!(run.rows.triggers[0].ts, None);
    assert_eq!(
        run.facts.skipped,
        SessionSkips {
            malformed: 0,
            untimed: 1,
            bad_timestamp: 1,
        }
    );
}

#[test]
fn legacy_documents_fold_with_structural_keys_and_explicit_nulls() {
    let run = fold_legacy(LEGACY, PrepOptions::default());
    let rows = &run.rows;
    let key = |view: &str, index: usize| Some(format!("document#/{view}/{index}"));
    let calls: Vec<_> = rows
        .calls
        .iter()
        .map(|c| (c.native_key.clone(), c.turn_no, c.call_in_turn, c.gap_ms))
        .collect();
    assert_eq!(
        calls,
        vec![
            (key("chatMessages", 1), Some(1), Some(1), Some(-1)),
            (key("chatMessages", 3), Some(1), Some(2), None),
            (key("chatMessages", 5), Some(2), Some(1), None),
        ]
    );
    for call in &rows.calls {
        // The legacy view records no id, time, model, usage or stop reason.
        assert_eq!(
            (&call.msg_id, &call.request_id, &call.ts, call.ts_ms),
            (&None, &None, &None, None)
        );
        assert_eq!((&call.model, &call.stop_reason), (&None, &None));
        assert_eq!(
            [
                call.input,
                call.cw_1h,
                call.cw_5m,
                call.cache_read,
                call.output
            ],
            [None; 5]
        );
        assert_eq!(call.cache_write_basis, CacheWriteBasis::None);
        assert_eq!((call.native_offset, call.generation), (None, 3));
    }
    let triggers: Vec<_> = rows
        .triggers
        .iter()
        .map(|t| {
            (
                t.native_key.clone(),
                t.kind,
                t.sender.as_deref(),
                t.ts.clone(),
            )
        })
        .collect();
    assert_eq!(
        triggers,
        vec![
            (key("chatMessages", 0), TurnOrigin::Human, None, None),
            (
                key("chatMessages", 4),
                TurnOrigin::Peer,
                Some("pij-quiet-heron"),
                None
            ),
        ]
    );
    let turns: Vec<_> = rows
        .turns
        .iter()
        .map(|t| (t.turn_no, t.origin, t.native_key.clone(), t.started_ts_ms))
        .collect();
    assert_eq!(
        turns,
        vec![
            (1, TurnOrigin::Human, key("chatMessages", 1), None),
            (2, TurnOrigin::Peer, key("chatMessages", 5), None),
        ]
    );
    let uses: Vec<_> = rows
        .tool_uses
        .iter()
        .map(|t| {
            (
                t.sighting,
                t.native_key.clone(),
                t.tool_use_id.as_deref(),
                t.name.as_deref(),
                t.outcome,
                t.result_bytes,
                t.ts.clone(),
            )
        })
        .collect();
    assert_eq!(
        uses,
        vec![
            (
                ToolSighting::Use,
                key("chatMessages", 1),
                Some("call-1"),
                Some("view"),
                None,
                None,
                None
            ),
            (
                ToolSighting::Result,
                key("chatMessages", 2),
                Some("call-1"),
                Some("view"),
                Some(ToolOutcome::Unknown),
                Some("placeholder result".len() as i64),
                None
            ),
        ]
    );
    assert!(rows.events.is_empty());
    assert_eq!(
        run.facts,
        SessionFacts {
            context_window: None,
            session_id: Some("legacy-0001".into()),
            parent_session_id: None,
            is_sidechain: false,
            cwd: None,
            first_event_ts: Some("2026-01-10T00:00:00.000Z".into()),
            first_event_ms: Some(ms(0)),
            last_event_ts: Some("2026-01-10T00:00:09.000Z".into()),
            last_event_ms: Some(ms(9)),
            records: 13,
            calls: 3,
            turns: 2,
            latest_context: Some(ContextSample {
                ts_ms: None,
                model: None,
                stop_reason: None,
                input: None,
                cache_read: None,
                cache_write: None,
                total: None,
            }),
            // No native compaction marker in the legacy dialect.
            compactions: None,
            last_compaction: None,
            last_model_switch: None,
            seat_hint: None,
            // Six untimed chat messages plus one untimed timeline entry.
            skipped: SessionSkips {
                malformed: 1,
                untimed: 7,
                bad_timestamp: 1,
            },
        }
    );
}

#[test]
fn legacy_input_is_refused_unless_it_is_one_selected_json_document_folded_once() {
    let fold = CopilotCliLegacyPrepFold;
    let meta = fold.describe("legacy-0001.json");
    let kind = |input: &PrepInput| {
        let mut session = fold.open(&meta, "s", 0, None).unwrap();
        session
            .fold(input, PrepOptions::default())
            .err()
            .map(|e| e.kind())
    };
    let PrepInput::Snapshot(good) = snapshot(LEGACY) else {
        unreachable!()
    };
    let mut journal = good.clone();
    journal.source.format = SnapshotFormat::JsonJournal;
    assert_eq!(
        kind(&PrepInput::Snapshot(journal)),
        Some(PipelineErrorKind::InvalidInput)
    );
    let mut other = good.clone();
    other.source.session_id = Some("another-session".into());
    assert_eq!(
        kind(&PrepInput::Snapshot(other)),
        Some(PipelineErrorKind::InvalidInput)
    );
    let mut selected = good.clone();
    selected.source.session_id = Some("legacy-0001".into());
    assert_eq!(kind(&PrepInput::Snapshot(selected)), None);
    let mut two = good.clone();
    two.records.push(good.records[0].clone());
    assert_eq!(
        kind(&PrepInput::Snapshot(two)),
        Some(PipelineErrorKind::InvalidData)
    );
    let mut garbage = good.clone();
    garbage.records[0].bytes = b"[1,2]".to_vec();
    assert_eq!(
        kind(&PrepInput::Snapshot(garbage)),
        Some(PipelineErrorKind::InvalidData)
    );
    assert_eq!(
        kind(&PrepInput::Records(records(EVENTS))),
        Some(PipelineErrorKind::InvalidInput)
    );
    // A generation is one revision: a resumed, already folded session refuses more.
    let saved = fold_legacy(LEGACY, PrepOptions::default()).checkpoint;
    let mut resumed = fold.open(&meta, "s", 0, Some(&saved)).unwrap();
    assert_eq!(
        resumed
            .fold(&snapshot(LEGACY), PrepOptions::default())
            .err()
            .map(|e| e.kind()),
        Some(PipelineErrorKind::InvalidInput)
    );
    assert_eq!(resumed.checkpoint(), saved);
}

#[test]
fn foreign_checkpoints_and_the_other_representation_are_refused() {
    let events = CopilotCliPrepFold;
    let legacy = CopilotCliLegacyPrepFold;
    let meta = PrepSourceMeta::default();
    let good = fold(EVENTS).checkpoint;
    assert_eq!(good.format, PREP_CHECKPOINT_FORMAT);
    assert_eq!(good.policy, "copilot-cli/prep-v1");
    let refused = |fold: &dyn PrepFold, checkpoint: PrepCheckpoint| {
        fold.open(&meta, "s", 0, Some(&checkpoint))
            .err()
            .map(|e| e.kind())
    };
    let mut older = good.clone();
    older.policy = "copilot-cli/prep-v0".into();
    assert_eq!(
        refused(&events, older),
        Some(PipelineErrorKind::InvalidData)
    );
    let mut newer = good.clone();
    newer.format += 1;
    assert_eq!(
        refused(&events, newer),
        Some(PipelineErrorKind::InvalidData)
    );
    let mut damaged = good.clone();
    damaged.fold["committed"] = serde_json::json!(["not hex"]);
    assert_eq!(
        refused(&events, damaged),
        Some(PipelineErrorKind::InvalidData)
    );
    // Each fold refuses the other's checkpoint.
    let legacy_saved = fold_legacy(LEGACY, PrepOptions::default()).checkpoint;
    assert_eq!(legacy_saved.policy, "copilot-cli-snapshot/prep-v1");
    assert_eq!(
        refused(&events, legacy_saved),
        Some(PipelineErrorKind::InvalidData)
    );
    assert_eq!(refused(&legacy, good), Some(PipelineErrorKind::InvalidData));
    let mut session = events.open(&meta, "s", 0, None).unwrap();
    assert_eq!(
        session
            .fold(&snapshot(LEGACY), PrepOptions::default())
            .err()
            .map(|e| e.kind()),
        Some(PipelineErrorKind::InvalidInput)
    );
}

#[test]
fn content_appears_only_under_explicit_opt_in() {
    let opted = PrepOptions {
        include_content: true,
    };
    let plain = fold(EVENTS);
    let emitted = format!("{:?}{:?}{}", plain.rows, plain.facts, plain.checkpoint.fold);
    assert!(!emitted.contains("placeholder"));
    let legacy = fold_legacy(LEGACY, PrepOptions::default());
    let emitted = format!(
        "{:?}{:?}{}",
        legacy.rows, legacy.facts, legacy.checkpoint.fold
    );
    assert!(!emitted.contains("placeholder"));

    let heads = |rows: &PrepRows| -> Vec<Option<String>> {
        rows.triggers
            .iter()
            .map(|t| t.content_head.clone())
            .collect()
    };
    let events = fold_with(EVENTS, &[], false, opted).rows;
    assert_eq!(
        heads(&events)[..2],
        [
            Some("placeholder human prompt".into()),
            Some("placeholder peer body".into())
        ]
    );
    let legacy_opted = fold_legacy(LEGACY, opted).rows;
    assert_eq!(
        heads(&legacy_opted),
        vec![
            Some("placeholder legacy prompt".into()),
            Some("placeholder peer body".into())
        ]
    );
    // Opt-in changes only the content column.
    for (mut opted, plain) in [(events, plain.rows), (legacy_opted, legacy.rows)] {
        for trigger in &mut opted.triggers {
            trigger.content_head = None;
        }
        assert_eq!(opted, plain);
    }
}

#[test]
fn sensitive_fixture_values_never_leave_the_default_fold() {
    let events = fold(SENSITIVE_EVENTS);
    let legacy = fold_legacy(SENSITIVE_LEGACY, PrepOptions::default());
    for run in [events, legacy] {
        assert!(!run.rows.is_empty());
        // cwd is session metadata by contract; nothing else carries fixture text.
        let facts = SessionFacts {
            cwd: None,
            ..run.facts
        };
        let emitted = format!("{:?}{:?}", run.rows, facts);
        assert!(!emitted.contains("SENSITIVE"), "{emitted}");
    }
}
