use std::{collections::BTreeSet, path::PathBuf};

use serde_json::{Value, json};
use unisphere_adapter_cursor::{CursorAdapter, CursorIdeAdapter, DESCRIPTOR, IDE_DESCRIPTOR};
use unisphere_core::{
    NativeSnapshot, SnapshotFormat, SnapshotRecord, SnapshotRef,
    query::{
        AdapterId, AvailabilityCode, BranchEvidence, ContentAccess, HarnessId, MembershipPolicy,
        NativeLocator, NativeQueryInput, ObservationFacet, ObservationPart, Outcome, QueryAdapter,
        QueryFailureCode, QueryLimits, SourceEvidence, SourceId, SourceLocator, SourceReadStatus,
        SourceViewKind,
    },
};

fn source(adapter: &str, representation: &str, path: &str, revision: &str) -> SourceEvidence {
    SourceEvidence {
        id: SourceId::derive([
            adapter.as_bytes(),
            representation.as_bytes(),
            path.as_bytes(),
        ]),
        adapter: AdapterId::new(adapter).unwrap(),
        harness: HarnessId::new("cursor").unwrap(),
        representation: representation.into(),
        locator: SourceLocator::LocalPath(PathBuf::from(path)),
        revision: revision.into(),
        query_policy_version: "provider-placeholder".into(),
        read_status: SourceReadStatus::Readable,
        associations: Vec::new(),
        available_fields: BTreeSet::new(),
    }
}

fn transcript_records() -> Vec<unisphere_core::query::NativeRecord> {
    include_str!("../fixtures/transcript.jsonl")
        .lines()
        .filter(|line| !line.is_empty())
        .enumerate()
        .map(|(index, line)| unisphere_core::query::NativeRecord {
            locator: NativeLocator::Jsonl {
                offset: u64::try_from(index * 100).unwrap(),
            },
            bytes: line.as_bytes().to_vec(),
        })
        .collect()
}

fn ide_snapshot() -> NativeSnapshot {
    let rows: Vec<Value> = serde_json::from_str(include_str!("../fixtures/ide.json")).unwrap();
    NativeSnapshot {
        source: SnapshotRef {
            path: "/synthetic/not-opened/state.vscdb".into(),
            format: SnapshotFormat::SqliteKeyValue {
                table: "cursorDiskKV".into(),
            },
            session_id: Some("alpha".into()),
        },
        revision: "synthetic-revision-one".into(),
        records: rows
            .into_iter()
            .map(|row| SnapshotRecord {
                key: row["key"].as_str().unwrap().into(),
                bytes: serde_json::to_vec(&row["value"]).unwrap(),
            })
            .collect(),
    }
}

fn snapshot_key(observation: &unisphere_core::query::Observation) -> &str {
    match &observation.source_ref.locator {
        NativeLocator::Snapshot { key } => key,
        _ => panic!("expected snapshot locator"),
    }
}

#[test]
fn transcript_keeps_native_absence_and_source_only_membership_explicit() {
    let source = source(
        DESCRIPTOR.id,
        "cursor-agent-transcript-jsonl",
        "/synthetic/project/agent-transcripts/id/id.jsonl",
        "transcript-r1",
    );
    let records = transcript_records();
    let inspected = CursorAdapter
        .inspect(
            NativeQueryInput::Records {
                source: &source,
                records: &records,
            },
            ContentAccess::default(),
            &QueryLimits::default(),
        )
        .unwrap();

    assert_eq!(
        inspected.source.query_policy_version,
        "cursor-transcript-query-v1"
    );
    assert_eq!(inspected.partitions.len(), 1);
    assert_eq!(inspected.partitions[0].view, SourceViewKind::SourceOnly);
    assert_eq!(
        inspected.partitions[0].membership,
        MembershipPolicy::Unavailable
    );
    assert!(inspected.observations.iter().all(|observation| {
        observation.native_record_id.is_none()
            && observation.session.is_none()
            && observation.timestamp.is_none()
            && matches!(&observation.branch, BranchEvidence::Unavailable { .. })
    }));
    assert!(inspected.issues.iter().any(|issue| {
        issue.code == AvailabilityCode::NotCaptured
            && issue.field == Some(unisphere_core::query::FieldId::CallId)
    }));
    assert!(inspected.issues.iter().any(|issue| {
        issue.code == AvailabilityCode::NotCaptured
            && issue.field == Some(unisphere_core::query::FieldId::Timestamp)
    }));

    let assistant = inspected
        .observations
        .iter()
        .flat_map(|observation| observation.facets.iter())
        .find_map(|facet| match facet {
            ObservationFacet::Message { role, parts, .. }
                if *role == unisphere_core::query::MessageRole::Assistant =>
            {
                Some(parts)
            }
            _ => None,
        })
        .unwrap();
    assert!(assistant.iter().any(|part| matches!(
        part,
        ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)
    )));
    assert!(
        assistant
            .iter()
            .all(|part| matches!(part, ObservationPart::Unavailable(_)))
    );
    assert!(inspected.observations.iter().all(|observation| {
        observation.facets.iter().all(|facet| {
            !matches!(
                facet,
                ObservationFacet::ToolCall { .. } | ObservationFacet::ToolResult { .. }
            )
        })
    }));
}

