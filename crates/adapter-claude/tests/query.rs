use std::{collections::BTreeSet, path::PathBuf};

use serde_json::json;
use unisphere_adapter_claude::ClaudeCodeAdapter;
use unisphere_core::{
    MappingOptions, NativeRecord as MappingRecord, SessionAdapter, SessionRef,
    query::{
        AdapterId, AssociationBasis, AvailabilityCode, ContentAccess, HarnessId, MembershipPolicy,
        MessageRole, NativeLocator, NativeQueryInput, NativeRecord, ObservationFacet,
        ObservationPart, Outcome, QueryAdapter, QueryLimits, RequestMarker, SourceEvidence,
        SourceId, SourceLocator, SourceReadStatus, UsageScope,
    },
};

fn source() -> SourceEvidence {
    SourceEvidence {
        id: SourceId::derive([&b"claude-query-test"[..]]),
        adapter: AdapterId::new("claude-jsonl").unwrap(),
        harness: HarnessId::new("claude").unwrap(),
        representation: "claude-jsonl-v1".into(),
        locator: SourceLocator::LocalPath(PathBuf::from("/fixtures/claude.jsonl")),
        revision: "r1".into(),
        query_policy_version: "claude-query-v1".into(),
        read_status: SourceReadStatus::Readable,
        associations: Vec::new(),
        available_fields: BTreeSet::new(),
    }
}

fn records() -> Vec<NativeRecord> {
    [
        json!({"type":"user","uuid":"u1","sessionId":"s1","cwd":"/repo","timestamp":"2026-09-01T00:00:00Z","message":{"role":"user","content":"prompt"}}),
        json!({"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":"s1","message":{"role":"assistant","id":"m1","usage":{"input_tokens":3,"output_tokens":2},"content":[{"type":"tool_use","id":"call-1","name":"Read","input":{"path":"secret"}}]}}),
        json!({"type":"user","uuid":"u2","parentUuid":"a1","sessionId":"s1","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"call-1","is_error":false,"content":"result"}]}}),
        json!({"type":"user","uuid":"u3","sessionId":"s1","isMeta":true,"message":{"role":"user","content":"context"}}),
        json!({"type":"user","uuid":"u4","sessionId":"s1","isCompactSummary":true,"message":{"role":"user","content":"summary"}}),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, value)| NativeRecord {
        locator: NativeLocator::Jsonl {
            offset: (index * 100) as u64,
        },
        bytes: serde_json::to_vec(&value).unwrap(),
    })
    .collect()
}

#[test]
fn query_preserves_native_membership_markers_calls_and_content_consent() {
    let adapter = ClaudeCodeAdapter;
    let source = source();
    let records = records();
    let inspected = adapter
        .inspect(
            NativeQueryInput::Records {
                source: &source,
                records: &records,
            },
            ContentAccess::default(),
            &QueryLimits::default(),
        )
        .unwrap();

    assert_eq!(inspected.partitions.len(), 1);
    assert_eq!(inspected.partitions[0].native_session_id.as_deref(), Some("s1"));
    assert_eq!(inspected.partitions[0].membership, MembershipPolicy::NativeContainment);
    assert!(inspected.source.associations.iter().any(|association| {
        association.basis == AssociationBasis::NativeCwd
            && association.path.as_deref() == Some(std::path::Path::new("/repo"))
    }));

    let message = |index| {
        inspected.observations[index]
            .facets
            .iter()
            .find_map(|facet| match facet {
                ObservationFacet::Message {
                    role,
                    parts,
                    request_marker,
                    ..
                } => Some((*role, parts, *request_marker)),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(message(0).0, MessageRole::User);
    assert_eq!(message(0).2, RequestMarker::Initiating);
    assert_eq!(message(2).2, RequestMarker::ToolResponse);
    assert_eq!(message(3).2, RequestMarker::Injected);
    assert_eq!(message(4).2, RequestMarker::Summary);
    assert!(matches!(message(0).1.as_slice(), [ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)]));

    assert!(inspected.observations[1].facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolCall { native_call_id, native_name, family, input, .. }
            if native_call_id == "call-1" && native_name == "Read"
                && family.as_deref() == Some("file-read")
                && matches!(input.as_slice(), [ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)])
    )));
    assert!(inspected.observations[2].facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolResult { native_call_id, outcome, .. }
            if native_call_id == "call-1" && *outcome == Outcome::Succeeded
    )));
    assert!(inspected.observations[1].facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Usage { scope: UsageScope::Invocation, .. }
    )));

    let content = adapter
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
    assert!(matches!(
        &content.observations[0].facets[0],
        ObservationFacet::Message { parts, .. }
            if matches!(parts.as_slice(), [ObservationPart::Text(text)] if text == "prompt")
    ));
}

#[test]
fn query_inspection_does_not_change_otlp_mapping() {
    let adapter = ClaudeCodeAdapter;
    let source = source();
    let records = records();
    let session = SessionRef {
        path: PathBuf::from("/fixtures/claude.jsonl"),
    };
    let mapping_records: Vec<_> = records
        .iter()
        .map(|record| MappingRecord {
            offset: match &record.locator {
                NativeLocator::Jsonl { offset } => *offset,
                _ => unreachable!(),
            },
            bytes: record.bytes.clone(),
        })
        .collect();
    let before = adapter
        .map(&session, &mapping_records, MappingOptions::default())
        .unwrap();
    adapter
        .inspect(
            NativeQueryInput::Records {
                source: &source,
                records: &records,
            },
            ContentAccess::default(),
            &QueryLimits::default(),
        )
        .unwrap();
    let after = adapter
        .map(&session, &mapping_records, MappingOptions::default())
        .unwrap();
    assert!(before == after);
}
