use std::{collections::BTreeSet, path::PathBuf};

use serde_json::{Value, json};
use unisphere_adapter_vscode_copilot::{QUERY_POLICY_VERSION, VsCodeCopilotAdapter};
use unisphere_core::{
    NativeSnapshot, SnapshotFormat, SnapshotRecord, SnapshotRef,
    query::{
        AdapterId, AssociationBasis, AssociationExtent, AssociationObservation, AvailabilityCode,
        ContentAccess, FieldId, HarnessId, MembershipPolicy, MessageRole, NativeQueryInput,
        ObservationFacet, ObservationPart, Outcome, PartitionId, QueryAdapter, QueryLimits,
        RequestMarker, SourceEvidence, SourceId, SourceLocator, SourceReadStatus, SourceViewKind,
    },
};

const V1: &[u8] = include_bytes!("fixtures/session-v1.json");
const V2: &[u8] = include_bytes!("fixtures/session-v2.json");
const V3: &[u8] = include_bytes!("fixtures/session-v3.json");
const JOURNAL: &[u8] = include_bytes!("fixtures/session-journal.jsonl");

fn document(bytes: &[u8]) -> NativeSnapshot {
    NativeSnapshot {
        source: SnapshotRef {
            path: "/synthetic/workspaceStorage/hash/chatSessions/session.json".into(),
            format: SnapshotFormat::JsonDocument,
            session_id: None,
        },
        revision: "document-r1".into(),
        records: vec![SnapshotRecord {
            key: "document".into(),
            bytes: bytes.to_vec(),
        }],
    }
}

fn native(value: Value) -> NativeSnapshot {
    document(&serde_json::to_vec(&value).expect("serialize synthetic fixture"))
}

fn journal() -> NativeSnapshot {
    NativeSnapshot {
        source: SnapshotRef {
            path: "/synthetic/workspaceStorage/hash/chatSessions/session.jsonl".into(),
            format: SnapshotFormat::JsonJournal,
            session_id: None,
        },
        revision: "journal-r9".into(),
        records: JOURNAL
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .enumerate()
            .map(|(index, line)| SnapshotRecord {
                key: format!("journal:{index}"),
                bytes: line.to_vec(),
            })
            .collect(),
    }
}

fn evidence(snapshot: &NativeSnapshot) -> SourceEvidence {
    SourceEvidence {
        id: SourceId::derive([
            b"vscode-query-fixture".as_slice(),
            snapshot.source.path.as_os_str().as_encoded_bytes(),
        ]),
        adapter: AdapterId::new("vscode-copilot").expect("valid adapter"),
        harness: HarnessId::new("copilot").expect("valid harness"),
        representation: match &snapshot.source.format {
            SnapshotFormat::JsonDocument => "vscode-chat-session-json-v1",
            SnapshotFormat::JsonJournal => "vscode-chat-session-journal-v1",
            SnapshotFormat::SqliteKeyValue { .. } => "unsupported",
        }
        .into(),
        locator: SourceLocator::LocalPath(snapshot.source.path.clone()),
        revision: snapshot.revision.clone(),
        query_policy_version: QUERY_POLICY_VERSION.into(),
        read_status: SourceReadStatus::Readable,
        associations: Vec::new(),
        available_fields: BTreeSet::new(),
    }
}

fn inspect(snapshot: &NativeSnapshot, emit_content: bool) -> unisphere_core::query::InspectedSource {
    let source = evidence(snapshot);
    VsCodeCopilotAdapter
        .inspect(
            NativeQueryInput::Snapshot {
                source: &source,
                snapshot,
            },
            ContentAccess {
                inspect_fields: BTreeSet::new(),
                emit_content,
            },
            &QueryLimits::default(),
        )
        .expect("inspect synthetic VS Code snapshot")
}

