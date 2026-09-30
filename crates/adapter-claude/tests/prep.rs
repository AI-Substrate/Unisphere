//! ClaudePrepFold behaviour on synthetic, content-free fixtures.
//!
//! The expected call, trigger and event values were produced by running a
//! scratch copy of the consumer's reference parser (`extract.py`, window
//! widened) over these same fixtures; deliberate differences are commented.

use unisphere_adapter_claude::{ClaudePrepFold, PREP_POLICY_VERSION};
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineErrorKind, SnapshotFormat, SnapshotRef,
    prep::{
        CacheWriteBasis, CallSighting, CompactionCounts, CompactionSample, ContextSample,
        ModelSwitch, PREP_CHECKPOINT_FORMAT, PrepCallRow, PrepCheckpoint, PrepEventKind, PrepFold,
        PrepInput, PrepOptions, PrepRows, PrepSourceKind, PrepSourceMeta, SessionFacts,
        ToolOutcome, ToolSighting, TurnOrigin,
    },
};

const MAIN_FILE: &str = "-Users-dev-demo/sess-0001.jsonl";
const MAIN: &str = include_str!("fixtures/prep/-Users-dev-demo/sess-0001.jsonl");
const SUB_FILE: &str = "-Users-dev-demo/sess-0001/subagents/agent-a1.jsonl";
const SUB: &str = include_str!("fixtures/prep/-Users-dev-demo/sess-0001/subagents/agent-a1.jsonl");

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

fn ms(ts: &str) -> i64 {
    let parsed =
        time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339).unwrap();
    (parsed.unix_timestamp_nanos() / 1_000_000) as i64
}

fn ts(seconds: u32) -> String {
    format!("2026-01-10T00:{:02}:{:02}.000Z", seconds / 60, seconds % 60)
}

struct Run {
    rows: PrepRows,
    facts: SessionFacts,
    checkpoint: PrepCheckpoint,
}

