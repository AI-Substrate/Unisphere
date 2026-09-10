use std::collections::BTreeSet;

use serde_json::{Map, Value};
use unisphere_core::{
    GitNotesError,
    query::{
        AdapterId, AssociationBasis, AssociationExtent, AssociationObservation, AvailabilityCode,
        AvailabilityIssue, BranchEvidence, ContentAccess, Digest, FieldId, IdentityKind,
        InspectedSource, LimitKind, MembershipPolicy, NativeLocator, NativeQueryInput,
        NativeSequence, Observation, ObservationFacet, PartitionId, QueryAdapter,
        QueryErrorLocation, QueryFailure, QueryFailureCode, QueryLimits, RecoveryAction,
        SessionEvidenceKey, SourceEvidence, SourceLocator, SourcePartition, SourceProblem,
        SourceRef, SourceViewKind,
    },
};

use crate::{DESCRIPTOR, GitAiAdapter, NativeAttribution, classify_note};

/// Query reconstruction policy applied to supplied authorship/3.0.0 notes.
pub const QUERY_POLICY_VERSION: &str = "git-ai/authorship3/query-v1";
const SOURCE_ONLY_KEY: &[u8] = b"git-ai/authorship3/source-only";
const SESSION_NAMESPACE: &str = "git-ai/authorship3/session";

