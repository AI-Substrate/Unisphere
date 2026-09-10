//! Pure, stateless projection of supplied Codex rollout JSONL records.
//!
//! The physical OTLP mapper never carries header context into later records;
//! query inspection uses only explicit versioned header and turn boundaries.
#![forbid(unsafe_code)]

use std::{collections::BTreeMap, path::PathBuf};

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedBatch, MappingDiagnostic,
    MappingDiagnosticCode, MappingOptions, NativeRecord, PipelineError, PipelineErrorKind,
    SessionAdapter, SessionRef, TelemetryRecord,
    query::{
        AssociationBasis, AssociationExtent, AssociationObservation, AvailabilityCode,
        AvailabilityIssue, BranchEvidence, BranchLink, ContentAccess, ControlKind, FieldId,
        InspectedSource, LimitKind, LineageKind, MembershipPolicy, MessageRole, NativeLocator,
        NativeQueryInput, NativeSequence, Observation, ObservationFacet, ObservationPart, Outcome,
        PartitionId, QueryAdapter, QueryErrorLocation, QueryFailure, QueryFailureCode, QueryLimits,
        RecoveryAction, RequestMarker, SessionEvidenceKey, SourcePartition, SourceProblem,
        SourceRef, SourceViewKind, Timestamp, TimestampBasis, UsageCounters, UsageScope,
    },
};

/// Declarative metadata; this mapper never expands or reads the location hint.
pub const DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "codex",
    application: "Codex",
    description: "Physical Codex rollout JSONL records; not reconstructed turns or a lossless archive.",
    locations: &[LocationHint {
        platforms: &["linux", "macos", "windows"],
        base: "home",
        path: ".codex/sessions",
        session_glob: "????/??/??/rollout-*.jsonl",
        storage_format: "jsonl",
    }],
    capabilities: AdapterCapabilities {
        export_platforms: &["unix"],
        output_formats: &["otlp-jsonl"],
        sdk_caller_owned_cursor: true,
        cursor_source_assumption: "append_only",
        cli_persisted_resume: false,
        delayed_revision_reconciliation: false,
        lossless_archive: false,
    },
};

/// Query reconstruction policy applied to supplied Codex rollout records.
pub const QUERY_POLICY_VERSION: &str = "codex/query-v1";

/// Maps complete supplied records without filesystem, environment, or clock access.
#[derive(Debug, Clone, Copy, Default)]
pub struct CodexAdapter;

impl SessionAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        DESCRIPTOR.id
    }

    fn map(
        &self,
        source: &SessionRef,
        records: &[NativeRecord],
        options: MappingOptions,
    ) -> Result<MappedBatch, PipelineError> {
        source.validate()?;
        let path = source
            .path
            .to_str()
            .ok_or_else(|| PipelineError::new(PipelineErrorKind::InvalidInput, None))?;
        let mut batch = MappedBatch::default();
        for native in records {
            let value = decode_codex_json(&native.bytes).map_err(|_| {
                PipelineError::new(PipelineErrorKind::InvalidData, Some(native.offset))
            })?;
            let mapping = Mapping {
                offset: native.offset,
                include_content: options.include_content,
                content_present: false,
                attributes: BTreeMap::from([
                    ("unisphere.profile.version".into(), json!(1)),
                    ("unisphere.source.adapter".into(), json!(DESCRIPTOR.id)),
                    ("unisphere.source.path".into(), json!(path)),
                    ("unisphere.source.offset".into(), json!(native.offset)),
                ]),
                diagnostics: &mut batch.diagnostics,
            };
            batch.records.push(mapping.record(value));
        }
        Ok(batch)
    }
}

#[derive(Clone)]
struct CodexQuerySession {
    key: SessionEvidenceKey,
    partition: PartitionId,
}

