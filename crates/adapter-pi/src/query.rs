use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde_json::{Map, Value};
use unisphere_core::query::{
    AssociationBasis, AssociationExtent, AssociationObservation, AvailabilityCode,
    AvailabilityIssue, BranchEvidence, BranchLink, ContentAccess, ControlKind, FieldId,
    InspectedSource, LineageKind, MembershipPolicy, MessageRole, NativeLocator, NativeQueryInput,
    NativeSequence, Observation, ObservationFacet, ObservationPart, Outcome, PartitionId,
    QueryAdapter, QueryErrorLocation, QueryFailure, QueryFailureCode, QueryLimits, RecoveryAction,
    RequestMarker, SessionEvidenceKey, SourcePartition, SourceProblem, SourceRef, SourceViewKind,
    Timestamp, TimestampBasis, UsageCounters, UsageScope,
};

use crate::{PiAdapter, decode};

/// Reconstruction policy applied to supplied Pi records.
pub const POLICY_VERSION: &str = "pi-v3-query-v1";
const NAMESPACE: &str = "pi-v3";

impl QueryAdapter for PiAdapter {
    fn inspect(
        &self,
        input: NativeQueryInput<'_>,
        access: ContentAccess,
        limits: &QueryLimits,
    ) -> Result<InspectedSource, QueryFailure> {
        limits.validate()?;
        let NativeQueryInput::Records { source, records } = input else {
            return Err(unsupported_source());
        };
        source.validate()?;
        let input_bytes = records.iter().try_fold(0_usize, |total, record| {
            total
                .checked_add(record.bytes.len())
                .ok_or_else(|| QueryFailure::limit(unisphere_core::query::LimitKind::SourceBytes))
        })?;
        if input_bytes > limits.max_source_bytes {
            return Err(QueryFailure::limit(
                unisphere_core::query::LimitKind::SourceBytes,
            ));
        }
        if input_bytes > limits.max_total_input_bytes {
            return Err(QueryFailure::limit(
                unisphere_core::query::LimitKind::TotalInputBytes,
            ));
        }
        if (access.emit_content || !access.inspect_fields.is_empty())
            && input_bytes > limits.max_retained_bytes
        {
            return Err(QueryFailure::limit(
                unisphere_core::query::LimitKind::RetainedBytes,
            ));
        }

        let mut decoded = Vec::with_capacity(records.len());
        for record in records {
            let offset = jsonl_offset(&record.locator)?;
            let value = decode(&record.bytes).map_err(|()| {
                QueryFailure::invalid_data().at(QueryErrorLocation {
                    source: Some(source.id),
                    offset: Some(offset),
                    ..QueryErrorLocation::default()
                })
            })?;
            decoded.push((record, offset, value));
        }

        let header_ids: BTreeSet<String> = decoded
            .iter()
            .filter_map(|(_, _, value)| valid_header(value).map(str::to_owned))
            .collect();
        let default_session = (header_ids.len() == 1)
            .then(|| header_ids.iter().next().cloned())
            .flatten();
        let source_only = PartitionId::derive(source.id, b"pi-source-only");
        let partition_ids: BTreeMap<String, PartitionId> = header_ids
            .iter()
            .map(|id| (id.clone(), PartitionId::derive(source.id, id.as_bytes())))
            .collect();
        let mut output_source = source.clone();
        output_source.query_policy_version = POLICY_VERSION.into();
        output_source.available_fields.extend(available_fields());
        let mut current_session = default_session;
        let mut observations: Vec<Observation> = Vec::with_capacity(records.len());
        let mut issues = Vec::new();
        let mut seen_ids: BTreeMap<String, usize> = BTreeMap::new();
        if header_ids.len() > 1 {
            issues.push(issue(
                AvailabilityCode::Conflict,
                Some(FieldId::SessionId),
                source.id,
                0,
            ));
        }
        let mut partition_associations: BTreeMap<PartitionId, Vec<AssociationObservation>> =
            BTreeMap::new();

        for (record, offset, value) in &decoded {
            let object = value.as_object();
            let kind = object
                .and_then(|object| object.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if let Some(id) = valid_header(value) {
                current_session = Some(id.to_owned());
            }
            let session_id = current_session.as_deref();
            let partition = session_id
                .and_then(|id| partition_ids.get(id).copied())
                .unwrap_or(source_only);
            let session = session_id.map(session_key);
            let native_id = object
                .and_then(|object| object.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let parent = object
                .and_then(|object| object.get("parentId"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let mut diagnostics = Vec::new();
            let timestamp = native_timestamp(
                object.and_then(|object| object.get("timestamp")),
                source.id,
                *offset,
                &mut diagnostics,
            );
            let links = control_links(object, kind);
            let branch = BranchEvidence::Node {
                partition,
                native_id: native_id
                    .clone()
                    .unwrap_or_else(|| format!("offset:{offset}")),
                parent: parent.clone(),
                declared_branch: None,
                links: links.clone(),
            };
            let source_ref = SourceRef {
                source_id: source.id,
                revision: source.revision.clone(),
                locator: record.locator.clone(),
                subrecord: subrecord(kind, native_id.as_deref(), *offset),
            };
            let mut facets = Vec::new();
            if let Some(object) = object {
                inspect_record(
                    object,
                    kind,
                    native_id.as_deref(),
                    session_id,
                    partition,
                    timestamp.clone(),
                    &access,
                    &mut facets,
                    &mut diagnostics,
                    &links,
                    &mut partition_associations,
                    source.id,
                    *offset,
                );
            } else {
                diagnostics.push(issue(
                    AvailabilityCode::NotSupported,
                    Some(FieldId::Parts),
                    source.id,
                    *offset,
                ));
            }

            let index = observations.len();
            if let Some(id) = &native_id
                && let Some(previous) = seen_ids.insert(id.clone(), index)
            {
                let conflict = issue(
                    AvailabilityCode::Conflict,
                    Some(FieldId::NativeId),
                    source.id,
                    *offset,
                );
                diagnostics.push(conflict.clone());
                observations[previous].diagnostics.push(conflict.clone());
            }
            issues.extend(diagnostics.iter().cloned());
            observations.push(Observation {
                source_ref: source_ref.clone(),
                native_record_id: native_id.clone(),
                session: session.clone(),
                branch: branch.clone(),
                parent_ids: parent.into_iter().collect(),
                sequence: sequence(*offset, 0),
                timestamp,
                facets,
                diagnostics,
            });

            if let Some(message_clock) = object
                .and_then(|object| object.get("message"))
                .and_then(Value::as_object)
                .and_then(|message| message.get("timestamp"))
                .and_then(epoch_millis)
            {
                observations.push(Observation {
                    source_ref: SourceRef {
                        subrecord: format!("{}:message-clock", source_ref.subrecord),
                        ..source_ref
                    },
                    native_record_id: native_id,
                    session,
                    branch,
                    parent_ids: Vec::new(),
                    sequence: sequence(*offset, 1),
                    timestamp: Some(message_clock),
                    facets: Vec::new(),
                    diagnostics: Vec::new(),
                });
            }
        }

        let mut partitions = Vec::new();
        if header_ids.is_empty() {
            partitions.push(SourcePartition {
                id: source_only,
                native_session_id: None,
                participant_id: None,
                view: SourceViewKind::SourceOnly,
                membership: MembershipPolicy::Unavailable,
                associations: Vec::new(),
            });
            issues.push(issue(
                AvailabilityCode::NotCaptured,
                Some(FieldId::SessionId),
                source.id,
                0,
            ));
        } else {
            for id in &header_ids {
                let partition = partition_ids[id];
                partitions.push(SourcePartition {
                    id: partition,
                    native_session_id: Some(id.clone()),
                    participant_id: None,
                    view: SourceViewKind::Conversation,
                    membership: MembershipPolicy::ValidatedHeader,
                    associations: partition_associations
                        .remove(&partition)
                        .unwrap_or_default(),
                });
            }
            if observations
                .iter()
                .any(|observation| observation.branch.partition() == source_only)
            {
                partitions.push(SourcePartition {
                    id: source_only,
                    native_session_id: None,
                    participant_id: None,
                    view: SourceViewKind::SourceOnly,
                    membership: MembershipPolicy::Unavailable,
                    associations: Vec::new(),
                });
            }
        }
        output_source.associations = partitions
            .iter()
            .flat_map(|partition| partition.associations.iter().cloned())
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
fn inspect_record(
    object: &Map<String, Value>,
    kind: &str,
    native_record_id: Option<&str>,
    session_id: Option<&str>,
    partition: PartitionId,
    timestamp: Option<Timestamp>,
    access: &ContentAccess,
    facets: &mut Vec<ObservationFacet>,
    diagnostics: &mut Vec<AvailabilityIssue>,
    links: &[BranchLink],
    associations: &mut BTreeMap<PartitionId, Vec<AssociationObservation>>,
    source_id: unisphere_core::query::SourceId,
    offset: u64,
) {
    match kind {
        "session" if object.get("version").and_then(Value::as_u64) == Some(3) => {
            let Some(native_id) = object.get("id").and_then(Value::as_str) else {
                diagnostics.push(issue(
                    AvailabilityCode::NotCaptured,
                    Some(FieldId::NativeId),
                    source_id,
                    offset,
                ));
                return;
            };
            let association =
                object
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(|cwd| AssociationObservation {
                        basis: AssociationBasis::NativeCwd,
                        path: Some(PathBuf::from(cwd)),
                        partition,
                        applies_to: AssociationExtent::Partition,
                    });
            if let Some(association) = association.clone() {
                associations.entry(partition).or_default().push(association);
            }
            let lineage = object
                .get("parentSession")
                .and_then(Value::as_str)
                .map(|target| BranchLink {
                    kind: LineageKind::Parent,
                    target: target.to_owned(),
                })
                .into_iter()
                .collect();
            facets.push(ObservationFacet::SessionMetadata {
                native_id: native_id.to_owned(),
                name: None,
                models: Vec::new(),
                created_at: timestamp,
                associations: association.into_iter().collect(),
                lineage,
            });
        }
        "session" => diagnostics.push(issue(
            AvailabilityCode::NotSupported,
            Some(FieldId::Format),
            source_id,
            offset,
        )),
        "message" => inspect_message(
            object.get("message"),
            native_record_id,
            session_id,
            access,
            facets,
            diagnostics,
            (source_id, offset),
        ),
        "model_change" => {
            if let (Some(native_id), Some(model)) =
                (session_id, object.get("modelId").and_then(Value::as_str))
            {
                facets.push(ObservationFacet::SessionMetadata {
                    native_id: native_id.to_owned(),
                    name: None,
                    models: vec![model.to_owned()],
                    created_at: None,
                    associations: Vec::new(),
                    lineage: Vec::new(),
                });
            }
            facets.push(ObservationFacet::Control {
                kind: ControlKind::ContextChange,
                links: Vec::new(),
            });
        }
        "compaction" => {
            facets.push(ObservationFacet::Control {
                kind: ControlKind::Compaction,
                links: links.to_vec(),
            });
            if let Some(usage) = usage_facet(
                object.get("usage"),
                native_record_id.map(str::to_owned),
                UsageScope::CumulativeSnapshot,
            ) {
                facets.push(usage);
            }
        }
        "branch_summary" => {
            facets.push(ObservationFacet::Control {
                kind: ControlKind::Summary,
                links: links.to_vec(),
            });
            if let Some(usage) = usage_facet(
                object.get("usage"),
                native_record_id.map(str::to_owned),
                UsageScope::CumulativeSnapshot,
            ) {
                facets.push(usage);
            }
        }
        "custom_message" => facets.push(ObservationFacet::Message {
            native_id: native_record_id.map(str::to_owned),
            role: MessageRole::System,
            parts: content_parts(object.get("content"), access, FieldId::Parts),
            request_marker: RequestMarker::Injected,
            turn_id: None,
        }),
        "session_info" => {
            if let Some(native_id) = session_id {
                let value = object.get("name").and_then(Value::as_str);
                let name = value
                    .filter(|_| access.permits_payload(FieldId::Name))
                    .map(str::to_owned);
                if value.is_some() && name.is_none() {
                    diagnostics.push(issue(
                        AvailabilityCode::SensitiveOmitted,
                        Some(FieldId::Name),
                        source_id,
                        offset,
                    ));
                }
                facets.push(ObservationFacet::SessionMetadata {
                    native_id: native_id.to_owned(),
                    name,
                    models: Vec::new(),
                    created_at: None,
                    associations: Vec::new(),
                    lineage: Vec::new(),
                });
            }
        }
        "thinking_level_change" | "custom" | "label" => facets.push(ObservationFacet::Control {
            kind: ControlKind::ContextChange,
            links: Vec::new(),
        }),
        _ => diagnostics.push(issue(
            AvailabilityCode::NotSupported,
            Some(FieldId::Kind),
            source_id,
            offset,
        )),
    }
}

fn inspect_message(
    value: Option<&Value>,
    native_record_id: Option<&str>,
    session_id: Option<&str>,
    access: &ContentAccess,
    facets: &mut Vec<ObservationFacet>,
    diagnostics: &mut Vec<AvailabilityIssue>,
    (source_id, offset): (unisphere_core::query::SourceId, u64),
) {
    let Some(message) = value.and_then(Value::as_object) else {
        diagnostics.push(issue(
            AvailabilityCode::NotCaptured,
            Some(FieldId::Parts),
            source_id,
            offset,
        ));
        return;
    };
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if message
        .get("timestamp")
        .is_some_and(|value| epoch_millis(value).is_none())
    {
        diagnostics.push(issue(
            AvailabilityCode::InvalidClock,
            Some(FieldId::Timestamp),
            source_id,
            offset,
        ));
    }
    match role {
        "user" | "assistant" => {
            let parts = content_parts(message.get("content"), access, FieldId::Text);
            if role == "assistant" {
                facets.extend(tool_calls(message.get("content"), access));
                if let Some(session_id) = session_id {
                    let models = ["model", "responseModel"]
                        .into_iter()
                        .filter_map(|key| message.get(key).and_then(Value::as_str))
                        .map(str::to_owned)
                        .collect();
                    facets.push(ObservationFacet::SessionMetadata {
                        native_id: session_id.to_owned(),
                        name: None,
                        models,
                        created_at: None,
                        associations: Vec::new(),
                        lineage: Vec::new(),
                    });
                }
                if let Some(usage) = usage_facet(
                    message.get("usage"),
                    native_record_id.map(str::to_owned),
                    UsageScope::Turn,
                ) {
                    facets.push(usage);
                }
            }
            facets.push(ObservationFacet::Message {
                native_id: native_record_id.map(str::to_owned),
                role: if role == "user" {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                parts,
                request_marker: if role == "user" {
                    RequestMarker::Initiating
                } else {
                    RequestMarker::Unknown
                },
                turn_id: None,
            });
        }
        "toolResult" => {
            let call_id = message
                .get("toolCallId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let name = message
                .get("toolName")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let output = content_parts(message.get("content"), access, FieldId::Output);
            facets.push(ObservationFacet::Message {
                native_id: native_record_id.map(str::to_owned),
                role: MessageRole::Tool,
                parts: output.clone(),
                request_marker: RequestMarker::ToolResponse,
                turn_id: None,
            });
            if let Some(call_id) = call_id {
                facets.push(ObservationFacet::ToolResult {
                    native_call_id: call_id.clone(),
                    native_name: name,
                    output,
                    outcome: match message.get("isError").and_then(Value::as_bool) {
                        Some(true) => Outcome::Failed,
                        Some(false) => Outcome::Succeeded,
                        None => Outcome::Unknown,
                    },
                    exit_code: None,
                    reported_duration_ms: None,
                    turn_id: None,
                });
                if let Some(usage) =
                    usage_facet(message.get("usage"), Some(call_id), UsageScope::Invocation)
                {
                    facets.push(usage);
                }
            }
        }
        "custom" => facets.push(ObservationFacet::Message {
            native_id: native_record_id.map(str::to_owned),
            role: MessageRole::System,
            parts: content_parts(message.get("content"), access, FieldId::Parts),
            request_marker: RequestMarker::Injected,
            turn_id: None,
        }),
        "branchSummary" | "compactionSummary" => {
            let links = message
                .get("fromId")
                .and_then(Value::as_str)
                .map(|target| BranchLink {
                    kind: if role == "branchSummary" {
                        LineageKind::Fork
                    } else {
                        LineageKind::CompactionFrom
                    },
                    target: target.to_owned(),
                })
                .into_iter()
                .collect();
            facets.push(ObservationFacet::Message {
                native_id: native_record_id.map(str::to_owned),
                role: MessageRole::System,
                parts: content_parts(message.get("summary"), access, FieldId::Text),
                request_marker: RequestMarker::Summary,
                turn_id: None,
            });
            facets.push(ObservationFacet::Control {
                kind: if role == "branchSummary" {
                    ControlKind::Summary
                } else {
                    ControlKind::Compaction
                },
                links,
            });
        }
        "bashExecution" => facets.push(ObservationFacet::Control {
            kind: ControlKind::Other,
            links: Vec::new(),
        }),
        _ => diagnostics.push(issue(
            AvailabilityCode::NotSupported,
            Some(FieldId::Role),
            source_id,
            offset,
        )),
    }
}

fn tool_calls(value: Option<&Value>, access: &ContentAccess) -> Vec<ObservationFacet> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("toolCall"))
        .filter_map(|part| {
            let id = part.get("id")?.as_str()?.to_owned();
            let name = part.get("name")?.as_str()?.to_owned();
            let input = if access.permits_payload(FieldId::Input) {
                part.get("arguments")
                    .cloned()
                    .map(ObservationPart::Structured)
                    .into_iter()
                    .collect()
            } else {
                vec![ObservationPart::Unavailable(
                    AvailabilityCode::SensitiveOmitted,
                )]
            };
            Some(ObservationFacet::ToolCall {
                native_call_id: id,
                family: tool_family(&name).map(str::to_owned),
                native_name: name,
                input,
                turn_id: None,
            })
        })
        .collect()
}

fn content_parts(
    value: Option<&Value>,
    access: &ContentAccess,
    field: FieldId,
) -> Vec<ObservationPart> {
    let permitted = access.permits_payload(field) || access.permits_payload(FieldId::Parts);
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(text)) => {
            if permitted {
                vec![ObservationPart::Text(text.clone())]
            } else {
                vec![ObservationPart::Unavailable(
                    AvailabilityCode::SensitiveOmitted,
                )]
            }
        }
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|part| {
                let object = part.as_object()?;
                match object.get("type").and_then(Value::as_str) {
                    Some("text") => Some(if permitted {
                        ObservationPart::Text(object.get("text")?.as_str()?.to_owned())
                    } else {
                        ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)
                    }),
                    Some("thinking") => Some(if permitted {
                        ObservationPart::Reasoning(object.get("thinking")?.as_str()?.to_owned())
                    } else {
                        ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)
                    }),
                    Some("image") => Some(if permitted {
                        ObservationPart::Structured(part.clone())
                    } else {
                        ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)
                    }),
                    Some("toolCall") => None,
                    _ => Some(ObservationPart::Unavailable(AvailabilityCode::NotSupported)),
                }
            })
            .collect(),
        Some(_) => vec![ObservationPart::Unavailable(AvailabilityCode::NotSupported)],
    }
}

