//! VsCodeCopilotPrepFold behaviour on synthetic, content-free fixtures.
//!
//! `sess-prep.jsonl` is a native mutation journal whose reduction is the
//! document `sess-prep.json`, through obsolete drafts, a truncated request and
//! late token/state updates. Every text value is a `PRIVATE-MARKER` sentinel.

use std::path::PathBuf;

use unisphere_adapter_vscode_copilot::{
    DESCRIPTOR, DOCUMENT_PREP_POLICY_VERSION, JOURNAL_PREP_POLICY_VERSION, VsCodeCopilotPrepFold,
};
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineErrorKind, SnapshotFormat, SnapshotRecord, SnapshotRef,
    prep::{
        CacheWriteBasis, CallSighting, ContextSample, PREP_CHECKPOINT_FORMAT, PrepCheckpoint,
        PrepFold, PrepFoldSession, PrepInput, PrepOptions, PrepRows, PrepSourceKind,
        PrepSourceMeta, SessionFacts, SessionSkips, ToolOutcome, ToolSighting, TurnOrigin,
    },
};

const DIR: &str = "ws-0001/chatSessions";
const DOCUMENT: &str = include_str!("fixtures/prep/ws-0001/chatSessions/sess-prep.json");
const JOURNAL: &str = include_str!("fixtures/prep/ws-0001/chatSessions/sess-prep.jsonl");
const LEGACY: &str = include_str!("fixtures/prep/ws-0001/chatSessions/sess-legacy.json");
const SOURCE: &str = "vscode-copilot/default/ws-0001/chatSessions/sess-prep";
const MARKER: &str = "PRIVATE-MARKER";

fn snapshot(format: SnapshotFormat, text: &str, revision: &str) -> NativeSnapshot {
    let records = match format {
        SnapshotFormat::JsonJournal => text
            .lines()
            .enumerate()
            .map(|(index, line)| SnapshotRecord {
                key: format!("journal:{index}"),
                bytes: line.as_bytes().to_vec(),
            })
            .collect(),
        _ => vec![SnapshotRecord {
            key: "document".into(),
            bytes: text.as_bytes().to_vec(),
        }],
    };
    NativeSnapshot {
        source: SnapshotRef {
            path: PathBuf::from("/synthetic/workspaceStorage/ws-0001/chatSessions/sess-prep"),
            format,
            session_id: None,
        },
        revision: revision.into(),
        records,
    }
}

fn document(text: &str) -> PrepInput {
    PrepInput::Snapshot(snapshot(SnapshotFormat::JsonDocument, text, "rev-1"))
}

fn journal(text: &str) -> PrepInput {
    PrepInput::Snapshot(snapshot(SnapshotFormat::JsonJournal, text, "rev-1"))
}

fn open(fold: VsCodeCopilotPrepFold) -> Box<dyn PrepFoldSession> {
    let meta = fold.describe(&format!("{DIR}/sess-prep.json"));
    fold.open(&meta, SOURCE, 3, None).unwrap()
}

fn fold_once(
    fold: VsCodeCopilotPrepFold,
    input: &PrepInput,
    options: PrepOptions,
) -> (PrepRows, SessionFacts, PrepCheckpoint) {
    let mut session = open(fold);
    let rows = session.fold(input, options).unwrap();
    (rows, session.facts(), session.checkpoint())
}

fn ts(ms: i64) -> Option<String> {
    // Every fixture instant is on 2023-11-14 between 22:15 and 22:16 UTC.
    let offset = ms - 1_700_000_100_000;
    assert!((0..60_000).contains(&offset));
    Some(format!(
        "2023-11-14T22:15:{:02}.{:03}Z",
        offset / 1_000,
        offset % 1_000
    ))
}

