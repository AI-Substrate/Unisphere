//! PiPrepFold behaviour on synthetic, content-free fixtures: every body says
//! "placeholder", every id and path is invented.

use unisphere_adapter_pi::{DESCRIPTOR, PREP_POLICY_VERSION, PiPrepFold};
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineErrorKind, SnapshotFormat, SnapshotRef,
    prep::{
        CacheWriteBasis, CallSighting, CompactionCounts, CompactionSample, ContextSample,
        ModelSwitch, PREP_CHECKPOINT_FORMAT, PrepCallRow, PrepCheckpoint, PrepEventKind, PrepFold,
        PrepInput, PrepOptions, PrepRows, PrepSourceKind, PrepSourceMeta, SessionFacts,
        SessionSkips, ToolOutcome, ToolSighting, TurnOrigin,
    },
};

const MAIN_FILE: &str = "--Users-dev-demo--/2026-01-10T00-00-00-000Z_sess-0001.jsonl";
const MAIN: &str =
    include_str!("fixtures/prep/--Users-dev-demo--/2026-01-10T00-00-00-000Z_sess-0001.jsonl");

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

fn ts(seconds: u32) -> String {
    format!("2026-01-10T00:{:02}:{:02}.000Z", seconds / 60, seconds % 60)
}

fn ms(seconds: u32) -> i64 {
    1_768_003_200_000 + i64::from(seconds) * 1000
}

fn line_offset(text: &str, line: usize) -> u64 {
    text.lines().take(line).map(|l| l.len() as u64 + 1).sum()
}

struct Run {
    rows: PrepRows,
    facts: SessionFacts,
    checkpoint: PrepCheckpoint,
}

