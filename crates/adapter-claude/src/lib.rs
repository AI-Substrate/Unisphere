//! Deterministic Claude Code record mapping over caller-supplied bytes.
//!
//! This adapter emits physical source fragments, not inference operations or a
//! reconstructed conversation. Metadata-only is the default. No source, sidecar,
//! attachment, environment, clock, or destination is accessed here.
#![forbid(unsafe_code)]

mod prep;
pub use prep::{ClaudePrepFold, PREP_POLICY_VERSION};

use std::{collections::BTreeMap, path::PathBuf};

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    MappedBatch, MappingDiagnostic, MappingDiagnosticCode, MappingOptions, NativeRecord,
    PipelineError, PipelineErrorKind, SessionAdapter, SessionRef, TelemetryRecord,
    query::{
        AssociationBasis, AssociationExtent, AssociationObservation, AvailabilityCode,
        AvailabilityIssue, BranchEvidence, ContentAccess, FieldId, InspectedSource, LimitKind,
        MembershipPolicy, MessageRole, NativeLocator, NativeQueryInput, NativeSequence,
        Observation, ObservationFacet, ObservationPart, Outcome, PartitionId, QueryAdapter,
        QueryErrorLocation, QueryFailure, QueryFailureCode, QueryLimits, RecoveryAction,
        RequestMarker, SessionEvidenceKey, SourcePartition, SourceProblem, SourceRef,
        SourceViewKind, Timestamp, TimestampBasis, UsageCounters, UsageScope,
    },
};

/// Query reconstruction policy applied to supplied Claude Code records.
pub const QUERY_POLICY_VERSION: &str = "claude-code/query-v1";

/// Stateless mapper for the documented Claude Code JSONL dialect.
///
/// Malformed JSON/UTF-8 fails the entire supplied batch with a fixed public error
/// and its physical offset. Valid but unsupported structures remain observable
/// through provenance records and structural diagnostics. Equal input and options
/// produce equal output, including diagnostic order.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeCodeAdapter;

impl SessionAdapter for ClaudeCodeAdapter {
    fn name(&self) -> &'static str {
        "claude-code"
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
            let value: Value = decode_native_json(&native.bytes).map_err(|_| {
                PipelineError::new(PipelineErrorKind::InvalidData, Some(native.offset))
            })?;
            let mut mapping = Mapping {
                offset: native.offset,
                include_content: options.include_content,
                diagnostics: &mut batch.diagnostics,
            };
            batch.records.push(mapping.record(value, path));
        }
        Ok(batch)
    }
}

impl QueryAdapter for ClaudeCodeAdapter {
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
        let mut partitions = BTreeMap::<(String, Option<String>), usize>::new();
        let source_only = PartitionId::derive(source.id, b"claude/source-only/v1");