#[test]
fn fold_identity_matches_the_catalogue_descriptor() {
    let globs: Vec<(&str, &str)> = DESCRIPTOR
        .locations
        .iter()
        .map(|location| (location.storage_format, location.session_glob))
        .collect();
    for (fold, format, policy) in [
        (
            VsCodeCopilotPrepFold::Document,
            "json_document",
            "vscode-copilot/document-prep-v1",
        ),
        (
            VsCodeCopilotPrepFold::Journal,
            "json_journal",
            "vscode-copilot/journal-prep-v1",
        ),
    ] {
        assert_eq!(fold.harness(), DESCRIPTOR.id);
        assert_eq!(fold.harness(), "vscode-copilot");
        assert_eq!(fold.policy(), policy);
        assert_eq!(fold.kind(), PrepSourceKind::Snapshot);
        assert!(globs.contains(&(format, fold.pattern())), "{format}");
        assert_eq!(
            fold.describe(&format!("{DIR}/sess-prep.json")),
            PrepSourceMeta {
                is_sub: false,
                agent_id: None,
                project: Some("ws-0001".into()),
            }
        );
    }
    assert_eq!(
        DOCUMENT_PREP_POLICY_VERSION,
        "vscode-copilot/document-prep-v1"
    );
    assert_eq!(
        JOURNAL_PREP_POLICY_VERSION,
        "vscode-copilot/journal-prep-v1"
    );
}