#[test]
fn all_document_versions_use_native_request_containment_and_revision_provenance() {
    for (bytes, expected_session) in [
        (V1, "synthetic-vscode-v1"),
        (V2, "synthetic-vscode-v2"),
        (V3, "synthetic-vscode-v3"),
    ] {
        let snapshot = document(bytes);
        let inspected = inspect(&snapshot, true);
        assert_eq!(inspected.partitions.len(), 1);
        let partition = &inspected.partitions[0];
        assert_eq!(partition.native_session_id.as_deref(), Some(expected_session));
        assert!(partition.view == SourceViewKind::Conversation);
        assert!(partition.membership == MembershipPolicy::NativeContainment);

        let messages: Vec<_> = inspected
            .observations
            .iter()
            .flat_map(|observation| {
                observation.facets.iter().filter_map(move |facet| match facet {
                    ObservationFacet::Message {
                        role,
                        request_marker,
                        turn_id,
                        ..
                    } => Some((observation, role, request_marker, turn_id)),
                    _ => None,
                })
            })
            .collect();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].1 == &MessageRole::User);
        assert!(messages[0].2 == &RequestMarker::Initiating);
        assert!(messages[1].1 == &MessageRole::Assistant);
        assert_eq!(messages[0].3, messages[1].3);
        for (observation, _, _, _) in messages {
            assert_eq!(observation.source_ref.revision, snapshot.revision);
            assert!(matches!(&observation.source_ref.locator, unisphere_core::query::NativeLocator::Snapshot { .. }));
            assert_eq!(observation.sequence.version, 1);
            let session = observation.session.as_ref().expect("native session containment");
            assert_eq!(session.namespace, "vscode-chat-session");
            assert_eq!(session.native_id, expected_session);
            assert!(session.membership_basis == MembershipPolicy::NativeContainment);
        }
    }
}

#[test]
fn reduced_journal_exposes_only_the_current_revision() {
    let snapshot = journal();
    let inspected = inspect(&snapshot, true);
    assert!(inspected.observations.iter().all(|observation| {
        observation.source_ref.revision == "journal-r9"
            && observation.source_ref.subrecord.starts_with("journal:reduced")
            && observation.native_record_id.as_deref() != Some("removed-request")
    }));
    let text: Vec<_> = inspected
        .observations
        .iter()
        .flat_map(|observation| observation.facets.iter())
        .filter_map(|facet| match facet {
            ObservationFacet::Message { parts, .. } => Some(parts),
            _ => None,
        })
        .flatten()
        .filter_map(|part| match part {
            ObservationPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(text.contains(&"SENSITIVE-FINAL-TEXT"));
    assert!(!text.contains(&"SENSITIVE-OBSOLETE-TEXT"));
    assert!(!text.contains(&"SENSITIVE-REMOVED-REQUEST"));

    let lifecycle = inspected
        .observations
        .iter()
        .flat_map(|observation| observation.facets.iter())
        .find_map(|facet| match facet {
            ObservationFacet::ToolProgress {
                native_call_id,
                parts,
            } if native_call_id == "journal-tool" => Some(parts),
            _ => None,
        })
        .expect("serialized tool lifecycle facts");
    assert!(matches!(
        lifecycle.as_slice(),
        [ObservationPart::Structured(value)] if value["is_complete"] == true && value["is_confirmed"] == true
    ));
}

#[test]
fn serialized_tool_status_is_not_execution_success() {
    let inspected = inspect(&document(V3), true);
    let tool = inspected
        .observations
        .iter()
        .find(|observation| {
            observation.facets.iter().any(|facet| {
                matches!(facet, ObservationFacet::ToolCall { native_call_id, .. } if native_call_id == "tool-call-v3")
            })
        })
        .expect("serialized tool observation");
    assert!(tool.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolCall {
            native_call_id,
            native_name,
            family: Some(family),
            input,
            turn_id: Some(_),
        } if native_call_id == "tool-call-v3"
            && native_name == "read_file"
            && family == "file-read"
            && input == &vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)]
    )));
    assert!(tool.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolProgress { native_call_id, parts }
            if native_call_id == "tool-call-v3"
                && matches!(parts.as_slice(), [ObservationPart::Structured(value)]
                    if value["is_complete"] == true && value["confirmation_kind"] == 1)
    )));
    assert!(tool.facets.iter().any(|facet| match facet {
        ObservationFacet::ToolResult {
            native_call_id,
            output,
            outcome,
            exit_code,
            reported_duration_ms,
            ..
        } => native_call_id == "tool-call-v3"
            && outcome == &Outcome::Unknown
            && exit_code.is_none()
            && reported_duration_ms.is_none()
            && matches!(output.as_slice(), [ObservationPart::Structured(value)]
                if value["output"] == "SENSITIVE-NATIVE-OUTPUT-DISPLAY"),
        _ => false,
    }));
    assert!(inspected.issues.iter().any(|issue| {
        issue.code == AvailabilityCode::NotCaptured && issue.field == Some(FieldId::DurationMs)
    }));
}