        for record in records {
            let offset = locator_offset(&record.locator);
            let value: Value = decode_native_json(&record.bytes).map_err(|_| {
                QueryFailure::invalid_data().at(QueryErrorLocation {
                    source: Some(source.id),
                    offset,
                    ..QueryErrorLocation::default()
                })
            })?;
            let mut diagnostics = Vec::new();
            let Some(object) = value.as_object() else {
                diagnostics.push(query_issue(
                    source.id,
                    offset,
                    None,
                    AvailabilityCode::NotSupported,
                ));
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
                    sequence: native_sequence(&record.locator),
                    timestamp: None,
                    facets: Vec::new(),
                    diagnostics: diagnostics.clone(),
                });
                inspected.issues.extend(diagnostics);
                continue;
            };

            let kind = string_field(object, "type");
            let native_record_id = string_field(object, "uuid").map(str::to_owned);
            let parent_ids = string_field(object, "parentUuid")
                .map(|value| vec![value.to_owned()])
                .unwrap_or_default();
            let session_id = string_field(object, "sessionId").map(str::to_owned);
            let participant_id = string_field(object, "agentId").map(str::to_owned);
            let (partition, session, branch) = if let Some(session_id) = session_id.as_ref() {
                let key = (session_id.clone(), participant_id.clone());
                let partition_index = if let Some(index) = partitions.get(&key) {
                    *index
                } else {
                    let id = PartitionId::derive(
                        source.id,
                        &partition_key(session_id, participant_id.as_deref()),
                    );
                    let index = inspected.partitions.len();
                    inspected.partitions.push(SourcePartition {
                        id,
                        native_session_id: Some(session_id.clone()),
                        participant_id: participant_id.clone(),
                        view: SourceViewKind::Conversation,
                        membership: MembershipPolicy::NativeContainment,
                        associations: Vec::new(),
                    });
                    partitions.insert(key, index);
                    index
                };
                let id = inspected.partitions[partition_index].id;
                (
                    id,
                    Some(SessionEvidenceKey {
                        namespace: "claude-code/session".into(),
                        native_id: session_id.clone(),
                        participant_id: participant_id.clone(),
                        parent_native_id: None,
                        fork_native_id: None,
                        membership_basis: MembershipPolicy::NativeContainment,
                    }),
                    BranchEvidence::Linear { partition: id },
                )
            } else {
                (
                    source_only,
                    None,
                    BranchEvidence::Unavailable {
                        partition: source_only,
                        reason: AvailabilityCode::NotCaptured,
                    },
                )
            };
            let branch = match native_record_id.as_ref() {
                Some(native_id) if session.is_some() => BranchEvidence::Node {
                    partition,
                    native_id: native_id.clone(),
                    parent: parent_ids.first().cloned(),
                    declared_branch: object
                        .get("isSidechain")
                        .and_then(Value::as_bool)
                        .map(|sidechain| if sidechain { "sidechain" } else { "main" }.into()),
                    links: Vec::new(),
                },
                _ => branch,
            };

            let sequence = native_sequence(&record.locator);
            if let Some(cwd) = string_field(object, "cwd") {
                let association = AssociationObservation {
                    basis: AssociationBasis::NativeCwd,
                    path: Some(PathBuf::from(cwd)),
                    partition,
                    applies_to: AssociationExtent::Record(sequence.clone()),
                };
                if let Some(index) = partitions
                    .get(&(
                        session_id.clone().unwrap_or_default(),
                        participant_id.clone(),
                    ))
                    .copied()
                {
                    inspected.partitions[index]
                        .associations
                        .push(association.clone());
                }
                inspected.source.associations.push(association);
            }

            let timestamp =
                parse_query_timestamp(object.get("timestamp"), source.id, offset, &mut diagnostics);
            let mut facets = Vec::new();
            match kind {
                Some("user" | "assistant") => inspect_claude_message(
                    object,
                    kind.unwrap_or_default(),
                    &access,
                    source.id,
                    offset,
                    &mut facets,
                    &mut diagnostics,
                ),
                Some("summary") => facets.push(ObservationFacet::Control {
                    kind: unisphere_core::query::ControlKind::Summary,
                    links: Vec::new(),
                }),
                _ => diagnostics.push(query_issue(
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
                    subrecord: kind.unwrap_or("unknown").to_owned(),
                },
                native_record_id,
                session,
                branch,
                parent_ids,
                sequence,
                timestamp,
                facets,
                diagnostics: diagnostics.clone(),
            });
            inspected.issues.extend(diagnostics);
        }

        if inspected.partitions.is_empty()
            || inspected
                .observations
                .iter()
                .any(|observation| observation.session.is_none())
        {
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

fn decode_native_json(bytes: &[u8]) -> serde_json::Result<Value> {
    serde_json::from_slice(bytes)
}

fn inspect_claude_message(
    object: &Map<String, Value>,
    outer_kind: &str,
    access: &ContentAccess,
    source_id: unisphere_core::query::SourceId,
    offset: Option<u64>,
    facets: &mut Vec<ObservationFacet>,
    diagnostics: &mut Vec<AvailabilityIssue>,
) {
    let Some(message) = object.get("message").and_then(Value::as_object) else {
        diagnostics.push(query_issue(
            source_id,
            offset,
            Some(FieldId::Parts),
            AvailabilityCode::NotCaptured,
        ));
        return;
    };
    let role = match string_field(message, "role") {
        Some("user") => MessageRole::User,
        Some("assistant") => MessageRole::Assistant,
        _ => MessageRole::Unknown,
    };
    let message_id = string_field(message, "id").map(str::to_owned);
    let mut parts = Vec::new();
    let mut has_text = false;
    let mut has_tool_result = false;
    match message.get("content") {
        Some(Value::String(text)) => {
            has_text = true;
            parts.push(retained_part(access, FieldId::Text, || {
                ObservationPart::Text(text.clone())
            }));
        }
        Some(Value::Array(native_parts)) => {
            for part in native_parts {
                let Some(part) = part.as_object() else {
                    diagnostics.push(query_issue(
                        source_id,
                        offset,
                        Some(FieldId::Parts),
                        AvailabilityCode::NotSupported,
                    ));
                    continue;
                };
                match string_field(part, "type") {
                    Some("text") => {
                        if let Some(text) = string_field(part, "text") {
                            has_text = true;
                            parts.push(retained_part(access, FieldId::Text, || {
                                ObservationPart::Text(text.into())
                            }));
                        }
                    }
                    Some("thinking") => {
                        if let Some(text) = string_field(part, "thinking") {
                            parts.push(retained_part(access, FieldId::Parts, || {
                                ObservationPart::Reasoning(text.into())
                            }));
                        }
                    }
                    Some("tool_use") => {
                        let Some(call_id) = string_field(part, "id") else {
                            diagnostics.push(query_issue(
                                source_id,
                                offset,
                                Some(FieldId::CallId),
                                AvailabilityCode::NotCaptured,
                            ));
                            continue;
                        };
                        let Some(name) = string_field(part, "name") else {
                            diagnostics.push(query_issue(
                                source_id,
                                offset,
                                Some(FieldId::ToolName),
                                AvailabilityCode::NotCaptured,
                            ));
                            continue;
                        };
                        let input = part.get("input").map_or_else(
                            || vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)],
                            |value| {
                                vec![retained_part(access, FieldId::Input, || {
                                    ObservationPart::Structured(value.clone())
                                })]
                            },
                        );
                        facets.push(ObservationFacet::ToolCall {
                            native_call_id: call_id.into(),
                            native_name: name.into(),
                            family: tool_family(name).map(str::to_owned),
                            input,
                            turn_id: None,
                        });
                    }
                    Some("tool_result") => {
                        has_tool_result = true;
                        let Some(call_id) = string_field(part, "tool_use_id") else {
                            diagnostics.push(query_issue(
                                source_id,
                                offset,
                                Some(FieldId::CallId),
                                AvailabilityCode::NotCaptured,
                            ));
                            continue;
                        };
                        let output = part.get("content").map_or_else(
                            || vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)],
                            |value| {
                                vec![retained_part(access, FieldId::Output, || {
                                    ObservationPart::Structured(value.clone())
                                })]
                            },
                        );
                        let outcome = match part.get("is_error").and_then(Value::as_bool) {
                            Some(true) => Outcome::Failed,
                            Some(false) => Outcome::Succeeded,
                            None => Outcome::Unknown,
                        };
                        facets.push(ObservationFacet::ToolResult {
                            native_call_id: call_id.into(),
                            native_name: None,
                            output,
                            outcome,
                            exit_code: None,
                            reported_duration_ms: None,
                            turn_id: None,
                        });
                    }
                    Some(_) | None => {
                        parts.push(ObservationPart::Unavailable(AvailabilityCode::NotSupported))
                    }
                }
            }
        }
        _ => diagnostics.push(query_issue(
            source_id,
            offset,
            Some(FieldId::Parts),
            AvailabilityCode::NotCaptured,
        )),
    }
    let marker = if object.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
        RequestMarker::Summary
    } else if object.get("isMeta").and_then(Value::as_bool) == Some(true) {
        RequestMarker::Injected
    } else if role == MessageRole::User && has_text {
        RequestMarker::Initiating
    } else if role == MessageRole::User && has_tool_result {
        RequestMarker::ToolResponse
    } else {
        RequestMarker::Unknown
    };
    facets.push(ObservationFacet::Message {
        native_id: message_id.clone(),
        role,
        parts,
        request_marker: marker,
        turn_id: None,
    });
    inspect_claude_usage(message, message_id, source_id, offset, facets, diagnostics);
    if outer_kind != role_name(role) {
        diagnostics.push(query_issue(
            source_id,
            offset,
            Some(FieldId::Role),
            AvailabilityCode::Conflict,
        ));
    }
}