#[test]
fn document_folds_requests_into_calls_turns_triggers_and_tool_uses() {
    let (rows, facts, _) = fold_once(
        VsCodeCopilotPrepFold::Document,
        &document(DOCUMENT),
        PrepOptions::default(),
    );
    for key in rows
        .calls
        .iter()
        .map(|r| (&r.source, r.generation, r.native_offset, &r.native_key))
        .chain(
            rows.turns
                .iter()
                .map(|r| (&r.source, r.generation, r.native_offset, &r.native_key)),
        )
        .chain(
            rows.triggers
                .iter()
                .map(|r| (&r.source, r.generation, r.native_offset, &r.native_key)),
        )
        .chain(
            rows.tool_uses
                .iter()
                .map(|r| (&r.source, r.generation, r.native_offset, &r.native_key)),
        )
    {
        assert_eq!((key.0.as_str(), key.1, key.2), (SOURCE, 3, None));
        assert!(
            key.3
                .as_deref()
                .is_some_and(|k| k.starts_with("/requests/"))
        );
    }
    // The dialect has no compaction, recap, limit or model-switch marker.
    assert!(rows.events.is_empty());

    let triggers: Vec<_> = rows
        .triggers
        .iter()
        .map(|t| {
            (
                t.native_key.as_deref().unwrap(),
                t.ts.clone(),
                t.ts_ms,
                t.kind,
                t.chars,
                t.next_turn_no,
            )
        })
        .collect();
    assert_eq!(
        triggers,
        [
            (
                "/requests/0/message",
                ts(1_700_000_101_000),
                Some(1_700_000_101_000),
                TurnOrigin::Human,
                18,
                1
            ),
            (
                "/requests/1/message",
                ts(1_700_000_110_000),
                Some(1_700_000_110_000),
                TurnOrigin::Other,
                18,
                2
            ),
            // Unanswered: its turn is never opened.
            (
                "/requests/2/message",
                ts(1_700_000_120_000),
                Some(1_700_000_120_000),
                TurnOrigin::Human,
                20,
                3
            ),
            // `/requests/3` is not an object; the invalid timestamp stays null.
            ("/requests/4/message", None, None, TurnOrigin::Human, 19, 3),
        ]
    );
    for trigger in &rows.triggers {
        assert!(trigger.sender.is_none() && trigger.pij_msg_id.is_none());
        assert!(trigger.content_head.is_none());
        assert_eq!(trigger.body_key.as_ref().map(String::len), Some(16));
    }

    let turns: Vec<_> = rows
        .turns
        .iter()
        .map(|t| {
            (
                t.turn_no,
                t.native_key.as_deref().unwrap(),
                t.started_ts.clone(),
                t.started_ts_ms,
                t.origin,
                t.opener_ts_ms,
                t.opener_chars,
            )
        })
        .collect();
    assert_eq!(
        turns,
        [
            (
                1,
                "/requests/0/response",
                ts(1_700_000_102_000),
                Some(1_700_000_102_000),
                TurnOrigin::Human,
                Some(1_700_000_101_000),
                Some(18)
            ),
            (
                2,
                "/requests/1/response",
                ts(1_700_000_112_500),
                Some(1_700_000_112_500),
                TurnOrigin::Other,
                Some(1_700_000_110_000),
                Some(18)
            ),
            (
                3,
                "/requests/4/response",
                None,
                None,
                TurnOrigin::Human,
                None,
                Some(19)
            ),
        ]
    );
    for (turn, trigger) in
        rows.turns
            .iter()
            .zip([&rows.triggers[0], &rows.triggers[1], &rows.triggers[3]])
    {
        assert_eq!(turn.body_key, trigger.body_key);
        assert!(turn.first_call_offset.is_none() && turn.opener_offset.is_none());
        assert!(turn.sender.is_none() && turn.pij_msg_id.is_none());
    }

    type Call<'a> = (
        Option<&'a str>,
        Option<&'a str>,
        Option<String>,
        Option<&'a str>,
        Option<&'a str>,
        [Option<i64>; 2],
        Option<i64>,
        Option<i64>,
    );
    let calls: Vec<Call<'_>> = rows
        .calls
        .iter()
        .map(|c| {
            assert_eq!(c.sighting, CallSighting::First);
            // VS Code records no per-call cache counters.
            assert_eq!((c.cw_1h, c.cw_5m, c.cache_read), (None, None, None));
            assert_eq!(c.cache_write_basis, CacheWriteBasis::None);
            assert!(!c.is_sidechain);
            assert_eq!((c.call_in_turn, c.records), (Some(1), 1));
            (
                c.msg_id.as_deref(),
                c.request_id.as_deref(),
                c.ts.clone(),
                c.model.as_deref(),
                c.stop_reason.as_deref(),
                [c.input, c.output],
                c.gap_ms,
                c.turn_no,
            )
        })
        .collect();
    assert_eq!(
        calls,
        [
            (
                Some("resp-1"),
                Some("req-1"),
                ts(1_700_000_102_000),
                Some("synthetic-model-a"),
                Some("complete"),
                [Some(1200), Some(80)],
                Some(-1),
                Some(1)
            ),
            // Whole-turn modelTotals of request 0 are never folded into a call;
            // request 1 records no prompt/completion counters.
            (
                Some("resp-2"),
                Some("req-2"),
                ts(1_700_000_112_500),
                Some("synthetic-model-b"),
                Some("cancelled"),
                [None, None],
                Some(10_500),
                Some(2)
            ),
            // No valid timestamps: untimed, with an unknown gap, never zero.
            (
                Some("resp-5"),
                Some("req-5"),
                None,
                None,
                None,
                [None, None],
                None,
                Some(3)
            ),
        ]
    );

    let tools: Vec<_> = rows
        .tool_uses
        .iter()
        .map(|t| {
            assert_eq!((t.ts.as_deref(), t.ts_ms), (None, None));
            assert_eq!((t.input_hash.as_deref(), t.input_bytes), (None, None));
            assert_eq!(
                (t.result_offset, t.result_bytes, t.duration_ms),
                (None, None, None)
            );
            (
                t.sighting,
                t.native_key.as_deref().unwrap(),
                t.tool_use_id.as_deref().unwrap(),
                t.call_msg_id.as_deref().unwrap(),
                t.name.as_deref().unwrap(),
                t.family.as_deref(),
                t.outcome,
                t.turn_no,
            )
        })
        .collect();
    use ToolSighting::{Result as Res, Use};
    assert_eq!(
        tools,
        [
            (
                Use,
                "/requests/0/response/1",
                "tool-1",
                "resp-1",
                "read_file",
                Some("file-read"),
                None,
                Some(1)
            ),
            (
                Res,
                "/requests/0/response/1",
                "tool-1",
                "resp-1",
                "read_file",
                Some("file-read"),
                Some(ToolOutcome::Ok),
                Some(1)
            ),
            (
                Use,
                "/requests/0/response/2",
                "tool-2",
                "resp-1",
                "run_in_terminal",
                Some("shell"),
                None,
                Some(1)
            ),
            (
                Res,
                "/requests/0/response/2",
                "tool-2",
                "resp-1",
                "run_in_terminal",
                Some("shell"),
                Some(ToolOutcome::Error),
                Some(1)
            ),
            // Incomplete: no result sighting; unknown names have no family.
            (
                Use,
                "/requests/0/response/3",
                "tool-3",
                "resp-1",
                "synthetic_custom_tool",
                None,
                None,
                Some(1)
            ),
            (
                Use,
                "/requests/1/response/0",
                "tool-4",
                "resp-2",
                "grep_search",
                Some("search"),
                None,
                Some(2)
            ),
            // Complete without a native isError: outcome unknown, not ok.
            (
                Res,
                "/requests/1/response/0",
                "tool-4",
                "resp-2",
                "grep_search",
                Some("search"),
                Some(ToolOutcome::Unknown),
                Some(2)
            ),
        ]
    );

    assert_eq!(
        facts,
        SessionFacts {
            context_window: None,
            session_id: Some("sess-prep-0001".into()),
            parent_session_id: None,
            is_sidechain: false,
            cwd: None,
            first_event_ts: ts(1_700_000_100_000),
            first_event_ms: Some(1_700_000_100_000),
            last_event_ts: ts(1_700_000_120_000),
            last_event_ms: Some(1_700_000_120_000),
            records: 5,
            calls: 3,
            turns: 3,
            // The latest call recorded nothing; nothing is carried over.
            latest_context: Some(ContextSample {
                ts_ms: None,
                model: None,
                stop_reason: None,
                input: None,
                cache_read: None,
                cache_write: None,
                total: None,
            }),
            compactions: None,
            last_compaction: None,
            last_model_switch: None,
            seat_hint: None,
            skipped: SessionSkips {
                malformed: 1,
                untimed: 0,
                bad_timestamp: 1,
            },
        }
    );
}

