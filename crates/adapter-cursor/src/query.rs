use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use unisphere_core::{
    MappingDiagnosticCode, NativeSnapshot, PipelineError, PipelineErrorKind,
    query::{
        AdapterId, AssociationObservation, AvailabilityCode, AvailabilityIssue, BranchEvidence,
        ContentAccess, ControlKind, FieldId, InspectedSource, LimitKind, MembershipPolicy,
        MessageRole, NativeLocator, NativeQueryInput, NativeSequence, Observation,
        ObservationFacet, ObservationPart, Outcome, PartitionId, QueryAdapter, QueryErrorLocation,
        QueryFailure, QueryFailureCode, QueryLimits, RecoveryAction, RequestMarker,
        SessionEvidenceKey, SourceEvidence, SourcePartition, SourceProblem, SourceRef,
        SourceViewKind, Timestamp, TimestampBasis,
    },
};

use crate::{CursorAdapter, CursorIdeAdapter, DESCRIPTOR, IDE_DESCRIPTOR};

/// Reconstruction policy applied to supplied Cursor transcript records.
pub const TRANSCRIPT_POLICY: &str = "cursor-transcript-query-v1";
/// Reconstruction policy applied to supplied Cursor IDE snapshots.
pub const IDE_POLICY: &str = "cursor-ide-query-v1";
const TRANSCRIPT_PARTITION: &[u8] = b"cursor-transcript-source-only";
const IDE_SOURCE_ONLY_PARTITION: &[u8] = b"cursor-ide-source-only";

