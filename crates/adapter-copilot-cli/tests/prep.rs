//! Copilot CLI prep folds on synthetic, content-free fixtures.
//!
//! `fixtures/prep/sess-0001/events.jsonl` exercises the events dialect (calls
//! split across message and usage records, subagent and utility calls, typed
//! events, tool uses, malformed and untimed lines); `fixtures/prep/sess-router/events.jsonl`
//! a current-CLI model-router (HydraFusion) session with `session.usage_record`
//! usage; `fixtures/prep/legacy-0001.json` the legacy document. Every text
//! field says "placeholder". The pre-existing `fixtures/{events.jsonl,legacy.json}`
//! mark every sensitive value "SENSITIVE".

use unisphere_adapter_copilot_cli::{
    CopilotCliLegacyPrepFold, CopilotCliPrepFold, DESCRIPTOR, LEGACY_PREP_POLICY_VERSION,
    PREP_POLICY_VERSION, SNAPSHOT_DESCRIPTOR,
};
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineErrorKind, SnapshotFormat, SnapshotRecord, SnapshotRef,
    prep::{
        CacheWriteBasis, CallSighting, CompactionCounts, CompactionSample, ContextSample,
        ModelSwitch, PREP_CHECKPOINT_FORMAT, PrepCallRow, PrepCheckpoint, PrepEventKind, PrepFold,
        PrepInput, PrepOptions, PrepRows, PrepSourceKind, PrepSourceMeta, RouteSighting,
        SessionFacts, SessionSkips, ToolOutcome, ToolSighting, TurnOrigin,
    },
};

