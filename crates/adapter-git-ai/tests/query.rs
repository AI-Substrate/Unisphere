use std::{collections::BTreeSet, path::PathBuf};

use unisphere_adapter_git_ai::{GitAiAdapter, QUERY_POLICY_VERSION};
use unisphere_core::query::{
    AdapterId, AssociationBasis, AssociationExtent, AssociationObservation, ContentAccess, FieldId,
    HarnessId, MembershipPolicy, NativeLocator, NativeQueryInput, ObservationFacet, PartitionId,
    QueryAdapter, QueryFailure, QueryFailureCode, QueryLimits, SourceEvidence, SourceId,
    SourceLocator, SourceReadStatus, SourceViewKind,
};

const NOTE: &[u8] = include_bytes!("../fixtures/mixed.notes");
const NOTES_TIP: &str = "1111111111111111111111111111111111111111";
const NOTE_BLOB: &str = "2222222222222222222222222222222222222222";
const TARGET: &str = "3333333333333333333333333333333333333333";
const SESSION: &str = "s_123456789abcde";

fn source() -> SourceEvidence {
    let id = SourceId::derive([
        b"git-ai".as_slice(),
        b"git_notes".as_slice(),
        b"/private/tmp/repository/.git".as_slice(),
        b"refs/notes/ai".as_slice(),
        TARGET.as_bytes(),
    ]);
    let seed_partition = PartitionId::derive(id, b"bridge-seed");
    SourceEvidence {
        id,
        adapter: AdapterId::new("git-ai").unwrap(),
        harness: HarnessId::new("git-ai").unwrap(),
        representation: "git_notes".into(),
        locator: SourceLocator::LocalPath(PathBuf::from("/tmp/repository-alias")),
        revision: format!("{NOTES_TIP}:{NOTE_BLOB}"),
        query_policy_version: "bridge-unclassified".into(),
        read_status: SourceReadStatus::Readable,
        associations: vec![AssociationObservation {
            basis: AssociationBasis::GitRepositoryIdentity,
            path: Some(PathBuf::from("/private/tmp/repository")),
            partition: seed_partition,
            applies_to: AssociationExtent::Partition,
        }],
        available_fields: BTreeSet::new(),
    }
}

fn locator() -> NativeLocator {
    NativeLocator::GitNote {
        repository_id: "/private/tmp/repository/.git".into(),
        notes_ref: "refs/notes/ai".into(),
        notes_tip: NOTES_TIP.into(),
        target_commit: TARGET.into(),
        note_blob: NOTE_BLOB.into(),
    }
}

fn inspect(bytes: &[u8], access: ContentAccess) -> unisphere_core::query::InspectedSource {
    let source = source();
    let locator = locator();
    GitAiAdapter
        .inspect(
            NativeQueryInput::ProvidedObject {
                source: &source,
                bytes,
                locator: &locator,
            },
            access,
            &QueryLimits::default(),
        )
        .unwrap_or_else(|error| panic!("unexpected query failure: {error}"))
}

fn failure(bytes: &[u8]) -> QueryFailure {
    let source = source();
    let locator = locator();
    match GitAiAdapter.inspect(
        NativeQueryInput::ProvidedObject {
            source: &source,
            bytes,
            locator: &locator,
        },
        ContentAccess::default(),
        &QueryLimits::default(),
    ) {
        Ok(_) => panic!("expected query failure"),
        Err(error) => error,
    }
}

fn failure_with_limits(bytes: &[u8], limits: QueryLimits) -> QueryFailure {
    let source = source();
    let locator = locator();
    match GitAiAdapter.inspect(
        NativeQueryInput::ProvidedObject {
            source: &source,
            bytes,
            locator: &locator,
        },
        ContentAccess::default(),
        &limits,
    ) {
        Ok(_) => panic!("expected bounded query failure"),
        Err(error) => error,
    }
}

#[test]
fn native_authorship_maps_only_declared_sessions_and_attribution_facts() {
    let supplied = source();
    let inspected = inspect(NOTE, ContentAccess::default());

    assert_eq!(inspected.source.id, supplied.id);
    assert_eq!(inspected.source.revision, supplied.revision);
    assert_eq!(inspected.source.query_policy_version, QUERY_POLICY_VERSION);
    assert!(inspected.source.associations.iter().all(|association| {
        association.basis == AssociationBasis::GitRepositoryIdentity
            && association.path.as_deref() == Some(std::path::Path::new("/private/tmp/repository"))
            && matches!(&association.applies_to, AssociationExtent::Partition)
    }));

    let sessions: Vec<_> = inspected
        .partitions
        .iter()
        .filter(|partition| partition.native_session_id.is_some())
        .collect();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].native_session_id.as_deref(), Some(SESSION));
    assert_eq!(sessions[0].view, SourceViewKind::SourceOnly);
    assert_eq!(sessions[0].membership, MembershipPolicy::NativeContainment);
    assert!(inspected.partitions.iter().any(|partition| {
        partition.native_session_id.is_none() && partition.view == SourceViewKind::SourceOnly
    }));

    let declared_session = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some(SESSION))
        .expect("declared session observation");
    assert!(declared_session.session.as_ref().is_some_and(|session| {
        session.native_id == SESSION
            && session.namespace.starts_with("git-ai/authorship3/session/")
            && session.membership_basis == MembershipPolicy::NativeContainment
    }));
    assert!(declared_session.facets.iter().any(|facet| matches!(
        facet,
        ObservationFacet::SessionMetadata {
            native_id,
            name: None,
            models,
            created_at: None,
            lineage,
            ..
        } if native_id == SESSION && models.is_empty() && lineage.is_empty()
    )));

    let session_range = inspected
        .observations
        .iter()
        .find(|observation| {
            observation.native_record_id.as_deref() == Some("s_123456789abcde::t_11111111111111")
        })
        .expect("session attribution");
    assert!(session_range.session.is_some());
    let legacy = inspected
        .observations
        .iter()
        .find(|observation| observation.native_record_id.as_deref() == Some("0123456789abcdef"))
        .expect("legacy attribution");
    assert!(legacy.session.is_none());
    assert!(inspected.observations.iter().all(|observation| {
        observation.timestamp.is_none()
            && observation.facets.iter().all(|facet| {
                matches!(
                    facet,
                    ObservationFacet::Control { .. }
                        | ObservationFacet::SessionMetadata { .. }
                        | ObservationFacet::Attribution { .. }
                )
            })
    }));
}