impl QueryAdapter for CursorAdapter {
    fn inspect(
        &self,
        input: NativeQueryInput<'_>,
        access: ContentAccess,
        limits: &QueryLimits,
    ) -> Result<InspectedSource, QueryFailure> {
        limits.validate()?;
        let NativeQueryInput::Records { source, records } = input else {
            return Err(unsupported_source(None, DESCRIPTOR.id));
        };
        require_adapter(source, DESCRIPTOR.id)?;
        if !matches!(
            &source.locator,
            unisphere_core::query::SourceLocator::LocalPath(_)
        ) {
            return Err(unsupported_source(Some(source.id), DESCRIPTOR.id));
        }

        let mut total = 0_usize;
        let mut prior_offset = None;
        let mut classified = Vec::with_capacity(records.len());
        for record in records {
            let NativeLocator::Jsonl { offset } = &record.locator else {
                return Err(invalid_data(source.id, None));
            };
            if prior_offset.is_some_and(|prior| *offset <= prior) {
                return Err(invalid_data(source.id, Some(*offset)));
            }
            prior_offset = Some(*offset);
            total = total
                .checked_add(record.bytes.len())
                .ok_or_else(|| QueryFailure::limit(LimitKind::SourceBytes))?;
            if total > limits.max_source_bytes {
                return Err(QueryFailure::limit(LimitKind::SourceBytes));
            }
            classified.push((
                *offset,
                crate::classify_transcript(&record.bytes, *offset)
                    .map_err(|error| map_pipeline_error(error, source.id, DESCRIPTOR.id))?,
            ));
        }
        if classified.len() > limits.max_observations_and_rows {
            return Err(QueryFailure::limit(LimitKind::ObservationsAndRows));
        }

        let mut inspected_source = source.clone();
        inspected_source.query_policy_version = TRANSCRIPT_POLICY.into();
        inspected_source.available_fields = transcript_fields();
        let partition_id = single_association_partition(source)
            .unwrap_or_else(|| PartitionId::derive(source.id, TRANSCRIPT_PARTITION));
        let partition = SourcePartition {
            id: partition_id,
            native_session_id: None,
            participant_id: None,
            view: SourceViewKind::SourceOnly,
            membership: MembershipPolicy::Unavailable,
            associations: associations_for(source, partition_id),
        };
        let retain_content = retains_any(
            &access,
            &[
                FieldId::Text,
                FieldId::Parts,
                FieldId::ToolName,
                FieldId::Input,
            ],
        );
        let mut observations = Vec::with_capacity(classified.len());
        for (record, (offset, native)) in records.iter().zip(classified) {
            let mut diagnostics: Vec<_> = native
                .diagnostics
                .into_iter()
                .map(|code| mapping_issue(source.id, code, Some(offset), None))
                .collect();
            if native.sensitive_content && !retain_content {
                diagnostics.push(mapping_issue(
                    source.id,
                    MappingDiagnosticCode::ContentOmitted,
                    Some(offset),
                    None,
                ));
            }
            let role = native.role.as_deref().and_then(message_role);
            let facets = if let Some(role) = role {
                diagnostics.extend([
                    unavailable(source.id, FieldId::NativeId, Some(offset)),
                    unavailable(source.id, FieldId::Timestamp, Some(offset)),
                    unavailable(source.id, FieldId::TurnId, Some(offset)),
                ]);
                vec![ObservationFacet::Message {
                    native_id: None,
                    role,
                    parts: message_parts(native.body.as_ref(), &access),
                    request_marker: if role == MessageRole::User {
                        RequestMarker::Initiating
                    } else {
                        RequestMarker::Unknown
                    },
                    turn_id: None,
                }]
            } else {
                vec![ObservationFacet::Control {
                    kind: if native.kind == "metadata" {
                        ControlKind::Summary
                    } else {
                        ControlKind::Other
                    },
                    links: Vec::new(),
                }]
            };
            observations.push(Observation {
                source_ref: SourceRef {
                    source_id: source.id,
                    revision: source.revision.clone(),
                    locator: record.locator.clone(),
                    subrecord: "record".into(),
                },
                native_record_id: None,
                session: None,
                branch: BranchEvidence::Unavailable {
                    partition: partition_id,
                    reason: AvailabilityCode::NotCaptured,
                },
                parent_ids: Vec::new(),
                sequence: NativeSequence {
                    version: 1,
                    key: offset.to_be_bytes().to_vec(),
                },
                timestamp: None,
                facets,
                diagnostics,
            });
        }
        let mut issues = transcript_source_issues(source.id);
        issues.extend(
            observations
                .iter()
                .flat_map(|observation| observation.diagnostics.iter().cloned()),
        );
        let inspected = InspectedSource {
            source: inspected_source,
            partitions: vec![partition],
            observations,
            issues,
        };
        inspected.validate(limits)?;
        Ok(inspected)
    }
}