const EVENTS_FILE: &str = "sess-0001/events.jsonl";
const EVENTS: &str = include_str!("fixtures/prep/sess-0001/events.jsonl");
const LEGACY: &str = include_str!("fixtures/prep/legacy-0001.json");
const SENSITIVE_EVENTS: &str = include_str!("fixtures/events.jsonl");
const SENSITIVE_LEGACY: &str = include_str!("fixtures/legacy.json");
/// Synthetic router session shaped like a current (1.0.97) Copilot CLI
/// HydraFusion session: `session.usage_record` usage, fusion decisions,
/// phases and totals, real model names and counters, placeholder text.
const ROUTER: &str = include_str!("fixtures/prep/sess-router/events.jsonl");

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
/// first sighting's ordering facts, per-field maxima, the sum of records, the
/// first non-null router/TTL/cost-unit facts and a native-TTL basis.
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
                if call.cache_write_basis == CacheWriteBasis::Split {
                    first.cache_write_basis = CacheWriteBasis::Split;
                }
                let (into, from) = (&mut first.detail, &call.detail);
                into.cw_other = max(into.cw_other, from.cw_other);
                into.native_cost = max(into.native_cost, from.native_cost);
                into.cache_write_ttl_s = into.cache_write_ttl_s.or(from.cache_write_ttl_s);
                for (slot, value) in [
                    (&mut into.selected_model, &from.selected_model),
                    (&mut into.route_id, &from.route_id),
                    (&mut into.route_phase_id, &from.route_phase_id),
                    (&mut into.native_cost_unit, &from.native_cost_unit),
                ] {
                    if slot.is_none() {
                        slot.clone_from(value);
                    }
                }
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
    assert_eq!(PREP_POLICY_VERSION, "copilot-cli/prep-v5");
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
            // Uncached input: inputTokens 100 − cache read 50 − cache write 20.
            [Some(30), Some(20), Some(0), Some(50), Some(12)],
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
            [Some(70), Some(10), Some(0), Some(120), Some(4)],
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
            [Some(25), Some(0), Some(5), Some(0), Some(3)],
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
            // Utility: prompt_tokens 40 − cached 10 − cache creation 8.
            [Some(22), Some(0), Some(8), Some(10), Some(6)],
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
        (CallSighting::Update, Some(30), Some(20), 1)
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
                // The latest call is the subagent's: 25 + 5 + 0, 7 s before.
                (Some(30), Some(7000)),
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
                (Some(40), Some(3000)),
                2
            ),
            (
                Some(line_offset(23)),
                PrepEventKind::Compaction,
                None,
                Some("manual"),
                None,
                (Some(300), Some(60)),
                (Some(40), Some(4000)),
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
            // The manual compaction after the model change records the new limit.
            context_window: Some(700),
            cache_ttl_seconds: None,
            cache_expires_ms: None,
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
fn context_window_is_the_latest_main_chain_token_limit_until_the_model_changes() {
    let window = |lines: usize| {
        let prefix: Vec<&str> = EVENTS.lines().take(lines).collect();
        fold(&(prefix.join("\n") + "\n")).facts.context_window
    };
    assert_eq!(window(17), None, "no limit recorded yet");
    assert_eq!(window(18), Some(900), "compaction_start records one");
    assert_eq!(window(21), None, "the model change clears it");
    assert_eq!(
        window(24),
        Some(700),
        "the next compaction records the new one"
    );

    let text = concat!(
        r#"{"type":"session.truncation","timestamp":"2026-01-10T00:00:01.000Z","data":{"tokenLimit":500}}"#,
        "\n",
        r#"{"type":"session.truncation","timestamp":"2026-01-10T00:00:02.000Z","agentId":"agent-1","data":{"tokenLimit":90}}"#,
        "\n",
        r#"{"type":"session.compaction_complete","timestamp":"2026-01-10T00:00:03.000Z","data":{"success":true,"tokenLimit":0}}"#,
        "\n",
        r#"{"type":"session.shutdown","timestamp":"2026-01-10T00:00:04.000Z","data":{"tokenLimit":80}}"#,
        "\n",
    );
    assert_eq!(
        fold(text).facts.context_window,
        Some(500),
        "subagent, zero and non-limit records leave the main-chain limit"
    );
}

#[test]
fn cache_lifetime_is_the_current_models_entry_in_the_latest_main_checkpoint() {
    let text = concat!(
        r#"{"type":"session.start","timestamp":"2026-01-10T00:00:00.000Z","data":{"sessionId":"s","selectedModel":"model-a"}}"#,
        "\n",
        r#"{"type":"usage_placeholder","timestamp":"2026-01-10T00:00:01.000Z","data":{}}"#,
        "\n",
        r#"{"type":"session.usage_checkpoint","timestamp":"2026-01-10T00:00:02.000Z","data":{"modelCacheState":[{"modelId":"model-a","cacheTtlSeconds":300,"cacheExpiresAt":"2026-01-10T00:05:02.000Z"},{"modelId":"model-b","cacheTtlSeconds":3600,"cacheExpiresAt":"2026-01-10T01:00:02.000Z"}]}}"#,
        "\n",
        r#"{"type":"assistant.message","timestamp":"2026-01-10T00:00:03.000Z","data":{"apiCallId":"api-1","messageId":"m1","model":"model-a"}}"#,
        "\n",
        r#"{"type":"session.usage_checkpoint","timestamp":"2026-01-10T00:00:04.000Z","agentId":"agent-1","data":{"modelCacheState":[{"modelId":"model-a","cacheTtlSeconds":86400,"cacheExpiresAt":"2026-01-11T00:00:04.000Z"}]}}"#,
        "\n",
        r#"{"type":"assistant.message","timestamp":"2026-01-10T00:00:05.000Z","agentId":"agent-1","data":{"apiCallId":"api-2","messageId":"m2","model":"model-b"}}"#,
        "\n",
        r#"{"type":"assistant.message","timestamp":"2026-01-10T00:00:06.000Z","data":{"apiCallId":"api-3","messageId":"m3"}}"#,
        "\n",
        r#"{"type":"session.model_change","timestamp":"2026-01-10T00:00:07.000Z","data":{"previousModel":"model-a","newModel":"model-b"}}"#,
        "\n",
        r#"{"type":"assistant.message","timestamp":"2026-01-10T00:00:08.000Z","data":{"apiCallId":"api-4","messageId":"m4","model":"model-b"}}"#,
        "\n",
        r#"{"type":"session.usage_checkpoint","timestamp":"2026-01-10T00:00:09.000Z","data":{"totalPremiumRequests":1}}"#,
        "\n",
        r#"{"type":"session.usage_checkpoint","timestamp":"2026-01-10T00:00:10.000Z","data":{"modelCacheState":[{"modelId":"model-a","cacheTtlSeconds":300,"cacheExpiresAt":"2026-01-10T00:05:10.000Z"}]}}"#,
        "\n",
        r#"{"type":"session.usage_checkpoint","timestamp":"2026-01-10T00:00:11.000Z","data":{"modelCacheState":[{"modelId":"model-b","cacheTtlSeconds":1800}]}}"#,
        "\n",
    );
    let lifetime = |lines: usize| {
        let prefix: Vec<&str> = text.lines().take(lines).collect();
        let facts = fold(&(prefix.join("\n") + "\n")).facts;
        (facts.cache_ttl_seconds, facts.cache_expires_ms)
    };
    assert_eq!(lifetime(3), (None, None), "no call has named a model yet");
    assert_eq!(
        lifetime(4),
        (Some(300), Some(ms(302))),
        "the named model's entry, whichever came first"
    );
    assert_eq!(
        lifetime(7),
        (Some(300), Some(ms(302))),
        "subagent checkpoints and calls, and calls naming no model, change nothing"
    );
    assert_eq!(
        lifetime(8),
        (Some(300), Some(ms(302))),
        "a requested switch is not current until a call uses it"
    );
    assert_eq!(
        lifetime(9),
        (Some(3600), Some(ms(3602))),
        "the new model's entry"
    );
    assert_eq!(
        lifetime(10),
        (Some(3600), Some(ms(3602))),
        "a checkpoint without cache state keeps the last one"
    );
    assert_eq!(
        lifetime(11),
        (None, None),
        "the latest checkpoint has no entry for the current model"
    );
    assert_eq!(
        lifetime(12),
        (Some(1800), None),
        "unrecorded expiry stays None"
    );

    let whole = fold(text);
    let n = records(text).len();
    for split in 1..n {
        let run = fold_with(text, &[split], true, PrepOptions::default());
        assert_eq!(run.facts, whole.facts, "split={split}");
        assert_eq!(run.checkpoint, whole.checkpoint, "split={split}");
    }
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
            cache_ttl_seconds: None,
            cache_expires_ms: None,
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
    assert_eq!(good.policy, "copilot-cli/prep-v5");
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
    for (part, key, bad) in [
        ("core", "committed", serde_json::json!(["not hex"])),
        ("cache", "call_model", serde_json::json!(7)),
        (
            "cache",
            "lifetimes",
            serde_json::json!({"m": {"ttl_seconds": "300"}}),
        ),
    ] {
        let mut damaged = good.clone();
        damaged.fold[part][key] = bad;
        assert_eq!(
            refused(&events, damaged),
            Some(PipelineErrorKind::InvalidData),
            "{part}.{key}"
        );
    }
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

// ---------------------------------------------------------------------------
// Model routers and `session.usage_record`
// ---------------------------------------------------------------------------

/// (call id, model ran, selected model, router phase, sidechain,
/// [input, cw_1h, cw_5m, cache_read, output, cw_other], native TTL, basis,
/// nano-AIU, records)
type RouterCall<'a> = (
    &'a str,
    &'a str,
    Option<&'a str>,
    Option<&'a str>,
    bool,
    [Option<i64>; 6],
    Option<i64>,
    CacheWriteBasis,
    Option<i64>,
    i64,
);

fn router_call(call: &PrepCallRow) -> RouterCall<'_> {
    let d = &call.detail;
    (
        call.msg_id.as_deref().unwrap_or_default(),
        call.model.as_deref().unwrap_or_default(),
        d.selected_model.as_deref(),
        d.route_phase_id.as_deref(),
        call.is_sidechain,
        [
            call.input,
            call.cw_1h,
            call.cw_5m,
            call.cache_read,
            call.output,
            d.cw_other,
        ],
        d.cache_write_ttl_s,
        call.cache_write_basis,
        d.native_cost,
        call.records,
    )
}