impl QueryAdapter for CodexAdapter {
    fn inspect(
        &self,
        input: NativeQueryInput<'_>,
        access: ContentAccess,
        limits: &QueryLimits,
    ) -> Result<InspectedSource, QueryFailure> {
        limits.validate()?;
        let NativeQueryInput::Records { source, records } = input else {
            return Err(QueryFailure::new(
                QueryFailureCode::UnsupportedSource,
                RecoveryAction::FixSource {
                    reason: SourceProblem::UnsupportedDialect,
                    source: None,
                },
            ));
        };
        if records.len() > limits.max_observations_and_rows {
            return Err(QueryFailure::limit(LimitKind::ObservationsAndRows));
        }
        let input_bytes = records.iter().try_fold(0_usize, |total, record| {
            total.checked_add(record.bytes.len())
        });
        if input_bytes.is_none_or(|bytes| bytes > limits.max_source_bytes) {
            return Err(QueryFailure::limit(LimitKind::SourceBytes));
        }

        let mut inspected = InspectedSource {
            source: source.clone(),
            partitions: Vec::new(),
            observations: Vec::with_capacity(records.len()),
            issues: Vec::new(),
        };
        inspected.source.query_policy_version = QUERY_POLICY_VERSION.into();
        let source_only = PartitionId::derive(source.id, b"codex/source-only/v1");
        let mut partition_indices = BTreeMap::<String, usize>::new();
        let mut current_session: Option<CodexQuerySession> = None;
        let mut current_turn: Option<String> = None;
        let mut used_source_only = false;

        for record in records {
            let offset = codex_locator_offset(&record.locator);
            let value: Value = decode_codex_json(&record.bytes).map_err(|_| {
                QueryFailure::invalid_data().at(QueryErrorLocation {
                    source: Some(source.id),
                    offset,
                    ..QueryErrorLocation::default()
                })
            })?;
            let mut diagnostics = Vec::new();
            let Some(object) = value.as_object() else {
                used_source_only = true;
                let issue = codex_issue(source.id, offset, None, AvailabilityCode::NotSupported);
                inspected.issues.push(issue.clone());
                inspected.observations.push(Observation {
                    source_ref: SourceRef {
                        source_id: source.id,
                        revision: source.revision.clone(),
                        locator: record.locator.clone(),
                        subrecord: "unknown".into(),
                    },
                    native_record_id: None,
                    session: None,
                    branch: BranchEvidence::Unavailable {
                        partition: source_only,
                        reason: AvailabilityCode::NotCaptured,
                    },
                    parent_ids: Vec::new(),
                    sequence: codex_sequence(&record.locator),
                    timestamp: None,
                    facets: Vec::new(),
                    diagnostics: vec![issue],
                });
                continue;
            };
            let kind = codex_string(object, "type").unwrap_or("unknown");
            let payload = object.get("payload").and_then(Value::as_object);
            let subtype = payload.and_then(|payload| codex_string(payload, "type"));
            let timestamp =
                codex_timestamp(object.get("timestamp"), source.id, offset, &mut diagnostics);

            let mut header_associations = Vec::new();
            if kind == "session_meta" {
                current_turn = None;
                current_session = payload.and_then(|payload| {
                    let native_id = codex_string(payload, "id")?;
                    let partition = PartitionId::derive(source.id, &codex_partition_key(native_id));
                    let parent_native_id =
                        codex_string(payload, "parent_thread_id").map(str::to_owned);
                    let fork_native_id = codex_string(payload, "forked_from_id").map(str::to_owned);
                    let key = SessionEvidenceKey {
                        namespace: "codex/thread".into(),
                        native_id: native_id.into(),
                        participant_id: None,
                        parent_native_id,
                        fork_native_id,
                        membership_basis: MembershipPolicy::ValidatedHeader,
                    };
                    let index = if let Some(index) = partition_indices.get(native_id) {
                        *index
                    } else {
                        let index = inspected.partitions.len();
                        inspected.partitions.push(SourcePartition {
                            id: partition,
                            native_session_id: Some(native_id.into()),
                            participant_id: None,
                            view: SourceViewKind::Conversation,
                            membership: MembershipPolicy::ValidatedHeader,
                            associations: Vec::new(),
                        });
                        partition_indices.insert(native_id.into(), index);
                        index
                    };
                    if let Some(cwd) = codex_string(payload, "cwd") {
                        let association = AssociationObservation {
                            basis: AssociationBasis::NativeCwd,
                            path: Some(PathBuf::from(cwd)),
                            partition,
                            applies_to: AssociationExtent::Partition,
                        };
                        inspected.partitions[index]
                            .associations
                            .push(association.clone());
                        inspected.source.associations.push(association.clone());
                        header_associations.push(association);
                    }
                    Some(CodexQuerySession { key, partition })
                });
            }
            if kind == "turn_context" {
                current_turn = payload
                    .and_then(|payload| codex_string(payload, "turn_id"))
                    .map(str::to_owned);
            }

            let session = current_session.as_ref().map(|session| session.key.clone());
            let (_, branch) = if let Some(session) = current_session.as_ref() {
                (
                    session.partition,
                    BranchEvidence::Linear {
                        partition: session.partition,
                    },
                )
            } else {
                used_source_only = true;
                (
                    source_only,
                    BranchEvidence::Unavailable {
                        partition: source_only,
                        reason: AvailabilityCode::NotCaptured,
                    },
                )
            };
            let explicit_turn = payload
                .and_then(|payload| codex_string(payload, "turn_id"))
                .or_else(|| {
                    payload
                        .and_then(|payload| {
                            payload.get("internal_chat_message_metadata_passthrough")
                        })
                        .and_then(Value::as_object)
                        .and_then(|metadata| codex_string(metadata, "turn_id"))
                })
                .map(str::to_owned);
            let turn_id = explicit_turn.or_else(|| current_turn.clone());
            let native_record_id = payload.and_then(codex_native_record_id).map(str::to_owned);
            let mut facets = Vec::new();
            match kind {
                "session_meta" => {
                    if let (Some(payload), Some(session)) = (payload, current_session.as_ref()) {
                        let mut lineage = Vec::new();
                        if let Some(parent) = &session.key.parent_native_id {
                            lineage.push(BranchLink {
                                kind: LineageKind::Parent,
                                target: parent.clone(),
                            });
                        }
                        if let Some(fork) = &session.key.fork_native_id {
                            lineage.push(BranchLink {
                                kind: LineageKind::Fork,
                                target: fork.clone(),
                            });
                        }
                        if let Some(root) = codex_string(payload, "session_id") {
                            lineage.push(BranchLink {
                                kind: LineageKind::NativeLink,
                                target: root.into(),
                            });
                        }
                        facets.push(ObservationFacet::SessionMetadata {
                            native_id: session.key.native_id.clone(),
                            name: None,
                            models: Vec::new(),
                            created_at: timestamp.clone(),
                            associations: header_associations,
                            lineage,
                        });
                    } else {
                        diagnostics.push(codex_issue(
                            source.id,
                            offset,
                            Some(FieldId::NativeId),
                            AvailabilityCode::NotCaptured,
                        ));
                    }
                }
                "turn_context" => facets.push(ObservationFacet::Control {
                    kind: ControlKind::ContextChange,
                    links: current_turn
                        .iter()
                        .map(|turn| BranchLink {
                            kind: LineageKind::NativeLink,
                            target: turn.clone(),
                        })
                        .collect(),
                }),
                "response_item" => inspect_codex_response(
                    payload,
                    subtype,
                    turn_id.as_deref(),
                    &access,
                    (source.id, offset),
                    &mut facets,
                    &mut diagnostics,
                ),
                "event_msg" => inspect_codex_event(
                    payload,
                    subtype,
                    turn_id.as_deref(),
                    &access,
                    source.id,
                    offset,
                    current_session
                        .as_ref()
                        .map(|session| session.key.native_id.as_str()),
                    &mut facets,
                    &mut diagnostics,
                ),
                "compacted" => facets.push(ObservationFacet::Control {
                    kind: ControlKind::Compaction,
                    links: Vec::new(),
                }),
                _ => diagnostics.push(codex_issue(
                    source.id,
                    offset,
                    None,
                    AvailabilityCode::NotSupported,
                )),
            }

            inspected.observations.push(Observation {
                source_ref: SourceRef {
                    source_id: source.id,
                    revision: source.revision.clone(),
                    locator: record.locator.clone(),
                    subrecord: match subtype {
                        Some(subtype) => format!("{kind}/{subtype}"),
                        None => kind.into(),
                    },
                },
                native_record_id,
                session,
                branch,
                parent_ids: Vec::new(),
                sequence: codex_sequence(&record.locator),
                timestamp,
                facets,
                diagnostics: diagnostics.clone(),
            });
            inspected.issues.extend(diagnostics);
        }

        if used_source_only || inspected.partitions.is_empty() {
            inspected.partitions.push(SourcePartition {
                id: source_only,
                native_session_id: None,
                participant_id: None,
                view: SourceViewKind::SourceOnly,
                membership: MembershipPolicy::Unavailable,
                associations: Vec::new(),
            });
        }
        inspected.validate(limits)?;
        Ok(inspected)
    }
}