impl QueryAdapter for CursorIdeAdapter {
    fn inspect(
        &self,
        input: NativeQueryInput<'_>,
        access: ContentAccess,
        limits: &QueryLimits,
    ) -> Result<InspectedSource, QueryFailure> {
        limits.validate()?;
        let NativeQueryInput::Snapshot { source, snapshot } = input else {
            return Err(unsupported_source(None, IDE_DESCRIPTOR.id));
        };
        require_adapter(source, IDE_DESCRIPTOR.id)?;
        validate_snapshot_binding(source, snapshot, limits)?;
        let rows = crate::ide::index_snapshot(snapshot)
            .map_err(|error| map_pipeline_error(error, source.id, IDE_DESCRIPTOR.id))?;

        let mut inspected_source = source.clone();
        inspected_source.query_policy_version = IDE_POLICY.into();
        inspected_source.available_fields = ide_fields();
        let mut partitions = Vec::new();
        let mut observations = Vec::with_capacity(snapshot.records.len());
        let mut diagnostics: BTreeMap<String, Vec<AvailabilityIssue>> = BTreeMap::new();
        let mut used = BTreeSet::new();
        let mut referenced = BTreeSet::new();
        let selected = snapshot.source.session_id.as_deref();
        let mut found = false;
        let mut ordinal = 0_u64;

        for (&key, &row) in &rows {
            let Some(session_id) = key.strip_prefix("composerData:") else {
                continue;
            };
            if selected.is_some_and(|selected| selected != session_id) {
                continue;
            }
            found = true;
            let (object, headers) = match crate::ide::classify_composer(row, session_id) {
                Ok(classified) => classified,
                Err(code) => {
                    add_ide_issue(&mut diagnostics, source.id, key, code, None);
                    continue;
                }
            };
            let partition_id = PartitionId::derive(source.id, session_id.as_bytes());
            let associations = associations_for(source, partition_id);
            partitions.push(SourcePartition {
                id: partition_id,
                native_session_id: Some(session_id.to_owned()),
                participant_id: None,
                view: SourceViewKind::MainSpine,
                membership: MembershipPolicy::ValidatedHeader,
                associations: associations.clone(),
            });
            used.insert(key.to_owned());
            observations.push(ide_native_observation(
                source,
                key,
                &object,
                session_id,
                session_id,
                partition_id,
                ordinal,
                true,
                &access,
                &associations,
            )?);
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| QueryFailure::limit(LimitKind::ObservationsAndRows))?;

            for header in headers {
                let Value::Object(header) = header else {
                    add_ide_issue(
                        &mut diagnostics,
                        source.id,
                        key,
                        MappingDiagnosticCode::InvalidField,
                        Some(partition_id),
                    );
                    continue;
                };
                let Some(bubble_id) = header
                    .get("bubbleId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                else {
                    add_ide_issue(
                        &mut diagnostics,
                        source.id,
                        key,
                        MappingDiagnosticCode::InvalidField,
                        Some(partition_id),
                    );
                    continue;
                };
                let bubble_key = format!("bubbleId:{session_id}:{bubble_id}");
                if !referenced.insert(bubble_key.clone()) {
                    add_ide_issue(
                        &mut diagnostics,
                        source.id,
                        key,
                        MappingDiagnosticCode::InvalidField,
                        Some(partition_id),
                    );
                    continue;
                }
                let Some(bubble_row) = rows.get(bubble_key.as_str()) else {
                    diagnostics
                        .entry(bubble_key)
                        .or_default()
                        .push(AvailabilityIssue {
                            code: AvailabilityCode::Absent,
                            field: None,
                            source: Some(source.id),
                            entity: Some(partition_id.entity()),
                            offset: None,
                        });
                    continue;
                };
                let (bubble, _) = match crate::ide::classify_bubble(
                    bubble_row,
                    bubble_id,
                    header.get("type").and_then(Value::as_i64),
                ) {
                    Ok(classified) => classified,
                    Err(code) => {
                        add_ide_issue(
                            &mut diagnostics,
                            source.id,
                            &bubble_key,
                            code,
                            Some(partition_id),
                        );
                        continue;
                    }
                };
                used.insert(bubble_key.clone());
                observations.push(ide_native_observation(
                    source,
                    &bubble_key,
                    &bubble,
                    session_id,
                    bubble_id,
                    partition_id,
                    ordinal,
                    false,
                    &access,
                    &associations,
                )?);
                ordinal = ordinal
                    .checked_add(1)
                    .ok_or_else(|| QueryFailure::limit(LimitKind::ObservationsAndRows))?;
            }
        }

        if !found && let Some(selected) = selected {
            add_ide_issue(
                &mut diagnostics,
                source.id,
                &format!("composerData:{selected}"),
                MappingDiagnosticCode::UnsupportedRecord,
                None,
            );
        }
        let selected_prefix = selected.map(|id| format!("bubbleId:{id}:"));
        for &key in rows.keys() {
            if key.starts_with("bubbleId:")
                && selected_prefix
                    .as_ref()
                    .is_none_or(|prefix| key.starts_with(prefix))
                && !referenced.contains(key)
            {
                add_ide_issue(
                    &mut diagnostics,
                    source.id,
                    key,
                    MappingDiagnosticCode::UnsupportedRecord,
                    None,
                );
            }
        }

