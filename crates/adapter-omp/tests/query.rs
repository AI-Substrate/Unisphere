use std::collections::BTreeSet;

use unisphere_adapter_omp::OmpAdapter;
use unisphere_core::query::{
    AdapterId, AssociationBasis, AvailabilityCode, BranchEvidence, ContentAccess, ControlKind,
    HarnessId, LineageKind, MembershipPolicy, MessageRole, NativeLocator, NativeQueryInput,
    ObservationFacet, ObservationPart, QueryAdapter, QueryLimits, RequestMarker, SourceEvidence,
    SourceId, SourceLocator, SourceReadStatus, SourceViewKind, UsageScope,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/native.jsonl");

fn source() -> SourceEvidence {
    SourceEvidence {
        id: SourceId::derive([&b"omp-query-fixture"[..]]),
        adapter: AdapterId::new("oh-my-pi").unwrap(),
        harness: HarnessId::new("oh-my-pi").unwrap(),
        representation: "oh-my-pi-jsonl-v3".into(),
        locator: SourceLocator::LocalPath("/synthetic/omp/session.jsonl".into()),
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
    OmpAdapter
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
fn query_inspection_preserves_header_tree_control_calls_and_native_facts() {
    let inspected = inspect(ContentAccess::default());
    inspected.validate(&QueryLimits::default()).unwrap();
    assert_eq!(
        inspected.source.query_policy_version,
        "oh-my-pi-v3-query-v1"
    );
    assert_eq!(inspected.partitions.len(), 1);
    let partition = &inspected.partitions[0];
    assert_eq!(partition.native_session_id.as_deref(), Some("session-1"));
    assert_eq!(partition.view, SourceViewKind::Conversation);
    assert_eq!(partition.membership, MembershipPolicy::ValidatedHeader);
    assert!(partition.associations.iter().any(|association| {
        association.basis == AssociationBasis::NativeCwd
            && association.path.as_deref() == Some(std::path::Path::new("/SENSITIVE-project"))
    }));

    let user = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("u1"))
        .unwrap();
    assert!(matches!(
        &user.branch,
        BranchEvidence::Node { parent: None, .. }
    ));
    assert!(user.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Message {
            native_id: Some(id),
            role: MessageRole::User,
            request_marker: RequestMarker::Initiating,
            parts,
            ..
        } if id == "u1" && matches!(parts.as_slice(), [ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)])
    )));

    let assistant = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("a1"))
        .unwrap();
    assert!(assistant.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolCall { native_call_id, native_name, family: Some(family), input, .. }
            if native_call_id == "call-1" && native_name == "read" && family == "file-read"
                && matches!(input.as_slice(), [ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)])
    )));
    assert!(assistant.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Usage { scope: UsageScope::CumulativeSnapshot, counters, .. }
            if counters.input_tokens == Some(10) && counters.cache_read_tokens == Some(8)
    )));

    let result = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("t1"))
        .unwrap();
    assert!(result.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Message {
            role: MessageRole::Tool,
            request_marker: RequestMarker::ToolResponse,
            ..
        }
    )));
    assert!(result.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolResult { native_call_id, reported_duration_ms: None, .. }
            if native_call_id == "call-1"
    )));

    let compaction = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("c1"))
        .unwrap();
    assert!(compaction.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Control { kind: ControlKind::Compaction, links }
            if links.iter().any(|link| link.kind == LineageKind::FirstKept && link.target == "a1")
    )));
    assert!(
        inspected
            .observations
            .iter()
            .any(|observation| observation.source_ref.subrecord.ends_with(":message-clock"))
    );
    assert_eq!(
        inspected
            .observations
            .iter()
            .flat_map(|observation| observation.facets.iter())
            .filter(|facet| matches!(
                facet,
                ObservationFacet::Message {
                    request_marker: RequestMarker::Initiating,
                    ..
                }
            ))
            .count(),
        1
    );
}

#[test]
fn content_opt_in_retains_supported_payloads_without_turning_response_timing_into_tool_time() {
    let inspected = inspect(ContentAccess {
        inspect_fields: BTreeSet::new(),
        emit_content: true,
    });
    let assistant = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("a1"))
        .unwrap();
    assert!(assistant.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Message { parts, .. }
            if parts.iter().any(|part| matches!(part, ObservationPart::Text(text) if text == "SENSITIVE-answer"))
    )));
    assert!(assistant.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolCall { input, .. }
            if matches!(input.as_slice(), [ObservationPart::Structured(value)] if value["path"] == "/SENSITIVE-input")
    )));
    assert!(
        inspected
            .observations
            .iter()
            .flat_map(|observation| observation.facets.iter())
            .all(|facet| !matches!(
                facet,
                ObservationFacet::ToolResult {
                    reported_duration_ms: Some(_),
                    ..
                }
            ))
    );
}
