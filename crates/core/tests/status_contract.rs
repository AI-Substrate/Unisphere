//! Contract freeze for core::status v1: the serialized vocabulary external
//! consumers (the CLI JSON, embedding SDK callers such as pij) depend on.

use std::path::PathBuf;

use serde_json::{Value, json};
use unisphere_core::status::{
    Basis, CompactionCounts, Fact, MODEL_WINDOWS_TABLE, ResolveBasis, Resolved,
    STATUS_SCHEMA_VERSION, SessionStatus, StatusFailure, StatusFailureKind, StatusQuery,
    StatusTarget, UNKNOWN_FACTS,
};

fn target() -> StatusTarget {
    StatusTarget {
        harness: "claude-code".into(),
        session_id: "00000000-0000-0000-0000-000000000001".into(),
        transcript: Some(PathBuf::from("/tmp/fixture/session.jsonl")),
    }
}

#[test]
fn versions_are_the_published_contract() {
    assert_eq!(STATUS_SCHEMA_VERSION, 1);
    assert_eq!(MODEL_WINDOWS_TABLE, "model-windows@1");
}

#[test]
fn basis_vocabulary_is_snake_case() {
    let names: Vec<Value> = [
        Basis::Native,
        Basis::Derived,
        Basis::Table,
        Basis::MtimeFallback,
    ]
    .into_iter()
    .map(|b| serde_json::to_value(b).unwrap())
    .collect();
    assert_eq!(
        names,
        vec![
            json!("native"),
            json!("derived"),
            json!("table"),
            json!("mtime_fallback")
        ]
    );
    assert_eq!(
        serde_json::to_value(Fact::new(250_000u64, Basis::Native)).unwrap(),
        json!({"value": 250000, "basis": "native"})
    );
}

#[test]
fn empty_status_is_unknown_not_zero() {
    let status = SessionStatus::empty(target());
    let value = serde_json::to_value(&status).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["model"]["current"], Value::Null);
    assert_eq!(value["context"]["used_tokens"], Value::Null);
    assert_eq!(value["context"]["percent"], Value::Null);
    assert_eq!(value["compaction"]["counts"], Value::Null);
    assert_eq!(value["last_call"], Value::Null);
    assert_eq!(value["timeline"]["last_updated_ms"], Value::Null);
    assert_eq!(status.unknown, UNKNOWN_FACTS);
    let distinct: std::collections::BTreeSet<_> = UNKNOWN_FACTS.iter().collect();
    assert_eq!(distinct.len(), UNKNOWN_FACTS.len());
}

#[test]
fn status_round_trips() {
    let mut status = SessionStatus::empty(target());
    status.model.current = Some(Fact::new("claude-opus-5-5".into(), Basis::Native));
    status.context.used_tokens = Some(Fact::new(250_112, Basis::Native));
    status.context.window_tokens = Some(Fact::new(1_000_000, Basis::Table));
    status.context.window_table = Some(MODEL_WINDOWS_TABLE.into());
    status.context.percent = Some(25.0);
    status.compaction.counts = Some(CompactionCounts {
        manual: 1,
        auto: 2,
        unknown_trigger: 0,
    });
    status.turns.by_origin.insert("peer".into(), 4);
    status.unknown.push("last_call.stop_reason".into());
    status.resolved = Some(Resolved {
        query: StatusQuery::Pane("%75".into()),
        target: target(),
        pij_id: Some("pij-example-seat".into()),
        pane: Some("%75".into()),
        basis: ResolveBasis::PijRegistry,
        conflicts: vec![],
    });
    let json = serde_json::to_string(&status).unwrap();
    let back: SessionStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, status);
}

#[test]
fn queries_and_resolve_basis_are_tagged() {
    assert_eq!(
        serde_json::to_value(StatusQuery::Pij("pij-x".into())).unwrap(),
        json!({"pij": "pij-x"})
    );
    assert_eq!(
        serde_json::to_value(StatusQuery::Pane("%0".into())).unwrap(),
        json!({"pane": "%0"})
    );
    assert_eq!(
        serde_json::to_value(ResolveBasis::NativePane).unwrap(),
        json!("native_pane")
    );
    let t = StatusTarget {
        transcript: None,
        ..target()
    };
    assert!(
        serde_json::to_value(&t)
            .unwrap()
            .get("transcript")
            .is_none()
    );
}

#[test]
fn failure_codes_are_stable_and_distinct() {
    let kinds = [
        StatusFailureKind::UnsupportedHarness,
        StatusFailureKind::TranscriptNotFound,
        StatusFailureKind::Read,
        StatusFailureKind::PijUnavailable,
        StatusFailureKind::PijUnknownSeat,
        StatusFailureKind::PijNoSession,
        StatusFailureKind::DeadBinding,
        StatusFailureKind::PaneNotFound,
    ];
    let codes: std::collections::BTreeSet<_> = kinds.iter().map(|k| k.code()).collect();
    assert_eq!(codes.len(), kinds.len());
    assert!(codes.iter().all(|c| c.starts_with("UNI-STATUS-")));
    let failure = StatusFailure::new(StatusFailureKind::PijNoSession, "seat has no session");
    assert_eq!(
        failure.to_string(),
        "UNI-STATUS-PIJ-NO-SESSION: seat has no session"
    );
    assert!(!failure.recovery().is_empty());
}