#[test]
fn metadata_only_retains_no_message_or_tool_payload() {
    let inspected = inspect(&document(V3), false);
    for facet in inspected
        .observations
        .iter()
        .flat_map(|observation| observation.facets.iter())
    {
        match facet {
            ObservationFacet::Message { parts, .. } => assert!(parts.iter().all(|part| {
                matches!(
                    part,
                    ObservationPart::Unavailable(
                        AvailabilityCode::SensitiveOmitted | AvailabilityCode::NotSupported
                    )
                )
            })),
            ObservationFacet::ToolResult { output, .. } => assert!(
                output
                    == &vec![ObservationPart::Unavailable(
                        AvailabilityCode::SensitiveOmitted
                    )]
            ),
            ObservationFacet::SessionMetadata { name, .. } => assert!(name.is_none()),
            _ => {}
        }
    }
}

#[test]
fn only_native_or_verified_workspace_metadata_creates_associations() {
    let with_native_cwd = native(json!({
        "version": 3,
        "sessionId": "workspace-session",
        "workingDirectory": "file:///Users/example/repo%20one",
        "requests": []
    }));
    let inspected = inspect(&with_native_cwd, false);
    assert!(inspected.partitions[0].associations.iter().any(|association| {
        association.basis == AssociationBasis::NativeCwd
            && association.path.as_deref() == Some(std::path::Path::new("/Users/example/repo one"))
            && matches!(&association.applies_to, AssociationExtent::Partition)
    }));

    let hash_only = native(json!({
        "version": 3,
        "sessionId": "hash-only",
        "repoData": {"remoteUrl": "https://example.invalid/repo.git"},
        "requests": []
    }));
    let unassociated = inspect(&hash_only, false);
    assert!(unassociated.partitions[0].associations.is_empty());
    assert!(unassociated.issues.iter().any(|issue| {
        issue.code == AvailabilityCode::Unassociated
            && issue.field == Some(FieldId::ProjectPath)
    }));

    let snapshot = native(json!({
        "version": 3,
        "sessionId": "verified-session",
        "requests": []
    }));
    let mut source = evidence(&snapshot);
    let partition = PartitionId::derive(source.id, b"verified-session");
    source.associations.push(AssociationObservation {
        basis: AssociationBasis::VerifiedWorkspaceMetadata,
        path: Some(PathBuf::from("/verified/project")),
        partition,
        applies_to: AssociationExtent::Partition,
    });
    let verified = VsCodeCopilotAdapter
        .inspect(
            NativeQueryInput::Snapshot {
                source: &source,
                snapshot: &snapshot,
            },
            ContentAccess::default(),
            &QueryLimits::default(),
        )
        .expect("preserve provider-verified workspace metadata");
    assert!(verified.partitions[0].associations == source.associations);
}

#[test]
fn system_initiated_request_is_not_an_initiating_user_turn() {
    let inspected = inspect(
        &native(json!({
            "version": 3,
            "sessionId": "injected",
            "requests": [{
                "requestId": "system-request",
                "message": "injected context",
                "isSystemInitiated": true,
                "response": []
            }]
        })),
        false,
    );
    assert!(inspected.observations.iter().flat_map(|observation| observation.facets.iter()).any(
        |facet| matches!(
            facet,
            ObservationFacet::Message {
                request_marker: RequestMarker::Injected,
                ..
            }
        )
    ));
    assert!(!inspected.observations.iter().flat_map(|observation| observation.facets.iter()).any(
        |facet| matches!(
            facet,
            ObservationFacet::Message {
                request_marker: RequestMarker::Initiating,
                ..
            }
        )
    ));
}

#[test]
fn unknown_session_schema_stays_source_only_and_unsupported() {
    let inspected = inspect(
        &native(json!({
            "version": 4,
            "sessionId": "future",
            "requests": [{"message": "must not be interpreted"}]
        })),
        true,
    );
    assert!(inspected.source.read_status == SourceReadStatus::Unsupported);
    assert!(inspected.partitions.is_empty());
    assert!(inspected.observations.is_empty());
    assert!(inspected
        .issues
        .iter()
        .any(|issue| issue.code == AvailabilityCode::NotSupported));
}