        let unused: Vec<_> = rows
            .iter()
            .filter(|(key, _)| !used.contains(**key))
            .map(|(_, row)| *row)
            .collect();
        if !unused.is_empty() {
            let partition_id = PartitionId::derive(source.id, IDE_SOURCE_ONLY_PARTITION);
            partitions.push(SourcePartition {
                id: partition_id,
                native_session_id: None,
                participant_id: None,
                view: SourceViewKind::SourceOnly,
                membership: MembershipPolicy::Unavailable,
                associations: Vec::new(),
            });
            for record in unused {
                let issue = diagnostics
                    .get(&record.key)
                    .and_then(|issues| issues.first())
                    .cloned()
                    .unwrap_or(AvailabilityIssue {
                        code: AvailabilityCode::Ambiguous,
                        field: Some(FieldId::TurnId),
                        source: Some(source.id),
                        entity: None,
                        offset: None,
                    });
                observations.push(Observation {
                    source_ref: SourceRef {
                        source_id: source.id,
                        revision: source.revision.clone(),
                        locator: NativeLocator::Snapshot {
                            key: record.key.clone(),
                        },
                        subrecord: "source-only".into(),
                    },
                    native_record_id: Some(record.key.clone()),
                    session: None,
                    branch: BranchEvidence::Unavailable {
                        partition: partition_id,
                        reason: issue.code,
                    },
                    parent_ids: Vec::new(),
                    sequence: NativeSequence {
                        version: 1,
                        key: record.key.as_bytes().to_vec(),
                    },
                    timestamp: None,
                    facets: vec![ObservationFacet::Control {
                        kind: ControlKind::Branch,
                        links: Vec::new(),
                    }],
                    diagnostics: vec![issue],
                });
            }
        }

        let mut issues: Vec<_> = diagnostics.values().flatten().cloned().collect();
        issues.extend(observations.iter().flat_map(|observation| {
            observation
                .diagnostics
                .iter()
                .filter(|issue| {
                    issue.code == AvailabilityCode::NotCaptured
                        && issue.field == Some(FieldId::CallId)
                })
                .cloned()
        }));
        issues.extend([
            unavailable(source.id, FieldId::TurnId, None),
            unavailable(source.id, FieldId::DurationMs, None),
            unavailable(source.id, FieldId::ExitCode, None),
        ]);
        for partition in &partitions {
            if partition.view == SourceViewKind::MainSpine && partition.associations.is_empty() {
                issues.push(AvailabilityIssue {
                    code: AvailabilityCode::Unassociated,
                    field: Some(FieldId::Association),
                    source: Some(source.id),
                    entity: Some(partition.id.entity()),
                    offset: None,
                });
            }
        }
        let inspected = InspectedSource {
            source: inspected_source,
            partitions,
            observations,
            issues,
        };
        inspected.validate(limits)?;
        Ok(inspected)
    }
}

fn require_adapter(source: &SourceEvidence, expected: &str) -> Result<(), QueryFailure> {
    if source.adapter.as_str() == expected {
        Ok(())
    } else {
        Err(unsupported_source(Some(source.id), expected))
    }
}

fn validate_snapshot_binding(
    source: &SourceEvidence,
    snapshot: &NativeSnapshot,
    limits: &QueryLimits,
) -> Result<(), QueryFailure> {
    let path_matches = matches!(
        &source.locator,
        unisphere_core::query::SourceLocator::LocalPath(path) if path == &snapshot.source.path
    );
    if !path_matches || source.revision != snapshot.revision {
        return Err(invalid_data(source.id, None));
    }
    let mut total = 0_usize;
    for record in &snapshot.records {
        total = total
            .checked_add(record.key.len())
            .and_then(|size| size.checked_add(record.bytes.len()))
            .ok_or_else(|| QueryFailure::limit(LimitKind::SourceBytes))?;
        if total > limits.max_source_bytes {
            return Err(QueryFailure::limit(LimitKind::SourceBytes));
        }
    }
    if snapshot.records.len() > limits.max_observations_and_rows {
        return Err(QueryFailure::limit(LimitKind::ObservationsAndRows));
    }
    Ok(())
}

