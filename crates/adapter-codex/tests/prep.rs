//! CodexPrepFold behaviour on synthetic, content-free rollout fixtures.

use unisphere_adapter_codex::{CodexPrepFold, DESCRIPTOR, PREP_POLICY_VERSION};
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineErrorKind, SnapshotFormat, SnapshotRef,
    prep::{
        CacheWriteBasis, CallSighting, CompactionCounts, CompactionSample, ContextSample,
        ModelSwitch, PREP_CHECKPOINT_FORMAT, PrepCallRow, PrepCheckpoint, PrepEventKind, PrepFold,
        PrepInput, PrepOptions, PrepRows, PrepSourceKind, PrepSourceMeta, PrepToolUseRow,
        SessionFacts, SessionSkips, ToolOutcome, ToolSighting, TurnOrigin,
    },
};

const MAIN_FILE: &str =
    "2026/01/10/rollout-2026-01-10T00-00-00-00000000-0000-4000-8000-000000000001.jsonl";
const MAIN: &str = include_str!(
    "fixtures/prep/2026/01/10/rollout-2026-01-10T00-00-00-00000000-0000-4000-8000-000000000001.jsonl"
);
const SUB_FILE: &str =
    "2026/01/10/rollout-2026-01-10T00-00-00-00000000-0000-4000-8000-000000000002.jsonl";
const SUB: &str = include_str!(
    "fixtures/prep/2026/01/10/rollout-2026-01-10T00-00-00-00000000-0000-4000-8000-000000000002.jsonl"
);
const LEGACY_FILE: &str =
    "2025/05/01/rollout-2025-05-01T00-00-00-00000000-0000-4000-8000-000000000003.jsonl";
const LEGACY: &str = include_str!(
    "fixtures/prep/2025/05/01/rollout-2025-05-01T00-00-00-00000000-0000-4000-8000-000000000003.jsonl"
);
/// The crate's marker fixture: every content field reads `SENSITIVE-*`.
const MARKED: &str = include_str!("../fixtures/rollout.jsonl");

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

