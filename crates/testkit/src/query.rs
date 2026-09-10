use std::{collections::BTreeSet, path::PathBuf, sync::Mutex};

use unisphere_core::query::{
    AdapterId, AssociationBasis, AssociationExtent, AssociationObservation, AvailabilityCode,
    AvailabilityIssue, BranchEvidence, ContentAccess, HarnessId, InspectedSource, MembershipPolicy,
    MessageRole, NativeLocator, NativeSequence, Observation, ObservationFacet, ObservationPart,
    PartitionId, QueryApi, QueryFailure, QueryInput, QueryLimits, QueryRequest, QueryResponse,
    QueryScope, QuerySource, RequestMarker, SessionEvidenceKey, SourceEvidence, SourceLocator,
    SourcePartition, SourceReadStatus, SourceRef, SourceSelection, SourceViewKind, Timestamp,
    TimestampBasis,
};

#[derive(Clone, PartialEq, Eq)]
pub struct QueryLoadCall {
    pub scope: QueryScope,
    pub selection: SourceSelection,
    pub limits: QueryLimits,
    pub access: ContentAccess,
}

/// Deterministic query source with an observable pre-I/O call boundary.
pub struct FakeQuerySource {
    result: Result<QueryInput, QueryFailure>,
    calls: Mutex<Vec<QueryLoadCall>>,
}

