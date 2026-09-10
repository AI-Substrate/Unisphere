use std::{collections::BTreeSet, path::PathBuf};

use serde_json::json;
use unisphere_adapter_codex::CodexAdapter;
use unisphere_core::{
    MappingOptions, NativeRecord as MappingRecord, SessionAdapter, SessionRef,
    query::{
        AdapterId, AssociationBasis, AvailabilityCode, ContentAccess, ControlKind, HarnessId,
        MembershipPolicy, MessageRole, NativeLocator, NativeQueryInput, NativeRecord,
        ObservationFacet, ObservationPart, Outcome, QueryAdapter, QueryLimits, RequestMarker,
        SourceEvidence, SourceId, SourceLocator, SourceReadStatus, UsageScope,
    },
};

fn source() -> SourceEvidence {
    SourceEvidence {
        id: SourceId::derive([&b"codex-query-test"[..]]),
        adapter: AdapterId::new("codex-jsonl").unwrap(),
        harness: HarnessId::new("codex").unwrap(),
        representation: "codex-rollout-v1".into(),
        locator: SourceLocator::LocalPath(PathBuf::from("/fixtures/rollout.jsonl")),
        revision: "r1".into(),
        query_policy_version: "codex-query-v1".into(),
        read_status: SourceReadStatus::Readable,
        associations: Vec::new(),
        available_fields: BTreeSet::new(),
    }
}

fn records() -> Vec<NativeRecord> {
    [
        json!({"timestamp":"2026-09-01T00:00:00Z","type":"session_meta","payload":{"id":"thread-1","session_id":"root-1","cwd":"/repo"}}),
        json!({"timestamp":"2026-09-01T00:00:01Z","type":"turn_context","payload":{"turn_id":"turn-1","model":"gpt"}}),
        json!({"timestamp":"2026-09-01T00:00:02Z","type":"response_item","payload":{"type":"message","id":"msg-1","role":"user","content":[{"type":"input_text","text":"prompt"}]}}),
        json!({"timestamp":"2026-09-01T00:00:03Z","type":"event_msg","payload":{"type":"user_message","message":"duplicate prompt"}}),
        json!({"timestamp":"2026-09-01T00:00:04Z","type":"response_item","payload":{"type":"function_call","id":"item-1","call_id":"call-1","name":"read_file","arguments":"{\"path\":\"secret\"}"}}),
        json!({"timestamp":"2026-09-01T00:00:05Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"result"}}),
        json!({"timestamp":"2026-09-01T00:00:06Z","type":"event_msg","payload":{"type":"exec_command_end","call_id":"call-1","exit_code":0,"aggregated_output":"duplicate result"}}),
        json!({"timestamp":"2026-09-01T00:00:07Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":3,"cached_input_tokens":1,"output_tokens":2},"total_token_usage":{"input_tokens":30,"cached_input_tokens":10,"output_tokens":20}}}}),
        json!({"timestamp":"2026-09-01T00:00:08Z","type":"compacted","payload":{"message":"summary"}}),
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
fn query_uses_header_turn_boundaries_and_keeps_event_summaries_distinct() {
    let adapter = CodexAdapter;
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
    assert_eq!(
        inspected.partitions[0].native_session_id.as_deref(),
        Some("thread-1")
    );
    assert_eq!(
        inspected.partitions[0].membership,
        MembershipPolicy::ValidatedHeader
    );
    assert!(inspected.source.associations.iter().any(|association| {
        association.basis == AssociationBasis::NativeCwd
            && association.path.as_deref() == Some(std::path::Path::new("/repo"))
    }));
    assert!(matches!(
        &inspected.observations[2].facets[0],
        ObservationFacet::Message {
            role: MessageRole::User,
            request_marker: RequestMarker::Initiating,
            turn_id: Some(turn_id),
            parts,
            ..
        } if turn_id == "turn-1"
            && matches!(parts.as_slice(), [ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)])
    ));
    assert!(
        inspected.observations[3]
            .facets
            .iter()
            .all(|facet| !matches!(facet, ObservationFacet::Message { .. }))
    );
    assert!(
        inspected.observations[3]
            .facets
            .iter()
            .any(|facet| matches!(
                facet,
                ObservationFacet::Control {
                    kind: ControlKind::Summary,
                    ..
                }
            ))
    );
    assert!(inspected.observations[4].facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolCall { native_call_id, native_name, family, turn_id, input }
            if native_call_id == "call-1" && native_name == "read_file"
                && family.as_deref() == Some("file-read")
                && turn_id.as_deref() == Some("turn-1")
                && matches!(input.as_slice(), [ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)])
    )));
    assert!(
        inspected.observations[5]
            .facets
            .iter()
            .any(|facet| matches!(
                facet,
                ObservationFacet::ToolResult { native_call_id, outcome: Outcome::Unknown, .. }
                    if native_call_id == "call-1"
            ))
    );
    assert!(inspected.observations[6].facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::ToolResult { native_call_id, outcome: Outcome::Succeeded, exit_code: Some(0), .. }
            if native_call_id == "call-1"
    )));
    assert!(inspected.observations[7].facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Usage { scope: UsageScope::Turn, owner: Some(owner), .. } if owner == "turn-1"
    )));
    assert!(inspected.observations[7].facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::Usage { scope: UsageScope::CumulativeSnapshot, owner: Some(owner), .. } if owner == "thread-1"
    )));
    assert!(
        inspected.observations[8]
            .facets
            .iter()
            .any(|facet| matches!(
                facet,
                ObservationFacet::Control {
                    kind: ControlKind::Compaction,
                    ..
                }
            ))
    );
}

#[test]
fn query_content_opt_in_is_field_aware_and_otlp_mapping_stays_identical() {
    let adapter = CodexAdapter;
    let source = source();
    let records = records();
    let inspected = adapter
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
        &inspected.observations[2].facets[0],
        ObservationFacet::Message { parts, .. }
            if matches!(parts.as_slice(), [ObservationPart::Text(text)] if text == "prompt")
    ));

    let session = SessionRef {
        path: PathBuf::from("/fixtures/rollout.jsonl"),
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