fn decode_codex_json(bytes: &[u8]) -> serde_json::Result<Value> {
    serde_json::from_slice(bytes)
}

fn inspect_codex_response(
    payload: Option<&Map<String, Value>>,
    subtype: Option<&str>,
    turn_id: Option<&str>,
    access: &ContentAccess,
    (source, offset): (unisphere_core::query::SourceId, Option<u64>),
    facets: &mut Vec<ObservationFacet>,
    diagnostics: &mut Vec<AvailabilityIssue>,
) {
    let Some(payload) = payload else {
        diagnostics.push(codex_issue(
            source,
            offset,
            None,
            AvailabilityCode::NotCaptured,
        ));
        return;
    };
    match subtype {
        Some("message") => {
            let role = match codex_string(payload, "role") {
                Some("user") => MessageRole::User,
                Some("assistant") => MessageRole::Assistant,
                Some("system") => MessageRole::System,
                Some("developer") => MessageRole::Developer,
                _ => MessageRole::Unknown,
            };
            let marker = match role {
                MessageRole::User => RequestMarker::Initiating,
                MessageRole::System | MessageRole::Developer => RequestMarker::Injected,
                _ => RequestMarker::Unknown,
            };
            facets.push(ObservationFacet::Message {
                native_id: codex_string(payload, "id").map(str::to_owned),
                role,
                parts: codex_parts(payload.get("content"), access, FieldId::Text),
                request_marker: marker,
                turn_id: turn_id.map(str::to_owned),
            });
        }
        Some("reasoning") => {
            let mut parts = codex_reasoning_parts(payload.get("summary"), access);
            parts.extend(codex_reasoning_parts(payload.get("content"), access));
            if payload
                .get("encrypted_content")
                .is_some_and(|value| !value.is_null())
            {
                parts.push(ObservationPart::Unavailable(AvailabilityCode::NotSupported));
            }
            facets.push(ObservationFacet::Message {
                native_id: codex_string(payload, "id").map(str::to_owned),
                role: MessageRole::Assistant,
                parts,
                request_marker: RequestMarker::Summary,
                turn_id: turn_id.map(str::to_owned),
            });
        }
        Some("function_call" | "custom_tool_call") => {
            let Some(call_id) = codex_string(payload, "call_id") else {
                diagnostics.push(codex_issue(
                    source,
                    offset,
                    Some(FieldId::CallId),
                    AvailabilityCode::NotCaptured,
                ));
                return;
            };
            let Some(name) = codex_string(payload, "name") else {
                diagnostics.push(codex_issue(
                    source,
                    offset,
                    Some(FieldId::ToolName),
                    AvailabilityCode::NotCaptured,
                ));
                return;
            };
            let key = if subtype == Some("function_call") {
                "arguments"
            } else {
                "input"
            };
            let input = payload.get(key).map_or_else(
                || vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)],
                |value| {
                    vec![codex_retained(access, FieldId::Input, || {
                        ObservationPart::Structured(value.clone())
                    })]
                },
            );
            facets.push(ObservationFacet::ToolCall {
                native_call_id: call_id.into(),
                native_name: name.into(),
                family: codex_tool_family(name).map(str::to_owned),
                input,
                turn_id: turn_id.map(str::to_owned),
            });
        }
        Some("function_call_output" | "custom_tool_call_output") => {
            let Some(call_id) = codex_string(payload, "call_id") else {
                diagnostics.push(codex_issue(
                    source,
                    offset,
                    Some(FieldId::CallId),
                    AvailabilityCode::NotCaptured,
                ));
                return;
            };
            let output = payload.get("output").map_or_else(
                || vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)],
                |value| {
                    vec![codex_retained(access, FieldId::Output, || {
                        ObservationPart::Structured(value.clone())
                    })]
                },
            );
            facets.push(ObservationFacet::ToolResult {
                native_call_id: call_id.into(),
                native_name: codex_string(payload, "name").map(str::to_owned),
                output,
                outcome: Outcome::Unknown,
                exit_code: None,
                reported_duration_ms: None,
                turn_id: turn_id.map(str::to_owned),
            });
        }
        Some("compaction" | "compaction_summary" | "context_compaction") => {
            facets.push(ObservationFacet::Control {
                kind: ControlKind::Compaction,
                links: Vec::new(),
            });
        }
        Some(_) | None => diagnostics.push(codex_issue(
            source,
            offset,
            None,
            AvailabilityCode::NotSupported,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn inspect_codex_event(
    payload: Option<&Map<String, Value>>,
    subtype: Option<&str>,
    turn_id: Option<&str>,
    access: &ContentAccess,
    source: unisphere_core::query::SourceId,
    offset: Option<u64>,
    session_id: Option<&str>,
    facets: &mut Vec<ObservationFacet>,
    diagnostics: &mut Vec<AvailabilityIssue>,
) {
    let Some(payload) = payload else {
        diagnostics.push(codex_issue(
            source,
            offset,
            None,
            AvailabilityCode::NotCaptured,
        ));
        return;
    };
    match subtype {
        Some("token_count") => inspect_codex_usage(
            payload.get("info"),
            turn_id,
            session_id,
            source,
            offset,
            facets,
            diagnostics,
        ),
        Some(
            "user_message" | "agent_message" | "agent_reasoning" | "agent_reasoning_raw_content",
        ) => {
            facets.push(ObservationFacet::Control {
                kind: ControlKind::Summary,
                links: Vec::new(),
            });
        }
        Some("context_compacted") => facets.push(ObservationFacet::Control {
            kind: ControlKind::Compaction,
            links: Vec::new(),
        }),
        Some(
            "turn_started" | "task_started" | "turn_complete" | "task_complete" | "turn_aborted",
        ) => {
            facets.push(ObservationFacet::Control {
                kind: ControlKind::Other,
                links: Vec::new(),
            });
        }
        Some("exec_command_begin") => {
            if let Some(call_id) = codex_string(payload, "call_id") {
                facets.push(ObservationFacet::ToolProgress {
                    native_call_id: call_id.into(),
                    parts: Vec::new(),
                });
            }
        }
        Some("exec_command_end") => {
            if let Some(call_id) = codex_string(payload, "call_id") {
                let exit_code = payload.get("exit_code").and_then(Value::as_i64);
                facets.push(ObservationFacet::ToolResult {
                    native_call_id: call_id.into(),
                    native_name: None,
                    output: payload.get("aggregated_output").map_or_else(Vec::new, |_| {
                        vec![codex_retained(access, FieldId::Output, || {
                            ObservationPart::Unavailable(AvailabilityCode::NotSupported)
                        })]
                    }),
                    outcome: match exit_code {
                        Some(0) => Outcome::Succeeded,
                        Some(_) => Outcome::Failed,
                        None => Outcome::Unknown,
                    },
                    exit_code,
                    reported_duration_ms: None,
                    turn_id: turn_id.map(str::to_owned),
                });
            }
        }
        Some(_) | None => diagnostics.push(codex_issue(
            source,
            offset,
            None,
            AvailabilityCode::NotSupported,
        )),
    }
}

fn inspect_codex_usage(
    info: Option<&Value>,
    turn_id: Option<&str>,
    session_id: Option<&str>,
    source: unisphere_core::query::SourceId,
    offset: Option<u64>,
    facets: &mut Vec<ObservationFacet>,
    diagnostics: &mut Vec<AvailabilityIssue>,
) {
    let Some(info) = info.and_then(Value::as_object) else {
        return;
    };
    for (field, scope, owner) in [
        ("last_token_usage", UsageScope::Turn, turn_id),
        (
            "total_token_usage",
            UsageScope::CumulativeSnapshot,
            session_id,
        ),
    ] {
        let Some(usage) = info.get(field).and_then(Value::as_object) else {
            continue;
        };
        let mut count = |native: &str, query_field: FieldId| {
            usage.get(native).and_then(|value| {
                let valid = value
                    .as_i64()
                    .filter(|value| *value >= 0)
                    .map(|value| value as u64);
                if valid.is_none() {
                    diagnostics.push(codex_issue(
                        source,
                        offset,
                        Some(query_field),
                        AvailabilityCode::ProjectionMissing,
                    ));
                }
                valid
            })
        };
        let counters = UsageCounters {
            input_tokens: count("input_tokens", FieldId::InputTokens),
            output_tokens: count("output_tokens", FieldId::OutputTokens),
            cache_read_tokens: count("cached_input_tokens", FieldId::CacheReadTokens),
            cache_write_tokens: count("cache_write_input_tokens", FieldId::CacheWriteTokens),
        };
        if counters.input_tokens.is_some()
            || counters.output_tokens.is_some()
            || counters.cache_read_tokens.is_some()
            || counters.cache_write_tokens.is_some()
        {
            facets.push(ObservationFacet::Usage {
                owner: owner.map(str::to_owned),
                scope,
                counters,
            });
        }
    }
}

fn codex_parts(
    value: Option<&Value>,
    access: &ContentAccess,
    field: FieldId,
) -> Vec<ObservationPart> {
    let Some(parts) = value.and_then(Value::as_array) else {
        return vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)];
    };
    parts
        .iter()
        .map(|part| {
            let text = part.as_object().and_then(|part| codex_string(part, "text"));
            match text {
                Some(text) => codex_retained(access, field, || ObservationPart::Text(text.into())),
                None => ObservationPart::Unavailable(AvailabilityCode::NotSupported),
            }
        })
        .collect()
}

