use std::collections::BTreeSet;

use unisphere_adapter_pi::PiAdapter;
use unisphere_core::query::{
    AdapterId, AssociationBasis, AvailabilityCode, BranchEvidence, ContentAccess, ControlKind,
    FieldId, HarnessId, LineageKind, MembershipPolicy, MessageRole, NativeLocator,
    NativeQueryInput, ObservationFacet, ObservationPart, QueryAdapter, QueryLimits, RequestMarker,
    SourceEvidence, SourceId, SourceLocator, SourceReadStatus, SourceViewKind, UsageScope,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/v3-tree.jsonl");

fn source() -> SourceEvidence {
    SourceEvidence {
        id: SourceId::derive([&b"pi-query-fixture"[..]]),
        adapter: AdapterId::new("pi").unwrap(),
        harness: HarnessId::new("pi").unwrap(),
        representation: "pi-jsonl-v3".into(),
        locator: SourceLocator::LocalPath("/synthetic/pi/session.jsonl".into()),
        revision: "fixture-r1".into(),
        query_policy_version: "loader-placeholder".into(),
        read_status: SourceReadStatus::Readable,
        associations: Vec::new(),
        available_fields: BTreeSet::new(),
    }
}

fn records() -> Vec<unisphere_core::query::NativeRecord> {
    let mut offset = 0_u64;
    FIXTURE
        .split_inclusive(|byte| *byte == b'\n')
        .filter_map(|line| {
            let bytes = line.strip_suffix(b"\n").unwrap_or(line);
            if bytes.is_empty() {
                return None;
            }
            let record = unisphere_core::query::NativeRecord {
                locator: NativeLocator::Jsonl { offset },
                bytes: bytes.to_vec(),
            };
            offset += line.len() as u64;
            Some(record)
        })
        .collect()
}

fn inspect(access: ContentAccess) -> unisphere_core::query::InspectedSource {
    let source = source();
    let records = records();
    PiAdapter
        .inspect(
            NativeQueryInput::Records {
                source: &source,
                records: &records,
            },
            access,
            &QueryLimits::default(),
        )
        .unwrap()
}

#[test]
fn query_inspection_preserves_header_tree_conflicts_controls_calls_and_usage_scopes() {
    let inspected = inspect(ContentAccess::default());
    inspected.validate(&QueryLimits::default()).unwrap();
    assert_eq!(inspected.source.query_policy_version, "pi-v3-query-v1");
    assert_eq!(inspected.partitions.len(), 1);
    let partition = &inspected.partitions[0];
    assert_eq!(partition.native_session_id.as_deref(), Some("session-a"));
    assert_eq!(partition.view, SourceViewKind::Conversation);
    assert_eq!(partition.membership, MembershipPolicy::ValidatedHeader);
    assert!(partition.associations.iter().any(|association| {
        association.basis == AssociationBasis::NativeCwd
            && association.path.as_deref()
                == Some(std::path::Path::new("/SENSITIVE-project"))
    }));

    let user = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("user-a"))
        .unwrap();
    assert!(user.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Message {
            native_id: Some(id),
            role: MessageRole::User,
            request_marker: RequestMarker::Initiating,
            parts,
            ..
        } if id == "user-a" && parts.iter().all(|part| matches!(part, ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)))
    )));

    let assistant = inspected
        .observations
        .iter()
        .find(|observation| {
            observation.native_record_id.as_deref() == Some("assistant-a")
                && observation.facets.iter().any(|facet| matches!(facet, ObservationFacet::ToolCall { .. }))
        })
        .unwrap();
    assert!(matches!(
        &assistant.branch,
        BranchEvidence::Node { parent: Some(parent), .. } if parent == "user-a"
    ));
    assert!(assistant.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolCall { native_call_id, native_name, family: Some(family), .. }
            if native_call_id == "call-a" && native_name == "read" && family == "file-read"
    )));
    assert!(assistant.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Usage { scope: UsageScope::Turn, counters, .. }
            if counters.input_tokens == Some(11) && counters.cache_write_tokens == Some(5)
    )));

    let result = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("tool-a"))
        .unwrap();
    assert!(result.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Message { role: MessageRole::Tool, request_marker: RequestMarker::ToolResponse, .. }
    )));
    assert!(result.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Usage { scope: UsageScope::Invocation, counters, .. }
            if counters.input_tokens == Some(2)
    )));

    let compaction = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("compact-a"))
        .unwrap();
    assert!(compaction.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Control { kind: ControlKind::Compaction, links }
            if links.iter().any(|link| link.kind == LineageKind::FirstKept && link.target == "assistant-a")
    )));
    assert!(inspected.issues.iter().any(|issue| {
        issue.code == AvailabilityCode::Conflict && issue.field == Some(FieldId::NativeId)
    }));
    assert_eq!(
        inspected
            .observations
            .iter()
            .flat_map(|observation| observation.facets.iter())
            .filter(|facet| matches!(facet, ObservationFacet::Message { request_marker: RequestMarker::Initiating, .. }))
            .count(),
        1
    );
    assert!(inspected
        .observations
        .iter()
        .any(|observation| observation.source_ref.subrecord.ends_with(":message-clock")));
}

#[test]
fn content_opt_in_retains_supported_arguments_and_results_only() {
    let inspected = inspect(ContentAccess {
        inspect_fields: BTreeSet::new(),
        emit_content: true,
    });
    assert!(inspected
        .observations
        .iter()
        .flat_map(|observation| observation.facets.iter())
        .any(|facet| matches!(
            facet,
            ObservationFacet::ToolCall { input, .. }
                if matches!(input.as_slice(), [ObservationPart::Structured(value)] if value["path"] == "SENSITIVE-tool-argument")
        )));
    assert!(inspected
        .observations
        .iter()
        .flat_map(|observation| observation.facets.iter())
        .any(|facet| matches!(
            facet,
            ObservationFacet::ToolResult { output, .. }
                if output.iter().any(|part| matches!(part, ObservationPart::Text(text) if text == "SENSITIVE-tool-result"))
        )));
}