#[test]
fn journal_reduces_to_the_equivalent_document() {
    let (doc_rows, doc_facts, _) = fold_once(
        VsCodeCopilotPrepFold::Document,
        &document(DOCUMENT),
        PrepOptions::default(),
    );
    let (rows, facts, checkpoint) = fold_once(
        VsCodeCopilotPrepFold::Journal,
        &journal(JOURNAL),
        PrepOptions::default(),
    );
    assert_eq!(rows, doc_rows);
    assert_eq!(facts, doc_facts);
    assert_eq!(checkpoint.policy, JOURNAL_PREP_POLICY_VERSION);
    // The truncated request and obsolete drafts left no trace.
    assert!(!format!("{rows:?}").contains("req-removed"));
}

#[test]
fn content_is_absent_unless_explicitly_requested() {
    for (fold, input) in [
        (VsCodeCopilotPrepFold::Document, document(DOCUMENT)),
        (VsCodeCopilotPrepFold::Journal, journal(JOURNAL)),
    ] {
        let (rows, facts, checkpoint) = fold_once(fold, &input, PrepOptions::default());
        let text = format!(
            "{rows:?}{facts:?}{}",
            serde_json::to_string(&checkpoint).unwrap()
        );
        assert!(!text.contains(MARKER), "{fold:?}");

        let (opted, opted_facts, _) = fold_once(
            fold,
            &input,
            PrepOptions {
                include_content: true,
            },
        );
        let heads: Vec<_> = opted
            .triggers
            .iter()
            .map(|t| t.content_head.as_deref())
            .collect();
        assert_eq!(
            heads,
            [
                Some("PRIVATE-MARKER-ONE"),
                Some("PRIVATE-MARKER-TWO"),
                Some("PRIVATE-MARKER-THREE"),
                Some("PRIVATE-MARKER-FIVE"),
            ]
        );
        // Opt-in adds the trigger head column only.
        let mut stripped = opted.clone();
        for trigger in &mut stripped.triggers {
            trigger.content_head = None;
        }
        assert_eq!(stripped, rows);
        assert_eq!(opted_facts, facts);
    }
}