/// Fold `text` in batches ending at `splits`; with `resume`, every batch after
/// the first starts from a checkpoint serialised to JSON text and reopened.
fn fold_with(file: &str, text: &str, splits: &[usize], resume: bool, options: PrepOptions) -> Run {
    let fold = ClaudePrepFold;
    let meta = fold.describe(file);
    let source = format!("claude-code/default/{file}");
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

fn fold(file: &str, text: &str) -> Run {
    fold_with(file, text, &[], false, PrepOptions::default())
}

/// The canonical reader's merge: one row per (msg_id, request_id) keeping the
/// first sighting's ordering facts, per-field maxima, the last non-null stop
/// reason and the sum of merged records.
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
                if call.stop_reason.is_some() {
                    first.stop_reason.clone_from(&call.stop_reason);
                }
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

fn line_offset(text: &str, line: usize) -> u64 {
    text.lines().take(line).map(|l| l.len() as u64 + 1).sum()
}

#[test]
fn fold_identity_matches_the_frozen_interface() {
    let fold = ClaudePrepFold;
    assert_eq!(fold.harness(), "claude-code");
    assert_eq!(fold.policy(), "claude-code/prep-v3");
    assert_eq!(PREP_POLICY_VERSION, "claude-code/prep-v3");
    assert_eq!(fold.kind(), PrepSourceKind::Append);
    assert_eq!(fold.pattern(), "**/*.jsonl");
    assert_eq!(
        fold.describe(SUB_FILE),
        PrepSourceMeta {
            is_sub: true,
            agent_id: Some("agent-a1".into()),
            project: Some("demo".into()),
        }
    );
    assert_eq!(
        fold.describe(MAIN_FILE),
        PrepSourceMeta {
            is_sub: false,
            agent_id: None,
            project: Some("demo".into()),
        }
    );
}

type CallExpectation = (
    &'static str,
    [Option<i64>; 5],
    i64,
    i64,
    i64,
    CacheWriteBasis,
);

#[test]
fn calls_reproduce_the_reference_parser_rows() {
    use CacheWriteBasis::{Fallback1h, Fallback5m, None as NoCache, Split};
    let s = Some;
    // (msg id, [input, cw_1h, cw_5m, cache_read, output], gap_ms, turn_no,
    // call_in_turn, basis); reference gap_s × 1000.
    let main: [CallExpectation; 13] = [
        ("msg_00", [s(10), s(100), s(0), s(0), s(5)], -1, 0, 1, Split),
        (
            "msg_01",
            [s(3), s(50), s(0), s(110), s(40)],
            11_000,
            1,
            1,
            Split,
        ),
        (
            "msg_02",
            [s(1), s(10), s(5), s(160), s(9)],
            8_000,
            1,
            2,
            Split,
        ),
        (
            "msg_03",
            [s(2), s(0), s(0), s(170), s(3)],
            45_000,
            2,
            1,
            Split,
        ),
        (
            "msg_04",
            [s(2), s(0), s(0), s(175), s(3)],
            60_000,
            3,
            1,
            Split,
        ),
        (
            "msg_05",
            [s(2), s(0), s(0), s(180), s(3)],
            60_000,
            4,
            1,
            Split,
        ),
        (
            "msg_06",
            [s(4), s(30), s(0), s(25), s(8)],
            95_000,
            5,
            1,
            Split,
        ),
        (
            "msg_07",
            [s(1), s(0), s(0), s(60), s(1)],
            925_000,
            6,
            1,
            Split,
        ),
        (
            "msg_08",
            [s(1), s(0), s(0), s(61), s(1)],
            60_000,
            7,
            1,
            Split,
        ),
        (
            "msg_09",
            [s(1), s(40), s(0), s(62), s(2)],
            175_000,
            8,
            1,
            Fallback1h,
        ),
        // The reference writes 0 for unrecorded cache writes; prep writes null.
        (
            "msg_10",
            [s(1), None, None, s(63), s(2)],
            365_000,
            9,
            1,
            NoCache,
        ),
        (
            "msg_11",
            [s(1), s(1), s(0), s(64), s(2)],
            60_000,
            10,
            1,
            Split,
        ),
        (
            "msg_12",
            [s(1), s(0), s(0), s(900), s(2)],
            55_000,
            10,
            2,
            Split,
        ),
    ];
    let sub: [CallExpectation; 2] = [
        (
            "msg_20",
            [s(3), s(0), s(30), s(0), s(4)],
            -1,
            1,
            1,
            Fallback5m,
        ),
        (
            "msg_21",
            [s(3), s(0), s(7), s(30), s(4)],
            3_000,
            1,
            2,
            Split,
        ),
    ];
    for (file, text, expected) in [(MAIN_FILE, MAIN, &main[..]), (SUB_FILE, SUB, &sub[..])] {
        let rows = fold(file, text).rows;
        let actual: Vec<_> = rows
            .calls
            .iter()
            .map(|c| {
                (
                    c.msg_id.clone().unwrap(),
                    [c.input, c.cw_1h, c.cw_5m, c.cache_read, c.output],
                    c.gap_ms.unwrap(),
                    c.turn_no.unwrap(),
                    c.call_in_turn.unwrap(),
                    c.cache_write_basis,
                )
            })
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .map(|&(id, tokens, gap, turn, call, basis)| {
                (id.to_owned(), tokens, gap, turn, call, basis)
            })
            .collect();
        assert_eq!(actual, expected, "{file}");
        assert!(rows.calls.iter().all(|c| c.sighting == CallSighting::First));
    }
}

#[test]
fn repeated_records_merge_by_field_maximum_and_last_stop_reason() {
    let rows = fold(MAIN_FILE, MAIN).rows;
    let call = |id: &str| {
        rows.calls
            .iter()
            .find(|c| c.msg_id.as_deref() == Some(id))
            .unwrap()
    };
    let first = call("msg_02");
    assert_eq!((first.records, first.output), (2, Some(9)));
    assert_eq!(first.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(first.request_id.as_deref(), Some("req_02"));
    // The first record's timestamp, offset and model carry the row.
    assert_eq!(first.ts.as_deref(), Some(ts(20).as_str()));
    assert_eq!(first.native_offset, Some(line_offset(MAIN, 9)));
    // A null stop reason never overwrites a recorded one.
    let streamed = call("msg_01");
    assert_eq!((streamed.records, streamed.output), (2, Some(40)));
    assert_eq!(streamed.stop_reason.as_deref(), Some("tool_use"));
    assert_eq!(rows.calls.len(), 13);
    assert!(
        rows.calls
            .iter()
            .all(|c| c.model.as_deref() != Some("<synthetic>"))
    );
}

#[test]
fn a_repeat_in_a_later_batch_is_an_update_sighting_without_ordering_facts() {
    // Split between the two records of msg_01 and resume from a checkpoint.
    let run = fold_with(MAIN_FILE, MAIN, &[7], true, PrepOptions::default());
    let sightings: Vec<_> = run
        .rows
        .calls
        .iter()
        .filter(|c| c.msg_id.as_deref() == Some("msg_01"))
        .collect();
    assert_eq!(sightings.len(), 2);
    assert_eq!(sightings[0].sighting, CallSighting::First);
    assert_eq!(
        (sightings[0].output, sightings[0].stop_reason.as_deref()),
        (Some(1), None)
    );
    let update = sightings[1];
    assert_eq!(update.sighting, CallSighting::Update);
    assert_eq!(
        (update.output, update.stop_reason.as_deref()),
        (Some(40), Some("tool_use"))
    );
    assert_eq!(
        (update.gap_ms, update.turn_no, update.call_in_turn),
        (None, None, None)
    );
    assert_eq!(update.native_offset, Some(line_offset(MAIN, 7)));
    assert_eq!(run.facts.calls, 13);
}

#[test]
fn every_batch_split_and_checkpoint_resume_equals_one_fold() {
    for (file, text) in [(MAIN_FILE, MAIN), (SUB_FILE, SUB)] {
        let whole = fold(file, text);
        let expected = canonical(&whole.rows);
        let n = records(text).len();
        let mut plans: Vec<Vec<usize>> = (1..n).map(|k| vec![k]).collect();
        plans.push((1..n).collect());
        for splits in plans {
            for resume in [false, true] {
                let run = fold_with(file, text, &splits, resume, PrepOptions::default());
                let label = format!("{file} splits={splits:?} resume={resume}");
                assert_eq!(canonical(&run.rows), expected, "{label}");
                assert_eq!(run.facts, whole.facts, "{label}");
                assert_eq!(run.checkpoint, whole.checkpoint, "{label}");
            }
        }
    }
}

#[test]
fn cache_write_basis_names_the_native_split_or_the_documented_fallback() {
    let main = fold(MAIN_FILE, MAIN).rows;
    let basis = |rows: &PrepRows, id: &str| {
        let c = rows
            .calls
            .iter()
            .find(|c| c.msg_id.as_deref() == Some(id))
            .unwrap();
        (c.cache_write_basis, c.cw_1h, c.cw_5m)
    };
    assert_eq!(
        basis(&main, "msg_02"),
        (CacheWriteBasis::Split, Some(10), Some(5))
    );
    // Aggregate only: all of it is 1 h in a main session ...
    assert_eq!(
        basis(&main, "msg_09"),
        (CacheWriteBasis::Fallback1h, Some(40), Some(0))
    );
    // ... and nothing recorded stays null, never zero.
    assert_eq!(basis(&main, "msg_10"), (CacheWriteBasis::None, None, None));
    // ... and 5 m in a subagent source.
    let sub = fold(SUB_FILE, SUB).rows;
    assert_eq!(
        basis(&sub, "msg_20"),
        (CacheWriteBasis::Fallback5m, Some(0), Some(30))
    );
    assert!(sub.calls.iter().all(|c| c.is_sidechain));
}

#[test]
fn synthetic_records_are_limit_notices_with_reset_instants_where_resolvable() {
    let rows = fold(MAIN_FILE, MAIN).rows;
    let notices: Vec<_> = rows
        .events
        .iter()
        .filter(|e| e.kind == PrepEventKind::LimitNotice)
        .map(|e| {
            (
                e.ts.clone().unwrap(),
                e.subkind.clone().unwrap(),
                e.resets_at.clone(),
                e.resets_at_ms,
            )
        })
        .collect();
    assert_eq!(
        notices,
        vec![
            (
                ts(1320),
                "session_limit".into(),
                Some("3:10pm (Australia/Brisbane)".into()),
                Some(ms("2026-01-10T15:10:00+10:00")),
            ),
            (
                ts(1410),
                "weekly_limit".into(),
                Some("Jan 12 at 9am (Australia/Brisbane)".into()),
                Some(ms("2026-01-12T09:00:00+10:00")),
            ),
            (ts(1411), "other".into(), None, None),
            // A daylight-saving zone cannot be resolved without a tz database.
            (
                ts(1412),
                "session_limit".into(),
                Some("3pm (Europe/London)".into()),
                None,
            ),
        ]
    );
}

#[test]
fn compaction_recap_schedule_queue_and_model_switch_are_typed_events() {
    let rows = fold(MAIN_FILE, MAIN).rows;
    let kinds: Vec<_> = rows.events.iter().map(|e| (e.kind, e.turn_no)).collect();
    use PrepEventKind::*;
    assert_eq!(
        kinds,
        vec![
            (QueueOp, 1),
            (QueueOp, 1),
            (ModelSwitch, 3),
            (Compaction, 4),
            (Recap, 5),
            (ScheduledFire, 5),
            (LimitNotice, 7),
            (LimitNotice, 7),
            (LimitNotice, 7),
            (LimitNotice, 7),
            (ModelSwitch, 9),
        ]
    );
    let event = |kind: PrepEventKind| rows.events.iter().find(|e| e.kind == kind).unwrap();
    let compaction = event(Compaction);
    assert_eq!(compaction.trigger.as_deref(), Some("manual"));
    assert_eq!(
        (
            compaction.pre_tokens,
            compaction.post_tokens,
            compaction.duration_ms
        ),
        (Some(180), Some(20), Some(30_000))
    );
    // Reference gap_s_since_last_call 85.0; context of the last call (msg_05).
    assert_eq!(
        (compaction.gap_ms, compaction.last_context),
        (Some(85_000), Some(182))
    );
    // Reference recap: last_context 59 (msg_06 after its repeat), gap 320.0.
    let recap = event(Recap);
    assert_eq!(
        (recap.last_context, recap.gap_ms),
        (Some(59), Some(320_000))
    );
    assert_eq!(event(ScheduledFire).ts.as_deref(), Some(ts(1200).as_str()));
    let queue: Vec<_> = rows.events.iter().filter(|e| e.kind == QueueOp).collect();
    assert_eq!(queue[0].subkind.as_deref(), Some("enqueue"));
    assert_eq!(queue[1].subkind.as_deref(), Some("queued_command"));
    assert!(queue[0].body_key.is_some() && queue[0].body_key == queue[1].body_key);
    let switches: Vec<_> = rows
        .events
        .iter()
        .filter(|e| e.kind == ModelSwitch)
        .map(|e| (e.ts.clone().unwrap(), e.model.clone().unwrap()))
        .collect();
    assert_eq!(
        switches,
        vec![(ts(181), "opus".into()), (ts(1861), "fable".into())]
    );
}

#[test]
fn turns_carry_the_reference_origin_and_sender_precedence() {
    let rows = fold(MAIN_FILE, MAIN).rows;
    use TurnOrigin::*;
    let triggers: Vec<_> = rows
        .triggers
        .iter()
        .map(|t| {
            (
                t.kind,
                t.sender.as_deref(),
                t.pij_msg_id.as_deref(),
                t.chars,
                t.next_turn_no,
            )
        })
        .collect();
    assert_eq!(
        triggers,
        vec![
            (Human, None, None, 24, 1),
            // from-name wins over [pij-rs from] and origin.from.
            (Peer, Some("pij-alpha-one"), Some("ab12-cd34"), 169, 2),
            (Peer, Some("pij-gamma-three"), None, 58, 3),
            (Peer, Some("pij-delta-four"), None, 28, 4),
            (ManualCompact, None, None, 8, 5),
            (CompactSummary, None, None, 42, 5),
            (Loop, None, None, 0, 6),
            (TaskNotification, None, None, 24, 7),
            (AutoContinuation, None, None, 20, 8),
            (Scheduled, None, None, 28, 9),
            (Coordinator, None, None, 23, 9),
            (Other, None, None, 17, 10),
        ]
    );
    let turns: Vec<_> = rows
        .turns
        .iter()
        .map(|t| (t.turn_no, t.origin, t.sender.as_deref()))
        .collect();
    assert_eq!(
        turns,
        vec![
            (0, Start, None),
            (1, Human, None),
            (2, Peer, Some("pij-alpha-one")),
            (3, Peer, Some("pij-gamma-three")),
            (4, Peer, Some("pij-delta-four")),
            // The compact summary does not replace a pending manual compact.
            (5, ManualCompact, None),
            (6, Loop, None),
            (7, TaskNotification, None),
            (8, AutoContinuation, None),
            // The latest classified opener wins.
            (9, Coordinator, None),
            (10, Other, None),
        ]
    );
    let peer = &rows.turns[2];
    assert_eq!(peer.pij_msg_id.as_deref(), Some("ab12-cd34"));
    assert_eq!(peer.opener_offset, Some(line_offset(MAIN, 12)));
    assert_eq!(peer.first_call_offset, Some(line_offset(MAIN, 15)));
    assert_eq!(peer.opener_ts_ms, Some(ms(&ts(60))));
    assert_eq!(peer.started_ts.as_deref(), Some(ts(65).as_str()));
    let sub = fold(SUB_FILE, SUB).rows;
    assert_eq!(sub.turns.len(), 1);
    assert_eq!(
        (sub.turns[0].turn_no, sub.turns[0].origin),
        (1, SubagentTask)
    );
}

#[test]
fn tool_uses_pair_use_and_result_across_batches_and_resume() {
    for splits in [vec![], vec![8], vec![10], (1..46).collect::<Vec<_>>()] {
        let rows = fold_with(MAIN_FILE, MAIN, &splits, true, PrepOptions::default()).rows;
        let uses: Vec<_> = rows
            .tool_uses
            .iter()
            .map(|t| {
                (
                    t.sighting,
                    t.tool_use_id.as_deref().unwrap(),
                    t.name.as_deref(),
                    t.family.as_deref(),
                    t.call_msg_id.as_deref(),
                    t.outcome,
                    t.duration_ms,
                    t.turn_no,
                )
            })
            .collect();
        use ToolSighting::{Result, Use};
        assert_eq!(
            uses,
            vec![
                (
                    Use,
                    "toolu_01",
                    Some("Bash"),
                    Some("shell"),
                    Some("msg_01"),
                    None,
                    None,
                    Some(1)
                ),
                (
                    Result,
                    "toolu_01",
                    Some("Bash"),
                    Some("shell"),
                    Some("msg_01"),
                    Some(ToolOutcome::Ok),
                    Some(250),
                    Some(1)
                ),
                (
                    Use,
                    "toolu_02",
                    Some("Read"),
                    Some("file-read"),
                    Some("msg_02"),
                    None,
                    None,
                    Some(1)
                ),
                (
                    Use,
                    "toolu_03",
                    Some("Grep"),
                    None,
                    Some("msg_02"),
                    None,
                    None,
                    Some(1)
                ),
                // Two results in one record: no duration is attributed.
                (
                    Result,
                    "toolu_02",
                    Some("Read"),
                    Some("file-read"),
                    Some("msg_02"),
                    Some(ToolOutcome::Error),
                    None,
                    Some(1)
                ),
                (
                    Result,
                    "toolu_03",
                    Some("Grep"),
                    None,
                    Some("msg_02"),
                    Some(ToolOutcome::Unknown),
                    None,
                    Some(1)
                ),
            ],
            "splits={splits:?}"
        );
        let bash = &rows.tool_uses[0];
        let canonical_input = br#"{"command":"true","description":"placeholder"}"#;
        assert_eq!(bash.input_bytes, Some(canonical_input.len() as i64));
        assert_eq!(bash.input_hash.as_ref().map(String::len), Some(16));
        assert_eq!((bash.result_offset, bash.result_bytes), (None, None));
        let result = &rows.tool_uses[1];
        assert_eq!(result.result_offset, Some(line_offset(MAIN, 8)));
        assert_eq!(result.result_bytes, Some("placeholder output".len() as i64));
        assert_eq!(
            (result.input_hash.as_deref(), result.input_bytes),
            (None, None)
        );
        let error = &rows.tool_uses[4];
        let parts = br#"[{"text":"placeholder error","type":"text"}]"#;
        assert_eq!(error.result_bytes, Some(parts.len() as i64));
    }
}

#[test]
fn session_facts_report_context_compaction_model_and_sidechain_link() {
    let main = fold(MAIN_FILE, MAIN).facts;
    assert_eq!(
        main,
        SessionFacts {
            session_id: Some("sess-0001".into()),
            parent_session_id: None,
            is_sidechain: false,
            cwd: Some("/work/demo".into()),
            first_event_ts: Some(ts(1)),
            first_event_ms: Some(ms(&ts(1))),
            last_event_ts: Some(ts(1920)),
            last_event_ms: Some(ms(&ts(1920))),
            records: 42,
            calls: 13,
            turns: 11,
            // msg_12 is a sidechain record in the main file: excluded.
            context_window: None,
            latest_context: Some(ContextSample {
                ts_ms: Some(ms(&ts(1865))),
                model: Some("claude-test-2".into()),
                stop_reason: Some("end_turn".into()),
                input: Some(1),
                cache_read: Some(64),
                cache_write: Some(1),
                total: Some(66),
            }),
            compactions: Some(CompactionCounts {
                manual: 1,
                auto: 0,
                unknown_trigger: 0,
            }),
            // msg_06 after its repeat: 4 + 30 + 25.
            last_compaction: Some(CompactionSample {
                ts_ms: Some(ms(&ts(270))),
                trigger: Some("manual".into()),
                pre_tokens: Some(180),
                post_tokens: Some(20),
                first_context_after: Some(59),
            }),
            last_model_switch: Some(ModelSwitch {
                ts_ms: Some(ms(&ts(1861))),
                requested_model: "fable".into(),
            }),
            seat_hint: Some("pij-seat-hint".into()),
            skipped: unisphere_core::prep::SessionSkips {
                malformed: 1,
                untimed: 1,
                bad_timestamp: 1,
            },
        }
    );
    let sub = fold(SUB_FILE, SUB).facts;
    assert_eq!(sub.session_id.as_deref(), Some("sess-0001"));
    assert_eq!(sub.parent_session_id.as_deref(), Some("sess-0001"));
    assert!(sub.is_sidechain);
    assert_eq!(sub.latest_context, None);
    assert_eq!(sub.compactions, Some(CompactionCounts::default()));
    assert_eq!((sub.calls, sub.turns), (2, 1));
}

#[test]
fn first_context_after_waits_for_the_first_main_chain_call() {
    // Stop right after the compaction boundary and the summary.
    let run = fold_with(
        MAIN_FILE,
        &MAIN.lines().take(25).collect::<Vec<_>>().join("\n"),
        &[],
        false,
        PrepOptions::default(),
    );
    let compaction = run.facts.last_compaction.unwrap();
    assert_eq!(compaction.first_context_after, None);
    // The first new call after the boundary before its repeat: 4 + 30 + 20.
    let text = MAIN.lines().take(26).collect::<Vec<_>>().join("\n");
    let run = fold_with(MAIN_FILE, &text, &[], false, PrepOptions::default());
    assert_eq!(
        run.facts.last_compaction.unwrap().first_context_after,
        Some(54)
    );
}

#[test]
fn content_appears_only_under_explicit_opt_in() {
    for (file, text) in [(MAIN_FILE, MAIN), (SUB_FILE, SUB)] {
        let run = fold(file, text);
        assert!(run.rows.triggers.iter().all(|t| t.content_head.is_none()));
        // No fixture text leaves the fold: every body says "placeholder".
        let emitted = format!("{:?}{:?}{}", run.rows, run.facts, run.checkpoint.fold);
        assert!(!emitted.contains("placeholder"), "{file}");
    }
    let opted = fold_with(
        MAIN_FILE,
        MAIN,
        &[],
        false,
        PrepOptions {
            include_content: true,
        },
    )
    .rows;
    let heads: Vec<_> = opted
        .triggers
        .iter()
        .map(|t| t.content_head.as_deref())
        .take(3)
        .collect();
    assert_eq!(
        heads,
        vec![
            Some("placeholder human prompt"),
            Some("placeholder peer body"),
            Some("placeholder peer body"),
        ]
    );
    // Opt-in changes only the content column.
    let mut stripped = opted.clone();
    for trigger in &mut stripped.triggers {
        trigger.content_head = None;
    }
    assert_eq!(stripped, fold(MAIN_FILE, MAIN).rows);
}

#[test]
fn foreign_checkpoints_and_snapshot_input_are_refused() {
    let fold = ClaudePrepFold;
    let meta = fold.describe(MAIN_FILE);
    let good = fold_with(MAIN_FILE, MAIN, &[], false, PrepOptions::default()).checkpoint;
    assert_eq!(good.format, PREP_CHECKPOINT_FORMAT);
    assert_eq!(good.policy, "claude-code/prep-v3");
    let refused = |checkpoint: PrepCheckpoint| {
        fold.open(&meta, "s", 0, Some(&checkpoint))
            .err()
            .map(|e| e.kind())
    };
    let mut older = good.clone();
    older.policy = "claude-code/prep-v2".into();
    assert_eq!(refused(older), Some(PipelineErrorKind::InvalidData));
    let mut newer = good.clone();
    newer.format += 1;
    assert_eq!(refused(newer), Some(PipelineErrorKind::InvalidData));
    let mut damaged = good.clone();
    damaged.fold["committed"] = serde_json::json!(["not hex"]);
    assert_eq!(refused(damaged), Some(PipelineErrorKind::InvalidData));
    let mut session = fold.open(&meta, "s", 0, None).unwrap();
    let snapshot = PrepInput::Snapshot(NativeSnapshot {
        source: SnapshotRef {
            path: "/fixture.json".into(),
            format: SnapshotFormat::JsonDocument,
            session_id: None,
        },
        revision: "r1".into(),
        records: Vec::new(),
    });
    assert_eq!(
        session
            .fold(&snapshot, PrepOptions::default())
            .err()
            .map(|e| e.kind()),
        Some(PipelineErrorKind::InvalidInput)
    );
}