/// Fold `text` in batches ending at `splits`; with `resume`, every batch after
/// the first starts from a checkpoint serialised to JSON text and reopened.
fn fold_with(text: &str, splits: &[usize], resume: bool, options: PrepOptions) -> Run {
    let fold = PiPrepFold;
    let meta = fold.describe(MAIN_FILE);
    let source = format!("pi/default/{MAIN_FILE}");
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

#[test]
fn fold_identity_matches_the_catalogue_descriptor() {
    let fold = PiPrepFold;
    assert_eq!(fold.harness(), "pi");
    assert_eq!(fold.harness(), DESCRIPTOR.id);
    assert_eq!(fold.policy(), "pi/prep-v1");
    assert_eq!(PREP_POLICY_VERSION, "pi/prep-v1");
    assert_eq!(fold.kind(), PrepSourceKind::Append);
    assert_eq!(fold.pattern(), "*/*.jsonl");
    assert_eq!(fold.pattern(), DESCRIPTOR.locations[0].session_glob);
    let capabilities = DESCRIPTOR.capabilities;
    assert!(capabilities.cli_persisted_resume);
    assert_eq!(
        fold.describe(MAIN_FILE),
        PrepSourceMeta {
            is_sub: false,
            agent_id: None,
            project: Some("demo".into()),
        }
    );
    // The dialect has no subagent files.
    assert!(!fold.describe("--p--/nested/extra.jsonl").is_sub);
}

type CallExpectation = (
    &'static str,
    [Option<i64>; 5],
    i64,
    i64,
    i64,
    CacheWriteBasis,
    &'static str,
);

#[test]
fn assistant_messages_with_usage_are_calls_keyed_by_native_id() {
    use CacheWriteBasis::{Fallback1h, Split};
    let s = Some;
    // (entry id, [input, cw_1h, cw_5m, cache_read, output], gap_ms, turn_no,
    // call_in_turn, basis, stop reason)
    let expected: [CallExpectation; 7] = [
        // cacheWrite1h is part of the aggregate: 5 m is the rest.
        (
            "e04",
            [s(10), s(40), s(60), s(0), s(5)],
            -1,
            1,
            1,
            Split,
            "toolUse",
        ),
        // Aggregate only: the documented main-session fallback.
        (
            "e06",
            [s(3), s(50), s(0), s(110), s(40)],
            5_000,
            1,
            2,
            Fallback1h,
            "stop",
        ),
        (
            "e08",
            [s(2), s(0), s(0), s(170), s(3)],
            55_000,
            2,
            1,
            Split,
            "toolUse",
        ),
        // The zero-usage error response (e10) is not a call.
        (
            "e11",
            [s(1), s(0), s(0), s(180), s(2)],
            10_000,
            2,
            2,
            Fallback1h,
            "stop",
        ),
        (
            "e13",
            [s(5), s(0), s(0), s(200), s(1)],
            50_000,
            3,
            1,
            Fallback1h,
            "aborted",
        ),
        (
            "e17",
            [s(4), s(30), s(0), s(25), s(9)],
            140_000,
            4,
            1,
            Split,
            "stop",
        ),
        (
            "e20",
            [s(1), s(0), s(0), s(60), s(1)],
            40_000,
            5,
            1,
            Fallback1h,
            "stop",
        ),
    ];
    let rows = fold(MAIN).rows;
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
                c.stop_reason.clone().unwrap(),
            )
        })
        .collect();
    let expected: Vec<_> = expected
        .iter()
        .map(|&(id, tokens, gap, turn, call, basis, stop)| {
            (
                id.to_owned(),
                tokens,
                gap,
                turn,
                call,
                basis,
                stop.to_owned(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
    for call in &rows.calls {
        assert_eq!(call.sighting, CallSighting::First);
        // The dialect records no request id; the entry id is the address key.
        assert_eq!(call.request_id, None);
        assert_eq!(call.native_key, call.msg_id);
        assert!(!call.is_sidechain);
    }
    let last = rows.calls.last().unwrap();
    assert_eq!(last.model.as_deref(), Some("model-b"));
    assert_eq!(last.ts.as_deref(), Some(ts(305).as_str()));
    assert_eq!(last.ts_ms, Some(ms(305)));
    assert_eq!(last.native_offset, Some(line_offset(MAIN, 21)));
}

#[test]
fn a_repeated_native_id_merges_in_batch_and_is_an_update_across_batches() {
    let whole = fold(MAIN).rows;
    let merged: Vec<_> = whole
        .calls
        .iter()
        .filter(|c| c.msg_id.as_deref() == Some("e17"))
        .collect();
    assert_eq!(merged.len(), 1);
    assert_eq!((merged[0].records, merged[0].output), (2, Some(9)));
    assert_eq!(merged[0].native_offset, Some(line_offset(MAIN, 17)));
    // Split between the two records of e17 and resume from a checkpoint.
    let run = fold_with(MAIN, &[18], true, PrepOptions::default());
    let sightings: Vec<_> = run
        .rows
        .calls
        .iter()
        .filter(|c| c.msg_id.as_deref() == Some("e17"))
        .collect();
    assert_eq!(sightings.len(), 2);
    assert_eq!(
        (sightings[0].sighting, sightings[0].output),
        (CallSighting::First, Some(8))
    );
    let update = sightings[1];
    assert_eq!(
        (update.sighting, update.output, update.records),
        (CallSighting::Update, Some(9), 1)
    );
    assert_eq!(
        (update.gap_ms, update.turn_no, update.call_in_turn),
        (None, None, None)
    );
    assert_eq!(update.native_offset, Some(line_offset(MAIN, 18)));
    assert_eq!(run.facts.calls, 7);
}

#[test]
fn every_batch_split_and_checkpoint_resume_equals_one_fold() {
    let whole = fold(MAIN);
    let expected = canonical(&whole.rows);
    let n = records(MAIN).len();
    let mut plans: Vec<Vec<usize>> = (1..n).map(|k| vec![k]).collect();
    plans.push((1..n).collect());
    for splits in plans {
        for resume in [false, true] {
            let run = fold_with(MAIN, &splits, resume, PrepOptions::default());
            let label = format!("splits={splits:?} resume={resume}");
            assert_eq!(canonical(&run.rows), expected, "{label}");
            assert_eq!(run.facts, whole.facts, "{label}");
            assert_eq!(run.checkpoint, whole.checkpoint, "{label}");
        }
    }
}

#[test]
fn branch_context_follows_parent_ids_across_splits_and_resume() {
    // Up to the branch summary that re-parents onto e11, abandoning e12/e13.
    let text = MAIN.lines().take(15).collect::<Vec<_>>().join("\n");
    for splits in [vec![], vec![14], vec![13], vec![12, 14]] {
        let run = fold_with(&text, &splits, true, PrepOptions::default());
        // The latest call in native order is e13, but the branch's is e11.
        let latest = run.facts.latest_context.unwrap();
        assert_eq!(
            (latest.ts_ms, latest.total, latest.stop_reason.as_deref()),
            (Some(ms(75)), Some(181), Some("stop")),
            "splits={splits:?}"
        );
        let summary = run.rows.events.last().unwrap();
        assert_eq!(summary.subkind.as_deref(), Some("branch_summary"));
        assert_eq!(
            (summary.last_context, summary.gap_ms),
            (Some(181), Some(5_000))
        );
    }
    // The compaction on that branch reports the branch context, not e13's 206.
    let rows = fold(MAIN).rows;
    let compaction = rows
        .events
        .iter()
        .find(|e| e.subkind.as_deref() == Some("compaction"))
        .unwrap();
    assert_eq!(
        (compaction.last_context, compaction.gap_ms),
        (Some(181), Some(75_000))
    );
}

#[test]
fn a_parent_outside_the_window_leaves_the_branch_context_unknown() {
    let entry = |id: &str, parent: &str, second: u32| {
        format!(
            r#"{{"type":"custom","id":"{id}","parentId":"{parent}","timestamp":"{}","customType":"placeholder-state","data":{{}}}}"#,
            ts(second)
        )
    };
    let mut lines = vec![
        r#"{"type":"session","version":3,"id":"sess-w","timestamp":"2026-01-10T00:00:00.000Z","cwd":"/work/w"}"#.to_owned(),
        r#"{"type":"message","id":"u0","parentId":null,"timestamp":"2026-01-10T00:00:01.000Z","message":{"role":"user","content":"placeholder"}}"#.to_owned(),
        r#"{"type":"message","id":"a0","parentId":"u0","timestamp":"2026-01-10T00:00:02.000Z","message":{"role":"assistant","model":"m","stopReason":"stop","usage":{"input":1,"output":1,"cacheRead":2,"cacheWrite":0},"content":[]}}"#.to_owned(),
    ];
    let mut parent = "a0".to_owned();
    for index in 0..300 {
        let id = format!("c{index}");
        lines.push(entry(&id, &parent, 3));
        parent = id;
    }
    // Recent parents resolve; the first custom entry has left the window.
    lines.push(entry("near", "c250", 4));
    let near = fold(&lines.join("\n")).facts;
    assert_eq!(near.latest_context.and_then(|c| c.total), Some(3));
    lines.push(entry("far", "c0", 5));
    lines.push(
        r#"{"type":"compaction","id":"k","parentId":"far","timestamp":"2026-01-10T00:00:06.000Z","tokensBefore":9}"#
            .to_owned(),
    );
    let text = lines.join("\n");
    for splits in [vec![], vec![200], vec![303, 304]] {
        let run = fold_with(&text, &splits, true, PrepOptions::default());
        assert_eq!(run.facts.latest_context, None, "splits={splits:?}");
        let compaction = run.rows.events.last().unwrap();
        assert_eq!(
            (compaction.last_context, compaction.gap_ms),
            (None, Some(4_000))
        );
    }
}

#[test]
fn openers_carry_human_peer_and_task_origins() {
    let rows = fold(MAIN).rows;
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
                t.native_key.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        triggers,
        vec![
            (Human, None, None, 24, 1, Some("e03")),
            // A pij custom message: sender from the envelope, id from details.
            (
                Peer,
                Some("pij-alpha-one"),
                Some("ab12-cd34"),
                56,
                2,
                Some("e07")
            ),
            (Human, None, None, 28, 3, Some("e12")),
            // A pij envelope in user text: id from the envelope tag.
            (
                Peer,
                Some("pij-gamma-three"),
                Some("ef56-7890"),
                83,
                4,
                Some("e16")
            ),
            (TaskNotification, None, None, 27, 5, Some("e18")),
        ]
    );
    assert_eq!(rows.triggers[1].body_key, rows.triggers[3].body_key);
    let turns: Vec<_> = rows
        .turns
        .iter()
        .map(|t| {
            (
                t.turn_no,
                t.origin,
                t.sender.as_deref(),
                t.native_key.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        turns,
        vec![
            (1, Human, None, Some("e04")),
            (2, Peer, Some("pij-alpha-one"), Some("e08")),
            (3, Human, None, Some("e13")),
            (4, Peer, Some("pij-gamma-three"), Some("e17")),
            (5, TaskNotification, None, Some("e20")),
        ]
    );
    let peer = &rows.turns[1];
    assert_eq!(peer.pij_msg_id.as_deref(), Some("ab12-cd34"));
    assert_eq!(peer.opener_offset, Some(line_offset(MAIN, 7)));
    assert_eq!(peer.first_call_offset, Some(line_offset(MAIN, 8)));
    assert_eq!(peer.opener_ts_ms, Some(ms(60)));
    assert_eq!(peer.started_ts.as_deref(), Some(ts(65).as_str()));
}

#[test]
fn compaction_branch_summary_model_and_error_entries_are_typed_events() {
    let rows = fold(MAIN).rows;
    use PrepEventKind::*;
    let events: Vec<_> = rows
        .events
        .iter()
        .map(|e| {
            (
                e.kind,
                e.subkind.as_deref(),
                e.model.as_deref(),
                e.turn_no,
                e.native_key.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        events,
        vec![
            (ModelSwitch, None, Some("model-a"), 0, Some("e01")),
            (ApiError, None, None, 2, Some("e10")),
            (Compaction, Some("branch_summary"), None, 3, Some("e14")),
            (Compaction, Some("compaction"), None, 3, Some("e15")),
            (ModelSwitch, None, Some("model-b"), 4, Some("e19")),
        ]
    );
    // Native pre-compaction tokens only: no trigger, post tokens or duration.
    let compaction = &rows.events[3];
    assert_eq!(
        (
            compaction.trigger.as_deref(),
            compaction.pre_tokens,
            compaction.post_tokens,
            compaction.duration_ms
        ),
        (None, Some(183), None, None)
    );
}

#[test]
fn tool_calls_and_results_pair_across_batches_and_resume() {
    for splits in [vec![], vec![5], vec![9], (1..27).collect::<Vec<_>>()] {
        let rows = fold_with(MAIN, &splits, true, PrepOptions::default()).rows;
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
                    "call-1",
                    Some("bash"),
                    Some("shell"),
                    Some("e04"),
                    None,
                    None,
                    Some(1)
                ),
                // The dialect records no tool duration: null, not zero.
                (
                    Result,
                    "call-1",
                    Some("bash"),
                    Some("shell"),
                    Some("e04"),
                    Some(ToolOutcome::Ok),
                    None,
                    Some(1)
                ),
                (
                    Use,
                    "call-2",
                    Some("read"),
                    Some("file-read"),
                    Some("e08"),
                    None,
                    None,
                    Some(2)
                ),
                (
                    Use,
                    "call-3",
                    Some("find"),
                    None,
                    Some("e08"),
                    None,
                    None,
                    Some(2)
                ),
                (
                    Result,
                    "call-2",
                    Some("read"),
                    Some("file-read"),
                    Some("e08"),
                    Some(ToolOutcome::Error),
                    None,
                    Some(2)
                ),
            ],
            "splits={splits:?}"
        );
        let bash = &rows.tool_uses[0];
        let canonical_input = br#"{"command":"placeholder","timeout":30}"#;
        assert_eq!(bash.input_bytes, Some(canonical_input.len() as i64));
        assert_eq!(bash.input_hash.as_ref().map(String::len), Some(16));
        let result = &rows.tool_uses[1];
        assert_eq!(result.result_offset, Some(line_offset(MAIN, 5)));
        let parts = br#"[{"text":"placeholder output","type":"text"}]"#;
        assert_eq!(result.result_bytes, Some(parts.len() as i64));
    }
}

#[test]
fn session_facts_report_branch_context_compaction_model_and_parent() {
    assert_eq!(
        fold(MAIN).facts,
        SessionFacts {
            session_id: Some("sess-0001".into()),
            // The parent file's stem suffix, never its path.
            parent_session_id: Some("sess-0000".into()),
            is_sidechain: false,
            cwd: Some("/work/demo".into()),
            first_event_ts: Some(ts(0)),
            first_event_ms: Some(ms(0)),
            last_event_ts: Some(ts(320)),
            last_event_ms: Some(ms(320)),
            records: 24,
            calls: 7,
            turns: 5,
            latest_context: Some(ContextSample {
                ts_ms: Some(ms(305)),
                model: Some("model-b".into()),
                stop_reason: Some("stop".into()),
                input: Some(1),
                cache_read: Some(60),
                cache_write: Some(0),
                total: Some(61),
            }),
            compactions: Some(CompactionCounts {
                manual: 0,
                auto: 0,
                unknown_trigger: 1,
            }),
            // e17 after its repeat: 4 + 30 + 25.
            last_compaction: Some(CompactionSample {
                ts_ms: Some(ms(200)),
                trigger: None,
                pre_tokens: Some(183),
                post_tokens: None,
                first_context_after: Some(59),
            }),
            last_model_switch: Some(ModelSwitch {
                ts_ms: Some(ms(301)),
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
fn fields_the_dialect_does_not_record_are_null_never_zero() {
    let text = [
        r#"{"type":"message","id":"a1","parentId":null,"timestamp":"2026-01-10T00:00:01.000Z","message":{"role":"assistant","usage":{"output":3},"content":[{"type":"toolCall","id":"t1","name":"custom-tool"}]}}"#,
        r#"{"type":"message","id":"r1","parentId":"a1","timestamp":"2026-01-10T00:00:02.000Z","message":{"role":"toolResult","toolCallId":"t1"}}"#,
        r#"{"type":"message","id":"a2","parentId":"r1","timestamp":"2026-01-10T00:00:03.000Z","message":{"role":"assistant","model":"m","content":[]}}"#,
        // A 1 h part larger than the aggregate cannot be split.
        r#"{"type":"message","id":"a3","parentId":"a2","timestamp":"2026-01-10T00:00:04.000Z","message":{"role":"assistant","usage":{"input":1,"cacheWrite":2,"cacheWrite1h":5},"content":[]}}"#,
        r#"{"type":"compaction","id":"k1","parentId":"r1","timestamp":"2026-01-10T00:00:05.000Z"}"#,
    ]
    .join("\n");
    let run = fold(&text);
    // A message without usage is not a call.
    assert_eq!(run.rows.calls.len(), 2);
    let call = &run.rows.calls[0];
    assert_eq!(
        (
            call.model.as_deref(),
            call.stop_reason.as_deref(),
            call.input,
            call.cw_1h,
            call.cw_5m,
            call.cache_read,
            call.output,
            call.cache_write_basis,
        ),
        (
            None,
            None,
            None,
            None,
            None,
            None,
            Some(3),
            CacheWriteBasis::None
        )
    );
    let inconsistent = &run.rows.calls[1];
    assert_eq!(
        (
            inconsistent.cw_1h,
            inconsistent.cw_5m,
            inconsistent.cache_write_basis
        ),
        (Some(2), Some(0), CacheWriteBasis::Fallback1h)
    );
    // A call before any opener opens turn 0.
    assert_eq!(run.rows.turns[0].origin, TurnOrigin::Start);
    assert_eq!(run.rows.turns[0].opener_offset, None);
    let (use_, result) = (&run.rows.tool_uses[0], &run.rows.tool_uses[1]);
    assert_eq!(
        (use_.family.as_deref(), use_.input_hash.as_deref()),
        (None, None)
    );
    assert_eq!(
        (result.name.as_deref(), result.outcome, result.result_bytes),
        (Some("custom-tool"), Some(ToolOutcome::Unknown), None)
    );
    // The compaction re-parents onto r1, whose branch call records no context.
    let compaction = &run.rows.events[0];
    assert_eq!(
        (
            compaction.pre_tokens,
            compaction.post_tokens,
            compaction.last_context
        ),
        (None, None, None)
    );
    let facts = run.facts;
    assert_eq!(
        (facts.session_id, facts.parent_session_id, facts.cwd),
        (None, None, None)
    );
    let latest = facts.latest_context.unwrap();
    assert_eq!(
        (latest.input, latest.cache_write, latest.total),
        (None, None, None)
    );
}

#[test]
fn content_appears_only_under_explicit_opt_in() {
    let run = fold(MAIN);
    assert!(run.rows.triggers.iter().all(|t| t.content_head.is_none()));
    let emitted = format!("{:?}{:?}{}", run.rows, run.facts, run.checkpoint.fold);
    assert!(!emitted.contains("placeholder"));
    let opted = fold_with(
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
        .collect();
    assert_eq!(
        heads,
        vec![
            Some("placeholder human prompt"),
            Some("placeholder peer body"),
            Some("placeholder human prompt two"),
            Some("placeholder peer body"),
            Some("placeholder subagent result"),
        ]
    );
    // Opt-in changes only the content column.
    let mut stripped = opted.clone();
    for trigger in &mut stripped.triggers {
        trigger.content_head = None;
    }
    assert_eq!(stripped, run.rows);
}

#[test]
fn foreign_checkpoints_and_snapshot_input_are_refused() {
    let fold = PiPrepFold;
    let meta = fold.describe(MAIN_FILE);
    let good = fold_with(MAIN, &[], false, PrepOptions::default()).checkpoint;
    assert_eq!(good.format, PREP_CHECKPOINT_FORMAT);
    assert_eq!(good.policy, "pi/prep-v1");
    let refused = |checkpoint: PrepCheckpoint| {
        fold.open(&meta, "s", 0, Some(&checkpoint))
            .err()
            .map(|e| e.kind())
    };
    let mut other = good.clone();
    other.policy = "oh-my-pi/prep-v1".into();
    assert_eq!(refused(other), Some(PipelineErrorKind::InvalidData));
    let mut newer = good.clone();
    newer.format += 1;
    assert_eq!(refused(newer), Some(PipelineErrorKind::InvalidData));
    let mut damaged = good.clone();
    damaged.fold["committed"] = serde_json::json!(["not hex"]);
    assert_eq!(refused(damaged), Some(PipelineErrorKind::InvalidData));
    let mut dangling = good.clone();
    dangling.fold["calls"] = serde_json::json!({});
    assert_eq!(refused(dangling), Some(PipelineErrorKind::InvalidData));
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
