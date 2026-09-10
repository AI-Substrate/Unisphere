use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use unisphere_core::{
    MappingDiagnosticCode, MappingOptions, NativeSnapshot, PipelineError, PipelineErrorKind,
    SessionAdapter, SessionRef, SnapshotAdapter,
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

const TRANSCRIPT_POLICY: &str = "cursor-transcript-query-v1";
const IDE_POLICY: &str = "cursor-ide-query-v1";
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
        let path = match &source.locator {
            unisphere_core::query::SourceLocator::LocalPath(path) => path.clone(),
            unisphere_core::query::SourceLocator::Provided(_) => {
                return Err(unsupported_source(Some(source.id), DESCRIPTOR.id));
            }
        };
        let mut total = 0_usize;
        let mut native = Vec::with_capacity(records.len());
        let mut prior_offset = None;
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
            native.push(unisphere_core::NativeRecord {
                offset: *offset,
                bytes: record.bytes.clone(),
            });
        }
        if native.len() > limits.max_observations_and_rows {
            return Err(QueryFailure::limit(LimitKind::ObservationsAndRows));
        }

        let retain_content = retains_any(
            &access,
            &[FieldId::Text, FieldId::Parts, FieldId::ToolName, FieldId::Input],
        );
        let mapped = self
            .map(
                &SessionRef { path },
                &native,
                MappingOptions {
                    include_content: retain_content,
                },
            )
            .map_err(|error| map_pipeline_error(error, source.id, DESCRIPTOR.id))?;

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
        let diagnostic_map = transcript_diagnostics(source, &mapped.diagnostics);
        let mut observations = Vec::with_capacity(mapped.records.len());
        for (record, telemetry) in records.iter().zip(mapped.records) {
            let offset = match &record.locator {
                NativeLocator::Jsonl { offset } => *offset,
                _ => unreachable!("record locators were validated"),
            };
            let role = telemetry
                .attributes
                .get("unisphere.message.role")
                .and_then(Value::as_str)
                .and_then(message_role);
            let kind = telemetry
                .attributes
                .get("unisphere.source.kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let mut diagnostics = diagnostic_map.get(&offset).cloned().unwrap_or_default();
            let facets = if let Some(role) = role {
                diagnostics.extend([
                    unavailable(source.id, FieldId::NativeId, Some(offset)),
                    unavailable(source.id, FieldId::Timestamp, Some(offset)),
                    unavailable(source.id, FieldId::TurnId, Some(offset)),
                ]);
                vec![ObservationFacet::Message {
                    native_id: None,
                    role,
                    parts: message_parts(telemetry.body.as_ref(), &access),
                    request_marker: if role == MessageRole::User {
                        RequestMarker::Initiating
                    } else {
                        RequestMarker::Unknown
                    },
                    turn_id: None,
                }]
            } else {
                vec![ObservationFacet::Control {
                    kind: if kind == "metadata" {
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
                facets,
                diagnostics,
            });
        }
        let mut issues = transcript_source_issues(source.id);
        issues.extend(diagnostic_map.values().flatten().cloned());
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
        let retain_content = retains_any(
            &access,
            &[
                FieldId::Text,
                FieldId::Parts,
                FieldId::ToolName,
                FieldId::Input,
                FieldId::Output,
            ],
        );
        let mapped = self
            .map_snapshot(
                snapshot,
                MappingOptions {
                    include_content: retain_content,
                },
            )
            .map_err(|error| map_pipeline_error(error, source.id, IDE_DESCRIPTOR.id))?;

        let mut inspected_source = source.clone();
        inspected_source.query_policy_version = IDE_POLICY.into();
        inspected_source.available_fields = ide_fields();

        let mut partitions = Vec::new();
        let mut partition_by_session = BTreeMap::new();
        for record in &mapped.records {
            if record
                .attributes
                .get("unisphere.source.kind")
                .and_then(Value::as_str)
                != Some("composerData")
            {
                continue;
            }
            let Some(session) = record
                .attributes
                .get("unisphere.source.session.id")
                .and_then(Value::as_str)
            else {
                continue;
            };
            let partition_id = PartitionId::derive(source.id, session.as_bytes());
            partition_by_session.insert(session.to_owned(), partition_id);
            partitions.push(SourcePartition {
                id: partition_id,
                native_session_id: Some(session.to_owned()),
                participant_id: None,
                view: SourceViewKind::MainSpine,
                membership: MembershipPolicy::ValidatedHeader,
                associations: associations_for(source, partition_id),
            });
        }

        let diagnostic_map = ide_diagnostics(source, snapshot, &mapped.diagnostics, &partition_by_session);
        let mut observations = Vec::with_capacity(snapshot.records.len());
        let mut mapped_keys = BTreeSet::new();
        let native_rows: BTreeMap<_, _> = snapshot
            .records
            .iter()
            .map(|record| (record.key.as_str(), record))
            .collect();
        for (ordinal, telemetry) in mapped.records.into_iter().enumerate() {
            let key = telemetry
                .attributes
                .get("unisphere.source.key")
                .and_then(Value::as_str)
                .ok_or_else(QueryFailure::invalid_data)?
                .to_owned();
            let native_value: Value = serde_json::from_slice(
                &native_rows
                    .get(key.as_str())
                    .ok_or_else(QueryFailure::invalid_data)?
                    .bytes,
            )
            .map_err(|_| QueryFailure::invalid_data())?;
            let native_object = native_value
                .as_object()
                .ok_or_else(QueryFailure::invalid_data)?;
            mapped_keys.insert(key.clone());
            let session_id = telemetry
                .attributes
                .get("unisphere.source.session.id")
                .and_then(Value::as_str)
                .ok_or_else(QueryFailure::invalid_data)?
                .to_owned();
            let partition_id = *partition_by_session
                .get(&session_id)
                .ok_or_else(QueryFailure::invalid_data)?;
            let kind = telemetry
                .attributes
                .get("unisphere.source.kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let native_record_id = if kind == "composerData" {
                session_id.clone()
            } else {
                telemetry
                    .attributes
                    .get("unisphere.message.id")
                    .and_then(Value::as_str)
                    .unwrap_or(&key)
                    .to_owned()
            };
            let timestamp = telemetry
                .timestamp_unix_nano
                .map(|value| Timestamp::new(i128::from(value), TimestampBasis::Native))
                .transpose()?;
            let session = SessionEvidenceKey {
                namespace: "cursor-ide-composer".into(),
                native_id: session_id.clone(),
                participant_id: None,
                parent_native_id: None,
                fork_native_id: None,
                membership_basis: MembershipPolicy::ValidatedHeader,
            };
            let mut diagnostics = diagnostic_map.get(&key).cloned().unwrap_or_default();
            diagnostics.extend(ide_tool_issues(source.id, native_object));
            let session_associations = associations_for(source, partition_id);
            let facets = ide_facets(
                &telemetry,
                native_object,
                kind,
                &session_id,
                timestamp.clone(),
                &access,
                &session_associations,
            );
            observations.push(Observation {
                source_ref: SourceRef {
                    source_id: source.id,
                    revision: source.revision.clone(),
                    locator: NativeLocator::Snapshot { key: key.clone() },
                    subrecord: "record".into(),
                },
                native_record_id: Some(native_record_id),
                session: Some(session),
                branch: BranchEvidence::Linear {
                    partition: partition_id,
                },
                parent_ids: Vec::new(),
                sequence: NativeSequence {
                    version: 1,
                    key: u64::try_from(ordinal)
                        .map_err(|_| QueryFailure::limit(LimitKind::ObservationsAndRows))?
                        .to_be_bytes()
                        .to_vec(),
                },
                timestamp,
                facets,
                diagnostics,
            });
        }

        let mut unused: Vec<_> = snapshot
            .records
            .iter()
            .filter(|record| !mapped_keys.contains(&record.key))
            .collect();
        unused.sort_by(|left, right| left.key.cmp(&right.key));
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
                let issue = diagnostic_map
                    .get(&record.key)
                    .and_then(|issues| issues.first())
                    .cloned()
                    .unwrap_or_else(|| AvailabilityIssue {
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

        let mut issues: Vec<_> = diagnostic_map.values().flatten().cloned().collect();
        issues.extend(
            observations
                .iter()
                .flat_map(|observation| observation.diagnostics.iter())
                .filter(|issue| {
                    issue.code == AvailabilityCode::NotCaptured
                        && issue.field == Some(FieldId::CallId)
                })
                .cloned(),
        );
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

fn unsupported_source(source: Option<unisphere_core::query::SourceId>, adapter: &str) -> QueryFailure {
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

fn associations_for(source: &SourceEvidence, partition: PartitionId) -> Vec<AssociationObservation> {
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
    partitions.all(|partition| partition == first).then_some(first)
}

fn retains_any(access: &ContentAccess, fields: &[FieldId]) -> bool {
    access.emit_content || fields.iter().any(|field| access.inspect_fields.contains(field))
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
        return vec![ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)];
    };
    let mut output = Vec::new();
    for part in parts {
        let kind = part.get("type").and_then(Value::as_str).unwrap_or("unknown");
        match kind {
            "text" if retains_any(access, &[FieldId::Text, FieldId::Parts]) => {
                if let Some(text) = part.get("content").and_then(Value::as_str) {
                    output.push(ObservationPart::Text(text.to_owned()));
                }
            }
            "tool_call"
                if retains_any(
                    access,
                    &[FieldId::Parts, FieldId::ToolName, FieldId::Input],
                ) =>
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
            _ => output.push(ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)),
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

fn transcript_diagnostics(
    source: &SourceEvidence,
    diagnostics: &[unisphere_core::MappingDiagnostic],
) -> BTreeMap<u64, Vec<AvailabilityIssue>> {
    let mut by_offset = BTreeMap::new();
    for diagnostic in diagnostics {
        by_offset
            .entry(diagnostic.offset)
            .or_insert_with(Vec::new)
            .push(mapping_issue(source.id, diagnostic.code, Some(diagnostic.offset), None));
    }
    by_offset
}

fn ide_diagnostics(
    source: &SourceEvidence,
    snapshot: &NativeSnapshot,
    diagnostics: &[unisphere_core::SnapshotDiagnostic],
    partitions: &BTreeMap<String, PartitionId>,
) -> BTreeMap<String, Vec<AvailabilityIssue>> {
    let keys: BTreeSet<_> = snapshot.records.iter().map(|record| record.key.as_str()).collect();
    let mut by_key = BTreeMap::new();
    for diagnostic in diagnostics {
        let entity = diagnostic
            .key
            .strip_prefix("composerData:")
            .or_else(|| {
                diagnostic
                    .key
                    .strip_prefix("bubbleId:")
                    .and_then(|suffix| suffix.split_once(':').map(|(session, _)| session))
            })
            .and_then(|session| partitions.get(session))
            .map(|partition| partition.entity());
        let mut issue = mapping_issue(source.id, diagnostic.code, None, entity);
        if !keys.contains(diagnostic.key.as_str()) {
            issue.code = AvailabilityCode::Absent;
        }
        by_key
            .entry(diagnostic.key.clone())
            .or_insert_with(Vec::new)
            .push(issue);
    }
    by_key
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
    telemetry: &unisphere_core::TelemetryRecord,
    native: &Map<String, Value>,
    kind: &str,
    session_id: &str,
    timestamp: Option<Timestamp>,
    access: &ContentAccess,
    associations: &[AssociationObservation],
) -> Vec<ObservationFacet> {
    if kind == "composerData" {
        return vec![ObservationFacet::SessionMetadata {
            native_id: session_id.to_owned(),
            name: None,
            models: Vec::new(),
            created_at: timestamp,
            associations: associations.to_vec(),
            lineage: Vec::new(),
        }];
    }
    let role = telemetry
        .attributes
        .get("unisphere.message.role")
        .and_then(Value::as_str)
        .and_then(message_role);
    let body_parts = telemetry
        .body
        .as_ref()
        .and_then(|body| body.get("parts"))
        .and_then(Value::as_array);
    let native_id = telemetry
        .attributes
        .get("unisphere.message.id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut facets = Vec::new();
    if let Some(role) = role {
        let mut parts = Vec::new();
        if let Some(body_parts) = body_parts {
            for part in body_parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") if retains_any(access, &[FieldId::Text, FieldId::Parts]) => {
                        if let Some(text) = part.get("content").and_then(Value::as_str) {
                            parts.push(ObservationPart::Text(text.to_owned()));
                        }
                    }
                    Some("reasoning") if access.emit_content || access.inspect_fields.contains(&FieldId::Parts) => {
                        if let Some(text) = part.get("content").and_then(Value::as_str) {
                            parts.push(ObservationPart::Reasoning(text.to_owned()));
                        }
                    }
                    _ => {}
                }
            }
        } else if telemetry.attributes.contains_key("unisphere.content.omitted") {
            parts.push(ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted));
        }
        facets.push(ObservationFacet::Message {
            native_id: native_id.clone(),
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
            kind: if telemetry
                .attributes
                .get("unisphere.cursor.capability_type")
                .and_then(Value::as_i64)
                == Some(22)
            {
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
            let Some(value) = tool.get(field) else { continue };
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

fn tool_family(name: &str) -> Option<&'static str> {
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