#[test]
fn content_head_is_bounded_to_200_characters() {
    let long = "é".repeat(250);
    let text = format!(
        r#"{{"version":3,"sessionId":"s","requests":[{{"requestId":"r","message":{{"text":"{long}"}},"response":null}}]}}"#
    );
    let (rows, _, _) = fold_once(
        VsCodeCopilotPrepFold::Document,
        &document(&text),
        PrepOptions {
            include_content: true,
        },
    );
    let trigger = &rows.triggers[0];
    assert_eq!(trigger.chars, 250);
    assert_eq!(
        trigger.content_head.as_deref(),
        Some("é".repeat(200).as_str())
    );
    assert!(rows.calls.is_empty() && rows.turns.is_empty());
}

#[test]
fn legacy_document_keeps_unrecorded_fields_null() {
    let (rows, facts, _) = fold_once(
        VsCodeCopilotPrepFold::Document,
        &document(LEGACY),
        PrepOptions::default(),
    );
    let [call] = rows.calls.as_slice() else {
        panic!("one call")
    };
    assert_eq!(call.msg_id.as_deref(), Some("resp-legacy"));
    assert_eq!((call.ts.as_deref(), call.ts_ms), (None, None));
    assert_eq!(
        (call.model.as_deref(), call.stop_reason.as_deref()),
        (None, None)
    );
    assert_eq!([call.input, call.output], [None, None]);
    assert_eq!(call.gap_ms, Some(-1));
    assert_eq!(
        rows.triggers[0].ts.as_deref(),
        Some("2023-11-14T22:16:41.000Z")
    );
    assert_eq!(rows.turns[0].started_ts_ms, None);
    assert!(rows.tool_uses.is_empty());
    assert_eq!(facts.session_id.as_deref(), Some("sess-prep-legacy"));
    assert_eq!(facts.first_event_ms, Some(1_700_000_200_000));
    assert_eq!(facts.last_event_ms, Some(1_700_000_201_000));
    assert_eq!(facts.compactions, None);
}

#[test]
fn checkpoint_resumes_the_folded_revision_only() {
    for (fold, input) in [
        (VsCodeCopilotPrepFold::Document, document(DOCUMENT)),
        (VsCodeCopilotPrepFold::Journal, journal(JOURNAL)),
    ] {
        let meta = fold.describe(&format!("{DIR}/sess-prep.json"));
        let fresh = fold.open(&meta, SOURCE, 3, None).unwrap();
        assert_eq!(fresh.facts(), SessionFacts::default());
        let (_, facts, checkpoint) = fold_once(fold, &input, PrepOptions::default());
        assert_eq!(checkpoint.format, PREP_CHECKPOINT_FORMAT);
        assert_eq!(checkpoint.policy, fold.policy());

        let json = serde_json::to_string(&checkpoint).unwrap();
        let saved: PrepCheckpoint = serde_json::from_str(&json).unwrap();
        let mut resumed = fold.open(&meta, SOURCE, 3, Some(&saved)).unwrap();
        assert_eq!(resumed.facts(), facts);
        assert_eq!(resumed.checkpoint(), checkpoint);
        // The same revision again adds nothing; a new revision is a new generation.
        assert!(
            resumed
                .fold(&input, PrepOptions::default())
                .unwrap()
                .is_empty()
        );
        let PrepInput::Snapshot(mut changed) = input.clone() else {
            unreachable!()
        };
        changed.revision = "rev-2".into();
        let error = resumed
            .fold(&PrepInput::Snapshot(changed), PrepOptions::default())
            .unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
        assert_eq!(resumed.checkpoint(), checkpoint);
    }
}