fn inspect_claude_usage(
    message: &Map<String, Value>,
    owner: Option<String>,
    source_id: unisphere_core::query::SourceId,
    offset: Option<u64>,
    facets: &mut Vec<ObservationFacet>,
    diagnostics: &mut Vec<AvailabilityIssue>,
) {
    let Some(usage) = message.get("usage").and_then(Value::as_object) else {
        return;
    };
    let mut count = |field: &str, query_field: FieldId| {
        usage.get(field).and_then(|value| {
            let valid = value
                .as_i64()
                .filter(|value| *value >= 0)
                .map(|value| value as u64);
            if valid.is_none() {
                diagnostics.push(query_issue(
                    source_id,
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
        cache_read_tokens: count("cache_read_input_tokens", FieldId::CacheReadTokens),
        cache_write_tokens: count("cache_creation_input_tokens", FieldId::CacheWriteTokens),
    };
    if counters.input_tokens.is_some()
        || counters.output_tokens.is_some()
        || counters.cache_read_tokens.is_some()
        || counters.cache_write_tokens.is_some()
    {
        facets.push(ObservationFacet::Usage {
            owner,
            scope: UsageScope::Invocation,
            counters,
        });
    }
}

fn retained_part(
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

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn parse_query_timestamp(
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
        diagnostics.push(query_issue(
            source,
            offset,
            Some(FieldId::Timestamp),
            AvailabilityCode::InvalidClock,
        ));
    }
    parsed
}

fn query_issue(
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

fn locator_offset(locator: &NativeLocator) -> Option<u64> {
    match locator {
        NativeLocator::Jsonl { offset } => Some(*offset),
        NativeLocator::Snapshot { .. } | NativeLocator::GitNote { .. } => None,
    }
}

fn native_sequence(locator: &NativeLocator) -> NativeSequence {
    let key = match locator {
        NativeLocator::Jsonl { offset } => offset.to_be_bytes().to_vec(),
        NativeLocator::Snapshot { key } => key.as_bytes().to_vec(),
        NativeLocator::GitNote { note_blob, .. } => note_blob.as_bytes().to_vec(),
    };
    NativeSequence { version: 1, key }
}

fn partition_key(session: &str, participant: Option<&str>) -> Vec<u8> {
    let participant = participant.unwrap_or_default().as_bytes();
    let mut key = Vec::with_capacity(16 + session.len() + participant.len());
    key.extend_from_slice(&(session.len() as u64).to_le_bytes());
    key.extend_from_slice(session.as_bytes());
    key.extend_from_slice(&(participant.len() as u64).to_le_bytes());
    key.extend_from_slice(participant);
    key
}

fn role_name(role: MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        _ => "unknown",
    }
}

fn tool_family(name: &str) -> Option<&'static str> {
    match name {
        "Bash" | "Shell" | "exec_command" => Some("shell"),
        "Read" | "read_file" => Some("file-read"),
        "Write" | "Edit" | "MultiEdit" | "apply_patch" => Some("file-write"),
        _ => None,
    }
}

struct Mapping<'a> {
    offset: u64,
    include_content: bool,
    diagnostics: &'a mut Vec<MappingDiagnostic>,
}

impl Mapping<'_> {
    fn diagnostic(&mut self, code: MappingDiagnosticCode) {
        self.diagnostics.push(MappingDiagnostic {
            offset: self.offset,
            code,
        });
    }

    fn string<'a>(&mut self, object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
        match object.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => Some(value),
            Some(_) => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                None
            }
        }
    }

    fn record(&mut self, value: Value, path: &str) -> TelemetryRecord {
        let mut attributes = BTreeMap::from([
            ("unisphere.profile.version".into(), json!(1)),
            ("unisphere.source.adapter".into(), json!("claude-code")),
            ("unisphere.source.path".into(), json!(path)),
            ("unisphere.source.offset".into(), json!(self.offset)),
        ]);
        let mut object = match value {
            Value::Object(object) => object,
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                Map::new()
            }
        };
        let kind = match object.remove("type") {
            Some(Value::String(kind)) => kind,
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                "unknown".to_owned()
            }
        };
        let supported = matches!(kind.as_str(), "user" | "assistant");
        attributes.insert("unisphere.source.kind".into(), Value::String(kind));
        for (native, profile) in [
            ("uuid", "unisphere.source.record.id"),
            ("parentUuid", "unisphere.source.parent.id"),
            ("sessionId", "gen_ai.conversation.id"),
        ] {
            if let Some(value) = self.string(&object, native) {
                attributes.insert(profile.into(), json!(value));
            }
        }
        if let Some(value) = object.get("isSidechain") {
            if value.is_boolean() {
                attributes.insert("unisphere.source.is_sidechain".into(), value.clone());
            } else {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
            }
        }
        let timestamp_unix_nano = self.timestamp(object.get("timestamp"));
        let body = if supported {
            self.message(object.remove("message"), &mut attributes)
        } else {
            self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
            None
        };
        TelemetryRecord {
            event_name: "unisphere.session.record".into(),
            timestamp_unix_nano,
            attributes,
            body,
        }
    }

    fn timestamp(&mut self, value: Option<&Value>) -> Option<u64> {
        let value = value?;
        let parsed = value
            .as_str()
            .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
            .and_then(|value| u64::try_from(value.unix_timestamp_nanos()).ok());
        if parsed.is_none() {
            self.diagnostic(MappingDiagnosticCode::InvalidTimestamp);
        }
        parsed
    }

    fn message(
        &mut self,
        value: Option<Value>,
        attributes: &mut BTreeMap<String, Value>,
    ) -> Option<Value> {
        let Some(Value::Object(mut message)) = value else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        for (native, profile) in [
            ("id", "unisphere.message.id"),
            ("model", "gen_ai.response.model"),
        ] {
            if let Some(value) = self.string(&message, native) {
                attributes.insert(profile.into(), json!(value));
            }
        }
        self.usage(message.get("usage"), attributes);
        let role = match message.remove("role") {
            Some(Value::String(role)) if matches!(role.as_str(), "user" | "assistant") => {
                attributes.insert("unisphere.message.role".into(), json!(role));
                Some(role)
            }
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                None
            }
        };
        let Some(content) = message.remove("content") else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        if !self.include_content {
            attributes.insert("unisphere.content.omitted".into(), json!(true));
            self.diagnostic(MappingDiagnosticCode::ContentOmitted);
        }
        let parts = match content {
            Value::String(content) => {
                if self.include_content {
                    let mut part = json!({"type": "text"});
                    part["content"] = Value::String(content);
                    vec![part]
                } else {
                    Vec::new()
                }
            }
            Value::Array(parts) => parts
                .into_iter()
                .filter_map(|part| self.part(part))
                .collect(),
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                return None;
            }
        };
        if self.include_content {
            role.map(|role| {
                Value::Object(Map::from_iter([
                    ("role".into(), Value::String(role)),
                    ("parts".into(), Value::Array(parts)),
                ]))
            })
        } else {
            None
        }
    }

    fn usage(&mut self, value: Option<&Value>, attributes: &mut BTreeMap<String, Value>) {
        let Some(value) = value else { return };
        let Some(usage) = value.as_object() else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return;
        };
        let mut retained = false;
        for (native, profile) in [
            ("input_tokens", "unisphere.usage.input_tokens"),
            ("output_tokens", "unisphere.usage.output_tokens"),
            (
                "cache_read_input_tokens",
                "unisphere.usage.cache_read_input_tokens",
            ),
            (
                "cache_creation_input_tokens",
                "unisphere.usage.cache_creation_input_tokens",
            ),
        ] {
            if let Some(value) = usage.get(native) {
                if let Some(value) = value.as_i64().filter(|value| *value >= 0) {
                    attributes.insert(profile.into(), json!(value));
                    retained = true;
                } else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                }
            }
        }
        if retained {
            attributes.insert(
                "unisphere.usage.scope".into(),
                json!("native_record_snapshot"),
            );
        }
    }

    fn part(&mut self, value: Value) -> Option<Value> {
        let Value::Object(mut part) = value else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let kind = match part.remove("type") {
            Some(Value::String(kind)) => kind,
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                return None;
            }
        };
        match kind.as_str() {
            "text" | "thinking" => {
                let native = if kind == "text" { "text" } else { "thinking" };
                let Some(Value::String(content)) = part.remove(native) else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                self.include_content.then(|| {
                    let mut value = json!({
                        "type": if kind == "text" { "text" } else { "reasoning" },
                    });
                    value["content"] = Value::String(content);
                    value
                })
            }
            "tool_use" => {
                let id = self
                    .string(&part, "id")
                    .filter(|_| self.include_content)
                    .map(str::to_owned);
                let Some(Value::String(name)) = part.remove("name") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                let Some(arguments) = part.remove("input") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                self.include_content.then(|| {
                    let mut value = json!({"type": "tool_call"});
                    value["name"] = Value::String(name);
                    value["arguments"] = arguments;
                    if let Some(id) = id {
                        value["id"] = Value::String(id);
                    }
                    value
                })
            }
            "tool_result" => {
                let id = self
                    .string(&part, "tool_use_id")
                    .filter(|_| self.include_content)
                    .map(str::to_owned);
                let error = match part.remove("is_error") {
                    None => None,
                    Some(Value::Bool(value)) => Some(value),
                    Some(_) => {
                        self.diagnostic(MappingDiagnosticCode::InvalidField);
                        None
                    }
                };
                let Some(response) = part.remove("content") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                self.include_content.then(|| {
                    let mut value = json!({"type": "tool_call_response"});
                    value["response"] = response;
                    if let Some(id) = id {
                        value["id"] = Value::String(id);
                    }
                    if let Some(error) = error {
                        value["unisphere.is_error"] = Value::Bool(error);
                    }
                    value
                })
            }
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                self.include_content.then(|| {
                    json!({
                        "type": "unisphere.unknown", "native_type": kind,
                    })
                })
            }
        }
    }
}