fn fnv_hex(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

struct Run {
    rows: PrepRows,
    facts: SessionFacts,
    checkpoint: PrepCheckpoint,
}

/// Fold `text` in batches ending at `splits`; with `resume`, every batch after
/// the first starts from a checkpoint serialised to JSON text and reopened.
fn fold_with(file: &str, text: &str, splits: &[usize], resume: bool, options: PrepOptions) -> Run {
    let fold = CodexPrepFold;
    let meta = fold.describe(file);
    let source = format!("codex/default/{file}");
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

/// The canonical reader's call merge: one row per keyed (msg_id, request_id)
/// with per-field maxima and summed records; an id-less call is its own row.
fn canonical(rows: &PrepRows) -> PrepRows {
    let max = |a: Option<i64>, b: Option<i64>| match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let mut calls: Vec<PrepCallRow> = Vec::new();
    for call in &rows.calls {
        let keyed = call.msg_id.is_some() || call.request_id.is_some();
        match calls
            .iter_mut()
            .find(|c| keyed && c.msg_id == call.msg_id && c.request_id == call.request_id)
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
fn fold_identity_matches_the_catalogue_descriptor() {
    let fold = CodexPrepFold;
    assert_eq!(fold.harness(), "codex");
    assert_eq!(fold.harness(), DESCRIPTOR.id);
    assert_eq!(fold.policy(), PREP_POLICY_VERSION);
    assert_eq!(fold.kind(), PrepSourceKind::Append);
    assert_eq!(fold.pattern(), "????/??/??/rollout-*.jsonl");
    assert_eq!(fold.pattern(), DESCRIPTOR.locations[0].session_glob);
    assert_eq!(fold.describe(MAIN_FILE), PrepSourceMeta::default());
}

/// (native line, msg_id, [input, cw_1h, cw_5m, cache_read, output], basis,
/// records, model, gap_ms, turn_no, call_in_turn)
type CallExpectation = (
    usize,
    Option<&'static str>,
    [Option<i64>; 5],
    CacheWriteBasis,
    i64,
    &'static str,
    i64,
    i64,
    i64,
);

fn assert_calls(text: &str, calls: &[PrepCallRow], sidechain: bool, expected: &[CallExpectation]) {
    assert_eq!(calls.len(), expected.len(), "{calls:#?}");
    for (call, (line, msg_id, tokens, basis, records, model, gap, turn, nth)) in
        calls.iter().zip(expected)
    {
        let at = format!("line {line}");
        assert_eq!(call.native_offset, Some(line_offset(text, *line)), "{at}");
        assert_eq!(call.native_key, None, "{at}");
        assert_eq!(call.sighting, CallSighting::First, "{at}");
        assert_eq!(call.msg_id.as_deref(), *msg_id, "{at}");
        assert_eq!(call.request_id, None, "{at}");
        assert_eq!(
            [
                call.input,
                call.cw_1h,
                call.cw_5m,
                call.cache_read,
                call.output
            ],
            *tokens,
            "{at}"
        );
        assert_eq!(call.cache_write_basis, *basis, "{at}");
        assert_eq!(call.records, *records, "{at}");
        assert_eq!(call.model.as_deref(), Some(*model), "{at}");
        assert_eq!(call.stop_reason, None, "{at}: Codex records no stop reason");
        assert_eq!(call.is_sidechain, sidechain, "{at}");
        assert_eq!(call.gap_ms, Some(*gap), "{at}");
        assert_eq!(call.turn_no, Some(*turn), "{at}");
        assert_eq!(call.call_in_turn, Some(*nth), "{at}");
        assert!(call.ts.is_some() && call.ts_ms.is_some(), "{at}");
    }
}

#[test]
fn token_counts_become_one_call_per_response_with_disjoint_token_classes() {
    use CacheWriteBasis::None as NoCache;
    let s = Some;
    let run = fold(MAIN_FILE, MAIN);
    // input = input_tokens − cached_input_tokens; no cache-write field → null.
    // Re-emitted token_counts (unchanged cumulative total, lines 8 and 26) and
    // the rate-limit-only token_count (line 31) are not calls.
    assert_calls(
        MAIN,
        &run.rows.calls,
        false,
        &[
            (
                7,
                None,
                [s(60), None, None, s(40), s(10)],
                NoCache,
                1,
                "model-a",
                -1,
                1,
                1,
            ),
            (
                14,
                None,
                [s(50), None, None, s(150), s(20)],
                NoCache,
                1,
                "model-a",
                7_000,
                1,
                2,
            ),
            (
                23,
                None,
                [s(50), None, None, s(250), s(5)],
                NoCache,
                1,
                "model-b",
                13_000,
                2,
                1,
            ),
            (
                29,
                None,
                [s(50), None, None, s(0), s(7)],
                NoCache,
                1,
                "model-b",
                15_000,
                3,
                1,
            ),
            (
                34,
                None,
                [s(30), None, None, s(50), s(4)],
                NoCache,
                1,
                "model-b",
                5_000,
                4,
                1,
            ),
        ],
    );
    assert_eq!(run.rows.calls[0].ts.as_deref(), Some(ts(5).as_str()));
    assert_eq!(run.rows.calls[0].ts_ms, Some(ms(5)));
}

#[test]
fn usage_records_key_calls_by_response_id_and_absorb_their_token_counts() {
    use CacheWriteBasis::{Fallback5m, None as NoCache};
    let s = Some;
    let run = fold(SUB_FILE, SUB);
    // A subagent thread is a sidechain: the aggregate cache write is attributed
    // to 5 m. Each token_count repeating a usage record's counters (and its
    // re-emission) is the same call, merged into its row.
    assert_calls(
        SUB,
        &run.rows.calls,
        true,
        &[
            (
                5,
                Some("resp-1"),
                [s(300), s(0), s(100), s(600), s(50)],
                Fallback5m,
                3,
                "model-a",
                -1,
                1,
                1,
            ),
            (
                8,
                Some("resp-2"),
                [s(200), s(0), s(0), s(1000), s(30)],
                Fallback5m,
                2,
                "model-a",
                4_000,
                1,
                2,
            ),
            (
                12,
                Some("resp-3"),
                [s(100), s(0), s(0), s(1200), s(10)],
                Fallback5m,
                1,
                "model-a",
                3_000,
                1,
                3,
            ),
            (
                14,
                None,
                [s(100), None, None, s(1300), s(5)],
                NoCache,
                1,
                "model-a",
                10_000,
                2,
                1,
            ),
        ],
    );
}

#[test]
fn a_main_thread_cache_write_is_attributed_to_one_hour() {
    let text = [
        r#"{"timestamp":"2026-01-10T00:00:00Z","type":"session_meta","payload":{"id":"thread-x"}}"#,
        r#"{"timestamp":"2026-01-10T00:00:01Z","type":"token_usage_record","payload":{"response_id":"resp-x","usage":{"input_tokens":90,"cached_input_tokens":20,"cache_write_input_tokens":30,"output_tokens":5,"total_tokens":95}}}"#,
    ]
    .join("\n");
    let call = &fold(MAIN_FILE, &text).rows.calls[0];
    assert_eq!(
        [
            call.input,
            call.cw_1h,
            call.cw_5m,
            call.cache_read,
            call.output
        ],
        [Some(40), Some(30), Some(0), Some(20), Some(5)]
    );
    assert_eq!(call.cache_write_basis, CacheWriteBasis::Fallback1h);
    assert_eq!(call.model, None, "no turn_context before the call");
}

#[test]
fn turns_open_at_the_next_call_with_the_native_opener() {
    let run = fold(MAIN_FILE, MAIN);
    let turns: Vec<_> = run
        .rows
        .turns
        .iter()
        .map(|t| {
            (
                t.turn_no,
                t.origin,
                t.sender.as_deref(),
                t.pij_msg_id.as_deref(),
                t.native_offset,
                t.opener_offset,
                t.opener_ts_ms,
                t.opener_chars,
            )
        })
        .collect();
    let at = |line| Some(line_offset(MAIN, line));
    let peer_len =
        "[pij-rs from pij-quiet-heron] synthetic ask [pijMessageId:0a1b-2c] [/pij]".len() as i64;
    let peer_chars = Some(peer_len);
    assert_eq!(
        turns,
        [
            // task_started (line 1) is superseded by the user message (line 5).
            (
                1,
                TurnOrigin::Human,
                None,
                None,
                at(7),
                at(5),
                Some(ms(2)),
                Some(20)
            ),
            (
                2,
                TurnOrigin::Peer,
                Some("pij-quiet-heron"),
                Some("0a1b-2c"),
                at(23),
                at(22),
                Some(ms(21)),
                peer_chars
            ),
            // A task with no input (line 24) opens an `other` turn.
            (
                3,
                TurnOrigin::Other,
                None,
                None,
                at(29),
                at(24),
                Some(ms(30)),
                None
            ),
            (
                4,
                TurnOrigin::TaskNotification,
                Some("/root/worker"),
                None,
                at(34),
                at(33),
                Some(ms(43)),
                Some(16)
            ),
        ]
    );
    assert_eq!(
        run.rows.turns[0].started_ts.as_deref(),
        Some(ts(5).as_str())
    );
    assert_eq!(run.rows.turns[0].first_call_offset, at(7));
    assert_eq!(
        run.rows.turns[1].body_key.as_deref(),
        Some(fnv_hex(b"synthetic ask").as_str())
    );
    assert_eq!(run.rows.turns[2].body_key, None);

    let triggers: Vec<_> = run
        .rows
        .triggers
        .iter()
        .map(|t| {
            (
                t.native_offset,
                t.kind,
                t.sender.as_deref(),
                t.next_turn_no,
                t.chars,
            )
        })
        .collect();
    // Developer and injected user response items (lines 2, 4) are not triggers.
    assert_eq!(
        triggers,
        [
            (at(5), TurnOrigin::Human, None, 1, 20),
            (
                at(22),
                TurnOrigin::Peer,
                Some("pij-quiet-heron"),
                2,
                peer_len
            ),
            (
                at(33),
                TurnOrigin::TaskNotification,
                Some("/root/worker"),
                4,
                16
            ),
        ]
    );
    assert!(run.rows.triggers.iter().all(|t| t.content_head.is_none()));
}

#[test]
fn inter_agent_messages_open_subagent_turns_unless_marked_otherwise() {
    let run = fold(SUB_FILE, SUB);
    let at = |line| Some(line_offset(SUB, line));
    let triggers: Vec<_> = run
        .rows
        .triggers
        .iter()
        .map(|t| (t.native_offset, t.kind, t.sender.as_deref(), t.next_turn_no))
        .collect();
    assert_eq!(
        triggers,
        [
            (at(4), TurnOrigin::SubagentTask, Some("/root"), 1),
            // trigger_turn = false: recorded, but it opens nothing.
            (at(11), TurnOrigin::Peer, Some("/root/other"), 2),
            // A plain user message in a subagent thread is its task.
            (at(13), TurnOrigin::SubagentTask, None, 2),
        ]
    );
    let turns: Vec<_> = run
        .rows
        .turns
        .iter()
        .map(|t| (t.turn_no, t.origin, t.opener_offset))
        .collect();
    assert_eq!(
        turns,
        [
            (1, TurnOrigin::SubagentTask, at(4)),
            (2, TurnOrigin::SubagentTask, at(13)),
        ]
    );
}

#[test]
fn model_switches_compactions_aborts_and_errors_are_typed_events() {
    let run = fold(MAIN_FILE, MAIN);
    let events: Vec<_> = run
        .rows
        .events
        .iter()
        .map(|e| {
            (
                e.native_offset,
                e.kind,
                e.subkind.as_deref(),
                e.model.as_deref(),
                e.last_context,
                e.gap_ms,
                e.duration_ms,
                e.turn_no,
            )
        })
        .collect();
    let at = |line| Some(line_offset(MAIN, line));
    // The `context_compacted` marker (line 27) pairs the compaction and is not
    // counted again.
    assert_eq!(
        events,
        [
            (
                at(21),
                PrepEventKind::ModelSwitch,
                None,
                Some("model-b"),
                None,
                None,
                None,
                2
            ),
            (
                at(25),
                PrepEventKind::Compaction,
                None,
                None,
                Some(300),
                Some(6_000),
                None,
                3
            ),
            (
                at(28),
                PrepEventKind::SystemOther,
                Some("turn_aborted"),
                None,
                None,
                None,
                Some(1_234),
                3
            ),
            (
                at(30),
                PrepEventKind::ApiError,
                None,
                None,
                None,
                None,
                None,
                3
            ),
        ]
    );
    let compaction = &run.rows.events[1];
    // The `compacted` record carries neither the trigger nor token counts; the
    // later post-compaction count reaches only the session facts.
    assert_eq!(
        (
            compaction.trigger.as_ref(),
            compaction.pre_tokens,
            compaction.post_tokens
        ),
        (None, None, None)
    );
    assert_eq!(compaction.ts_ms, Some(ms(31)));
}

type ToolExpectation = (
    usize,
    ToolSighting,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Option<ToolOutcome>,
    Option<i64>,
    i64,
);

#[test]
fn tool_uses_pair_calls_outputs_and_native_outcomes() {
    let run = fold(MAIN_FILE, MAIN);
    use ToolOutcome::{Error, Ok as Fine};
    use ToolSighting::{Result as Res, Use};
    // (line, sighting, call id, name, family, outcome, duration_ms, turn_no)
    let expected: [ToolExpectation; 9] = [
        // The use precedes its response's usage: it belongs to the pending turn.
        (
            6,
            Use,
            "call-1",
            Some("exec_command"),
            Some("shell"),
            None,
            None,
            1,
        ),
        // exec_command_end (line 9) wins over the output header.
        (
            10,
            Res,
            "call-1",
            Some("exec_command"),
            Some("shell"),
            Some(Error),
            Some(1_500),
            1,
        ),
        (
            11,
            Use,
            "call-2",
            Some("apply_patch"),
            Some("file-write"),
            None,
            None,
            1,
        ),
        (
            13,
            Res,
            "call-2",
            Some("apply_patch"),
            Some("file-write"),
            Some(Fine),
            None,
            1,
        ),
        (
            15,
            Use,
            "call-3",
            Some("shell"),
            Some("shell"),
            None,
            None,
            1,
        ),
        // Structured output metadata: exit code 0 in 0.25 s.
        (
            16,
            Res,
            "call-3",
            Some("shell"),
            Some("shell"),
            Some(Fine),
            Some(250),
            1,
        ),
        (17, Use, "call-4", Some("update_plan"), None, None, None, 1),
        // No native outcome with the output: null, not guessed.
        (18, Res, "call-4", Some("update_plan"), None, None, None, 1),
        // An end event after its output is a second result sighting.
        (19, Res, "call-4", None, None, Some(Error), Some(2), 1),
    ];
    let rows = &run.rows.tool_uses;
    assert_eq!(rows.len(), expected.len(), "{rows:#?}");
    for (row, (line, sighting, id, name, family, outcome, duration, turn)) in
        rows.iter().zip(expected)
    {
        let at = format!("line {line}");
        assert_eq!(row.native_offset, Some(line_offset(MAIN, line)), "{at}");
        assert_eq!(row.sighting, sighting, "{at}");
        assert_eq!(row.tool_use_id.as_deref(), Some(id), "{at}");
        assert_eq!(row.name.as_deref(), name, "{at}");
        assert_eq!(row.family.as_deref(), family, "{at}");
        assert_eq!(row.outcome, outcome, "{at}");
        assert_eq!(row.duration_ms, duration, "{at}");
        assert_eq!(row.turn_no, Some(turn), "{at}");
        assert_eq!(row.call_msg_id, None, "{at}");
    }
    let use_of = |id: &str| -> &PrepToolUseRow {
        rows.iter()
            .find(|r| r.sighting == Use && r.tool_use_id.as_deref() == Some(id))
            .unwrap()
    };
    // JSON arguments are hashed canonically; free-form input by its bytes.
    let args = br#"{"cmd":"x","yield_time_ms":1}"#;
    assert_eq!(
        use_of("call-1").input_hash.as_deref(),
        Some(fnv_hex(args).as_str())
    );
    assert_eq!(use_of("call-1").input_bytes, Some(args.len() as i64));
    assert_eq!(
        use_of("call-2").input_hash.as_deref(),
        Some(fnv_hex(b"synthetic patch").as_str())
    );
    assert_eq!(use_of("call-4").input_bytes, Some(2));
    let result_bytes: Vec<_> = rows
        .iter()
        .filter(|r| r.sighting == Res)
        .map(|r| (r.result_offset, r.result_bytes))
        .collect();
    let at = |line| Some(line_offset(MAIN, line));
    assert_eq!(
        result_bytes,
        [
            (
                at(10),
                Some("Exit code: 3\nWall time: 9 seconds\nOutput:\nsynthetic".len() as i64)
            ),
            (
                at(13),
                Some(r#"[{"text":"synthetic","type":"input_text"}]"#.len() as i64)
            ),
            (
                at(16),
                Some(
                    r#"{"output":"x","metadata":{"exit_code":0,"duration_seconds":0.25}}"#.len()
                        as i64
                )
            ),
            (at(18), Some(12)),
            (None, None),
        ]
    );
}

#[test]
fn session_facts_report_identity_context_compaction_and_skips() {
    let run = fold(MAIN_FILE, MAIN);
    assert_eq!(
        run.facts,
        SessionFacts {
            // The latest token_count's native window, not an earlier one.
            context_window: Some(2000),
            session_id: Some("thread-main".into()),
            parent_session_id: Some("thread-origin".into()),
            is_sidechain: false,
            cwd: Some("/work/demo".into()),
            first_event_ts: Some(ts(0)),
            first_event_ms: Some(ms(0)),
            last_event_ts: Some(ts(45)),
            last_event_ms: Some(ms(45)),
            records: 35,
            calls: 5,
            turns: 4,
            latest_context: Some(ContextSample {
                ts_ms: Some(ms(45)),
                model: Some("model-b".into()),
                stop_reason: None,
                input: Some(30),
                cache_read: Some(50),
                cache_write: None,
                // Unrecorded writes are inside `input`: the native input_tokens.
                total: Some(80),
            }),
            compactions: Some(CompactionCounts {
                manual: 0,
                auto: 0,
                unknown_trigger: 1,
            }),
            last_compaction: Some(CompactionSample {
                ts_ms: Some(ms(31)),
                trigger: None,
                pre_tokens: None,
                // Codex's own count from the zero-usage token_count right after
                // the boundary, which is not a call.
                post_tokens: Some(45),
                first_context_after: Some(50),
            }),
            last_model_switch: Some(ModelSwitch {
                ts_ms: Some(ms(20)),
                requested_model: "model-b".into(),
            }),
            seat_hint: Some("pij-brave-otter".into()),
            skipped: SessionSkips {
                malformed: 1,
                untimed: 0,
                bad_timestamp: 1,
            },
        }
    );
}

#[test]
fn a_subagent_thread_is_a_sidechain_linked_to_its_parent() {
    let facts = fold(SUB_FILE, SUB).facts;
    assert_eq!(facts.session_id.as_deref(), Some("thread-sub"));
    assert_eq!(facts.parent_session_id.as_deref(), Some("thread-main"));
    assert!(facts.is_sidechain);
    assert_eq!((facts.calls, facts.turns), (4, 2));
    // Sidechain calls never become the main-chain context.
    assert_eq!(facts.latest_context, None);
    assert_eq!(facts.compactions, Some(CompactionCounts::default()));
}

#[test]
fn legacy_rollouts_fold_untimed_records_with_null_timestamps() {
    let run = fold(LEGACY_FILE, LEGACY);
    assert!(run.rows.calls.is_empty() && run.rows.turns.is_empty());
    let tools: Vec<_> = run
        .rows
        .tool_uses
        .iter()
        .map(|r| {
            (
                r.sighting,
                r.name.as_deref(),
                r.ts.as_ref(),
                r.ts_ms,
                r.outcome,
                r.duration_ms,
            )
        })
        .collect();
    assert_eq!(
        tools,
        [
            (ToolSighting::Use, Some("shell"), None, None, None, None),
            (
                ToolSighting::Result,
                Some("shell"),
                None,
                None,
                Some(ToolOutcome::Error),
                Some(100)
            ),
        ]
    );
    let facts = run.facts;
    assert_eq!(facts.session_id.as_deref(), Some("legacy-thread"));
    assert_eq!(
        facts.first_event_ts.as_deref(),
        Some("2025-05-01T00:00:00.000Z")
    );
    assert_eq!(facts.last_event_ts, facts.first_event_ts);
    assert_eq!(facts.records, 5);
    assert_eq!(facts.cwd, None);
    assert_eq!(facts.skipped, SessionSkips::default());
    assert_eq!(facts.context_window, None, "no window recorded: unknown");
}

#[test]
fn any_batch_split_or_resume_equals_one_uninterrupted_fold() {
    for (file, text) in [(MAIN_FILE, MAIN), (SUB_FILE, SUB), (LEGACY_FILE, LEGACY)] {
        let whole = fold(file, text);
        let lines = text.lines().count();
        for split in 1..lines {
            for resume in [false, true] {
                let run = fold_with(file, text, &[split], resume, PrepOptions::default());
                let at = format!("{file} split {split} resume {resume}");
                assert_eq!(canonical(&run.rows), canonical(&whole.rows), "{at}");
                assert_eq!(run.facts, whole.facts, "{at}");
                assert_eq!(run.checkpoint, whole.checkpoint, "{at}");
            }
        }
        let every: Vec<usize> = (1..lines).collect();
        let run = fold_with(file, text, &every, true, PrepOptions::default());
        assert_eq!(
            canonical(&run.rows),
            canonical(&whole.rows),
            "{file} per record"
        );
        assert_eq!(run.facts, whole.facts, "{file} per record");
    }
}

#[test]
fn a_repeat_committed_by_an_earlier_batch_is_an_update_sighting() {
    // Split between resp-1's usage record (line 5) and its token_count (line 6).
    let run = fold_with(SUB_FILE, SUB, &[6], true, PrepOptions::default());
    let resp_1: Vec<_> = run
        .rows
        .calls
        .iter()
        .filter(|c| c.msg_id.as_deref() == Some("resp-1"))
        .map(|c| (c.sighting, c.records, c.native_offset, c.gap_ms))
        .collect();
    assert_eq!(
        resp_1,
        [
            (CallSighting::First, 1, Some(line_offset(SUB, 5)), Some(-1)),
            (CallSighting::Update, 2, Some(line_offset(SUB, 6)), None),
        ]
    );
}

#[test]
fn checkpoints_of_another_policy_or_format_or_shape_are_refused() {
    let fold = CodexPrepFold;
    let meta = fold.describe(MAIN_FILE);
    let good = fold_with(MAIN_FILE, MAIN, &[], false, PrepOptions::default()).checkpoint;
    let refused = |checkpoint: PrepCheckpoint| {
        fold.open(&meta, "codex/default/x", 0, Some(&checkpoint))
            .err()
            .map(|e| e.kind())
    };
    let other_policy = PrepCheckpoint {
        policy: "codex/prep-v0".into(),
        ..good.clone()
    };
    let other_format = PrepCheckpoint {
        format: PREP_CHECKPOINT_FORMAT + 1,
        ..good.clone()
    };
    let mut other_shape = good.clone();
    other_shape.fold["turn_no"] = serde_json::json!("one");
    for checkpoint in [other_policy, other_format, other_shape] {
        assert_eq!(refused(checkpoint), Some(PipelineErrorKind::InvalidData));
    }
    assert_eq!(refused(good), None);
}

#[test]
fn snapshot_input_is_refused() {
    let fold = CodexPrepFold;
    let mut session = fold
        .open(&PrepSourceMeta::default(), "codex/default/x", 0, None)
        .unwrap();
    let snapshot = PrepInput::Snapshot(NativeSnapshot {
        source: SnapshotRef {
            path: "/x".into(),
            format: SnapshotFormat::JsonDocument,
            session_id: None,
        },
        revision: "r".into(),
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

#[test]
fn content_leaves_the_fold_only_under_explicit_opt_in() {
    // Every content-bearing field of the marker fixture reads SENSITIVE-*; only
    // the header's cwd is metadata.
    let run = fold("2026/01/02/rollout-x.jsonl", MARKED);
    assert!(!run.rows.tool_uses.is_empty());
    let rows = format!("{:?}", run.rows);
    assert!(!rows.contains("SENSITIVE"), "{rows}");
    let facts = SessionFacts {
        cwd: None,
        ..run.facts
    };
    let facts = format!("{facts:?}");
    assert!(!facts.contains("SENSITIVE"), "{facts}");
    let checkpoint = run.checkpoint.fold.to_string();
    assert!(
        !checkpoint
            .replace("/SENSITIVE-CWD", "")
            .contains("SENSITIVE"),
        "{checkpoint}"
    );

    let opted = fold_with(
        MAIN_FILE,
        MAIN,
        &[],
        false,
        PrepOptions {
            include_content: true,
        },
    );
    let heads: Vec<_> = opted
        .rows
        .triggers
        .iter()
        .map(|t| t.content_head.as_deref())
        .collect();
    assert_eq!(
        heads,
        [
            Some("synthetic prompt one"),
            Some("synthetic ask"),
            Some("synthetic report")
        ]
    );
}