#[test]
fn usage_records_fold_into_calls_with_native_ttl_cost_and_the_model_that_ran() {
    use CacheWriteBasis::{Fallback5m, Split};
    let s = Some;
    let hf = Some("hydrafusion");
    let run = fold(ROUTER);
    let calls = canonical(&run.rows).calls;
    // `input` is uncached: native inputTokens minus the cache reads and
    // writes Copilot also records (e.g. 38803 − 0 − 38799 = 4).
    let expected: Vec<RouterCall<'static>> = vec![
        // Subagent call: 1800 s TTL is neither 1 h nor 5 m, so it is cw_other.
        (
            "api-s1",
            "gpt-5.6-luna",
            None,
            None,
            true,
            [s(3), s(0), s(0), s(0), s(15), s(1662)],
            s(1800),
            Split,
            s(43_410_000),
            2,
        ),
        (
            "api-f1-1",
            "gpt-6-luna",
            hf,
            Some("fusion-0001:phase:0"),
            false,
            [s(3), s(0), s(0), s(0), s(32), s(22884)],
            s(1800),
            Split,
            s(287_680_000),
            2,
        ),
        // Utility call: its native 1800 s TTL is honoured the same way, and a
        // recorded zero cost is 0, not null.
        (
            "util-0001",
            "gpt-5.4-nano",
            None,
            None,
            true,
            [s(533), s(0), s(0), s(0), s(43), s(12)],
            s(1800),
            Split,
            s(0),
            1,
        ),
        (
            "api-f2-s1",
            "gpt-6.1-sol",
            hf,
            Some("fusion-0002:phase:0"),
            false,
            [s(3), s(0), s(0), s(0), s(18), s(23112)],
            s(1800),
            Split,
            s(5_796_600_000),
            1,
        ),
        (
            "api-f2-s2",
            "gpt-6.1-sol",
            hf,
            Some("fusion-0002:phase:0"),
            false,
            [s(3), s(0), s(0), s(23112), s(20), s(235)],
            s(1800),
            Split,
            s(310_470_000),
            1,
        ),
        // The judge reviews outside the root conversation: a sidechain call;
        // no native TTL, so the documented fallback applies.
        (
            "api-f2-j1",
            "gpt-6-luna",
            hf,
            Some("fusion-0002:judge"),
            true,
            [s(3), s(0), s(1508), s(0), s(62), s(0)],
            None,
            Fallback5m,
            s(21_980_000),
            1,
        ),
        (
            "api-f2-j2",
            "gpt-6-luna",
            hf,
            Some("fusion-0002:judge"),
            true,
            [s(3), s(0), s(92), s(1508), s(35), s(0)],
            None,
            Fallback5m,
            s(4_438_000),
            1,
        ),
        // Claude's native 300 s TTL is the 5 m class.
        (
            "api-f2-r1",
            "claude-opus-5.5",
            hf,
            Some("fusion-0002:repair"),
            false,
            [s(4), s(0), s(38799), s(0), s(187), s(0)],
            s(300),
            Split,
            s(19_775_100_000),
            2,
        ),
        (
            "api-f2-r2",
            "claude-opus-5.5",
            hf,
            Some("fusion-0002:repair"),
            false,
            [s(2), s(0), s(1702), s(38799), s(122), s(0)],
            s(300),
            Split,
            s(1_871_780_000),
            2,
        ),
        (
            "api-s2",
            "gpt-5.6-luna",
            None,
            None,
            true,
            [s(3), s(0), s(0), s(1662), s(143), s(2585)],
            s(1800),
            Split,
            s(85_169_000),
            2,
        ),
    ];
    assert_eq!(calls.iter().map(router_call).collect::<Vec<_>>(), expected);
    for call in &calls {
        let unit = call.detail.native_cost.map(|_| "nano_aiu");
        assert_eq!(call.detail.native_cost_unit.as_deref(), unit);
    }
    // Root-scope router calls open and stay in the turn their route serves.
    let turn = |id: &str| {
        calls
            .iter()
            .find(|c| c.msg_id.as_deref() == Some(id))
            .unwrap()
            .turn_no
    };
    assert_eq!(turn("api-f1-1"), Some(1));
    assert_eq!(turn("api-f2-s1"), Some(2));
    assert_eq!(turn("api-f2-r2"), Some(2));
    assert_eq!(run.facts.turns, 2);
    // The latest main-chain context is the repair call's, never the judge's:
    // uncached input + cache_read + every cache-write class, which equals the
    // native prompt count (inputTokens 40503).
    let latest = run.facts.latest_context.unwrap();
    assert_eq!(latest.model.as_deref(), Some("claude-opus-5.5"));
    assert_eq!(latest.input, Some(2));
    assert_eq!(latest.total, Some(40503));
}

