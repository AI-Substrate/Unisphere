//! Cursor prep folds on synthetic, content-free fixtures: every fixture text is
//! the marker `SYNTHETIC-TEXT`, which must never leave a fold without opt-in.

use serde_json::Value;
use unisphere_adapter_cursor::{
    CursorIdePrepFold, CursorTranscriptPrepFold, DESCRIPTOR, IDE_DESCRIPTOR, IDE_PREP_POLICY,
    TRANSCRIPT_PREP_POLICY,
};
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineErrorKind, SnapshotFormat, SnapshotRecord, SnapshotRef,
    prep::{
        CacheWriteBasis, CallSighting, ContextSample, ModelSwitch, PREP_CHECKPOINT_FORMAT,
        PrepCheckpoint, PrepEventKind, PrepFold, PrepInput, PrepOptions, PrepRows, PrepSourceKind,
        SessionFacts, SessionSkips, ToolOutcome, ToolSighting, TurnOrigin,
    },
};

const TRANSCRIPT_FILE: &str = "proj-demo/agent-transcripts/sess-0001/sess-0001.jsonl";
const TRANSCRIPT: &str =
    include_str!("fixtures/prep/proj-demo/agent-transcripts/sess-0001/sess-0001.jsonl");
const IDE: &str = include_str!("fixtures/prep/ide.json");
const MARKER: &str = "SYNTHETIC-TEXT";

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

fn offset(line: usize) -> u64 {
    TRANSCRIPT
        .lines()
        .take(line)
        .map(|l| l.len() as u64 + 1)
        .sum()
}

fn fnv_hex(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

fn ms(ts: &str) -> i64 {
    let parsed =
        time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339).unwrap();
    (parsed.unix_timestamp_nanos() / 1_000_000) as i64
}

struct Run {
    rows: PrepRows,
    facts: SessionFacts,
    checkpoint: PrepCheckpoint,
}