#[test]
fn sensitive_identity_and_file_content_requires_explicit_access() {
    let metadata_only = inspect(NOTE, ContentAccess::default());
    assert!(metadata_only.observations.iter().all(|observation| {
        observation.facets.iter().all(|facet| match facet {
            ObservationFacet::SessionMetadata { name, models, .. } => {
                name.is_none() && models.is_empty()
            }
            ObservationFacet::Attribution {
                declared_agent,
                ranges,
                ..
            } => {
                declared_agent.is_none() && ranges.iter().all(|range| !range.contains("source.rs"))
            }
            _ => true,
        })
    }));

    let retained = inspect(
        NOTE,
        ContentAccess {
            inspect_fields: BTreeSet::from([FieldId::Name, FieldId::Models, FieldId::Parts]),
            emit_content: false,
        },
    );
    assert!(retained.observations.iter().any(|observation| {
        observation.facets.iter().any(|facet| {
            matches!(
                facet,
                ObservationFacet::SessionMetadata { name, models, .. }
                    if name.as_deref() == Some("synthetic-session")
                        && models.iter().map(String::as_str).eq(["synthetic-model"])
            )
        })
    }));
    assert!(retained.observations.iter().any(|observation| {
        observation.facets.iter().any(|facet| {
            matches!(
                facet,
                ObservationFacet::Attribution { declared_agent: Some(author), .. }
                    if author == "SENSITIVE-human <human@example.invalid>"
            )
        })
    }));
    assert!(retained.observations.iter().any(|observation| {
        observation.facets.iter().any(|facet| {
            matches!(
                facet,
                ObservationFacet::Attribution { ranges, .. }
                    if ranges.iter().any(|range| range.starts_with("source.rs:"))
            )
        })
    }));
}

#[test]
fn malformed_duplicate_and_unsupported_notes_fail_as_typed_source_errors() {
    let duplicate = br#"---
{"schema_version":"authorship/3.0.0","schema_version":"authorship/3.0.0"}"#;
    assert_eq!(failure(duplicate).kind(), QueryFailureCode::InvalidData);

    let unsupported = br#"---
{"schema_version":"authorship/2.0.0"}"#;
    assert_eq!(
        failure(unsupported).kind(),
        QueryFailureCode::UnsupportedSchema
    );

    let source = source();
    let records = [];
    let error = match GitAiAdapter.inspect(
        NativeQueryInput::Records {
            source: &source,
            records: &records,
        },
        ContentAccess::default(),
        &QueryLimits::default(),
    ) {
        Ok(_) => panic!("expected unsupported source"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), QueryFailureCode::UnsupportedSource);
}

#[test]
fn native_input_and_observation_expansion_respect_query_bounds() {
    let source_limited = QueryLimits {
        max_source_bytes: NOTE.len() - 1,
        ..QueryLimits::default()
    };
    assert_eq!(
        failure_with_limits(NOTE, source_limited).kind(),
        QueryFailureCode::ResourceLimit
    );

    let observation_limited = QueryLimits {
        max_observations_and_rows: 1,
        ..QueryLimits::default()
    };
    assert_eq!(
        failure_with_limits(NOTE, observation_limited).kind(),
        QueryFailureCode::ResourceLimit
    );
    let retained_limited = QueryLimits {
        max_retained_bytes: 1,
        ..QueryLimits::default()
    };
    assert_eq!(
        failure_with_limits(NOTE, retained_limited).kind(),
        QueryFailureCode::ResourceLimit
    );
}

#[test]
fn pinned_native_locator_and_repeated_snapshot_are_preserved_exactly() {
    let first = inspect(NOTE, ContentAccess::default());
    let second = inspect(NOTE, ContentAccess::default());
    let locator = locator();

    assert!(first == second);
    assert!(first.observations.iter().all(|observation| {
        observation.source_ref.source_id == first.source.id
            && observation.source_ref.revision == first.source.revision
            && observation.source_ref.locator == locator
    }));
}

#[test]
fn note_without_declared_sessions_remains_a_valid_source_only_snapshot() {
    let note = br#"---
{"schema_version":"authorship/3.0.0","sessions":{},"prompts":{},"humans":{}}"#;
    let inspected = inspect(note, ContentAccess::default());

    assert_eq!(inspected.partitions.len(), 1);
    assert!(inspected.partitions[0].native_session_id.is_none());
    assert!(
        inspected
            .observations
            .iter()
            .all(|observation| observation.session.is_none())
    );
}