fn codex_reasoning_parts(value: Option<&Value>, access: &ContentAccess) -> Vec<ObservationPart> {
    let Some(parts) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    parts
        .iter()
        .map(|part| {
            let text = part.as_object().and_then(|part| codex_string(part, "text"));
            match text {
                Some(text) => codex_retained(access, FieldId::Parts, || {
                    ObservationPart::Reasoning(text.into())
                }),
                None => ObservationPart::Unavailable(AvailabilityCode::NotSupported),
            }
        })
        .collect()
}

fn codex_retained(
    access: &ContentAccess,
    field: FieldId,
    value: impl FnOnce() -> ObservationPart,
) -> ObservationPart {
    if access.permits_payload(field) || access.permits_payload(FieldId::Parts) {
        value()
    } else {
        ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)
    }
}

fn codex_native_record_id(payload: &Map<String, Value>) -> Option<&str> {
    codex_string(payload, "id").or_else(|| codex_string(payload, "item_id"))
}

fn codex_string<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn codex_timestamp(
    value: Option<&Value>,
    source: unisphere_core::query::SourceId,
    offset: Option<u64>,
    diagnostics: &mut Vec<AvailabilityIssue>,
) -> Option<Timestamp> {
    let value = value?;
    let parsed = value
        .as_str()
        .and_then(|value| Timestamp::parse(value, TimestampBasis::Native).ok());
    if parsed.is_none() {
        diagnostics.push(codex_issue(
            source,
            offset,
            Some(FieldId::Timestamp),
            AvailabilityCode::InvalidClock,
        ));
    }
    parsed
}

