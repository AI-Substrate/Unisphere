//! Contract freeze for core::prep v2: the serialized vocabulary and key formats
//! that published tables, state.json and external consumers depend on.

use std::{collections::BTreeMap, path::PathBuf};

use serde_json::{Value, json};
use unisphere_core::{
    SourceIdentity,
    prep::{
        CacheWriteBasis, CallSighting, DEFAULT_ROOT_LABEL, PREP_CHECKPOINT_FORMAT,
        PREP_TABLE_SCHEMA_VERSION, PrepCallRow, PrepCheckpoint, PrepEventKind, PrepReplaceReason,
        PrepSetState, PrepSourceKind, PrepSourceMeta, PrepSourceSet, PrepSourceState,
        PrepSourceStatus, PrepState, SessionFacts, ToolOutcome, ToolSighting, TurnOrigin,
        derived_root_label,
    },
};

fn text<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn versions_are_the_published_contract() {
    assert_eq!(PREP_TABLE_SCHEMA_VERSION, 2);
    assert_eq!(PREP_CHECKPOINT_FORMAT, 1);
    assert_eq!(DEFAULT_ROOT_LABEL, "default");
}

#[test]
fn source_keys_are_harness_label_file() {
    let set = PrepSourceSet {
        harness: "claude-code".into(),
        label: DEFAULT_ROOT_LABEL.into(),
        root: PathBuf::from("/h/.claude/projects"),
    };
    assert_eq!(set.key(), "claude-code/default");
    assert_eq!(set.source_key("p/s.jsonl"), "claude-code/default/p/s.jsonl");
    let label = derived_root_label(&PathBuf::from("/h/.claude-alt/projects"));
    assert!(label.starts_with("root-") && label.len() == 13, "{label}");
    assert!(label[5..].bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(
        label,
        derived_root_label(&PathBuf::from("/h/.claude-alt/projects"))
    );
    assert_ne!(
        label,
        derived_root_label(&PathBuf::from("/h/.claude/projects"))
    );
}

#[test]
fn row_vocabularies_serialize_to_their_table_values() {
    assert_eq!(text(CallSighting::Update), "update");
    assert_eq!(text(CacheWriteBasis::Fallback1h), "fallback_1h");
    assert_eq!(text(CacheWriteBasis::Fallback5m), "fallback_5m");
    assert_eq!(text(CacheWriteBasis::Split), "split");
    assert_eq!(text(TurnOrigin::TaskNotification), "task-notification");
    assert_eq!(text(TurnOrigin::AutoContinuation), "auto-continuation");
    assert_eq!(text(TurnOrigin::CompactSummary), "compact-summary");
    assert_eq!(text(TurnOrigin::ManualCompact), "manual-compact");
    assert_eq!(text(TurnOrigin::SubagentTask), "subagent-task");
    assert_eq!(text(PrepEventKind::LimitNotice), "limit_notice");
    assert_eq!(text(PrepEventKind::ModelSwitch), "model_switch");
    assert_eq!(text(PrepEventKind::ScheduledFire), "scheduled_fire");
    assert_eq!(text(ToolSighting::Result), "result");
    assert_eq!(text(ToolOutcome::Unknown), "unknown");
    assert_eq!(text(PrepSourceKind::Snapshot), "snapshot");
}

#[test]
fn status_vocabulary_covers_every_reported_outcome() {
    let labels: Vec<&str> = [
        PrepSourceStatus::New,
        PrepSourceStatus::Unchanged,
        PrepSourceStatus::Appended,
        PrepSourceStatus::Replaced {
            reason: PrepReplaceReason::Rewritten,
        },
        PrepSourceStatus::Skipped,
        PrepSourceStatus::Unreadable,
        PrepSourceStatus::Unsupported,
        PrepSourceStatus::Missing,
    ]
    .into_iter()
    .map(PrepSourceStatus::label)
    .collect();
    assert_eq!(
        labels,
        [
            "new",
            "unchanged",
            "appended",
            "replaced",
            "skipped",
            "unreadable",
            "unsupported",
            "missing"
        ]
    );
    assert_eq!(
        serde_json::to_value(PrepSourceStatus::Replaced {
            reason: PrepReplaceReason::RootMoved
        })
        .unwrap(),
        json!({"status": "replaced", "reason": "root_moved"})
    );
}

#[test]
fn unrecorded_row_fields_serialize_as_null_not_zero() {
    let row = PrepCallRow {
        source: "cursor/default/p/t.jsonl".into(),
        generation: 0,
        native_offset: Some(0),
        native_key: None,
        sighting: CallSighting::First,
        msg_id: None,
        request_id: None,
        ts: None,
        ts_ms: None,
        model: None,
        stop_reason: None,
        input: None,
        cw_1h: None,
        cw_5m: None,
        cache_read: None,
        output: None,
        cache_write_basis: CacheWriteBasis::None,
        is_sidechain: false,
        gap_ms: None,
        turn_no: None,
        call_in_turn: None,
        records: 1,
    };
    let value = serde_json::to_value(&row).unwrap();
    for field in [
        "ts",
        "ts_ms",
        "model",
        "input",
        "cw_1h",
        "cache_read",
        "output",
    ] {
        assert_eq!(value[field], Value::Null, "{field}");
    }
}

#[test]
fn state_round_trips_with_checkpoint_and_facts() {
    let mut sources = BTreeMap::new();
    sources.insert(
        "claude-code/default/p/s.jsonl".to_owned(),
        PrepSourceState {
            set: "claude-code/default".into(),
            path: PathBuf::from("/h/.claude/projects/p/s.jsonl"),
            file: "p/s.jsonl".into(),
            kind: PrepSourceKind::Append,
            identity: SourceIdentity::Unix {
                device: 1,
                inode: 2,
            },
            size: 10,
            mtime_ns: 3,
            offset: 8,
            anchor: Some("sha256:00".into()),
            revision: None,
            generation: 1,
            meta: PrepSourceMeta {
                is_sub: false,
                agent_id: None,
                project: Some("p".into()),
            },
            checkpoint: PrepCheckpoint {
                format: PREP_CHECKPOINT_FORMAT,
                policy: "claude-code/prep-v3".into(),
                fold: json!({"turn_no": 1}),
            },
            facts: SessionFacts {
                calls: 2,
                ..SessionFacts::default()
            },
            status: PrepSourceStatus::Skipped,
        },
    );
    let mut sets = BTreeMap::new();
    sets.insert(
        "claude-code/default".to_owned(),
        PrepSetState {
            harness: "claude-code".into(),
            label: "default".into(),
            root: PathBuf::from("/h/.claude/projects"),
            policy: "claude-code/prep-v3".into(),
        },
    );
    let state = PrepState {
        table_schema_version: PREP_TABLE_SCHEMA_VERSION,
        checkpoint_format: PREP_CHECKPOINT_FORMAT,
        runs: 4,
        parts: vec!["tables/calls/run-000004.parquet".into()],
        sets,
        sources,
    };
    let bytes = serde_json::to_vec(&state).unwrap();
    let back: PrepState = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(back, state);
}
