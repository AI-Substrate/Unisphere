use std::collections::BTreeSet;

use serde_json::json;
use unisphere_adapter_copilot_cli::{
    CURRENT_QUERY_POLICY_VERSION, CopilotCliAdapter, CopilotCliAdapterSnapshot, DESCRIPTOR,
    LEGACY_QUERY_POLICY_VERSION, SNAPSHOT_DESCRIPTOR,
};
use unisphere_core::query::{
    AdapterId, AssociationBasis, AvailabilityCode, BranchEvidence, ContentAccess, FieldId,
    HarnessId, MembershipPolicy, MessageRole, NativeLocator, NativeQueryInput, NativeRecord,
    ObservationFacet, ObservationPart, Outcome, QueryAdapter, QueryLimits, RequestMarker,
    SourceEvidence, SourceId, SourceLocator, SourceReadStatus, SourceViewKind, UsageScope,
};
use unisphere_core::{NativeSnapshot, SnapshotFormat, SnapshotRecord, SnapshotRef};

const EVENTS: &[u8] = include_bytes!("fixtures/events.jsonl");
const LEGACY: &[u8] = include_bytes!("fixtures/legacy.json");

fn source(adapter: &str, representation: &str, revision: &str) -> SourceEvidence {
    SourceEvidence {
        id: SourceId::derive([
            adapter.as_bytes(),
            representation.as_bytes(),
            b"synthetic-source",
        ]),
        adapter: AdapterId::new(adapter).unwrap(),
        harness: HarnessId::new("copilot-cli").unwrap(),
        representation: representation.into(),
        locator: SourceLocator::Provided("synthetic-copilot-input".into()),
        revision: revision.into(),
        query_policy_version: "provider-policy-placeholder".into(),
        read_status: SourceReadStatus::Readable,
        associations: Vec::new(),
        available_fields: BTreeSet::new(),
    }
}

fn current_records() -> Vec<NativeRecord> {
    let mut offset = 0_u64;
    EVENTS
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            let record = NativeRecord {
                locator: NativeLocator::Jsonl { offset },
                bytes: line.to_vec(),
            };
            offset += line.len() as u64 + 1;
            record
        })
        .collect()
}

fn legacy_snapshot() -> NativeSnapshot {
    NativeSnapshot {
        source: SnapshotRef {
            path: "/synthetic/copilot/legacy.json".into(),
            format: SnapshotFormat::JsonDocument,
            session_id: None,
        },
        revision: "legacy-r1".into(),
        records: vec![SnapshotRecord {
            key: "document".into(),
            bytes: LEGACY.to_vec(),
        }],
    }
}

#[test]
fn current_events_preserve_native_ids_context_outcomes_and_scoped_usage() {
    let source = source(DESCRIPTOR.id, "copilot-cli-events-jsonl", "events-r1");
    let records = current_records();
    let inspected = CopilotCliAdapter
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
        CURRENT_QUERY_POLICY_VERSION
    );
    assert_eq!(inspected.observations.len(), 15);
    assert!(inspected.partitions.iter().any(|partition| {
        partition.native_session_id.as_deref() == Some("session-1")
            && partition.view == SourceViewKind::Conversation
            && partition.membership == MembershipPolicy::ValidatedHeader
    }));
    assert!(
        inspected
            .partitions
            .iter()
            .any(|partition| { partition.participant_id.as_deref() == Some("agent-1") })
    );

    let user = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("event-user"))
        .unwrap();
    assert_eq!(user.parent_ids, ["event-start"]);
    assert!(matches!(
        &user.branch,
        BranchEvidence::Node { parent: Some(parent), .. } if parent == "event-start"
    ));
    assert!(user.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Message {
            role: MessageRole::User,
            request_marker: RequestMarker::Initiating,
            parts,
            ..
        } if parts.iter().any(|part| matches!(part, ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)))
    )));

    let start = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("tool-start"))
        .unwrap();
    assert!(start.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolCall {
            native_call_id,
            native_name,
            family: Some(family),
            turn_id: Some(turn_id),
            ..
        } if native_call_id == "tool-1" && native_name == "read_file" && family == "file-read" && turn_id == "turn-1"
    )));

    let end = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("tool-end"))
        .unwrap();
    assert!(end.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolResult { native_call_id, outcome: Outcome::Succeeded, .. }
            if native_call_id == "tool-1"
    )));

    let usage = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("usage"))
        .unwrap();
    assert!(usage.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Usage {
            scope: UsageScope::Invocation,
            counters,
            ..
        } if counters.input_tokens == Some(15)
            && counters.output_tokens == Some(9)
            && counters.cache_read_tokens == Some(4)
            && counters.cache_write_tokens == Some(2)
    )));

    let associations: Vec<_> = inspected
        .partitions
        .iter()
        .flat_map(|partition| &partition.associations)
        .collect();
    assert!(associations.iter().any(|association| {
        association.basis == AssociationBasis::NativeCwd
            && association.path.as_deref() == Some(std::path::Path::new("/SENSITIVE-workspace"))
            && matches!(
                association.applies_to,
                unisphere_core::query::AssociationExtent::From { until: Some(_), .. }
            )
    }));
    assert!(associations.iter().any(|association| {
        association.basis == AssociationBasis::NativeCwd
            && association.path.as_deref() == Some(std::path::Path::new("/SENSITIVE-new-workspace"))
            && matches!(
                association.applies_to,
                unisphere_core::query::AssociationExtent::From { until: None, .. }
            )
    }));
}