fn usage_facet(
    value: Option<&Value>,
    owner: Option<String>,
    scope: UsageScope,
) -> Option<ObservationFacet> {
    let usage = value?.as_object()?;
    let counters = UsageCounters {
        input_tokens: counter(usage.get("input")),
        output_tokens: counter(usage.get("output")),
        cache_read_tokens: counter(usage.get("cacheRead")),
        cache_write_tokens: counter(usage.get("cacheWrite")),
    };
    (counters.input_tokens.is_some()
        || counters.output_tokens.is_some()
        || counters.cache_read_tokens.is_some()
        || counters.cache_write_tokens.is_some())
    .then_some(ObservationFacet::Usage {
        owner,
        scope,
        counters,
    })
}

fn valid_header(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    (object.get("type").and_then(Value::as_str) == Some("session")
        && object.get("version").and_then(Value::as_u64) == Some(3))
    .then(|| object.get("id").and_then(Value::as_str))
    .flatten()
}

fn session_key(id: &str) -> SessionEvidenceKey {
    SessionEvidenceKey {
        namespace: NAMESPACE.into(),
        native_id: id.into(),
        participant_id: None,
        parent_native_id: None,
        fork_native_id: None,
        membership_basis: MembershipPolicy::ValidatedHeader,
    }
}