fn add_ide_issue(
    diagnostics: &mut BTreeMap<String, Vec<AvailabilityIssue>>,
    source: unisphere_core::query::SourceId,
    key: &str,
    code: MappingDiagnosticCode,
    partition: Option<PartitionId>,
) {
    diagnostics
        .entry(key.to_owned())
        .or_default()
        .push(mapping_issue(
            source,
            code,
            None,
            partition.map(|partition| partition.entity()),
        ));
}

#[allow(clippy::too_many_arguments)]
fn ide_native_observation(
    source: &SourceEvidence,
    key: &str,
    native: &Map<String, Value>,
    session_id: &str,
    native_id: &str,
    partition_id: PartitionId,
    ordinal: u64,
    composer: bool,
    access: &ContentAccess,
    associations: &[AssociationObservation],
) -> Result<Observation, QueryFailure> {
    let native_timestamp = if composer {
        crate::ide::composer_timestamp(native)
    } else {
        crate::ide::bubble_timestamp(native)
    };
    let mut diagnostics = ide_tool_issues(source.id, native);
    let timestamp = match native_timestamp {
        Ok(Some(value)) => Some(Timestamp::new(i128::from(value), TimestampBasis::Native)?),
        Ok(None) => None,
        Err(code) => {
            diagnostics.push(mapping_issue(
                source.id,
                code,
                None,
                Some(partition_id.entity()),
            ));
            None
        }
    };
    let facets = ide_facets(
        native,
        composer,
        native_id,
        timestamp.clone(),
        access,
        associations,
    );
    Ok(Observation {
        source_ref: SourceRef {
            source_id: source.id,
            revision: source.revision.clone(),
            locator: NativeLocator::Snapshot {
                key: key.to_owned(),
            },
            subrecord: "record".into(),
        },
        native_record_id: Some(native_id.to_owned()),
        session: Some(SessionEvidenceKey {
            namespace: "cursor-ide-composer".into(),
            native_id: session_id.to_owned(),
            participant_id: None,
            parent_native_id: None,
            fork_native_id: None,
            membership_basis: MembershipPolicy::ValidatedHeader,
        }),
        branch: BranchEvidence::Linear {
            partition: partition_id,
        },
        parent_ids: Vec::new(),
        sequence: NativeSequence {
            version: 1,
            key: ordinal.to_be_bytes().to_vec(),
        },
        timestamp,
        facets,
        diagnostics,
    })
}

fn unsupported_source(
    source: Option<unisphere_core::query::SourceId>,
    adapter: &str,
) -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::UnsupportedSource,
        RecoveryAction::ChooseAdapter {
            allowed: vec![AdapterId::new(adapter).expect("static adapter id is valid")],
        },
    )
    .at(QueryErrorLocation {
        source,
        ..QueryErrorLocation::default()
    })
}

fn invalid_data(source: unisphere_core::query::SourceId, offset: Option<u64>) -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::InvalidData,
        RecoveryAction::FixSource {
            reason: SourceProblem::InvalidData,
            source: Some(source),
        },
    )
    .at(QueryErrorLocation {
        source: Some(source),
        offset,
        ..QueryErrorLocation::default()
    })
}