#[test]
fn router_decisions_and_phases_become_routes_and_route_phases() {
    let rows = fold(ROUTER).rows;
    let routes: Vec<_> = rows
        .routes
        .iter()
        .map(|r| (r.route_id.as_deref().unwrap(), r.sighting, r.turn_no))
        .collect();
    assert_eq!(
        routes,
        [
            ("fusion-0001", RouteSighting::Plan, None),
            ("fusion-0001", RouteSighting::Outcome, Some(1)),
            ("fusion-0002", RouteSighting::Plan, None),
            ("fusion-0002", RouteSighting::Outcome, Some(2)),
        ]
    );
    let plan = &rows.routes[2];
    assert_eq!(plan.selected_model.as_deref(), Some("hydrafusion"));
    assert_eq!(plan.pattern.as_deref(), Some("cascade"));
    assert_eq!(
        plan.rule_id.as_deref(),
        Some("cascade_sol61_medium_luna6_high_opus55_medium")
    );
    assert_eq!(
        (plan.route_source.as_deref(), plan.policy.as_deref()),
        (Some("capi_plan"), Some("standard"))
    );
    assert_eq!(
        plan.policy_version.as_deref(),
        Some("hydrafusion_v2_2026-10-07")
    );
    assert_eq!(plan.latency_ms, Some(1109));
    assert_eq!(
        plan.scores.as_deref(),
        Some(r#"{"codeGen":0.352,"debugging":0.0025,"reasoning":0.2118,"toolUse":0.1108}"#)
    );
    assert_eq!(
        plan.planned_phases.as_deref(),
        Some("primary/solver,judge/judge,repair/solver?")
    );
    assert_eq!(
        [
            plan.primary_model.as_deref(),
            plan.secondary_model.as_deref(),
            plan.judge_model.as_deref(),
            plan.repair_model.as_deref(),
            plan.fallback_model.as_deref(),
        ],
        [
            Some("gpt-6.1-sol"),
            Some("gpt-6-luna"),
            Some("gpt-6-luna"),
            Some("claude-opus-5.5"),
            Some("gpt-6-luna"),
        ]
    );
    assert_eq!(
        (plan.final_model.as_deref(), plan.request_count),
        (None, None)
    );
    let outcome = &rows.routes[3];
    assert_eq!(outcome.final_model.as_deref(), Some("claude-opus-5.5"));
    assert_eq!(outcome.outcome.as_deref(), Some("completed"));
    assert_eq!(
        (outcome.phase_count, outcome.request_count),
        (Some(3), Some(6))
    );
    assert_eq!(outcome.native_cost_unit.as_deref(), Some("nano_aiu"));
    assert_eq!(outcome.duration_ms, Some(163_458));

    let phases: Vec<_> = rows
        .route_phases
        .iter()
        .map(|p| {
            (
                p.phase_id.as_deref().unwrap(),
                p.kind.as_deref().unwrap(),
                p.scope.as_deref().unwrap(),
                p.model.as_deref().unwrap(),
                p.verdict.as_deref(),
                p.turn_no,
            )
        })
        .collect();
    assert_eq!(
        phases,
        [
            (
                "fusion-0001:phase:0",
                "primary",
                "root",
                "gpt-6-luna",
                None,
                Some(1)
            ),
            (
                "fusion-0002:phase:0",
                "primary",
                "root",
                "gpt-6.1-sol",
                None,
                Some(2)
            ),
            (
                "fusion-0002:judge",
                "judge",
                "review",
                "gpt-6-luna",
                Some("reject"),
                Some(2)
            ),
            (
                "fusion-0002:repair",
                "repair",
                "root",
                "claude-opus-5.5",
                None,
                Some(2)
            ),
        ]
    );
}

#[test]
fn route_phases_and_outcomes_reconcile_with_the_calls_they_attribute() {
    let run = fold(ROUTER);
    let calls = canonical(&run.rows).calls;
    let totals = |phase: Option<&str>, route: Option<&str>| {
        let mut sum = [0_i64; 6];
        for call in calls.iter().filter(|c| {
            phase.is_none_or(|p| c.detail.route_phase_id.as_deref() == Some(p))
                && route.is_none_or(|r| c.detail.route_id.as_deref() == Some(r))
        }) {
            let write = call.cw_1h.unwrap_or(0)
                + call.cw_5m.unwrap_or(0)
                + call.detail.cw_other.unwrap_or(0);
            for (slot, value) in sum.iter_mut().zip([
                1,
                call.input.unwrap(),
                call.output.unwrap(),
                call.cache_read.unwrap(),
                write,
                call.detail.native_cost.unwrap(),
            ]) {
                *slot += value;
            }
        }
        sum
    };
    let native = |requests, input, output, read, write, cost: Option<i64>| {
        [requests, input, output, read, write, cost].map(|v: Option<i64>| v.unwrap())
    };
    for phase in run
        .rows
        .route_phases
        .iter()
        .filter(|p| p.route_id.as_deref() == Some("fusion-0002"))
    {
        assert_eq!(
            totals(phase.phase_id.as_deref(), None),
            native(
                phase.request_count,
                phase.input,
                phase.output,
                phase.cache_read,
                phase.cache_write,
                phase.native_cost
            ),
            "{:?}",
            phase.phase_id
        );
    }
    let outcome = &run.rows.routes[3];
    assert_eq!(
        totals(None, Some("fusion-0002")),
        native(
            outcome.request_count,
            outcome.input,
            outcome.output,
            outcome.cache_read,
            outcome.cache_write,
            outcome.native_cost
        )
    );
    // Copilot also counts a subagent request made during the phase (api-s1)
    // that carries no fusion tag: the native totals exceed the tagged calls
    // by exactly that call.
    let first = &run.rows.route_phases[0];
    let tagged = totals(first.phase_id.as_deref(), None);
    let s1 = calls
        .iter()
        .find(|c| c.msg_id.as_deref() == Some("api-s1"))
        .unwrap();
    let s1 = [
        1,
        s1.input.unwrap(),
        s1.output.unwrap(),
        s1.cache_read.unwrap(),
        s1.detail.cw_other.unwrap(),
        s1.detail.native_cost.unwrap(),
    ];
    let combined: Vec<i64> = tagged.iter().zip(s1).map(|(a, b)| a + b).collect();
    assert_eq!(
        combined,
        native(
            first.request_count,
            first.input,
            first.output,
            first.cache_read,
            first.cache_write,
            first.native_cost
        )
    );
}

#[test]
fn router_session_folds_identically_under_every_batch_split_and_resume() {
    let whole = fold(ROUTER);
    let expected = canonical(&whole.rows);
    let n = records(ROUTER).len();
    let mut plans: Vec<Vec<usize>> = (1..n).map(|k| vec![k]).collect();
    plans.push((1..n).collect());
    for splits in plans {
        for resume in [false, true] {
            let run = fold_with(ROUTER, &splits, resume, PrepOptions::default());
            let label = format!("splits={splits:?} resume={resume}");
            assert_eq!(canonical(&run.rows), expected, "{label}");
            assert_eq!(run.facts, whole.facts, "{label}");
            assert_eq!(run.checkpoint, whole.checkpoint, "{label}");
        }
    }
}

#[test]
fn router_rows_carry_no_message_content() {
    // Message, phase, hint and handoff text are placeholders; none may leave
    // the fold as a row (the session's cwd is metadata, not a row field).
    let rows = fold(ROUTER).rows;
    let emitted = format!("{rows:?}");
    assert!(!emitted.contains("placeholder"), "{emitted}");
}

#[test]
fn inconsistent_native_prompt_counts_leave_uncached_input_unrecorded() {
    // inputTokens below cache reads plus writes cannot be split into an
    // uncached remainder: input is null (never clamped to zero); the cache
    // counters stay as recorded.
    let record = r#"{"type":"session.usage_record","timestamp":"2026-01-10T00:00:01.000Z","data":{"usage":{"apiCallId":"api-x","model":"model-a","inputTokens":10,"outputTokens":1,"cacheReadTokens":8,"cacheWriteTokens":5,"cacheTtlSeconds":300}}}"#;
    let call = &fold(record).rows.calls[0];
    assert_eq!(
        (call.input, call.cache_read, call.cw_5m, call.output),
        (None, Some(8), Some(5), Some(1))
    );
}