#[test]
fn transcript_content_opt_in_does_not_invent_call_or_result_identity() {
    let source = source(
        DESCRIPTOR.id,
        "cursor-agent-transcript-jsonl",
        "/synthetic/project/agent-transcripts/id/id.jsonl",
        "transcript-r1",
    );
    let records = transcript_records();
    let inspected = CursorAdapter
        .inspect(
            NativeQueryInput::Records {
                source: &source,
                records: &records,
            },
            ContentAccess {
                inspect_fields: BTreeSet::new(),
                emit_content: true,
            },
            &QueryLimits::default(),
        )
        .unwrap();

    let assistant_parts = inspected
        .observations
        .iter()
        .flat_map(|observation| observation.facets.iter())
        .find_map(|facet| match facet {
            ObservationFacet::Message { role, parts, .. }
                if *role == unisphere_core::query::MessageRole::Assistant =>
            {
                Some(parts)
            }
            _ => None,
        })
        .unwrap();
    assert!(assistant_parts.iter().any(|part| {
        matches!(part, ObservationPart::Structured(value) if value["type"] == "tool_call" && value.get("id").is_none())
    }));
    assert!(inspected.observations.iter().all(|observation| {
        observation.facets.iter().all(|facet| {
            !matches!(
                facet,
                ObservationFacet::ToolCall { .. } | ObservationFacet::ToolResult { .. }
            )
        })
    }));
}

#[test]
fn ide_uses_validated_main_spine_order_and_surfaces_mixed_store_fragments() {
    let source = source(
        IDE_DESCRIPTOR.id,
        "cursor-ide-cursor-disk-kv",
        "/synthetic/not-opened/state.vscdb",
        "synthetic-revision-one",
    );
    let snapshot = ide_snapshot();
    let inspected = CursorIdeAdapter
        .inspect(
            NativeQueryInput::Snapshot {
                source: &source,
                snapshot: &snapshot,
            },
            ContentAccess::default(),
            &QueryLimits::default(),
        )
        .unwrap();

    assert_eq!(inspected.source.query_policy_version, "cursor-ide-query-v1");
    assert_eq!(
        inspected
            .partitions
            .iter()
            .filter(|partition| partition.view == SourceViewKind::MainSpine)
            .count(),
        1
    );
    assert!(inspected.partitions.iter().any(|partition| {
        partition.view == SourceViewKind::SourceOnly
            && partition.membership == MembershipPolicy::Unavailable
    }));
    let keys: Vec<_> = inspected.observations.iter().map(snapshot_key).collect();
    assert_eq!(
        &keys[..4],
        [
            "composerData:alpha",
            "bubbleId:alpha:z",
            "bubbleId:alpha:a",
            "bubbleId:alpha:s"
        ]
    );
    for key in ["composerData:beta", "bubbleId:beta:z"] {
        let observation = inspected
            .observations
            .iter()
            .find(|observation| snapshot_key(observation) == key)
            .unwrap();
        assert!(observation.session.is_none());
        assert!(matches!(
            &observation.branch,
            BranchEvidence::Unavailable { .. }
        ));
    }
    assert!(inspected.issues.iter().any(|issue| {
        issue.code == AvailabilityCode::Unassociated
            && issue.field == Some(unisphere_core::query::FieldId::Association)
    }));
}