impl FakeQuerySource {
    pub fn new(result: Result<QueryInput, QueryFailure>) -> Self {
        Self {
            result,
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<QueryLoadCall> {
        self.calls
            .lock()
            .expect("fake query source call log poisoned")
            .clone()
    }
}

impl QuerySource for FakeQuerySource {
    fn load(
        &self,
        scope: &QueryScope,
        selection: &SourceSelection,
        limits: &QueryLimits,
        access: ContentAccess,
    ) -> Result<QueryInput, QueryFailure> {
        self.calls
            .lock()
            .expect("fake query source call log poisoned")
            .push(QueryLoadCall {
                scope: scope.clone(),
                selection: selection.clone(),
                limits: *limits,
                access,
            });
        self.result.clone()
    }
}

/// Deterministic public API fake for CLI, writer, and external-consumer tests.
pub struct FakeQueryApi {
    result: Result<QueryResponse, QueryFailure>,
    requests: Mutex<Vec<QueryRequest>>,
}

impl FakeQueryApi {
    pub fn new(result: Result<QueryResponse, QueryFailure>) -> Self {
        Self {
            result,
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn requests(&self) -> Vec<QueryRequest> {
        self.requests
            .lock()
            .expect("fake query API request log poisoned")
            .clone()
    }
}

impl QueryApi for FakeQueryApi {
    fn execute(&self, request: &QueryRequest) -> Result<QueryResponse, QueryFailure> {
        self.requests
            .lock()
            .expect("fake query API request log poisoned")
            .push(request.clone());
        self.result.clone()
    }
}

/// Typed native evidence shared by query adapters and external SDK consumers.
pub fn shared_query_fixture() -> Vec<InspectedSource> {
    let readable_id = unisphere_core::query::SourceId::derive([&b"fixture-readable"[..]]);
    let readable_partition = PartitionId::derive(readable_id, b"session-readable");
    let readable_association = AssociationObservation {
        basis: AssociationBasis::NativeGitRoot,
        path: Some(PathBuf::from("/fixtures/project")),
        partition: readable_partition,
        applies_to: AssociationExtent::Partition,
    };
    let readable_source = SourceEvidence {
        id: readable_id,
        adapter: AdapterId::new("claude-jsonl").expect("valid fixture adapter"),
        harness: HarnessId::new("claude").expect("valid fixture harness"),
        representation: "claude-jsonl-v1".into(),
        locator: SourceLocator::LocalPath(PathBuf::from("/fixtures/claude/session.jsonl")),
        revision: "readable-r1".into(),
        query_policy_version: "policy-v1".into(),
        read_status: SourceReadStatus::Readable,
        associations: vec![readable_association.clone()],
        available_fields: BTreeSet::from([
            unisphere_core::query::FieldId::Id,
            unisphere_core::query::FieldId::SourceRefs,
            unisphere_core::query::FieldId::NativeId,
            unisphere_core::query::FieldId::Harness,
            unisphere_core::query::FieldId::Adapter,
            unisphere_core::query::FieldId::Availability,
            unisphere_core::query::FieldId::Timestamp,
            unisphere_core::query::FieldId::Role,
            unisphere_core::query::FieldId::Parts,
        ]),
    };
    let readable_session = SessionEvidenceKey {
        namespace: "claude-session".into(),
        native_id: "session-readable".into(),
        participant_id: Some("assistant-main".into()),
        parent_native_id: None,
        fork_native_id: None,
        membership_basis: MembershipPolicy::NativeContainment,
    };

    let partial_id = unisphere_core::query::SourceId::derive([&b"fixture-partial"[..]]);
    let partial_partition = PartitionId::derive(partial_id, b"session-partial");
    let partial_source = SourceEvidence {
        id: partial_id,
        adapter: AdapterId::new("codex-jsonl").expect("valid fixture adapter"),
        harness: HarnessId::new("codex").expect("valid fixture harness"),
        representation: "codex-jsonl-v1".into(),
        locator: SourceLocator::Provided("synthetic-partial-input".into()),
        revision: "partial-r1".into(),
        query_policy_version: "policy-v1".into(),
        read_status: SourceReadStatus::Partial,
        associations: Vec::new(),
        available_fields: BTreeSet::from([
            unisphere_core::query::FieldId::Id,
            unisphere_core::query::FieldId::SourceRefs,
            unisphere_core::query::FieldId::Harness,
            unisphere_core::query::FieldId::Adapter,
            unisphere_core::query::FieldId::Availability,
            unisphere_core::query::FieldId::Role,
            unisphere_core::query::FieldId::Parts,
        ]),
    };
    let partial_issue = AvailabilityIssue {
        code: AvailabilityCode::Partial,
        field: Some(unisphere_core::query::FieldId::Parts),
        source: Some(partial_id),
        entity: None,
        offset: Some(0),
    };

    vec![
        InspectedSource {
            partitions: vec![SourcePartition {
                id: readable_partition,
                native_session_id: Some("session-readable".into()),
                participant_id: Some("assistant-main".into()),
                view: SourceViewKind::Conversation,
                membership: MembershipPolicy::NativeContainment,
                associations: vec![readable_association],
            }],
            observations: vec![
                Observation {
                    source_ref: SourceRef {
                        source_id: readable_id,
                        revision: readable_source.revision.clone(),
                        locator: NativeLocator::Jsonl { offset: 0 },
                        subrecord: "message-user".into(),
                    },
                    native_record_id: Some("message-user".into()),
                    session: Some(readable_session.clone()),
                    branch: BranchEvidence::Linear {
                        partition: readable_partition,
                    },
                    parent_ids: Vec::new(),
                    sequence: NativeSequence {
                        version: 1,
                        key: vec![0],
                    },
                    timestamp: Some(
                        Timestamp::parse("2026-09-01T12:00:00Z", TimestampBasis::SourceReported)
                            .expect("valid fixture timestamp"),
                    ),
                    facets: vec![ObservationFacet::Message {
                        native_id: Some("message-user".into()),
                        role: MessageRole::User,
                        parts: vec![ObservationPart::Text("fixture user request".into())],
                        request_marker: RequestMarker::Initiating,
                        turn_id: Some("turn-1".into()),
                    }],
                    diagnostics: Vec::new(),
                },
                Observation {
                    source_ref: SourceRef {
                        source_id: readable_id,
                        revision: readable_source.revision.clone(),
                        locator: NativeLocator::Jsonl { offset: 128 },
                        subrecord: "message-tool-response".into(),
                    },
                    native_record_id: Some("message-tool-response".into()),
                    session: Some(readable_session),
                    branch: BranchEvidence::Linear {
                        partition: readable_partition,
                    },
                    parent_ids: vec!["message-user".into()],
                    sequence: NativeSequence {
                        version: 1,
                        key: vec![1],
                    },
                    timestamp: Some(
                        Timestamp::parse(
                            "2026-09-01T12:00:01Z",
                            TimestampBasis::DerivedFromSupportedNative,
                        )
                        .expect("valid fixture timestamp"),
                    ),
                    facets: vec![ObservationFacet::Message {
                        native_id: Some("message-tool-response".into()),
                        role: MessageRole::Tool,
                        parts: vec![ObservationPart::Structured(serde_json::json!({
                            "status": "ok"
                        }))],
                        request_marker: RequestMarker::ToolResponse,
                        turn_id: Some("turn-1".into()),
                    }],
                    diagnostics: Vec::new(),
                },
            ],
            issues: Vec::new(),
            source: readable_source,
        },
        InspectedSource {
            partitions: vec![SourcePartition {
                id: partial_partition,
                native_session_id: Some("session-partial".into()),
                participant_id: None,
                view: SourceViewKind::SourceOnly,
                membership: MembershipPolicy::Unavailable,
                associations: Vec::new(),
            }],
            observations: vec![Observation {
                source_ref: SourceRef {
                    source_id: partial_id,
                    revision: partial_source.revision.clone(),
                    locator: NativeLocator::Jsonl { offset: 0 },
                    subrecord: "partial-message".into(),
                },
                native_record_id: Some("partial-message".into()),
                session: None,
                branch: BranchEvidence::Unavailable {
                    partition: partial_partition,
                    reason: AvailabilityCode::Partial,
                },
                parent_ids: Vec::new(),
                sequence: NativeSequence {
                    version: 1,
                    key: vec![0],
                },
                timestamp: None,
                facets: vec![ObservationFacet::Message {
                    native_id: Some("partial-message".into()),
                    role: MessageRole::Assistant,
                    parts: vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)],
                    request_marker: RequestMarker::Unknown,
                    turn_id: None,
                }],
                diagnostics: vec![partial_issue.clone()],
            }],
            issues: vec![partial_issue],
            source: partial_source,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use unisphere_core::query::QueryFailureCode;

    #[test]
    fn shared_fixture_validates_and_rejects_foreign_provenance() {
        let fixture = shared_query_fixture();
        for inspected in &fixture {
            assert!(inspected.validate(&QueryLimits::default()).is_ok());
        }

        let mut foreign = fixture[0].clone();
        foreign.observations[0].source_ref.source_id = fixture[1].source.id;
        assert_eq!(
            foreign
                .validate(&QueryLimits::default())
                .unwrap_err()
                .kind(),
            QueryFailureCode::InvalidData
        );
    }
}