fn control_links(object: Option<&Map<String, Value>>, kind: &str) -> Vec<BranchLink> {
    let Some(object) = object else {
        return Vec::new();
    };
    let mut links = Vec::new();
    if let Some(target) = object.get("firstKeptEntryId").and_then(Value::as_str) {
        links.push(BranchLink {
            kind: LineageKind::FirstKept,
            target: target.into(),
        });
    }
    if let Some(target) = object.get("fromId").and_then(Value::as_str) {
        links.push(BranchLink {
            kind: if kind == "compaction" {
                LineageKind::CompactionFrom
            } else {
                LineageKind::Fork
            },
            target: target.into(),
        });
    }
    links
}

fn native_timestamp(
    value: Option<&Value>,
    source: unisphere_core::query::SourceId,
    offset: u64,
    diagnostics: &mut Vec<AvailabilityIssue>,
) -> Option<Timestamp> {
    let value = value?;
    let timestamp = value
        .as_str()
        .and_then(|value| Timestamp::parse(value, TimestampBasis::Native).ok());
    if timestamp.is_none() {
        diagnostics.push(issue(
            AvailabilityCode::InvalidClock,
            Some(FieldId::Timestamp),
            source,
            offset,
        ));
    }
    timestamp
}

fn epoch_millis(value: &Value) -> Option<Timestamp> {
    let millis = value.as_u64()?;
    let nanos = i128::from(millis).checked_mul(1_000_000)?;
    Timestamp::new(nanos, TimestampBasis::Native).ok()
}