#[test]
fn ide_pairs_only_native_call_ids_and_does_not_guess_success_or_duration() {
    let source = source(
        IDE_DESCRIPTOR.id,
        "cursor-ide-cursor-disk-kv",
        "/synthetic/not-opened/state.vscdb",
        "synthetic-revision-one",
    );
    let snapshot = ide_snapshot();
    let inspected = CursorIdeAdapter
        .inspect(
            NativeQueryInput::Snapshot {
                source: &source,
                snapshot: &snapshot,
            },
            ContentAccess {
                inspect_fields: BTreeSet::new(),
                emit_content: true,
            },
            &QueryLimits::default(),
        )
        .unwrap();
    let assistant = inspected
        .observations
        .iter()
        .find(|observation| snapshot_key(observation) == "bubbleId:alpha:a")
        .unwrap();
    let call = assistant.facets.iter().find_map(|facet| match facet {
        ObservationFacet::ToolCall {
            native_call_id,
            native_name,
            family,
            input,
            turn_id,
        } => Some((native_call_id, native_name, family, input, turn_id)),
        _ => None,
    });
    let (call_id, name, family, input, turn_id) = call.unwrap();
    assert_eq!(call_id, "call-a");
    assert_eq!(name, "read_file");
    assert_eq!(family.as_deref(), Some("file-read"));
    assert!(
        matches!(input.as_slice(), [ObservationPart::Structured(value)] if value["path"] == "SENSITIVE-PATH")
    );
    assert!(turn_id.is_none());

    let result = assistant.facets.iter().find_map(|facet| match facet {
        ObservationFacet::ToolResult {
            native_call_id,
            outcome,
            reported_duration_ms,
            output,
            ..
        } => Some((native_call_id, outcome, reported_duration_ms, output)),
        _ => None,
    });
    let (result_id, outcome, duration, output) = result.unwrap();
    assert_eq!(result_id, "call-a");
    assert_eq!(*outcome, Outcome::Unknown);
    assert!(duration.is_none());
    assert!(
        matches!(output.as_slice(), [ObservationPart::Structured(value)] if value["contents"] == "SENSITIVE-RESULT")
    );
}

#[test]
fn ide_orphans_missing_rows_and_duplicates_are_not_repaired() {
    let source = source(
        IDE_DESCRIPTOR.id,
        "cursor-ide-cursor-disk-kv",
        "/synthetic/not-opened/state.vscdb",
        "synthetic-revision-one",
    );
    let mut snapshot = ide_snapshot();
    snapshot.records.push(SnapshotRecord {
        key: "bubbleId:alpha:alternate".into(),
        bytes: serde_json::to_vec(&json!({
            "bubbleId":"alternate", "type":2, "text":"SENSITIVE-ALTERNATE"
        }))
        .unwrap(),
    });
    snapshot
        .records
        .retain(|record| record.key != "bubbleId:alpha:z");
    let tool_row = snapshot
        .records
        .iter_mut()
        .find(|record| record.key == "bubbleId:alpha:a")
        .unwrap();
    let mut tool_value: Value = serde_json::from_slice(&tool_row.bytes).unwrap();
    tool_value["toolFormerData"]
        .as_object_mut()
        .unwrap()
        .remove("toolCallId");
    tool_row.bytes = serde_json::to_vec(&tool_value).unwrap();
    let inspected = CursorIdeAdapter
        .inspect(
            NativeQueryInput::Snapshot {
                source: &source,
                snapshot: &snapshot,
            },
            ContentAccess::default(),
            &QueryLimits::default(),
        )
        .unwrap();
    let alternate = inspected
        .observations
        .iter()
        .find(|observation| snapshot_key(observation) == "bubbleId:alpha:alternate")
        .unwrap();
    assert!(alternate.session.is_none());
    assert!(matches!(
        &alternate.branch,
        BranchEvidence::Unavailable { .. }
    ));
    assert!(
        inspected
            .issues
            .iter()
            .any(|issue| issue.code == AvailabilityCode::Absent)
    );
    let idless_tool = inspected
        .observations
        .iter()
        .find(|observation| snapshot_key(observation) == "bubbleId:alpha:a")
        .unwrap();
    assert!(idless_tool.facets.iter().all(|facet| {
        !matches!(
            facet,
            ObservationFacet::ToolCall { .. } | ObservationFacet::ToolResult { .. }
        )
    }));
    assert!(idless_tool.diagnostics.iter().any(|issue| {
        issue.code == AvailabilityCode::NotCaptured
            && issue.field == Some(unisphere_core::query::FieldId::CallId)
    }));

    snapshot.records.push(snapshot.records[0].clone());
    let error = match CursorIdeAdapter.inspect(
        NativeQueryInput::Snapshot {
            source: &source,
            snapshot: &snapshot,
        },
        ContentAccess::default(),
        &QueryLimits::default(),
    ) {
        Ok(_) => panic!("duplicate native keys must be rejected"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), QueryFailureCode::InvalidData);
}