fn map_pipeline_error(
    error: PipelineError,
    source: unisphere_core::query::SourceId,
    adapter: &str,
) -> QueryFailure {
    match error.kind() {
        PipelineErrorKind::Unsupported => unsupported_source(Some(source), adapter),
        PipelineErrorKind::RecordLimit
        | PipelineErrorKind::BatchLimit
        | PipelineErrorKind::ListingLimit
        | PipelineErrorKind::OutputLimit => QueryFailure::limit(LimitKind::SourceBytes),
        PipelineErrorKind::InvalidInput | PipelineErrorKind::InvalidData => {
            invalid_data(source, error.offset())
        }
        PipelineErrorKind::Read | PipelineErrorKind::SourceChanged => QueryFailure::new(
            QueryFailureCode::UnreadableSource,
            RecoveryAction::FixSource {
                reason: if error.kind() == PipelineErrorKind::SourceChanged {
                    SourceProblem::ChangedDuringRead
                } else {
                    SourceProblem::Permissions
                },
                source: Some(source),
            },
        ),
        PipelineErrorKind::Write => QueryFailure::new(
            QueryFailureCode::OutputFailure,
            RecoveryAction::ChooseNewOutput {
                discard_partial: true,
            },
        ),
    }
}

fn associations_for(
    source: &SourceEvidence,
    partition: PartitionId,
) -> Vec<AssociationObservation> {
    source
        .associations
        .iter()
        .filter(|association| association.partition == partition)
        .cloned()
        .collect()
}

fn single_association_partition(source: &SourceEvidence) -> Option<PartitionId> {
    let mut partitions = source
        .associations
        .iter()
        .map(|association| association.partition);
    let first = partitions.next()?;
    partitions
        .all(|partition| partition == first)
        .then_some(first)
}

fn retains_any(access: &ContentAccess, fields: &[FieldId]) -> bool {
    access.emit_content
        || fields
            .iter()
            .any(|field| access.inspect_fields.contains(field))
}

fn message_role(value: &str) -> Option<MessageRole> {
    match value {
        "user" => Some(MessageRole::User),
        "assistant" => Some(MessageRole::Assistant),
        "tool" => Some(MessageRole::Tool),
        _ => None,
    }
}

fn message_parts(body: Option<&Value>, access: &ContentAccess) -> Vec<ObservationPart> {
    let Some(parts) = body
        .and_then(|body| body.get("parts"))
        .and_then(Value::as_array)
    else {
        return vec![ObservationPart::Unavailable(
            AvailabilityCode::SensitiveOmitted,
        )];
    };
    let mut output = Vec::new();
    for part in parts {
        let kind = part
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        match kind {
            "text" if retains_any(access, &[FieldId::Text, FieldId::Parts]) => {
                if let Some(text) = part.get("content").and_then(Value::as_str) {
                    output.push(ObservationPart::Text(text.to_owned()));
                }
            }
            "tool_call"
                if retains_any(access, &[FieldId::Parts, FieldId::ToolName, FieldId::Input]) =>
            {
                let mut retained = Map::new();
                retained.insert("type".into(), Value::String("tool_call".into()));
                if retains_any(access, &[FieldId::Parts, FieldId::ToolName])
                    && let Some(name) = part.get("name")
                {
                    retained.insert("name".into(), name.clone());
                }
                if retains_any(access, &[FieldId::Parts, FieldId::Input])
                    && let Some(arguments) = part.get("arguments")
                {
                    retained.insert("arguments".into(), arguments.clone());
                }
                output.push(ObservationPart::Structured(Value::Object(retained)));
            }
            _ if access.emit_content || access.inspect_fields.contains(&FieldId::Parts) => {
                output.push(ObservationPart::Structured(part.clone()));
            }
            _ => output.push(ObservationPart::Unavailable(
                AvailabilityCode::SensitiveOmitted,
            )),
        }
    }
    output
}

fn transcript_source_issues(source: unisphere_core::query::SourceId) -> Vec<AvailabilityIssue> {
    [
        FieldId::NativeId,
        FieldId::Timestamp,
        FieldId::TurnId,
        FieldId::CallId,
        FieldId::Output,
        FieldId::DurationMs,
        FieldId::Status,
    ]
    .into_iter()
    .map(|field| unavailable(source, field, None))
    .collect()
}