fn jsonl_offset(locator: &NativeLocator) -> Result<u64, QueryFailure> {
    match locator {
        NativeLocator::Jsonl { offset } => Ok(*offset),
        _ => Err(unsupported_source()),
    }
}

fn sequence(offset: u64, subrecord: u8) -> NativeSequence {
    let mut key = offset.to_be_bytes().to_vec();
    key.push(subrecord);
    NativeSequence { version: 1, key }
}

fn subrecord(kind: &str, id: Option<&str>, offset: u64) -> String {
    match id {
        Some(id) => format!("{kind}:{id}"),
        None => format!("{kind}:offset:{offset}"),
    }
}

fn counter(value: Option<&Value>) -> Option<u64> {
    value?.as_i64().and_then(|value| u64::try_from(value).ok())
}

pub(crate) fn tool_family(name: &str) -> Option<&'static str> {
    match name {
        "bash" | "shell" | "exec" | "python" => Some("shell"),
        "read" => Some("file-read"),
        "write" | "edit" => Some("file-write"),
        "grep" | "search" => Some("search"),
        _ => None,
    }
}

fn issue(
    code: AvailabilityCode,
    field: Option<FieldId>,
    source: unisphere_core::query::SourceId,
    offset: u64,
) -> AvailabilityIssue {
    AvailabilityIssue {
        code,
        field,
        source: Some(source),
        entity: None,
        offset: Some(offset),
    }
}

fn unsupported_source() -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::UnsupportedSource,
        RecoveryAction::FixSource {
            reason: SourceProblem::UnsupportedDialect,
            source: None,
        },
    )
}

fn available_fields() -> BTreeSet<FieldId> {
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
        FieldId::ProjectPath,
        FieldId::SourcePath,
        FieldId::Name,
        FieldId::Models,
        FieldId::Timestamp,
        FieldId::Role,
        FieldId::Parts,
        FieldId::Input,
        FieldId::Output,
        FieldId::ToolName,
        FieldId::ToolFamily,
        FieldId::Status,
        FieldId::InputTokens,
        FieldId::OutputTokens,
        FieldId::CacheReadTokens,
        FieldId::CacheWriteTokens,
        FieldId::ParentIds,
        FieldId::BranchIds,
        FieldId::Kind,
    ])
}