/// Fold the transcript in batches ending at `splits`; with `resume`, every
/// batch after the first starts from a checkpoint round-tripped through JSON.
fn transcript(splits: &[usize], resume: bool, options: PrepOptions) -> Run {
    let fold = CursorTranscriptPrepFold;
    let meta = fold.describe(TRANSCRIPT_FILE);
    let source = format!("cursor-transcript/default/{TRANSCRIPT_FILE}");
    let all = records(TRANSCRIPT);
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

fn snapshot(rows: Vec<SnapshotRecord>) -> NativeSnapshot {
    NativeSnapshot {
        source: SnapshotRef {
            path: "/synthetic/state.vscdb".into(),
            format: SnapshotFormat::SqliteKeyValue {
                table: "cursorDiskKV".into(),
            },
            session_id: None,
        },
        revision: "rev-1".into(),
        records: rows,
    }
}

fn ide_records() -> Vec<SnapshotRecord> {
    let rows: Vec<Value> = serde_json::from_str(IDE).unwrap();
    rows.into_iter()
        .map(|row| SnapshotRecord {
            key: row["key"].as_str().unwrap().to_owned(),
            bytes: serde_json::to_vec(&row["value"]).unwrap(),
        })
        .collect()
}

fn ide(snapshot: &NativeSnapshot, options: PrepOptions) -> Run {
    let fold = CursorIdePrepFold;
    let meta = fold.describe("state.vscdb");
    let mut session = fold
        .open(&meta, "cursor-ide/default/state.vscdb", 3, None)
        .unwrap();
    let rows = session
        .fold(&PrepInput::Snapshot(snapshot.clone()), options)
        .unwrap();
    Run {
        rows,
        facts: session.facts(),
        checkpoint: session.checkpoint(),
    }
}

fn leaks(run: &Run) -> bool {
    format!("{:?}{:?}{}", run.rows, run.facts, run.checkpoint.fold).contains(MARKER)
}

#[test]
fn folds_bind_the_catalogue_descriptors_and_representations() {
    let transcript = CursorTranscriptPrepFold;
    assert_eq!(transcript.harness(), "cursor-transcript");
    assert_eq!(transcript.harness(), DESCRIPTOR.id);
    assert_eq!(transcript.kind(), PrepSourceKind::Append);
    assert_eq!(transcript.pattern(), "*/agent-transcripts/**/*.jsonl");
    assert_eq!(transcript.policy(), TRANSCRIPT_PREP_POLICY);
    let ide = CursorIdePrepFold;
    assert_eq!(ide.harness(), "cursor-ide");
    assert_eq!(ide.harness(), IDE_DESCRIPTOR.id);
    assert_eq!(ide.kind(), PrepSourceKind::Snapshot);
    assert_eq!(ide.pattern(), "state.vscdb");
    assert_eq!(ide.policy(), IDE_PREP_POLICY);
    assert_ne!(TRANSCRIPT_PREP_POLICY, IDE_PREP_POLICY);
    assert_eq!(
        transcript.describe(TRANSCRIPT_FILE).project.as_deref(),
        Some("proj-demo")
    );
}

#[test]
fn transcript_rows_are_offset_keyed_with_explicit_nulls() {
    let run = transcript(&[], false, PrepOptions::default());
    let rows = &run.rows;
    let source = format!("cursor-transcript/default/{TRANSCRIPT_FILE}");

    // One call per assistant record; nothing the writer does not record.
    let calls: Vec<_> = rows
        .calls
        .iter()
        .map(|c| (c.native_offset, c.turn_no, c.call_in_turn))
        .collect();
    assert_eq!(
        calls,
        [
            (Some(offset(1)), Some(0), Some(1)),
            (Some(offset(3)), Some(1), Some(1)),
            (Some(offset(4)), Some(1), Some(2)),
            (Some(offset(8)), Some(2), Some(1)),
        ]
    );
    for call in &rows.calls {
        assert_eq!(call.source, source);
        assert_eq!(call.generation, 0);
        assert_eq!(call.sighting, CallSighting::First);
        assert_eq!(call.cache_write_basis, CacheWriteBasis::None);
        assert_eq!(call.records, 1);
        assert!(!call.is_sidechain);
        assert_eq!(
            (&call.native_key, &call.msg_id, &call.request_id, &call.ts),
            (&None, &None, &None, &None)
        );
        assert_eq!((&call.model, &call.stop_reason), (&None, &None));
        assert_eq!(
            [
                call.ts_ms,
                call.input,
                call.cw_1h,
                call.cw_5m,
                call.cache_read,
                call.output,
                call.gap_ms
            ],
            [None; 7]
        );
    }

    let body = fnv_hex("SYNTHETIC-TEXT first prompt");
    let triggers: Vec<_> = rows
        .triggers
        .iter()
        .map(|t| {
            (
                t.native_offset,
                t.kind,
                t.chars,
                t.body_key.clone(),
                t.next_turn_no,
            )
        })
        .collect();
    let first_chars = "SYNTHETIC-TEXT   first\nprompt".chars().count() as i64;
    assert_eq!(
        triggers,
        [
            (
                Some(offset(2)),
                TurnOrigin::Human,
                first_chars,
                Some(body.clone()),
                1
            ),
            (Some(offset(7)), TurnOrigin::Other, 0, None, 2),
        ]
    );
    assert!(rows.triggers.iter().all(|t| t.ts.is_none()
        && t.ts_ms.is_none()
        && t.native_key.is_none()
        && t.content_head.is_none()));

    let turns: Vec<_> = rows
        .turns
        .iter()
        .map(|t| {
            (
                t.turn_no,
                t.origin,
                t.native_offset,
                t.first_call_offset,
                t.opener_offset,
                t.opener_chars,
                t.body_key.clone(),
            )
        })
        .collect();
    assert_eq!(
        turns,
        [
            (
                0,
                TurnOrigin::Start,
                Some(offset(1)),
                Some(offset(1)),
                None,
                None,
                None
            ),
            (
                1,
                TurnOrigin::Human,
                Some(offset(3)),
                Some(offset(3)),
                Some(offset(2)),
                Some(first_chars),
                Some(body)
            ),
            (
                2,
                TurnOrigin::Other,
                Some(offset(8)),
                Some(offset(8)),
                Some(offset(7)),
                Some(0),
                None
            ),
        ]
    );
    assert!(
        rows.turns
            .iter()
            .all(|t| t.started_ts_ms.is_none() && t.opener_ts_ms.is_none())
    );

    // Tool uses where structured; the dialect records no ids or results.
    let tools: Vec<_> = rows
        .tool_uses
        .iter()
        .map(|t| {
            (
                t.native_offset,
                t.sighting,
                t.name.as_deref(),
                t.family.as_deref(),
                t.turn_no,
            )
        })
        .collect();
    assert_eq!(
        tools,
        [
            (
                Some(offset(3)),
                ToolSighting::Use,
                Some("read_file"),
                Some("file-read"),
                Some(1)
            ),
            (
                Some(offset(4)),
                ToolSighting::Use,
                Some("Shell"),
                None,
                Some(1)
            ),
        ]
    );
    for tool in &rows.tool_uses {
        assert_eq!((&tool.tool_use_id, &tool.call_msg_id), (&None, &None));
        assert_eq!((tool.ts_ms, tool.duration_ms), (None, None));
        assert_eq!(tool.outcome, None);
        assert_eq!(tool.input_hash.as_ref().map(String::len), Some(16));
    }
    // Input hashes depend on the value, not its key order.
    let read_input = r#"{"lines":[1,2],"path":"SYNTHETIC-TEXT"}"#;
    assert_eq!(rows.tool_uses[0].input_hash, Some(fnv_hex(read_input)));
    assert_eq!(rows.tool_uses[0].input_bytes, Some(read_input.len() as i64));

    let events: Vec<_> = rows
        .events
        .iter()
        .map(|e| (e.native_offset, e.kind, e.subkind.as_deref(), e.turn_no))
        .collect();
    assert_eq!(
        events,
        [
            (
                Some(offset(0)),
                PrepEventKind::SystemOther,
                Some("metadata"),
                0
            ),
            (
                Some(offset(5)),
                PrepEventKind::SystemOther,
                Some("turn_ended:success"),
                1
            ),
            (
                Some(offset(9)),
                PrepEventKind::SystemOther,
                Some("turn_ended:error"),
                2
            ),
        ]
    );
    assert!(rows.events.iter().all(|e| e.ts_ms.is_none()));

    assert_eq!(
        run.facts,
        SessionFacts {
            session_id: Some("sess-0001".into()),
            records: 9,
            calls: 4,
            turns: 3,
            compactions: None,
            skipped: SessionSkips {
                malformed: 1,
                ..SessionSkips::default()
            },
            ..SessionFacts::default()
        }
    );
}

#[test]
fn every_transcript_batch_split_and_checkpoint_resume_equals_one_fold() {
    let whole = transcript(&[], false, PrepOptions::default());
    let lines = TRANSCRIPT.lines().count();
    for split in 0..=lines {
        for resume in [false, true] {
            let run = transcript(&[split], resume, PrepOptions::default());
            assert_eq!(run.rows, whole.rows, "split {split} resume {resume}");
            assert_eq!(run.facts, whole.facts, "split {split} resume {resume}");
            assert_eq!(run.checkpoint, whole.checkpoint);
        }
    }
    let every_line: Vec<usize> = (1..lines).collect();
    let run = transcript(&every_line, true, PrepOptions::default());
    assert_eq!(run.rows, whole.rows);
    assert_eq!(run.facts, whole.facts);
}

#[test]
fn ide_rows_are_keyed_by_composer_and_map_native_model_tokens_and_tools() {
    let run = ide(&snapshot(ide_records()), PrepOptions::default());
    let rows = &run.rows;

    // Every row's native key is the native cursorDiskKV key, composer id second.
    let keys: Vec<&str> = rows
        .calls
        .iter()
        .map(|r| r.native_key.as_deref())
        .chain(rows.turns.iter().map(|r| r.native_key.as_deref()))
        .chain(rows.triggers.iter().map(|r| r.native_key.as_deref()))
        .chain(rows.events.iter().map(|r| r.native_key.as_deref()))
        .chain(rows.tool_uses.iter().map(|r| r.native_key.as_deref()))
        .map(Option::unwrap)
        .collect();
    assert_eq!(keys.len(), rows.len());
    for key in &keys {
        let composer = key.split(':').nth(1).unwrap();
        assert!(["c-main", "c-sub"].contains(&composer), "{key}");
        assert!(
            key.starts_with(&format!("composerData:{composer}"))
                || key.starts_with(&format!("bubbleId:{composer}:")),
            "{key}"
        );
    }
    assert!(!keys.iter().any(|k| k.contains("orphan")));
    assert!(
        rows.calls.iter().all(|r| r.native_offset.is_none())
            && rows.tool_uses.iter().all(|r| r.native_offset.is_none())
    );

    let calls: Vec<_> = rows
        .calls
        .iter()
        .map(|c| {
            (
                c.native_key.as_deref().unwrap(),
                c.msg_id.as_deref(),
                c.ts_ms,
                c.model.as_deref(),
                c.input,
                c.output,
                c.gap_ms,
                c.turn_no,
                c.call_in_turn,
                c.is_sidechain,
            )
        })
        .collect();
    let t0 = ms("2026-01-01T00:00:00Z");
    assert_eq!(
        calls,
        [
            (
                "bubbleId:c-main:a1",
                Some("a1"),
                Some(t0 + 2_500),
                Some("model-a"),
                Some(120),
                Some(30),
                Some(-1),
                Some(1),
                Some(1),
                false
            ),
            (
                "bubbleId:c-main:a2",
                Some("a2"),
                Some(t0 + 4_000),
                None,
                // 0/0 is the writer's "not metered": not recorded, never free.
                None,
                None,
                Some(1_500),
                Some(1),
                Some(2),
                false
            ),
            (
                "bubbleId:c-main:a3",
                Some("a3"),
                None,
                None,
                None,
                None,
                None,
                Some(2),
                Some(1),
                false
            ),
            (
                "bubbleId:c-sub:b1",
                Some("b1"),
                Some(t0 + 65_000),
                None,
                // One non-zero counter: both are native values, verbatim.
                Some(50),
                Some(0),
                Some(-1),
                Some(3),
                Some(1),
                true
            ),
        ]
    );
    for call in &rows.calls {
        assert_eq!(
            call.request_id, None,
            "assistant bubbles carry no request id"
        );
        assert_eq!(call.cache_write_basis, CacheWriteBasis::None);
        assert_eq!([call.cw_1h, call.cw_5m, call.cache_read], [None; 3]);
        assert_eq!(call.stop_reason, None);
    }
    assert_eq!(
        rows.calls[0].ts.as_deref(),
        Some("2026-01-01T00:00:02.500Z")
    );

    let turns: Vec<_> = rows
        .turns
        .iter()
        .map(|t| {
            (
                t.turn_no,
                t.origin,
                t.native_key.as_deref().unwrap(),
                t.opener_ts_ms,
            )
        })
        .collect();
    assert_eq!(
        turns,
        [
            (1, TurnOrigin::Human, "bubbleId:c-main:a1", Some(t0 + 1_000)),
            (
                2,
                TurnOrigin::Human,
                "bubbleId:c-main:a3",
                Some(t0 + 10_000)
            ),
            (3, TurnOrigin::Start, "bubbleId:c-sub:b1", None),
        ]
    );

    let triggers: Vec<_> = rows
        .triggers
        .iter()
        .map(|t| {
            (
                t.native_key.as_deref().unwrap(),
                t.next_turn_no,
                t.body_key.clone(),
            )
        })
        .collect();
    assert_eq!(
        triggers,
        [
            (
                "bubbleId:c-main:u1",
                1,
                Some(fnv_hex("SYNTHETIC-TEXT first prompt"))
            ),
            (
                "bubbleId:c-main:u2",
                2,
                Some(fnv_hex("SYNTHETIC-TEXT second"))
            ),
        ]
    );

    let events: Vec<_> = rows
        .events
        .iter()
        .map(|e| {
            (
                e.native_key.as_deref().unwrap(),
                e.kind,
                e.subkind.as_deref(),
                e.model.as_deref(),
                e.ts_ms,
            )
        })
        .collect();
    assert_eq!(
        events,
        [
            (
                "composerData:c-main",
                PrepEventKind::SystemOther,
                Some("composer"),
                Some("model-config"),
                Some(t0)
            ),
            (
                "bubbleId:c-main:u1",
                PrepEventKind::ModelSwitch,
                None,
                Some("model-a"),
                Some(t0 + 1_000)
            ),
            (
                "bubbleId:c-main:s1",
                PrepEventKind::SystemOther,
                Some("control"),
                None,
                Some(t0 + 5_000)
            ),
            (
                "bubbleId:c-main:u2",
                PrepEventKind::ModelSwitch,
                None,
                Some("model-b"),
                Some(t0 + 10_000)
            ),
            (
                "composerData:c-sub",
                PrepEventKind::SystemOther,
                Some("composer"),
                None,
                Some(t0 + 60_000)
            ),
        ]
    );
    assert_eq!(rows.events[0].ts.as_deref(), Some("2026-01-01T00:00:00Z"));

    let tools: Vec<_> = rows
        .tool_uses
        .iter()
        .map(|t| {
            (
                t.sighting,
                t.tool_use_id.as_deref(),
                t.call_msg_id.as_deref(),
                t.family.as_deref(),
                t.outcome,
                t.result_bytes,
            )
        })
        .collect();
    assert_eq!(
        tools,
        [
            (
                ToolSighting::Use,
                Some("call-1"),
                Some("a2"),
                Some("file-read"),
                None,
                None
            ),
            (
                ToolSighting::Result,
                Some("call-1"),
                Some("a2"),
                Some("file-read"),
                Some(ToolOutcome::Ok),
                Some(MARKER.len() as i64)
            ),
            (
                ToolSighting::Use,
                Some("call-2"),
                Some("a3"),
                Some("shell"),
                None,
                None
            ),
            (
                ToolSighting::Result,
                Some("call-2"),
                Some("a3"),
                Some("shell"),
                Some(ToolOutcome::Error),
                Some(r#"{"detail":"SYNTHETIC-TEXT"}"#.len() as i64)
            ),
            (
                ToolSighting::Use,
                Some("call-3"),
                Some("b1"),
                Some("file-write"),
                None,
                None
            ),
            (
                ToolSighting::Result,
                Some("call-3"),
                Some("b1"),
                Some("file-write"),
                Some(ToolOutcome::Unknown),
                None
            ),
        ]
    );
    assert!(rows.tool_uses.iter().all(|t| t.duration_ms.is_none()));
    assert_eq!(
        rows.tool_uses[0].input_hash,
        Some(fnv_hex(r#"{"path":"SYNTHETIC-TEXT"}"#))
    );
}

#[test]
fn ide_facts_describe_the_database_and_count_what_was_not_folded() {
    let run = ide(&snapshot(ide_records()), PrepOptions::default());
    let t0 = ms("2026-01-01T00:00:00Z");
    assert_eq!(
        run.facts,
        SessionFacts {
            first_event_ts: Some("2026-01-01T00:00:00Z".into()),
            first_event_ms: Some(t0),
            last_event_ts: Some("2026-01-01T00:01:05Z".into()),
            last_event_ms: Some(t0 + 65_000),
            // Two v3 composers and seven referenced bubbles.
            records: 9,
            calls: 4,
            turns: 3,
            // Latest timed main-chain call; the sidechain composer is excluded.
            latest_context: Some(ContextSample {
                ts_ms: Some(t0 + 4_000),
                model: None,
                stop_reason: None,
                input: None,
                cache_read: None,
                cache_write: None,
                total: None,
            }),
            compactions: None,
            last_model_switch: Some(ModelSwitch {
                ts_ms: Some(t0 + 10_000),
                requested_model: "model-b".into(),
            }),
            // The v1 composer and the dangling header.
            skipped: SessionSkips {
                malformed: 2,
                ..SessionSkips::default()
            },
            ..SessionFacts::default()
        }
    );
    assert_eq!(run.checkpoint.fold["revision"], "rev-1");
}

#[test]
fn ide_output_depends_on_the_snapshot_not_its_row_order() {
    let forward = ide(&snapshot(ide_records()), PrepOptions::default());
    let mut reversed = ide_records();
    reversed.reverse();
    let backward = ide(&snapshot(reversed), PrepOptions::default());
    assert_eq!(forward.rows, backward.rows);
    assert_eq!(forward.facts, backward.facts);
}

#[test]
fn ide_checkpoint_reopens_the_database_facts() {
    let run = ide(&snapshot(ide_records()), PrepOptions::default());
    let json = serde_json::to_string(&run.checkpoint).unwrap();
    let saved: PrepCheckpoint = serde_json::from_str(&json).unwrap();
    let fold = CursorIdePrepFold;
    let reopened = fold
        .open(
            &fold.describe("state.vscdb"),
            "cursor-ide/default/state.vscdb",
            3,
            Some(&saved),
        )
        .unwrap();
    assert_eq!(reopened.facts(), run.facts);
    assert_eq!(reopened.checkpoint(), run.checkpoint);
}

#[test]
fn content_appears_only_under_explicit_opt_in() {
    let options = PrepOptions {
        include_content: true,
    };
    let transcript_default = transcript(&[], false, PrepOptions::default());
    let ide_default = ide(&snapshot(ide_records()), PrepOptions::default());
    assert!(!leaks(&transcript_default));
    assert!(!leaks(&ide_default));

    let transcript_opt_in = transcript(&[], false, options);
    let heads: Vec<_> = transcript_opt_in
        .rows
        .triggers
        .iter()
        .map(|t| t.content_head.as_deref())
        .collect();
    assert_eq!(heads, [Some("SYNTHETIC-TEXT first prompt"), Some("")]);
    let ide_opt_in = ide(&snapshot(ide_records()), options);
    let heads: Vec<_> = ide_opt_in
        .rows
        .triggers
        .iter()
        .map(|t| t.content_head.as_deref())
        .collect();
    assert_eq!(
        heads,
        [
            Some("SYNTHETIC-TEXT first prompt"),
            Some("SYNTHETIC-TEXT second")
        ]
    );
    // Opt-in changes nothing but the content column.
    let strip = |mut rows: PrepRows| {
        rows.triggers.iter_mut().for_each(|t| t.content_head = None);
        rows
    };
    assert_eq!(strip(transcript_opt_in.rows), transcript_default.rows);
    assert_eq!(strip(ide_opt_in.rows), ide_default.rows);
}

#[test]
fn foreign_checkpoints_and_mismatched_input_are_refused() {
    let transcript_fold = CursorTranscriptPrepFold;
    let ide_fold = CursorIdePrepFold;
    let meta = transcript_fold.describe(TRANSCRIPT_FILE);
    let foreign = |policy: &str, format: u32| PrepCheckpoint {
        format,
        policy: policy.into(),
        fold: serde_json::json!({}),
    };
    for (fold, own) in [
        (&transcript_fold as &dyn PrepFold, TRANSCRIPT_PREP_POLICY),
        (&ide_fold as &dyn PrepFold, IDE_PREP_POLICY),
    ] {
        for saved in [
            foreign("claude-code/prep-v3", PREP_CHECKPOINT_FORMAT),
            foreign(own, PREP_CHECKPOINT_FORMAT + 1),
            // Own policy and format, but not a state this fold wrote.
            foreign(own, PREP_CHECKPOINT_FORMAT),
        ] {
            let error = fold.open(&meta, "s", 0, Some(&saved)).err().unwrap();
            assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        }
    }
    // Neither fold accepts the other's checkpoint.
    let ide_checkpoint = ide(&snapshot(ide_records()), PrepOptions::default()).checkpoint;
    assert!(
        transcript_fold
            .open(&meta, "s", 0, Some(&ide_checkpoint))
            .is_err()
    );

    let snapshot_input = PrepInput::Snapshot(snapshot(ide_records()));
    let records_input = PrepInput::Records(records(TRANSCRIPT));
    let mut session = transcript_fold.open(&meta, "s", 0, None).unwrap();
    let error = session
        .fold(&snapshot_input, PrepOptions::default())
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
    let mut session = ide_fold.open(&meta, "s", 0, None).unwrap();
    let error = session
        .fold(&records_input, PrepOptions::default())
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);

    // Only the cursorDiskKV key/value table is an IDE snapshot.
    let mut other = snapshot(ide_records());
    other.source.format = SnapshotFormat::JsonDocument;
    let error = session
        .fold(&PrepInput::Snapshot(other), PrepOptions::default())
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::Unsupported);
}