fn unavailable(
    source: unisphere_core::query::SourceId,
    field: FieldId,
    offset: Option<u64>,
) -> AvailabilityIssue {
    AvailabilityIssue {
        code: AvailabilityCode::NotCaptured,
        field: Some(field),
        source: Some(source),
        entity: None,
        offset,
    }
}

fn mapping_issue(
    source: unisphere_core::query::SourceId,
    code: MappingDiagnosticCode,
    offset: Option<u64>,
    entity: Option<unisphere_core::query::EntityId>,
) -> AvailabilityIssue {
    let (code, field) = match code {
        MappingDiagnosticCode::UnsupportedRecord => (AvailabilityCode::NotSupported, None),
        MappingDiagnosticCode::UnsupportedPart => {
            (AvailabilityCode::NotSupported, Some(FieldId::Parts))
        }
        MappingDiagnosticCode::InvalidField => (AvailabilityCode::Conflict, None),
        MappingDiagnosticCode::InvalidTimestamp => {
            (AvailabilityCode::InvalidClock, Some(FieldId::Timestamp))
        }
        MappingDiagnosticCode::ContentOmitted => {
            (AvailabilityCode::SensitiveOmitted, Some(FieldId::Parts))
        }
    };
    AvailabilityIssue {
        code,
        field,
        source: Some(source),
        entity,
        offset,
    }
}

fn ide_tool_issues(
    source: unisphere_core::query::SourceId,
    native: &Map<String, Value>,
) -> Vec<AvailabilityIssue> {
    let Some(tool) = native.get("toolFormerData").and_then(Value::as_object) else {
        return Vec::new();
    };
    let has_call_fact = tool.contains_key("name")
        || tool.contains_key("params")
        || tool.contains_key("rawArgs")
        || tool.contains_key("result")
        || tool.contains_key("error");
    if has_call_fact
        && tool
            .get("toolCallId")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        vec![unavailable(source, FieldId::CallId, None)]
    } else {
        Vec::new()
    }
}