impl QueryAdapter for GitAiAdapter {
    fn inspect(
        &self,
        input: NativeQueryInput<'_>,
        access: ContentAccess,
        limits: &QueryLimits,
    ) -> Result<InspectedSource, QueryFailure> {
        limits.validate()?;
        let NativeQueryInput::ProvidedObject {
            source,
            bytes,
            locator,
        } = input
        else {
            return Err(unsupported_source(None));
        };
        source.validate()?;
        require_source(source)?;
        let NativeLocator::GitNote {
            repository_id,
            notes_tip,
            target_commit,
            note_blob,
            notes_ref,
        } = locator
        else {
            return Err(unsupported_source(Some(source.id)));
        };
        let SourceLocator::LocalPath(repository) = &source.locator else {
            return Err(unsupported_source(Some(source.id)));
        };
        unisphere_core::GitNoteRef {
            repository: repository.clone(),
            repository_id: repository_id.into(),
            notes_ref: notes_ref.clone(),
            notes_tip: notes_tip.clone(),
            target_commit: target_commit.clone(),
            note_blob: note_blob.clone(),
        }
        .validate()
        .map_err(|_| invalid_data(source))?;
        if source.revision != format!("{notes_tip}:{note_blob}") {
            return Err(invalid_data(source));
        }
        if bytes.len() > limits.max_source_bytes {
            return Err(QueryFailure::limit(LimitKind::SourceBytes));
        }
        if bytes.len() > limits.max_total_input_bytes {
            return Err(QueryFailure::limit(LimitKind::TotalInputBytes));
        }
        if (access.emit_content || !access.inspect_fields.is_empty())
            && bytes.len() > limits.max_retained_bytes
        {
            return Err(QueryFailure::limit(LimitKind::RetainedBytes));
        }

        let classified = classify_note(
            bytes,
            limits.max_observations_and_rows,
            limits.max_retained_bytes,
        )
        .map_err(|error| map_native_error(error, source))?;
        let identity_count = ["prompts", "sessions", "humans"]
            .into_iter()
            .filter_map(|name| classified.metadata.get(name).and_then(Value::as_object))
            .try_fold(0_usize, |total, identities| {
                total.checked_add(identities.len())
            })
            .ok_or_else(|| QueryFailure::limit(LimitKind::ObservationsAndRows))?;
        let observation_count = 1_usize
            .checked_add(identity_count)
            .and_then(|total| total.checked_add(classified.attributions.len()))
            .ok_or_else(|| QueryFailure::limit(LimitKind::ObservationsAndRows))?;
        if observation_count > limits.max_observations_and_rows {
            return Err(QueryFailure::limit(LimitKind::ObservationsAndRows));
        }

        let source_only = PartitionId::derive(source.id, SOURCE_ONLY_KEY);
        let repository_scope = Digest::of_bytes(repository_id.as_bytes()).to_string();
        let session_namespace = format!("{SESSION_NAMESPACE}/{repository_scope}");
        let mut partitions = Vec::new();
        partitions.push(SourcePartition {
            id: source_only,
            native_session_id: None,
            participant_id: None,
            view: SourceViewKind::SourceOnly,
            membership: MembershipPolicy::Unavailable,
            associations: vec![partition_association(source, source_only)?],
        });

        let sessions = classified
            .metadata
            .get("sessions")
            .and_then(Value::as_object);
        if let Some(sessions) = sessions {
            for key in sessions.keys() {
                let partition = session_partition(source, key);
                partitions.push(SourcePartition {
                    id: partition,
                    native_session_id: Some(key.clone()),
                    participant_id: None,
                    view: SourceViewKind::SourceOnly,
                    membership: MembershipPolicy::NativeContainment,
                    associations: vec![partition_association(source, partition)?],
                });
            }
        }

        let mut observations = Vec::with_capacity(observation_count);
        let mut sequence = 0_u64;
        observations.push(Observation {
            source_ref: source_ref(source, locator, "metadata/$note"),
            native_record_id: None,
            session: None,
            branch: BranchEvidence::Unavailable {
                partition: source_only,
                reason: AvailabilityCode::NotCaptured,
            },
            parent_ids: Vec::new(),
            sequence: native_sequence(sequence),
            timestamp: None,
            facets: vec![ObservationFacet::Control {
                kind: unisphere_core::query::ControlKind::Summary,
                links: Vec::new(),
            }],
            diagnostics: vec![timestamp_unavailable(source.id)],
        });
        sequence += 1;

        for map_name in ["prompts", "sessions", "humans"] {
            let Some(identities) = classified.metadata.get(map_name).and_then(Value::as_object)
            else {
                continue;
            };
            for (key, identity) in identities {
                let record = identity.as_object().ok_or_else(|| invalid_data(source))?;
                let (partition, session) = if map_name == "sessions" {
                    let partition = session_partition(source, key);
                    (partition, Some(session_key(&session_namespace, key)))
                } else {
                    (source_only, None)
                };
                let facets = if map_name == "sessions" {
                    vec![ObservationFacet::SessionMetadata {
                        native_id: key.clone(),
                        name: access
                            .permits_payload(FieldId::Name)
                            .then(|| agent_field(record, "id"))
                            .flatten(),
                        models: access
                            .permits_payload(FieldId::Models)
                            .then(|| agent_field(record, "model"))
                            .flatten()
                            .into_iter()
                            .collect(),
                        created_at: None,
                        associations: vec![partition_association(source, partition)?],
                        lineage: Vec::new(),
                    }]
                } else {
                    vec![ObservationFacet::Attribution {
                        identity_kind: identity_kind(map_name),
                        native_key: key.clone(),
                        declared_agent: declared_agent(record, map_name, &access),
                        target_commit: Some(target_commit.clone()),
                        ranges: Vec::new(),
                    }]
                };
                observations.push(Observation {
                    source_ref: source_ref(source, locator, &format!("metadata/{map_name}/{key}")),
                    native_record_id: Some(key.clone()),
                    session,
                    branch: BranchEvidence::Unavailable {
                        partition,
                        reason: AvailabilityCode::NotCaptured,
                    },
                    parent_ids: Vec::new(),
                    sequence: native_sequence(sequence),
                    timestamp: None,
                    facets,
                    diagnostics: vec![timestamp_unavailable(source.id)],
                });
                sequence += 1;
            }
        }

        for attribution in &classified.attributions {
            let declared = classified.identity(attribution.map_name, &attribution.identity_key);
            let resolved_session = (attribution.map_name == "sessions")
                .then_some(declared)
                .flatten()
                .map(|_| session_key(&session_namespace, &attribution.identity_key));
            let partition = resolved_session
                .as_ref()
                .map(|_| session_partition(source, &attribution.identity_key))
                .unwrap_or(source_only);
            observations.push(attribution_observation(
                source,
                locator,
                target_commit,
                attribution,
                declared,
                resolved_session,
                partition,
                sequence,
                &access,
            ));
            sequence += 1;
        }

        let mut output_source = source.clone();
        output_source.query_policy_version = QUERY_POLICY_VERSION.into();
        output_source.available_fields.extend(available_fields());
        output_source.associations = partitions
            .iter()
            .flat_map(|partition| partition.associations.iter().cloned())
            .collect();
        let issues = observations
            .iter()
            .flat_map(|observation| observation.diagnostics.iter().cloned())
            .collect();
        let inspected = InspectedSource {
            source: output_source,
            partitions,
            observations,
            issues,
        };
        inspected.validate(limits)?;
        Ok(inspected)
    }
}