fn codex_issue(
    source: unisphere_core::query::SourceId,
    offset: Option<u64>,
    field: Option<FieldId>,
    code: AvailabilityCode,
) -> AvailabilityIssue {
    AvailabilityIssue {
        code,
        field,
        source: Some(source),
        entity: None,
        offset,
    }
}

fn codex_locator_offset(locator: &NativeLocator) -> Option<u64> {
    match locator {
        NativeLocator::Jsonl { offset } => Some(*offset),
        NativeLocator::Snapshot { .. } | NativeLocator::GitNote { .. } => None,
    }
}

fn codex_sequence(locator: &NativeLocator) -> NativeSequence {
    let key = match locator {
        NativeLocator::Jsonl { offset } => offset.to_be_bytes().to_vec(),
        NativeLocator::Snapshot { key } => key.as_bytes().to_vec(),
        NativeLocator::GitNote { note_blob, .. } => note_blob.as_bytes().to_vec(),
    };
    NativeSequence { version: 1, key }
}

fn codex_partition_key(session: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(8 + session.len());
    key.extend_from_slice(&(session.len() as u64).to_le_bytes());
    key.extend_from_slice(session.as_bytes());
    key
}

fn codex_tool_family(name: &str) -> Option<&'static str> {
    match name {
        "shell" | "exec_command" | "container.exec" => Some("shell"),
        "read_file" | "cat" => Some("file-read"),
        "apply_patch" | "write_file" => Some("file-write"),
        _ => None,
    }
}