fn ide_facets(
    native: &Map<String, Value>,
    composer: bool,
    native_id: &str,
    timestamp: Option<Timestamp>,
    access: &ContentAccess,
    associations: &[AssociationObservation],
) -> Vec<ObservationFacet> {
    if composer {
        return vec![ObservationFacet::SessionMetadata {
            native_id: native_id.to_owned(),
            name: None,
            models: Vec::new(),
            created_at: timestamp,
            associations: associations.to_vec(),
            lineage: Vec::new(),
        }];
    }
    let kind = native.get("type").and_then(Value::as_i64);
    let control = native.get("capabilityType").and_then(Value::as_i64) == Some(22)
        || native.get("isSimulatedMsg").and_then(Value::as_bool) == Some(true);
    let role = (!control)
        .then_some(kind)
        .flatten()
        .and_then(|kind| match kind {
            1 => Some(MessageRole::User),
            2 => Some(MessageRole::Assistant),
            _ => None,
        });
    let mut facets = Vec::new();
    if let Some(role) = role {
        let mut parts = Vec::new();
        let mut sensitive_omitted = false;
        if let Some(value) = native.get("text")
            && let Some(text) = value.as_str()
        {
            if retains_any(access, &[FieldId::Text, FieldId::Parts]) {
                parts.push(ObservationPart::Text(text.to_owned()));
            } else {
                sensitive_omitted = true;
            }
        }
        if let Some(thinking) = native.get("thinking").and_then(Value::as_object)
            && let Some(text) = thinking.get("text").and_then(Value::as_str)
        {
            if access.emit_content || access.inspect_fields.contains(&FieldId::Parts) {
                parts.push(ObservationPart::Reasoning(text.to_owned()));
            } else {
                sensitive_omitted = true;
            }
        }
        if sensitive_omitted {
            parts.push(ObservationPart::Unavailable(
                AvailabilityCode::SensitiveOmitted,
            ));
        }
        facets.push(ObservationFacet::Message {
            native_id: Some(native_id.to_owned()),
            role,
            parts,
            request_marker: if role == MessageRole::User {
                RequestMarker::Initiating
            } else {
                RequestMarker::Unknown
            },
            turn_id: None,
        });
    } else {
        facets.push(ObservationFacet::Control {
            kind: if native.get("capabilityType").and_then(Value::as_i64) == Some(22) {
                ControlKind::Summary
            } else {
                ControlKind::Other
            },
            links: Vec::new(),
        });
    }

    if let Some(tool) = native.get("toolFormerData").and_then(Value::as_object)
        && let Some(call_id) = tool.get("toolCallId").and_then(Value::as_str)
        && !call_id.is_empty()
    {
        if let Some(name) = tool.get("name").and_then(Value::as_str) {
            let arguments = tool.get("params").or_else(|| tool.get("rawArgs"));
            let input = match arguments {
                Some(arguments) if retains_any(access, &[FieldId::Input, FieldId::Parts]) => {
                    vec![ObservationPart::Structured(arguments.clone())]
                }
                Some(_) => vec![ObservationPart::Unavailable(
                    AvailabilityCode::SensitiveOmitted,
                )],
                None => vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)],
            };
            facets.push(ObservationFacet::ToolCall {
                native_call_id: call_id.to_owned(),
                native_name: name.to_owned(),
                family: tool_family(name).map(str::to_owned),
                input,
                turn_id: None,
            });
        }
        for (field, outcome) in [("result", Outcome::Unknown), ("error", Outcome::Failed)] {
            let Some(value) = tool.get(field) else {
                continue;
            };
            let output = if retains_any(access, &[FieldId::Output, FieldId::Parts]) {
                vec![ObservationPart::Structured(value.clone())]
            } else {
                vec![ObservationPart::Unavailable(
                    AvailabilityCode::SensitiveOmitted,
                )]
            };
            facets.push(ObservationFacet::ToolResult {
                native_call_id: call_id.to_owned(),
                native_name: None,
                output,
                outcome,
                exit_code: None,
                reported_duration_ms: None,
                turn_id: None,
            });
        }
    }
    facets
}

pub(crate) fn tool_family(name: &str) -> Option<&'static str> {
    match name {
        "read_file" | "readFile" => Some("file-read"),
        "write_file" | "edit_file" | "apply_patch" => Some("file-write"),
        "run_terminal_cmd" | "shell" | "bash" => Some("shell"),
        _ => None,
    }
}

fn transcript_fields() -> BTreeSet<FieldId> {
    BTreeSet::from([
        FieldId::Id,
        FieldId::SourceRefs,
        FieldId::Harness,
        FieldId::Adapter,
        FieldId::Availability,
        FieldId::Format,
        FieldId::ReadStatus,
        FieldId::Revision,
        FieldId::SourcePath,
        FieldId::Role,
        FieldId::Text,
        FieldId::Parts,
        FieldId::Kind,
    ])
}

fn ide_fields() -> BTreeSet<FieldId> {
    BTreeSet::from([
        FieldId::Id,
        FieldId::SourceRefs,
        FieldId::NativeId,
        FieldId::Harness,
        FieldId::Adapter,
        FieldId::Availability,
        FieldId::Format,
        FieldId::ReadStatus,
        FieldId::Association,
        FieldId::Revision,
        FieldId::SourcePath,
        FieldId::StartedAt,
        FieldId::FirstEventAt,
        FieldId::Timestamp,
        FieldId::Role,
        FieldId::Text,
        FieldId::Parts,
        FieldId::MessageId,
        FieldId::CallId,
        FieldId::ToolName,
        FieldId::ToolFamily,
        FieldId::Input,
        FieldId::Output,
        FieldId::Status,
        FieldId::Kind,
    ])
}