#[allow(clippy::too_many_arguments)]
fn attribution_observation(
    source: &SourceEvidence,
    locator: &NativeLocator,
    target_commit: &str,
    attribution: &NativeAttribution,
    identity: Option<&Map<String, Value>>,
    session: Option<SessionEvidenceKey>,
    partition: PartitionId,
    sequence: u64,
    access: &ContentAccess,
) -> Observation {
    let range = if access.permits_payload(FieldId::Parts) {
        format!(
            "{}:{}-{}",
            attribution.file, attribution.start, attribution.end
        )
    } else {
        format!("{}-{}", attribution.start, attribution.end)
    };
    Observation {
        source_ref: source_ref(source, locator, &attribution.subrecord),
        native_record_id: Some(attribution.native_key.clone()),
        session,
        branch: BranchEvidence::Unavailable {
            partition,
            reason: AvailabilityCode::NotCaptured,
        },
        parent_ids: Vec::new(),
        sequence: native_sequence(sequence),
        timestamp: None,
        facets: vec![ObservationFacet::Attribution {
            identity_kind: identity_kind(attribution.map_name),
            native_key: attribution.identity_key.clone(),
            declared_agent: identity
                .and_then(|record| declared_agent(record, attribution.map_name, access)),
            target_commit: Some(target_commit.to_owned()),
            ranges: vec![range],
        }],
        diagnostics: vec![timestamp_unavailable(source.id)],
    }
}

fn require_source(source: &SourceEvidence) -> Result<(), QueryFailure> {
    if source.adapter.as_str() != DESCRIPTOR.id
        || source.harness.as_str() != DESCRIPTOR.id
        || source.representation != "git_notes"
        || !matches!(&source.locator, SourceLocator::LocalPath(_))
    {
        return Err(unsupported_source(Some(source.id)));
    }
    Ok(())
}

fn session_partition(source: &SourceEvidence, native_id: &str) -> PartitionId {
    PartitionId::derive(source.id, native_id.as_bytes())
}

fn session_key(namespace: &str, native_id: &str) -> SessionEvidenceKey {
    SessionEvidenceKey {
        namespace: namespace.to_owned(),
        native_id: native_id.to_owned(),
        participant_id: None,
        parent_native_id: None,
        fork_native_id: None,
        membership_basis: MembershipPolicy::NativeContainment,
    }
}

