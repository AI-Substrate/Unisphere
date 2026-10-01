//! OmpPrepFold behaviour on synthetic, content-free fixtures: every body says
//! "placeholder", every id and path is invented.

use unisphere_adapter_omp::{DESCRIPTOR, OmpPrepFold, PREP_POLICY_VERSION};
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
const SUB_FILE: &str = "--Users-dev-demo--/2026-01-10T00-00-00-000Z_sess-0001/Scout.jsonl";
const SUB: &str =
    include_str!("fixtures/prep/--Users-dev-demo--/2026-01-10T00-00-00-000Z_sess-0001/Scout.jsonl");

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
fn fold_with(file: &str, text: &str, splits: &[usize], resume: bool, options: PrepOptions) -> Run {
    let fold = OmpPrepFold;
    let meta = fold.describe(file);
    let source = format!("oh-my-pi/default/{file}");
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

#[test]
fn fold_identity_matches_the_catalogue_descriptor() {
    let fold = OmpPrepFold;
    assert_eq!(fold.harness(), "oh-my-pi");
    assert_eq!(fold.harness(), DESCRIPTOR.id);
    assert_eq!(fold.policy(), "oh-my-pi/prep-v2");
    assert_eq!(PREP_POLICY_VERSION, "oh-my-pi/prep-v2");
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
    assert_eq!(
        fold.describe(SUB_FILE),
        PrepSourceMeta {
            is_sub: true,
            agent_id: Some("Scout".into()),
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
    &'static str,
);

#[test]
fn assistant_messages_with_usage_are_calls_keyed_by_native_id() {
    use CacheWriteBasis::{None as NoTtl, Split};
    let s = Some;
    // (entry id, [input, cw_1h, cw_5m, cache_read, output], gap_ms, turn_no,
    // call_in_turn, basis, stop reason)
    let main: [CallExpectation; 10] = [
        // cttl records only 5 m: 1 h is the aggregate minus it.
        (
            "e05",
            [s(10), s(0), s(100), s(0), s(5)],
            -1,
            1,
            1,
            Split,
            "toolUse",
        ),
        (
            "e08",
            [s(3), s(30), s(20), s(110), s(40)],
            5_000,
            1,
            2,
            Split,
            "stop",
        ),
        // Aggregate only: the harness does not distinguish the TTL.
        (
            "e10",
            [s(2), None, None, s(170), s(3)],
            55_000,
            2,
            1,
            NoTtl,
            "toolUse",
        ),
        // The zero-usage error response (e12) is not a call.
        (
            "e13",
            [s(1), None, None, s(180), s(2)],
            10_000,
            2,
            2,
            NoTtl,
            "stop",
        ),
        // An abort that recorded tokens is a call.
        (
            "e15",
            [s(5), None, None, s(200), s(1)],
            50_000,
            3,
            1,
            NoTtl,
            "aborted",
        ),
        (
            "e19",
            [s(4), s(0), s(30), s(25), s(9)],
            140_000,
            4,
            1,
            Split,
            "stop",
        ),
        (
            "e23",
            [s(1), None, None, s(60), s(1)],
            40_000,
            5,
            1,
            NoTtl,
            "stop",
        ),
        // The zero-usage abort (e29) is not a call.
        (
            "e30",
            [s(6), None, None, s(64), s(2)],
            65_000,
            6,
            1,
            NoTtl,
            "stop",
        ),
        (
            "e32",
            [s(1), None, None, s(84), s(1)],
            15_000,
            7,
            1,
            NoTtl,
            "stop",
        ),
        (
            "e34",
            [s(2), None, None, s(85), s(1)],
            10_000,
            8,
            1,
            NoTtl,
            "stop",
        ),
    ];
    let sub: [CallExpectation; 2] = [
        (
            "s02",
            [s(3), None, None, s(0), s(4)],
            -1,
            1,
            1,
            NoTtl,
            "toolUse",
        ),
        (
            "s04",
            [s(3), s(0), s(7), s(30), s(4)],
            5_000,
            1,
            2,
            Split,
            "stop",
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
        assert_eq!(actual, expected, "{file}");
        for call in &rows.calls {
            assert_eq!(call.sighting, CallSighting::First);
            // The dialect records no request id; the entry id is the address key.
            assert_eq!(call.request_id, None);
            assert_eq!(call.native_key, call.msg_id);
            assert_eq!(call.is_sidechain, file == SUB_FILE);
        }
    }
    let rows = fold(MAIN_FILE, MAIN).rows;
    // The model is provider-qualified, as the harness's model_change names it.
    assert_eq!(rows.calls[0].model.as_deref(), Some("anthropic/model-a"));
    let last = rows.calls.last().unwrap();
    assert_eq!(last.model.as_deref(), Some("github-copilot/model-d"));
    assert_eq!(last.ts.as_deref(), Some(ts(395).as_str()));
    assert_eq!(last.ts_ms, Some(ms(395)));
    assert_eq!(last.native_offset, Some(line_offset(MAIN, 36)));
}

#[test]
fn an_abort_that_recorded_no_tokens_keeps_the_previous_context() {
    // Up to and including the zero-usage abort e29.
    let text = MAIN.lines().take(32).collect::<Vec<_>>().join("\n");
    for splits in [vec![], vec![31], vec![30, 31]] {
        let run = fold_with(MAIN_FILE, &text, &splits, true, PrepOptions::default());
        assert!(
            run.rows
                .calls
                .iter()
                .all(|c| c.msg_id.as_deref() != Some("e29"))
        );
        assert_eq!(run.facts.calls, 7, "splits={splits:?}");
        let latest = run.facts.latest_context.unwrap();
        assert_eq!(
            (latest.ts_ms, latest.total),
            (Some(ms(305)), Some(61)),
            "splits={splits:?}"
        );
    }
    // The next call, parented on the abort, carries its own context.
    let latest = fold(
        MAIN_FILE,
        &MAIN.lines().take(33).collect::<Vec<_>>().join("\n"),
    )
    .facts
    .latest_context
    .unwrap();
    assert_eq!((latest.ts_ms, latest.total), (Some(ms(370)), Some(82)));
}

#[test]
fn a_repeated_native_id_merges_in_batch_and_is_an_update_across_batches() {
    let whole = fold(MAIN_FILE, MAIN).rows;
    let merged: Vec<_> = whole
        .calls
        .iter()
        .filter(|c| c.msg_id.as_deref() == Some("e19"))
        .collect();
    assert_eq!(merged.len(), 1);
    assert_eq!((merged[0].records, merged[0].output), (2, Some(9)));
    assert_eq!(merged[0].native_offset, Some(line_offset(MAIN, 19)));
    // Split between the two records of e19 and resume from a checkpoint.
    let run = fold_with(MAIN_FILE, MAIN, &[20], true, PrepOptions::default());
    let sightings: Vec<_> = run
        .rows
        .calls
        .iter()
        .filter(|c| c.msg_id.as_deref() == Some("e19"))
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
    assert_eq!(update.native_offset, Some(line_offset(MAIN, 20)));
    assert_eq!(run.facts.calls, 10);
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
fn branch_context_follows_parent_ids_across_splits_and_resume() {
    // Up to the branch summary that re-parents onto e13, abandoning e14/e15.
    let text = MAIN.lines().take(17).collect::<Vec<_>>().join("\n");
    for splits in [vec![], vec![16], vec![15], vec![14, 16]] {
        let run = fold_with(MAIN_FILE, &text, &splits, true, PrepOptions::default());
        // The latest call in native order is e15, but the branch's is e13.
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
    // The compaction on that branch reports the branch context, not e15's 206.
    let rows = fold(MAIN_FILE, MAIN).rows;
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
            r#"{{"type":"custom","id":"{id}","parentId":"{parent}","timestamp":"{}","customType":"tool_execution_start","data":{{}}}}"#,
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
    let near = fold(MAIN_FILE, &lines.join("\n")).facts;
    assert_eq!(near.latest_context.and_then(|c| c.total), Some(3));
    lines.push(entry("far", "c0", 5));
    lines.push(
        r#"{"type":"compaction","id":"k","parentId":"far","timestamp":"2026-01-10T00:00:06.000Z","tokensBefore":9}"#
            .to_owned(),
    );
    let text = lines.join("\n");
    for splits in [vec![], vec![200], vec![303, 304]] {
        let run = fold_with(MAIN_FILE, &text, &splits, true, PrepOptions::default());
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
                t.native_key.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        triggers,
        vec![
            (Human, None, None, 24, 1, Some("e04")),
            // A pij custom message: sender from the envelope, id from details.
            (
                Peer,
                Some("pij-alpha-one"),
                Some("ab12-cd34"),
                56,
                2,
                Some("e09")
            ),
            (Human, None, None, 28, 3, Some("e14")),
            // A pij envelope in user text: id from the envelope tag.
            (
                Peer,
                Some("pij-gamma-three"),
                Some("ef56-7890"),
                83,
                4,
                Some("e18")
            ),
            (TaskNotification, None, None, 24, 5, Some("e20")),
            // IRC: the native sender, no Pij message id.
            (Peer, Some("pij-delta-four"), None, 20, 6, Some("e28")),
            (TaskNotification, None, None, 25, 7, Some("e31")),
            // A user-attributed skill prompt.
            (Human, None, None, 24, 8, Some("e33")),
        ]
    );
    // Both peer bodies normalise to the same payload.
    assert_eq!(rows.triggers[1].body_key, rows.triggers[3].body_key);
    assert_ne!(rows.triggers[0].body_key, rows.triggers[1].body_key);
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
            (1, Human, None, Some("e05")),
            (2, Peer, Some("pij-alpha-one"), Some("e10")),
            (3, Human, None, Some("e15")),
            (4, Peer, Some("pij-gamma-three"), Some("e19")),
            (5, TaskNotification, None, Some("e23")),
            (6, Peer, Some("pij-delta-four"), Some("e30")),
            (7, TaskNotification, None, Some("e32")),
            (8, Human, None, Some("e34")),
        ]
    );
    let peer = &rows.turns[1];
    assert_eq!(peer.pij_msg_id.as_deref(), Some("ab12-cd34"));
    assert_eq!(peer.opener_offset, Some(line_offset(MAIN, 9)));
    assert_eq!(peer.first_call_offset, Some(line_offset(MAIN, 10)));
    assert_eq!(peer.opener_ts_ms, Some(ms(60)));
    assert_eq!(peer.started_ts.as_deref(), Some(ts(65).as_str()));
    let sub = fold(SUB_FILE, SUB).rows;
    assert_eq!(sub.turns.len(), 1);
    assert_eq!(
        (sub.turns[0].turn_no, sub.turns[0].origin),
        (1, SubagentTask)
    );
}

#[test]
fn compaction_branch_summary_model_and_error_entries_are_typed_events() {
    let rows = fold(MAIN_FILE, MAIN).rows;
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
            (ModelSwitch, None, Some("model-a"), 0, Some("e02")),
            (ApiError, Some("529"), None, 2, Some("e12")),
            (Compaction, Some("branch_summary"), None, 3, Some("e16")),
            (Compaction, Some("compaction"), None, 3, Some("e17")),
            (
                ModelSwitch,
                Some("default"),
                Some("model-b"),
                4,
                Some("e21")
            ),
            (ModelSwitch, Some("smol"), Some("model-c"), 4, Some("e22")),
        ]
    );
    let compaction = &rows.events[3];
    // The dialect records native pre/post tokens but no trigger or duration.
    assert_eq!(
        (
            compaction.trigger.as_deref(),
            compaction.pre_tokens,
            compaction.post_tokens,
            compaction.duration_ms
        ),
        (None, Some(183), Some(20), None)
    );
    let summary = &rows.events[2];
    assert_eq!((summary.pre_tokens, summary.post_tokens), (None, None));
    assert_eq!(summary.ts.as_deref(), Some(ts(130).as_str()));
}

#[test]
fn tool_calls_and_results_pair_across_batches_and_resume() {
    for splits in [vec![], vec![6], vec![11], (1..30).collect::<Vec<_>>()] {
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
                    "call-1",
                    Some("bash"),
                    Some("shell"),
                    Some("e05"),
                    None,
                    None,
                    Some(1)
                ),
                (
                    Result,
                    "call-1",
                    Some("bash"),
                    Some("shell"),
                    Some("e05"),
                    Some(ToolOutcome::Ok),
                    Some(250),
                    Some(1)
                ),
                (
                    Use,
                    "call-2",
                    Some("read"),
                    Some("file-read"),
                    Some("e10"),
                    None,
                    None,
                    Some(2)
                ),
                (
                    Use,
                    "call-3",
                    Some("lsp"),
                    None,
                    Some("e10"),
                    None,
                    None,
                    Some(2)
                ),
                // No native duration: null, not zero.
                (
                    Result,
                    "call-2",
                    Some("read"),
                    Some("file-read"),
                    Some("e10"),
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
        assert_eq!((bash.result_offset, bash.result_bytes), (None, None));
        let result = &rows.tool_uses[1];
        assert_eq!(result.result_offset, Some(line_offset(MAIN, 7)));
        let parts = br#"[{"text":"placeholder output","type":"text"}]"#;
        assert_eq!(result.result_bytes, Some(parts.len() as i64));
        assert_eq!(
            (result.input_hash.as_deref(), result.input_bytes),
            (None, None)
        );
    }
}

#[test]
fn session_facts_report_branch_context_compaction_model_and_subagent_link() {
    let main = fold(MAIN_FILE, MAIN).facts;
    assert_eq!(
        main,
        SessionFacts {
            context_window: None,
            cache_ttl_seconds: None,
            cache_expires_ms: None,
            session_id: Some("sess-0001".into()),
            parent_session_id: None,
            is_sidechain: false,
            cwd: Some("/work/demo".into()),
            first_event_ts: Some(ts(0)),
            first_event_ms: Some(ms(0)),
            last_event_ts: Some(ts(395)),
            last_event_ms: Some(ms(395)),
            records: 33,
            calls: 10,
            turns: 8,
            // Without cttl the aggregate cache write still counts: 2 + 85 + 3.
            latest_context: Some(ContextSample {
                ts_ms: Some(ms(395)),
                model: Some("github-copilot/model-d".into()),
                stop_reason: Some("stop".into()),
                input: Some(2),
                cache_read: Some(85),
                cache_write: Some(3),
                total: Some(90),
            }),
            compactions: Some(CompactionCounts {
                manual: 0,
                auto: 0,
                unknown_trigger: 1,
            }),
            // e19 after its repeat: 4 + 30 + 25.
            last_compaction: Some(CompactionSample {
                ts_ms: Some(ms(200)),
                trigger: None,
                pre_tokens: Some(183),
                post_tokens: Some(20),
                first_context_after: Some(59),
            }),
            // The role-specific model-c selection is not the session's model.
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
    let sub = fold(SUB_FILE, SUB).facts;
    assert_eq!(sub.session_id.as_deref(), Some("sub-0001"));
    // The parent session id is the parent file's stem suffix.
    assert_eq!(sub.parent_session_id.as_deref(), Some("sess-0001"));
    assert!(sub.is_sidechain);
    assert_eq!(sub.latest_context, None);
    assert_eq!(sub.compactions, Some(CompactionCounts::default()));
    assert_eq!((sub.calls, sub.turns), (2, 1));
}

#[test]
fn fields_the_dialect_does_not_record_are_null_never_zero() {
    let text = [
        r#"{"type":"message","id":"a1","parentId":null,"timestamp":"2026-01-10T00:00:01.000Z","message":{"role":"assistant","usage":{"output":3},"content":[{"type":"toolCall","id":"t1","name":"custom-tool"}]}}"#,
        r#"{"type":"message","id":"r1","parentId":"a1","timestamp":"2026-01-10T00:00:02.000Z","message":{"role":"toolResult","toolCallId":"t1"}}"#,
        r#"{"type":"message","id":"a2","parentId":"r1","timestamp":"2026-01-10T00:00:03.000Z","message":{"role":"assistant","model":"m","content":[]}}"#,
        r#"{"type":"compaction","id":"k1","parentId":"a2","timestamp":"2026-01-10T00:00:04.000Z"}"#,
    ]
    .join("\n");
    let run = fold(MAIN_FILE, &text);
    // A message without usage is not a call.
    assert_eq!(run.rows.calls.len(), 1);
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
    assert_eq!((facts.session_id, facts.cwd), (None, None));
    let latest = facts.latest_context.unwrap();
    assert_eq!(
        (latest.input, latest.cache_write, latest.total),
        (None, None, None)
    );
}

#[test]
fn content_appears_only_under_explicit_opt_in() {
    for (file, text) in [(MAIN_FILE, MAIN), (SUB_FILE, SUB)] {
        let run = fold(file, text);
        assert!(run.rows.triggers.iter().all(|t| t.content_head.is_none()));
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
        .collect();
    assert_eq!(
        heads,
        vec![
            Some("placeholder human prompt"),
            Some("placeholder peer body"),
            Some("placeholder human prompt two"),
            Some("placeholder peer body"),
            Some("placeholder async result"),
            Some("placeholder irc body"),
            Some("placeholder launch notice"),
            Some("placeholder skill prompt"),
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
    let fold = OmpPrepFold;
    let meta = fold.describe(MAIN_FILE);
    let good = fold_with(MAIN_FILE, MAIN, &[], false, PrepOptions::default()).checkpoint;
    assert_eq!(good.format, PREP_CHECKPOINT_FORMAT);
    assert_eq!(good.policy, "oh-my-pi/prep-v2");
    let refused = |checkpoint: PrepCheckpoint| {
        fold.open(&meta, "s", 0, Some(&checkpoint))
            .err()
            .map(|e| e.kind())
    };
    let mut other = good.clone();
    other.policy = "pi/prep-v1".into();
    assert_eq!(refused(other), Some(PipelineErrorKind::InvalidData));
    let mut newer = good.clone();
    newer.format += 1;
    assert_eq!(refused(newer), Some(PipelineErrorKind::InvalidData));
    let mut damaged = good.clone();
    damaged.fold["committed"] = serde_json::json!(["not hex"]);
    assert_eq!(refused(damaged), Some(PipelineErrorKind::InvalidData));
    // A branch context naming a call the checkpoint does not carry.
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