#[test]
fn foreign_or_malformed_checkpoints_are_refused() {
    let (_, _, document_checkpoint) = fold_once(
        VsCodeCopilotPrepFold::Document,
        &document(DOCUMENT),
        PrepOptions::default(),
    );
    let journal_fold = VsCodeCopilotPrepFold::Journal;
    let meta = journal_fold.describe(&format!("{DIR}/sess-prep.jsonl"));
    let refused = |checkpoint: &PrepCheckpoint| {
        journal_fold
            .open(&meta, SOURCE, 0, Some(checkpoint))
            .err()
            .map(|error| error.kind())
    };
    // Another representation's policy is never reinterpreted.
    assert_eq!(
        refused(&document_checkpoint),
        Some(PipelineErrorKind::InvalidData)
    );
    let mut valid = document_checkpoint.clone();
    valid.policy = JOURNAL_PREP_POLICY_VERSION.into();
    assert_eq!(refused(&valid), None);
    for broken in [
        PrepCheckpoint {
            format: PREP_CHECKPOINT_FORMAT + 1,
            ..valid.clone()
        },
        PrepCheckpoint {
            fold: serde_json::json!({"revision": "rev-1"}),
            ..valid.clone()
        },
        PrepCheckpoint {
            fold: serde_json::json!({"revision": 7, "session": valid.fold["session"]}),
            ..valid.clone()
        },
    ] {
        assert_eq!(refused(&broken), Some(PipelineErrorKind::InvalidData));
    }
}

#[test]
fn unusable_input_is_refused_without_changing_state() {
    let fold = VsCodeCopilotPrepFold::Document;
    let mut session = open(fold);
    let before = session.checkpoint();
    let mut selected = snapshot(SnapshotFormat::JsonDocument, DOCUMENT, "rev-1");
    selected.source.session_id = Some("another-session".into());
    let mut two_records = snapshot(SnapshotFormat::JsonDocument, DOCUMENT, "rev-1");
    two_records.records.push(two_records.records[0].clone());
    let cases = [
        // Append records and the other representation belong to other folds.
        (
            PrepInput::Records(vec![NativeRecord {
                offset: 0,
                bytes: b"{}".to_vec(),
            }]),
            PipelineErrorKind::InvalidInput,
        ),
        (journal(JOURNAL), PipelineErrorKind::InvalidInput),
        (
            PrepInput::Snapshot(selected),
            PipelineErrorKind::InvalidInput,
        ),
        (
            PrepInput::Snapshot(two_records),
            PipelineErrorKind::InvalidData,
        ),
        (
            PrepInput::Snapshot(snapshot(SnapshotFormat::JsonDocument, DOCUMENT, "")),
            PipelineErrorKind::InvalidData,
        ),
        (document("{\"requests\": ["), PipelineErrorKind::InvalidData),
        (document("[]"), PipelineErrorKind::InvalidData),
        (
            document(r#"{"version":4,"sessionId":"s","requests":[]}"#),
            PipelineErrorKind::InvalidData,
        ),
        (
            document(r#"{"version":3,"sessionId":"s","requests":{}}"#),
            PipelineErrorKind::InvalidData,
        ),
    ];
    for (input, kind) in cases {
        let error = session.fold(&input, PrepOptions::default()).unwrap_err();
        assert_eq!(error.kind(), kind, "{input:?}");
        assert_eq!(session.checkpoint(), before);
        assert_eq!(session.facts(), SessionFacts::default());
    }
    // A malformed journal fails in the journal fold the same way.
    let mut journal_session = open(VsCodeCopilotPrepFold::Journal);
    let error = journal_session
        .fold(
            &journal("{\"kind\":1,\"k\":[\"x\"],\"v\":1}"),
            PrepOptions::default(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
    // After all refusals the document still folds completely.
    assert_eq!(
        session
            .fold(&document(DOCUMENT), PrepOptions::default())
            .unwrap()
            .calls
            .len(),
        3
    );
}