fn partition_association(
    source: &SourceEvidence,
    partition: PartitionId,
) -> Result<AssociationObservation, QueryFailure> {
    let path = source
        .associations
        .iter()
        .find(|association| {
            association.basis == AssociationBasis::GitRepositoryIdentity
                && association.path.is_some()
        })
        .and_then(|association| association.path.clone())
        .or_else(|| match &source.locator {
            SourceLocator::LocalPath(path) => Some(path.clone()),
            SourceLocator::Provided(_) => None,
        })
        .ok_or_else(|| invalid_data(source))?;
    if !path.is_absolute() {
        return Err(invalid_data(source));
    }
    Ok(AssociationObservation {
        basis: AssociationBasis::GitRepositoryIdentity,
        path: Some(path),
        partition,
        applies_to: AssociationExtent::Partition,
    })
}

fn source_ref(source: &SourceEvidence, locator: &NativeLocator, subrecord: &str) -> SourceRef {
    SourceRef {
        source_id: source.id,
        revision: source.revision.clone(),
        locator: locator.clone(),
        subrecord: subrecord.to_owned(),
    }
}

fn native_sequence(index: u64) -> NativeSequence {
    NativeSequence {
        version: 1,
        key: index.to_be_bytes().to_vec(),
    }
}

fn identity_kind(map_name: &str) -> IdentityKind {
    if map_name == "humans" {
        IdentityKind::Human
    } else {
        IdentityKind::Agent
    }
}

fn agent_field(record: &Map<String, Value>, field: &str) -> Option<String> {
    record
        .get("agent_id")
        .and_then(Value::as_object)
        .and_then(|agent| agent.get(field))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn declared_agent(
    record: &Map<String, Value>,
    map_name: &str,
    access: &ContentAccess,
) -> Option<String> {
    if !access.permits_payload(FieldId::Parts) {
        return None;
    }
    if map_name == "humans" {
        return record
            .get("author")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    match (agent_field(record, "tool"), agent_field(record, "id")) {
        (Some(tool), Some(id)) => Some(format!("{tool}:{id}")),
        (Some(tool), None) => Some(tool),
        (None, Some(id)) => Some(id),
        (None, None) => record
            .get("human_author")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

fn timestamp_unavailable(source: unisphere_core::query::SourceId) -> AvailabilityIssue {
    AvailabilityIssue {
        code: AvailabilityCode::NotCaptured,
        field: Some(FieldId::Timestamp),
        source: Some(source),
        entity: None,
        offset: None,
    }
}

fn available_fields() -> BTreeSet<FieldId> {
    BTreeSet::from([
        FieldId::Association,
        FieldId::Harness,
        FieldId::Adapter,
        FieldId::NativeId,
        FieldId::Revision,
        FieldId::SessionId,
        FieldId::Parts,
        FieldId::SourcePath,
        FieldId::Name,
        FieldId::Models,
        FieldId::Kind,
    ])
}

fn unsupported_source(source: Option<unisphere_core::query::SourceId>) -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::UnsupportedSource,
        RecoveryAction::ChooseAdapter {
            allowed: vec![AdapterId::new(DESCRIPTOR.id).expect("static adapter id")],
        },
    )
    .at(QueryErrorLocation {
        source,
        ..QueryErrorLocation::default()
    })
}

fn invalid_data(source: &SourceEvidence) -> QueryFailure {
    QueryFailure::invalid_data().at(QueryErrorLocation {
        source: Some(source.id),
        ..QueryErrorLocation::default()
    })
}

fn unsupported_schema(source: &SourceEvidence) -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::UnsupportedSchema,
        RecoveryAction::FixSource {
            reason: SourceProblem::UnsupportedDialect,
            source: Some(source.id),
        },
    )
    .at(QueryErrorLocation {
        source: Some(source.id),
        ..QueryErrorLocation::default()
    })
}

fn map_native_error(error: GitNotesError, source: &SourceEvidence) -> QueryFailure {
    match error {
        GitNotesError::UnsupportedFormat => unsupported_schema(source),
        GitNotesError::RecordLimit => QueryFailure::limit(LimitKind::ObservationsAndRows),
        GitNotesError::BatchLimit => QueryFailure::limit(LimitKind::RetainedBytes),
        _ => invalid_data(source),
    }
}