#[test]
fn content_access_is_field_scoped_and_never_required_for_structural_ids() {
    let source = source(DESCRIPTOR.id, "copilot-cli-events-jsonl", "events-r1");
    let records = current_records();
    let inspected = CopilotCliAdapter
        .inspect(
            NativeQueryInput::Records {
                source: &source,
                records: &records,
            },
            ContentAccess {
                inspect_fields: BTreeSet::from([FieldId::Text, FieldId::Input]),
                emit_content: false,
            },
            &QueryLimits::default(),
        )
        .unwrap();

    let assistant = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("message-event"))
        .unwrap();
    assert!(assistant.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Message { parts, .. }
            if parts.iter().any(|part| matches!(part, ObservationPart::Text(text) if text == "SENSITIVE-answer"))
    )));
    assert!(assistant.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolCall { native_call_id, input, .. }
            if native_call_id == "tool-1"
                && input.iter().any(|part| matches!(part, ObservationPart::Structured(value) if value["path"] == "/SENSITIVE-input"))
    )));
    let result = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("tool-end"))
        .unwrap();
    assert!(result.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolResult { output, .. }
            if output == &vec![ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)]
    )));
}

#[test]
fn legacy_chat_and_timeline_are_revision_qualified_separate_views() {
    let source = source(
        SNAPSHOT_DESCRIPTOR.id,
        "copilot-cli-legacy-json",
        "legacy-r1",
    );
    let snapshot = legacy_snapshot();
    let inspected = CopilotCliAdapterSnapshot
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

    assert_eq!(
        inspected.source.query_policy_version,
        LEGACY_QUERY_POLICY_VERSION
    );
    assert_eq!(inspected.observations.len(), 9);
    assert!(
        inspected
            .partitions
            .iter()
            .any(|partition| partition.view == SourceViewKind::LegacyChat)
    );
    assert!(
        inspected
            .partitions
            .iter()
            .any(|partition| partition.view == SourceViewKind::LegacyTimeline)
    );

    let chat_user = inspected
        .observations
        .iter()
        .find(|observation| {
            matches!(
                &observation.source_ref.locator,
                NativeLocator::Snapshot { key } if key == "document#/chatMessages/0"
            )
        })
        .unwrap();
    assert!(chat_user.timestamp.is_none());
    assert_eq!(chat_user.source_ref.revision, "legacy-r1");
    assert!(chat_user.diagnostics.iter().any(|issue| {
        issue.code == AvailabilityCode::NotCaptured && issue.field == Some(FieldId::Timestamp)
    }));
    assert!(chat_user.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Message {
            request_marker: RequestMarker::Initiating,
            parts,
            ..
        } if parts.iter().any(|part| matches!(part, ObservationPart::Text(text) if text == "SENSITIVE-legacy-user"))
    )));

    let timeline_user = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("user-1"))
        .unwrap();
    assert!(timeline_user.timestamp.is_some());
    assert_ne!(
        chat_user.branch.partition(),
        timeline_user.branch.partition()
    );

    let call_partitions: BTreeSet<_> =
        inspected
            .observations
            .iter()
            .filter(|observation| {
                observation.facets.iter().any(|facet| matches!(
            facet,
            ObservationFacet::ToolCall { native_call_id, .. } if native_call_id == "call-1"
        ))
            })
            .map(|observation| observation.branch.partition())
            .collect();
    assert_eq!(call_partitions.len(), 2);
}

#[test]
fn progress_and_cancellation_remain_scoped_to_native_call_id() {
    let values = [
        json!({"type":"session.start","id":"s","data":{"sessionId":"session"}}),
        json!({"type":"tool.execution_start","id":"a","parentId":"s","data":{"toolCallId":"call","toolName":"bash","turnId":"turn","arguments":{"command":"false"}}}),
        json!({"type":"tool.execution_progress","id":"p","parentId":"a","data":{"toolCallId":"call","progressMessage":"running"}}),
        json!({"type":"tool.execution_complete","id":"z","parentId":"p","data":{"toolCallId":"call","cancelled":true,"success":false}}),
    ];
    let records: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(index, value)| NativeRecord {
            locator: NativeLocator::Jsonl {
                offset: index as u64,
            },
            bytes: serde_json::to_vec(value).unwrap(),
        })
        .collect();
    let source = source(DESCRIPTOR.id, "copilot-cli-events-jsonl", "events-r2");
    let inspected = CopilotCliAdapter
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

    assert!(
        inspected.observations[1]
            .facets
            .iter()
            .any(|facet| matches!(
                facet,
                ObservationFacet::ToolCall { native_call_id, family: Some(family), .. }
                    if native_call_id == "call" && family == "shell"
            ))
    );
    assert!(inspected.observations[2].facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolProgress { native_call_id, parts }
            if native_call_id == "call"
                && parts.iter().any(|part| matches!(part, ObservationPart::Text(text) if text == "running"))
    )));
    assert!(
        inspected.observations[3]
            .facets
            .iter()
            .any(|facet| matches!(
                facet,
                ObservationFacet::ToolResult {
                    native_call_id,
                    outcome: Outcome::Cancelled,
                    exit_code: None,
                    reported_duration_ms: None,
                    ..
                } if native_call_id == "call"
            ))
    );
}

#[test]
fn declared_source_bounds_refuse_before_decoding() {
    let source = source(DESCRIPTOR.id, "copilot-cli-events-jsonl", "events-r1");
    let records = current_records();
    let limits = QueryLimits {
        max_source_bytes: 1,
        max_total_input_bytes: 1,
        ..QueryLimits::default()
    };
    let failure = match CopilotCliAdapter.inspect(
        NativeQueryInput::Records {
            source: &source,
            records: &records,
        },
        ContentAccess::default(),
        &limits,
    ) {
        Ok(_) => panic!("oversized source unexpectedly inspected"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.kind(),
        unisphere_core::query::QueryFailureCode::ResourceLimit
    );
}