struct Mapping<'a> {
    offset: u64,
    include_content: bool,
    content_present: bool,
    attributes: BTreeMap<String, Value>,
    diagnostics: &'a mut Vec<MappingDiagnostic>,
}

impl Mapping<'_> {
    fn diagnostic(&mut self, code: MappingDiagnosticCode) {
        self.diagnostics.push(MappingDiagnostic {
            offset: self.offset,
            code,
        });
    }

    fn object(&mut self, value: Option<Value>) -> Option<Map<String, Value>> {
        match value {
            Some(Value::Object(value)) => Some(value),
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                None
            }
        }
    }

    fn string(&mut self, object: &mut Map<String, Value>, key: &str) -> Option<String> {
        match object.remove(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => Some(value),
            Some(_) => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                None
            }
        }
    }

    fn strings(&mut self, object: &mut Map<String, Value>, fields: &[(&str, &str)]) {
        for (native, profile) in fields {
            if let Some(value) = self.string(object, native) {
                self.attributes
                    .insert((*profile).into(), Value::String(value));
            }
        }
    }

    fn kind(&mut self, object: &mut Map<String, Value>) -> String {
        match object.remove("type") {
            Some(Value::String(kind)) => kind,
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                "unknown".into()
            }
        }
    }

    fn record(mut self, value: Value) -> TelemetryRecord {
        let mut object = self.object(Some(value)).unwrap_or_default();
        let kind = self.kind(&mut object);
        let timestamp_unix_nano = object.remove("timestamp").and_then(|value| {
            let parsed = value
                .as_str()
                .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
                .and_then(|value| u64::try_from(value.unix_timestamp_nanos()).ok());
            if parsed.is_none() {
                self.diagnostic(MappingDiagnosticCode::InvalidTimestamp);
            }
            parsed
        });
        let body = self
            .object(object.remove("payload"))
            .and_then(|mut payload| {
                self.strings(
                    &mut payload,
                    &[
                        ("session_id", "unisphere.source.session.id"),
                        ("turn_id", "unisphere.source.turn.id"),
                        ("root_turn_id", "unisphere.source.root_turn.id"),
                    ],
                );
                match kind.as_str() {
                    "session_meta" => {
                        self.strings(
                            &mut payload,
                            &[
                                ("id", "gen_ai.conversation.id"),
                                ("forked_from_id", "unisphere.source.parent.id"),
                                ("parent_thread_id", "unisphere.codex.parent_thread.id"),
                                ("originator", "unisphere.codex.originator"),
                                ("cli_version", "unisphere.codex.cli_version"),
                                ("model_provider", "gen_ai.provider.name"),
                            ],
                        );
                        // Instructions/configuration are not conversation messages.
                        if ["base_instructions", "instructions"]
                            .iter()
                            .any(|key| payload.get(*key).is_some_and(|value| !value.is_null()))
                        {
                            self.content_present = true;
                            self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                        }
                        None
                    }
                    "turn_context" => {
                        self.strings(
                            &mut payload,
                            &[
                                ("model", "gen_ai.request.model"),
                                ("effort", "unisphere.codex.reasoning_effort"),
                            ],
                        );
                        if ["user_instructions", "developer_instructions"]
                            .iter()
                            .any(|key| payload.get(*key).is_some_and(|value| !value.is_null()))
                        {
                            self.content_present = true;
                            self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                        }
                        None
                    }
                    "response_item" | "event_msg" => {
                        let subtype = self.kind(&mut payload);
                        let body = if kind == "response_item" {
                            self.response(&subtype, payload)
                        } else {
                            self.event(&subtype, payload)
                        };
                        self.attributes
                            .insert("unisphere.codex.payload.type".into(), json!(subtype));
                        body
                    }
                    "compacted" => {
                        self.attributes
                            .insert("unisphere.codex.representation".into(), json!("compaction"));
                        if payload
                            .get("replacement_history")
                            .is_some_and(|value| !value.is_null())
                        {
                            self.content_present = true;
                            self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                        }
                        let part = self.text(payload.remove("message"), "unisphere.compaction");
                        self.body(None, part.into_iter().collect())
                    }
                    _ => {
                        self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
                        None
                    }
                }
            });
        self.attributes
            .insert("unisphere.source.kind".into(), json!(kind));
        if self.content_present && !self.include_content {
            self.attributes
                .insert("unisphere.content.omitted".into(), json!(true));
            self.diagnostic(MappingDiagnosticCode::ContentOmitted);
        }
        TelemetryRecord {
            event_name: "unisphere.session.record".into(),
            timestamp_unix_nano,
            attributes: self.attributes,
            body,
        }
    }

    fn response(&mut self, kind: &str, mut payload: Map<String, Value>) -> Option<Value> {
        self.strings(
            &mut payload,
            &[
                ("id", "unisphere.source.record.id"),
                ("call_id", "unisphere.tool.call.id"),
                ("name", "unisphere.tool.name"),
                ("namespace", "unisphere.codex.tool.namespace"),
                ("status", "unisphere.codex.status"),
                ("phase", "unisphere.codex.phase"),
            ],
        );
        if let Some(value) = payload
            .remove("internal_chat_message_metadata_passthrough")
            .filter(|value| !value.is_null())
            && let Some(mut metadata) = self.object(Some(value))
        {
            self.strings(&mut metadata, &[("turn_id", "unisphere.source.turn.id")]);
        }
        match kind {
            "message" => {
                let role = self.string(&mut payload, "role");
                let role = match role {
                    Some(role)
                        if matches!(
                            role.as_str(),
                            "user" | "assistant" | "system" | "developer"
                        ) =>
                    {
                        self.attributes
                            .insert("unisphere.message.role".into(), json!(role));
                        Some(role)
                    }
                    _ => {
                        self.diagnostic(MappingDiagnosticCode::InvalidField);
                        None
                    }
                };
                let parts = self.parts(payload.remove("content"), false);
                role.and_then(|role| self.body(Some(role), parts))
            }
            "reasoning" => {
                let mut parts = self.parts(payload.remove("summary"), true);
                if let Some(content) = payload.remove("content").filter(|value| !value.is_null()) {
                    parts.extend(self.parts(Some(content), true));
                }
                if payload
                    .get("encrypted_content")
                    .is_some_and(|value| !value.is_null())
                {
                    self.content_present = true;
                    self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                    if self.include_content {
                        parts.push(json!({"type": "unisphere.unknown", "native_type": "encrypted_content"}));
                    }
                }
                self.body(None, parts)
            }
            "function_call" | "custom_tool_call" => {
                let key = if kind == "function_call" {
                    "arguments"
                } else {
                    "input"
                };
                self.content_present |= payload.contains_key(key);
                let arguments = self.string(&mut payload, key);
                if arguments.is_none() || !self.attributes.contains_key("unisphere.tool.name") {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                }
                if payload
                    .get("encrypted_function_args")
                    .is_some_and(|value| !value.is_null())
                {
                    self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                }
                if !self.include_content {
                    return None;
                }
                let mut part = self.tool_part("tool_call");
                part["arguments"] = Value::String(arguments?);
                self.body(None, vec![part])
            }
            "function_call_output" | "custom_tool_call_output" => {
                self.content_present |= payload.contains_key("output");
                let response = match payload.remove("output") {
                    Some(Value::String(text)) => {
                        self.include_content.then_some(Value::String(text))
                    }
                    Some(value @ Value::Array(_)) => {
                        let parts = self.parts(Some(value), false);
                        self.include_content.then_some(Value::Array(parts))
                    }
                    _ => {
                        self.diagnostic(MappingDiagnosticCode::InvalidField);
                        None
                    }
                }?;
                let mut part = self.tool_part("tool_call_response");
                part["response"] = response;
                self.body(None, vec![part])
            }
            "local_shell_call" | "web_search_call" => {
                let action = self.action(payload.remove("action"), kind)?;
                let mut part = self.tool_part("tool_call");
                // These native kinds identify the tool; no invented native name.
                part["unisphere.codex.tool_kind"] = json!(kind);
                part["arguments"] = action;
                self.body(None, vec![part])
            }
            "compaction" | "compaction_summary" | "context_compaction" => {
                self.attributes
                    .insert("unisphere.codex.representation".into(), json!("compaction"));
                self.content_present = payload
                    .get("encrypted_content")
                    .is_some_and(|value| !value.is_null());
                self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                None
            }
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
                None
            }
        }
    }

    fn event(&mut self, kind: &str, mut payload: Map<String, Value>) -> Option<Value> {
        self.attributes.insert(
            "unisphere.codex.representation".into(),
            json!("native_event_summary"),
        );
        self.strings(
            &mut payload,
            &[
                ("call_id", "unisphere.tool.call.id"),
                ("item_id", "unisphere.source.record.id"),
                ("phase", "unisphere.codex.phase"),
            ],
        );
        match kind {
            "token_count" => {
                self.usage(payload.remove("info"));
                None
            }
            "user_message" | "agent_message" => {
                for key in ["images", "local_images", "audio", "local_audio"] {
                    if payload.get(key).is_some_and(|value| {
                        !value.is_null() && value.as_array().is_none_or(|items| !items.is_empty())
                    }) {
                        self.content_present = true;
                        self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                    }
                }
                let part = self.text(payload.remove("message"), "unisphere.codex.message_summary");
                self.body(None, part.into_iter().collect())
            }
            "agent_reasoning" | "agent_reasoning_raw_content" => {
                let part = self.text(payload.remove("text"), "unisphere.codex.reasoning_summary");
                self.body(None, part.into_iter().collect())
            }
            "task_started" | "turn_started" | "task_complete" | "turn_complete"
            | "turn_aborted" | "context_compacted" => None,
            // Execution summaries are not additional calls/results. Keep pairing
            // metadata, but do not re-emit their commands or output as tool parts.
            "exec_command_begin" | "exec_command_end" => {
                if let Some(value) = payload.remove("exit_code") {
                    if value.as_i64().is_some() {
                        self.attributes
                            .insert("unisphere.codex.exit_code".into(), value);
                    } else {
                        self.diagnostic(MappingDiagnosticCode::InvalidField);
                    }
                }
                None
            }
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
                None
            }
        }
    }

    fn usage(&mut self, value: Option<Value>) {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return;
        };
        let Some(mut info) = self.object(Some(value)) else {
            return;
        };
        for (native, scope) in [
            ("last_token_usage", "native_last_token_usage"),
            ("total_token_usage", "native_cumulative_token_usage"),
        ] {
            let Some(value) = info.remove(native).filter(|value| !value.is_null()) else {
                continue;
            };
            let Some(mut usage) = self.object(Some(value)) else {
                continue;
            };
            let mut retained = false;
            for field in [
                "input_tokens",
                "cached_input_tokens",
                "cache_write_input_tokens",
                "output_tokens",
                "reasoning_output_tokens",
                "total_tokens",
            ] {
                if let Some(value) = usage.remove(field) {
                    if value.as_i64().is_some_and(|value| value >= 0) {
                        self.attributes
                            .insert(format!("unisphere.usage.{native}.{field}"), value);
                        retained = true;
                    } else {
                        self.diagnostic(MappingDiagnosticCode::InvalidField);
                    }
                }
            }
            if retained {
                self.attributes
                    .insert(format!("unisphere.usage.{native}.scope"), json!(scope));
            }
        }
        if let Some(value) = info
            .remove("model_context_window")
            .filter(|value| !value.is_null())
        {
            if value.as_i64().is_some_and(|value| value >= 0) {
                self.attributes
                    .insert("unisphere.codex.model_context_window".into(), value);
            } else {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
            }
        }
    }

    fn text(&mut self, value: Option<Value>, kind: &str) -> Option<Value> {
        self.content_present |= value.is_some();
        match value {
            Some(Value::String(text)) => self.include_content.then(|| {
                let mut part = json!({"type": kind});
                part["content"] = Value::String(text);
                part
            }),
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                None
            }
        }
    }

    fn parts(&mut self, value: Option<Value>, reasoning: bool) -> Vec<Value> {
        self.content_present |= value.is_some();
        let Some(Value::Array(parts)) = value else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return Vec::new();
        };
        parts
            .into_iter()
            .filter_map(|part| {
                let mut part = self.object(Some(part))?;
                let kind = self.kind(&mut part);
                let supported = if reasoning {
                    matches!(kind.as_str(), "summary_text" | "reasoning_text" | "text")
                } else {
                    matches!(kind.as_str(), "input_text" | "output_text")
                };
                if supported {
                    let mut text = self.text(
                        part.remove("text"),
                        if reasoning { "reasoning" } else { "text" },
                    )?;
                    text["unisphere.codex.native_type"] = json!(kind);
                    Some(text)
                } else {
                    self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                    self.include_content
                        .then(|| json!({"type": "unisphere.unknown", "native_type": kind}))
                }
            })
            .collect()
    }

    fn tool_part(&self, kind: &str) -> Value {
        let mut part = json!({"type": kind});
        for (attribute, field) in [
            ("unisphere.tool.call.id", "id"),
            ("unisphere.tool.name", "name"),
        ] {
            if let Some(value) = self.attributes.get(attribute) {
                part[field] = value.clone();
            }
        }
        part
    }

    fn action(&mut self, value: Option<Value>, tool_kind: &str) -> Option<Value> {
        let value = value.filter(|value| !value.is_null())?;
        self.content_present = true;
        let mut action = self.object(Some(value))?;
        let kind = self.kind(&mut action);
        let fields: &[(&str, bool)] = match (tool_kind, kind.as_str()) {
            ("local_shell_call", "exec") => &[
                ("command", true),
                ("working_directory", false),
                ("user", false),
            ],
            ("web_search_call", "search") => &[("query", false), ("queries", true)],
            ("web_search_call", "open_page") => &[("url", false)],
            ("web_search_call", "find_in_page") => &[("url", false), ("pattern", false)],
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                return None;
            }
        };
        let mut retained = Map::new();
        for (field, array) in fields {
            if let Some(value) = action.remove(*field).filter(|value| !value.is_null()) {
                let valid = if *array {
                    value
                        .as_array()
                        .is_some_and(|values| values.iter().all(Value::is_string))
                } else {
                    value.is_string()
                };
                if !valid {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                } else if self.include_content {
                    retained.insert((*field).into(), value);
                }
            }
        }
        self.include_content.then(|| {
            retained.insert("type".into(), json!(kind));
            Value::Object(retained)
        })
    }

    fn body(&self, role: Option<String>, parts: Vec<Value>) -> Option<Value> {
        self.include_content.then(|| {
            let mut body = Map::from_iter([("parts".into(), Value::Array(parts))]);
            if let Some(role) = role {
                body.insert("role".into(), Value::String(role));
            }
            Value::Object(body)
        })
    }
}
