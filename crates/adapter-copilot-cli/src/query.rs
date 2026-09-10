use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use serde_json::{Map, Value};
use unisphere_core::query::{
    AdapterId, AssociationBasis, AssociationExtent, AssociationObservation, AvailabilityCode,
    AvailabilityIssue, BranchEvidence, BranchLink, ContentAccess, ControlKind, FieldId,
    InspectedSource, LineageKind, MembershipPolicy, MessageRole, NativeLocator, NativeQueryInput,
    NativeSequence, Observation, ObservationFacet, ObservationPart, Outcome, PartitionId,
    QueryFailure, QueryFailureCode, QueryLimits, RecoveryAction, RequestMarker, SessionEvidenceKey,
    SourceEvidence, SourcePartition, SourceRef, SourceViewKind, Timestamp, TimestampBasis,
    UsageCounters, UsageScope,
};
use unisphere_core::{NativeSnapshot, SnapshotFormat};

use super::{
    CURRENT_QUERY_POLICY_VERSION, DESCRIPTOR, LEGACY_QUERY_POLICY_VERSION, SNAPSHOT_DESCRIPTOR,
    decode_json,
};

const CURRENT_NAMESPACE: &str = "copilot-cli/events-v1";
const LEGACY_NAMESPACE: &str = "copilot-cli/legacy-document-v1";

pub(super) fn inspect_current(
    input: NativeQueryInput<'_>,
    access: ContentAccess,
    limits: &QueryLimits,
) -> Result<InspectedSource, QueryFailure> {
    let NativeQueryInput::Records { source, records } = input else {
        return Err(unsupported(DESCRIPTOR.id));
    };
    let mut source = prepare_source(
        source,
        DESCRIPTOR.id,
        CURRENT_QUERY_POLICY_VERSION,
        current_fields(),
        limits,
    )?;
    let input_bytes = records.iter().try_fold(0usize, |total, record| {
        total
            .checked_add(record.bytes.len())
            .ok_or_else(|| QueryFailure::limit(unisphere_core::query::LimitKind::TotalInputBytes))
    })?;
    check_input_bytes(
        input_bytes,
        limits,
        access.emit_content || !access.inspect_fields.is_empty(),
    )?;

    let mut partitions = Vec::new();
    let mut partition_indices = BTreeMap::new();
    let mut observations = Vec::with_capacity(records.len());
    let mut issues = Vec::new();
    let mut active_session = None::<String>;
    let mut active_parent_session = None::<String>;
    let mut active_branches = BTreeMap::<PartitionId, String>::new();
    let mut context_points = BTreeMap::<PartitionId, Vec<ContextPoint>>::new();

    for record in records {
        if observations.len() >= limits.max_observations_and_rows {
            return Err(QueryFailure::limit(
                unisphere_core::query::LimitKind::ObservationsAndRows,
            ));
        }
        let NativeLocator::Jsonl { offset } = &record.locator else {
            return Err(unsupported(DESCRIPTOR.id));
        };
        let value = decode_json(&record.bytes).map_err(|()| {
            QueryFailure::invalid_data().at(unisphere_core::query::QueryErrorLocation {
                source: Some(source.id),
                offset: Some(*offset),
                ..Default::default()
            })
        })?;
        let event = value.as_object().ok_or_else(|| {
            QueryFailure::invalid_data().at(unisphere_core::query::QueryErrorLocation {
                source: Some(source.id),
                offset: Some(*offset),
                ..Default::default()
            })
        })?;
        let data = event.get("data").and_then(Value::as_object);
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let event_id = nonempty_string(event, "id");
        let participant = nonempty_string(event, "agentId");

        if matches!(kind, "session.start" | "session.resume") {
            active_session = data.and_then(|data| nonempty_string(data, "sessionId"));
            active_parent_session =
                data.and_then(|data| nonempty_string(data, "detachedFromSpawningParentSessionId"));
        }
        let membership = if active_session.is_some() {
            MembershipPolicy::ValidatedHeader
        } else {
            MembershipPolicy::Unavailable
        };
        let view = if active_session.is_some() {
            SourceViewKind::Conversation
        } else {
            SourceViewKind::SourceOnly
        };
        let partition = ensure_partition(
            &mut partitions,
            &mut partition_indices,
            source.id,
            active_session.as_deref(),
            participant.as_deref(),
            view,
            membership,
        );
        let sequence = NativeSequence {
            version: 1,
            key: offset.to_be_bytes().to_vec(),
        };

        let mut observation_issues = Vec::new();
        if kind == "unknown" {
            observation_issues.push(issue(
                source.id,
                Some(FieldId::Kind),
                Some(*offset),
                AvailabilityCode::NotCaptured,
            ));
        }
        let timestamp = event_timestamp(
            event.get("timestamp"),
            source.id,
            Some(*offset),
            &mut observation_issues,
        );

        let context = match kind {
            "session.start" | "session.resume" => data
                .and_then(|data| data.get("context"))
                .and_then(Value::as_object),
            "session.context_changed" => data,
            _ => None,
        };
        if let Some(context) = context {
            let point = context_point(partition, sequence.clone(), context);
            if let Some(branch) = point.branch.as_ref() {
                active_branches.insert(partition, branch.clone());
            }
            context_points.entry(partition).or_default().push(point);
        }

        let mut facets = Vec::new();
        if let Some(data) = data {
            current_facets(
                kind,
                data,
                event_id.as_deref(),
                active_session.as_deref(),
                participant.as_deref(),
                &access,
                source.id,
                *offset,
                &mut facets,
                &mut observation_issues,
            );
        }
        let parent = nonempty_string(event, "parentId");
        let mut parent_ids = parent.iter().cloned().collect::<Vec<_>>();
        if let Some(parent_call) = data.and_then(|data| nonempty_string(data, "parentToolCallId"))
            && !parent_ids.contains(&parent_call)
        {
            parent_ids.push(parent_call);
        }
        let branch = if let Some(native_id) = event_id.clone() {
            BranchEvidence::Node {
                partition,
                native_id,
                parent,
                declared_branch: active_branches.get(&partition).cloned(),
                links: event_links(kind, data),
            }
        } else {
            BranchEvidence::Unavailable {
                partition,
                reason: AvailabilityCode::NotCaptured,
            }
        };
        let session = active_session.as_ref().map(|native_id| SessionEvidenceKey {
            namespace: CURRENT_NAMESPACE.into(),
            native_id: native_id.clone(),
            participant_id: participant.clone(),
            parent_native_id: active_parent_session.clone(),
            fork_native_id: None,
            membership_basis: MembershipPolicy::ValidatedHeader,
        });
        issues.extend(observation_issues.iter().cloned());
        observations.push(Observation {
            source_ref: SourceRef {
                source_id: source.id,
                revision: source.revision.clone(),
                locator: record.locator.clone(),
                subrecord: kind.into(),
            },
            native_record_id: event_id,
            session,
            branch,
            parent_ids,
            sequence,
            timestamp,
            facets,
            diagnostics: observation_issues,
        });
    }

    apply_context_intervals(&mut partitions, &context_points);
    source.associations.extend(
        partitions
            .iter()
            .flat_map(|partition| partition.associations.iter().cloned()),
    );
    let inspected = InspectedSource {
        source,
        partitions,
        observations,
        issues,
    };
    inspected.validate(limits)?;
    Ok(inspected)
}

pub(super) fn inspect_legacy(
    input: NativeQueryInput<'_>,
    access: ContentAccess,
    limits: &QueryLimits,
) -> Result<InspectedSource, QueryFailure> {
    let NativeQueryInput::Snapshot { source, snapshot } = input else {
        return Err(unsupported(SNAPSHOT_DESCRIPTOR.id));
    };
    let source = prepare_source(
        source,
        SNAPSHOT_DESCRIPTOR.id,
        LEGACY_QUERY_POLICY_VERSION,
        legacy_fields(),
        limits,
    )?;
    validate_legacy_snapshot(
        snapshot,
        &source,
        limits,
        access.emit_content || !access.inspect_fields.is_empty(),
    )?;
    let document =
        decode_json(&snapshot.records[0].bytes).map_err(|()| QueryFailure::invalid_data())?;
    let document = document
        .as_object()
        .ok_or_else(QueryFailure::invalid_data)?;
    let native_session_id = document
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    if let Some(selected) = snapshot.source.session_id.as_deref()
        && native_session_id.as_deref() != Some(selected)
    {
        return Err(QueryFailure::invalid_data());
    }
    let membership = if native_session_id.is_some() {
        MembershipPolicy::ValidatedHeader
    } else {
        MembershipPolicy::Unavailable
    };
    let header_partition = legacy_partition(
        source.id,
        b"legacy-header",
        SourceViewKind::SourceOnly,
        native_session_id.as_deref(),
        membership,
    );
    let chat_partition = legacy_partition(
        source.id,
        b"legacy-chat",
        SourceViewKind::LegacyChat,
        native_session_id.as_deref(),
        membership,
    );
    let timeline_partition = legacy_partition(
        source.id,
        b"legacy-timeline",
        SourceViewKind::LegacyTimeline,
        native_session_id.as_deref(),
        membership,
    );
    let mut partitions = vec![header_partition.clone()];
    let mut observations = Vec::new();
    let mut issues = Vec::new();
    let session = native_session_id
        .as_ref()
        .map(|native_id| SessionEvidenceKey {
            namespace: LEGACY_NAMESPACE.into(),
            native_id: native_id.clone(),
            participant_id: None,
            parent_native_id: None,
            fork_native_id: None,
            membership_basis: MembershipPolicy::ValidatedHeader,
        });

    let mut header_issues = Vec::new();
    let created_at = optional_timestamp(
        document.get("startTime"),
        source.id,
        None,
        &mut header_issues,
    );
    let mut header_facets = Vec::new();
    if let Some(native_id) = native_session_id.as_ref() {
        header_facets.push(ObservationFacet::SessionMetadata {
            native_id: native_id.clone(),
            name: None,
            models: Vec::new(),
            created_at: created_at.clone(),
            associations: Vec::new(),
            lineage: Vec::new(),
        });
    }
    issues.extend(header_issues.iter().cloned());
    observations.push(Observation {
        source_ref: snapshot_ref(&source, "document", "session"),
        native_record_id: None,
        session: session.clone(),
        branch: BranchEvidence::Linear {
            partition: header_partition.id,
        },
        parent_ids: Vec::new(),
        sequence: snapshot_sequence("document"),
        timestamp: created_at,
        facets: header_facets,
        diagnostics: header_issues,
    });

    if let Some(chat) = document.get("chatMessages") {
        partitions.push(chat_partition.clone());
        inspect_legacy_array(
            chat,
            "chatMessages",
            &source,
            &session,
            &chat_partition,
            &access,
            limits,
            &mut observations,
            &mut issues,
        )?;
    }
    if let Some(timeline) = document.get("timeline") {
        partitions.push(timeline_partition.clone());
        inspect_legacy_array(
            timeline,
            "timeline",
            &source,
            &session,
            &timeline_partition,
            &access,
            limits,
            &mut observations,
            &mut issues,
        )?;
    }

    let inspected = InspectedSource {
        source,
        partitions,
        observations,
        issues,
    };
    inspected.validate(limits)?;
    Ok(inspected)
}

#[allow(clippy::too_many_arguments)]
fn current_facets(
    kind: &str,
    data: &Map<String, Value>,
    event_id: Option<&str>,
    session_id: Option<&str>,
    participant: Option<&str>,
    access: &ContentAccess,
    source_id: unisphere_core::query::SourceId,
    offset: u64,
    facets: &mut Vec<ObservationFacet>,
    issues: &mut Vec<AvailabilityIssue>,
) {
    let turn_id = nonempty_string(data, "turnId");
    match kind {
        "session.start" | "session.resume" => {
            if let Some(native_id) = nonempty_string(data, "sessionId") {
                let raw_model = nonempty_string(data, "selectedModel");
                let model = raw_model.clone().filter(|_| permits_model(access));
                if raw_model.is_some() && model.is_none() {
                    issues.push(issue(
                        source_id,
                        Some(FieldId::Models),
                        Some(offset),
                        AvailabilityCode::SensitiveOmitted,
                    ));
                }
                let created_at = (kind == "session.start")
                    .then(|| {
                        optional_timestamp(data.get("startTime"), source_id, Some(offset), issues)
                    })
                    .flatten();
                let lineage = nonempty_string(data, "detachedFromSpawningParentSessionId")
                    .map(|target| BranchLink {
                        kind: LineageKind::Parent,
                        target,
                    })
                    .into_iter()
                    .collect();
                facets.push(ObservationFacet::SessionMetadata {
                    native_id,
                    name: None,
                    models: model.into_iter().collect(),
                    created_at,
                    associations: Vec::new(),
                    lineage,
                });
            }
        }
        "session.model_change" => {
            if let Some(native_id) = session_id {
                let raw_models: Vec<_> = ["previousModel", "newModel"]
                    .into_iter()
                    .filter_map(|field| nonempty_string(data, field))
                    .collect();
                let models = raw_models
                    .iter()
                    .filter(|_| permits_model(access))
                    .cloned()
                    .collect();
                if !raw_models.is_empty() && !permits_model(access) {
                    issues.push(issue(
                        source_id,
                        Some(FieldId::Models),
                        Some(offset),
                        AvailabilityCode::SensitiveOmitted,
                    ));
                }
                facets.push(ObservationFacet::SessionMetadata {
                    native_id: native_id.into(),
                    name: None,
                    models,
                    created_at: None,
                    associations: Vec::new(),
                    lineage: Vec::new(),
                });
            }
        }
        "session.title_changed" => {
            if let Some(native_id) = session_id {
                let name = nonempty_string(data, "title")
                    .filter(|_| access.permits_payload(FieldId::Name));
                if data.contains_key("title") && name.is_none() {
                    issues.push(issue(
                        source_id,
                        Some(FieldId::Name),
                        Some(offset),
                        AvailabilityCode::SensitiveOmitted,
                    ));
                }
                facets.push(ObservationFacet::SessionMetadata {
                    native_id: native_id.into(),
                    name,
                    models: Vec::new(),
                    created_at: None,
                    associations: Vec::new(),
                    lineage: Vec::new(),
                });
            }
        }
        "user.message"
        | "assistant.message"
        | "assistant.reasoning"
        | "assistant.message_delta"
        | "assistant.reasoning_delta"
        | "system.message" => {
            let role = current_role(kind, data);
            let parts = message_parts(kind, data, access, source_id, offset, issues);
            facets.push(ObservationFacet::Message {
                native_id: nonempty_string(data, "messageId")
                    .or_else(|| event_id.map(str::to_owned)),
                role,
                parts,
                request_marker: if kind == "user.message" {
                    RequestMarker::Initiating
                } else if kind == "system.message" {
                    RequestMarker::Injected
                } else {
                    RequestMarker::Unknown
                },
                turn_id: turn_id.clone(),
            });
            if kind == "assistant.message"
                && let Some(requests) = data.get("toolRequests").and_then(Value::as_array)
            {
                for request in requests {
                    if let Some(request) = request.as_object() {
                        push_tool_call(
                            request,
                            "name",
                            access,
                            (source_id, offset),
                            turn_id.clone(),
                            facets,
                            issues,
                        );
                    }
                }
            }
            if kind == "assistant.message" && data.get("outputTokens").is_some() {
                facets.push(ObservationFacet::Usage {
                    owner: nonempty_string(data, "messageId").or_else(|| turn_id.clone()),
                    scope: UsageScope::Invocation,
                    counters: UsageCounters {
                        input_tokens: None,
                        output_tokens: token(data, "outputTokens", source_id, offset, issues),
                        cache_read_tokens: None,
                        cache_write_tokens: None,
                    },
                });
            }
        }
        "tool.execution_start" => {
            push_tool_call(
                data,
                "toolName",
                access,
                (source_id, offset),
                turn_id,
                facets,
                issues,
            );
        }
        "tool.execution_complete" => {
            if let Some(native_call_id) = nonempty_string(data, "toolCallId") {
                facets.push(ObservationFacet::ToolResult {
                    native_call_id,
                    native_name: nonempty_string(data, "toolName"),
                    output: tool_output(data, access, source_id, offset, issues),
                    outcome: tool_outcome(data),
                    exit_code: None,
                    reported_duration_ms: None,
                    turn_id,
                });
            } else {
                issues.push(issue(
                    source_id,
                    Some(FieldId::CallId),
                    Some(offset),
                    AvailabilityCode::NotCaptured,
                ));
            }
        }
        "tool.execution_partial_result"
        | "tool.execution_progress"
        | "assistant.tool_call_delta" => {
            if let Some(native_call_id) = nonempty_string(data, "toolCallId") {
                let field = if kind == "assistant.tool_call_delta" {
                    FieldId::Input
                } else {
                    FieldId::Output
                };
                facets.push(ObservationFacet::ToolProgress {
                    native_call_id,
                    parts: progress_parts(data, access, field, source_id, offset, issues),
                });
            } else {
                issues.push(issue(
                    source_id,
                    Some(FieldId::CallId),
                    Some(offset),
                    AvailabilityCode::NotCaptured,
                ));
            }
        }
        "assistant.usage" => {
            let counters = usage_counters(data, source_id, offset, issues);
            if has_usage(&counters) {
                facets.push(ObservationFacet::Usage {
                    owner: nonempty_string(data, "providerCallId")
                        .or_else(|| nonempty_string(data, "apiCallId")),
                    scope: UsageScope::Invocation,
                    counters,
                });
            }
        }
        "session.usage_checkpoint" | "session.shutdown" => {
            let counters = usage_counters(data, source_id, offset, issues);
            if has_usage(&counters) {
                facets.push(ObservationFacet::Usage {
                    owner: session_id.map(str::to_owned),
                    scope: UsageScope::CumulativeSnapshot,
                    counters,
                });
            }
        }
        "session.compaction_start" | "session.compaction_complete" => {
            facets.push(ObservationFacet::Control {
                kind: ControlKind::Compaction,
                links: Vec::new(),
            });
            if let Some(usage) = data.get("compactionTokensUsed").and_then(Value::as_object) {
                let counters = usage_counters(usage, source_id, offset, issues);
                if has_usage(&counters) {
                    facets.push(ObservationFacet::Usage {
                        owner: event_id.map(str::to_owned),
                        scope: UsageScope::Invocation,
                        counters,
                    });
                }
            }
        }
        "session.context_changed" => facets.push(ObservationFacet::Control {
            kind: ControlKind::ContextChange,
            links: nonempty_string(data, "branch")
                .map(|target| BranchLink {
                    kind: LineageKind::NativeLink,
                    target,
                })
                .into_iter()
                .collect(),
        }),
        "assistant.turn_start" | "assistant.turn_end" | "assistant.message_start" => {
            let target = turn_id.or_else(|| nonempty_string(data, "messageId"));
            facets.push(ObservationFacet::Control {
                kind: ControlKind::Other,
                links: target
                    .map(|target| BranchLink {
                        kind: LineageKind::NativeLink,
                        target,
                    })
                    .into_iter()
                    .collect(),
            });
        }
        kind if kind.starts_with("subagent.") => {
            let mut links = Vec::new();
            if let Some(target) = participant
                .map(str::to_owned)
                .or_else(|| nonempty_string(data, "agentId"))
            {
                links.push(BranchLink {
                    kind: LineageKind::Subagent,
                    target,
                });
            }
            if let Some(target) = nonempty_string(data, "parentId") {
                links.push(BranchLink {
                    kind: LineageKind::Parent,
                    target,
                });
            }
            facets.push(ObservationFacet::Control {
                kind: ControlKind::Branch,
                links,
            });
        }
        "abort" => facets.push(ObservationFacet::Control {
            kind: ControlKind::Other,
            links: Vec::new(),
        }),
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn inspect_legacy_array(
    value: &Value,
    view: &str,
    source: &SourceEvidence,
    session: &Option<SessionEvidenceKey>,
    partition: &SourcePartition,
    access: &ContentAccess,
    limits: &QueryLimits,
    observations: &mut Vec<Observation>,
    issues: &mut Vec<AvailabilityIssue>,
) -> Result<(), QueryFailure> {
    let Some(items) = value.as_array() else {
        issues.push(issue(
            source.id,
            Some(FieldId::Parts),
            None,
            AvailabilityCode::ProjectionMissing,
        ));
        return Ok(());
    };
    for (index, item) in items.iter().enumerate() {
        if observations.len() >= limits.max_observations_and_rows {
            return Err(QueryFailure::limit(
                unisphere_core::query::LimitKind::ObservationsAndRows,
            ));
        }
        let key = format!("document#/{view}/{index}");
        let mut observation_issues = Vec::new();
        let mut facets = Vec::new();
        let data = item.as_object();
        let native_id = data.and_then(|data| nonempty_string(data, "id"));
        let timestamp = if view == "timeline" {
            event_timestamp(
                data.and_then(|data| data.get("timestamp")),
                source.id,
                None,
                &mut observation_issues,
            )
        } else {
            observation_issues.push(issue(
                source.id,
                Some(FieldId::Timestamp),
                None,
                AvailabilityCode::NotCaptured,
            ));
            None
        };
        if let Some(data) = data {
            if view == "chatMessages" {
                legacy_chat_facets(
                    data,
                    access,
                    source.id,
                    &mut facets,
                    &mut observation_issues,
                );
            } else {
                legacy_timeline_facets(
                    data,
                    access,
                    source.id,
                    &mut facets,
                    &mut observation_issues,
                );
            }
        } else {
            observation_issues.push(issue(
                source.id,
                Some(FieldId::Parts),
                None,
                AvailabilityCode::ProjectionMissing,
            ));
        }
        issues.extend(observation_issues.iter().cloned());
        observations.push(Observation {
            source_ref: snapshot_ref(source, &key, view),
            native_record_id: native_id,
            session: session.clone(),
            branch: BranchEvidence::Linear {
                partition: partition.id,
            },
            parent_ids: Vec::new(),
            sequence: snapshot_sequence(&key),
            timestamp,
            facets,
            diagnostics: observation_issues,
        });
    }
    Ok(())
}

fn legacy_chat_facets(
    data: &Map<String, Value>,
    access: &ContentAccess,
    source_id: unisphere_core::query::SourceId,
    facets: &mut Vec<ObservationFacet>,
    issues: &mut Vec<AvailabilityIssue>,
) {
    let role = match data.get("role").and_then(Value::as_str) {
        Some("user") => MessageRole::User,
        Some("assistant") => MessageRole::Assistant,
        Some("tool") => MessageRole::Tool,
        _ => MessageRole::Unknown,
    };
    let call_id = nonempty_string(data, "tool_call_id");
    let parts = legacy_text_parts(
        data,
        "content",
        access,
        if role == MessageRole::Tool {
            FieldId::Output
        } else {
            FieldId::Text
        },
        source_id,
        issues,
    );
    facets.push(ObservationFacet::Message {
        native_id: None,
        role,
        parts: parts.clone(),
        request_marker: if role == MessageRole::User {
            RequestMarker::Initiating
        } else if role == MessageRole::Tool {
            RequestMarker::ToolResponse
        } else {
            RequestMarker::Unknown
        },
        turn_id: None,
    });
    if role == MessageRole::Tool {
        if let Some(native_call_id) = call_id {
            facets.push(ObservationFacet::ToolResult {
                native_call_id,
                native_name: None,
                output: parts,
                outcome: Outcome::Unknown,
                exit_code: None,
                reported_duration_ms: None,
                turn_id: None,
            });
        }
    } else if let Some(calls) = data.get("tool_calls").and_then(Value::as_array) {
        for call in calls.iter().filter_map(Value::as_object) {
            if call.get("type").and_then(Value::as_str) != Some("function") {
                issues.push(issue(
                    source_id,
                    Some(FieldId::Input),
                    None,
                    AvailabilityCode::NotSupported,
                ));
                continue;
            }
            let Some(function) = call.get("function").and_then(Value::as_object) else {
                continue;
            };
            let (Some(native_call_id), Some(native_name)) = (
                nonempty_string(call, "id"),
                nonempty_string(function, "name"),
            ) else {
                continue;
            };
            let input = gated_value(
                function.get("arguments"),
                access,
                FieldId::Input,
                source_id,
                None,
                issues,
            );
            facets.push(ObservationFacet::ToolCall {
                native_call_id,
                family: tool_family(&native_name).map(str::to_owned),
                native_name,
                input,
                turn_id: None,
            });
        }
    }
}

fn legacy_timeline_facets(
    data: &Map<String, Value>,
    access: &ContentAccess,
    source_id: unisphere_core::query::SourceId,
    facets: &mut Vec<ObservationFacet>,
    issues: &mut Vec<AvailabilityIssue>,
) {
    let kind = data
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match kind {
        "user" | "copilot" => facets.push(ObservationFacet::Message {
            native_id: nonempty_string(data, "id"),
            role: if kind == "user" {
                MessageRole::User
            } else {
                MessageRole::Assistant
            },
            parts: legacy_timeline_parts(data, access, source_id, issues),
            request_marker: if kind == "user" {
                RequestMarker::Initiating
            } else {
                RequestMarker::Unknown
            },
            turn_id: None,
        }),
        "tool_call_requested" => {
            let Some(native_call_id) = nonempty_string(data, "callId") else {
                return;
            };
            let Some(native_name) = nonempty_string(data, "name") else {
                return;
            };
            facets.push(ObservationFacet::ToolCall {
                family: tool_family(&native_name).map(str::to_owned),
                native_call_id,
                native_name,
                input: gated_value(
                    data.get("arguments"),
                    access,
                    FieldId::Input,
                    source_id,
                    None,
                    issues,
                ),
                turn_id: None,
            });
        }
        "tool_call_completed" => {
            let Some(native_call_id) = nonempty_string(data, "callId") else {
                return;
            };
            facets.push(ObservationFacet::ToolResult {
                native_call_id,
                native_name: nonempty_string(data, "name"),
                output: gated_value(
                    data.get("result"),
                    access,
                    FieldId::Output,
                    source_id,
                    None,
                    issues,
                ),
                outcome: Outcome::Unknown,
                exit_code: None,
                reported_duration_ms: None,
                turn_id: None,
            });
        }
        "info" => facets.push(ObservationFacet::Control {
            kind: ControlKind::Other,
            links: Vec::new(),
        }),
        _ => issues.push(issue(
            source_id,
            Some(FieldId::Kind),
            None,
            AvailabilityCode::NotSupported,
        )),
    }
}

fn prepare_source(
    source: &SourceEvidence,
    expected_adapter: &str,
    policy: &str,
    fields: BTreeSet<FieldId>,
    limits: &QueryLimits,
) -> Result<SourceEvidence, QueryFailure> {
    limits.validate()?;
    source.validate()?;
    if source.adapter.as_str() != expected_adapter {
        return Err(unsupported(expected_adapter));
    }
    let mut source = source.clone();
    source.query_policy_version = policy.into();
    source.available_fields.extend(fields);
    Ok(source)
}

fn validate_legacy_snapshot(
    snapshot: &NativeSnapshot,
    source: &SourceEvidence,
    limits: &QueryLimits,
    retains_payload: bool,
) -> Result<(), QueryFailure> {
    if snapshot.source.format != SnapshotFormat::JsonDocument
        || snapshot.revision != source.revision
        || snapshot.records.len() != 1
        || snapshot.records[0].key != "document"
    {
        return Err(QueryFailure::invalid_data());
    }
    check_input_bytes(snapshot.records[0].bytes.len(), limits, retains_payload)
}

fn check_input_bytes(
    bytes: usize,
    limits: &QueryLimits,
    retains_payload: bool,
) -> Result<(), QueryFailure> {
    if bytes > limits.max_source_bytes {
        return Err(QueryFailure::limit(
            unisphere_core::query::LimitKind::SourceBytes,
        ));
    }
    if bytes > limits.max_total_input_bytes {
        return Err(QueryFailure::limit(
            unisphere_core::query::LimitKind::TotalInputBytes,
        ));
    }
    if retains_payload && bytes > limits.max_retained_bytes {
        return Err(QueryFailure::limit(
            unisphere_core::query::LimitKind::RetainedBytes,
        ));
    }
    Ok(())
}

fn unsupported(adapter: &str) -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::UnsupportedSource,
        RecoveryAction::ChooseAdapter {
            allowed: AdapterId::new(adapter).into_iter().collect(),
        },
    )
}

fn ensure_partition(
    partitions: &mut Vec<SourcePartition>,
    indices: &mut BTreeMap<(Option<String>, Option<String>), usize>,
    source_id: unisphere_core::query::SourceId,
    session_id: Option<&str>,
    participant: Option<&str>,
    view: SourceViewKind,
    membership: MembershipPolicy,
) -> PartitionId {
    let key = (
        session_id.map(str::to_owned),
        participant.map(str::to_owned),
    );
    if let Some(index) = indices.get(&key) {
        return partitions[*index].id;
    }
    let native_key = format!(
        "current\0{}\0{}",
        session_id.unwrap_or(""),
        participant.unwrap_or("")
    );
    let id = PartitionId::derive(source_id, native_key.as_bytes());
    indices.insert(key, partitions.len());
    partitions.push(SourcePartition {
        id,
        native_session_id: session_id.map(str::to_owned),
        participant_id: participant.map(str::to_owned),
        view,
        membership,
        associations: Vec::new(),
    });
    id
}

fn legacy_partition(
    source_id: unisphere_core::query::SourceId,
    key: &[u8],
    view: SourceViewKind,
    session_id: Option<&str>,
    membership: MembershipPolicy,
) -> SourcePartition {
    SourcePartition {
        id: PartitionId::derive(source_id, key),
        native_session_id: session_id.map(str::to_owned),
        participant_id: None,
        view,
        membership,
        associations: Vec::new(),
    }
}

#[derive(Clone)]
struct ContextPoint {
    partition: PartitionId,
    start: NativeSequence,
    paths: Vec<(AssociationBasis, PathBuf)>,
    branch: Option<String>,
}

fn context_point(
    partition: PartitionId,
    start: NativeSequence,
    data: &Map<String, Value>,
) -> ContextPoint {
    let mut paths = Vec::new();
    for (field, basis) in [
        ("cwd", AssociationBasis::NativeCwd),
        ("gitRoot", AssociationBasis::NativeGitRoot),
    ] {
        if let Some(path) = nonempty_string(data, field).map(PathBuf::from)
            && path.is_absolute()
        {
            paths.push((basis, path));
        }
    }
    ContextPoint {
        partition,
        start,
        paths,
        branch: nonempty_string(data, "branch"),
    }
}

fn apply_context_intervals(
    partitions: &mut [SourcePartition],
    points: &BTreeMap<PartitionId, Vec<ContextPoint>>,
) {
    for partition in partitions {
        let Some(points) = points.get(&partition.id) else {
            continue;
        };
        for (index, point) in points.iter().enumerate() {
            let until = points.get(index + 1).map(|next| next.start.clone());
            partition
                .associations
                .extend(
                    point
                        .paths
                        .iter()
                        .map(|(basis, path)| AssociationObservation {
                            basis: *basis,
                            path: Some(path.clone()),
                            partition: point.partition,
                            applies_to: AssociationExtent::From {
                                start: point.start.clone(),
                                until: until.clone(),
                            },
                        }),
                );
        }
    }
}

fn current_role(kind: &str, data: &Map<String, Value>) -> MessageRole {
    match kind {
        "user.message" => MessageRole::User,
        "system.message" => match data.get("role").and_then(Value::as_str) {
            Some("developer") => MessageRole::Developer,
            Some("system") => MessageRole::System,
            _ => MessageRole::Unknown,
        },
        _ => MessageRole::Assistant,
    }
}

fn message_parts(
    kind: &str,
    data: &Map<String, Value>,
    access: &ContentAccess,
    source_id: unisphere_core::query::SourceId,
    offset: u64,
    issues: &mut Vec<AvailabilityIssue>,
) -> Vec<ObservationPart> {
    let fields = match kind {
        "assistant.message_delta" | "assistant.reasoning_delta" => &["deltaContent"][..],
        "assistant.reasoning" => &["content"][..],
        "user.message" => &["content", "transformedContent"][..],
        "assistant.message" => &["content", "reasoningText"][..],
        _ => &["content"][..],
    };
    let mut parts = Vec::new();
    let mut omitted = false;
    for field in fields {
        if let Some(text) = data.get(*field).and_then(Value::as_str) {
            let reasoning = *field == "reasoningText" || kind.contains("reasoning");
            let allowed = access.permits_payload(FieldId::Parts)
                || (!reasoning && access.permits_payload(FieldId::Text));
            if allowed {
                if reasoning {
                    parts.push(ObservationPart::Reasoning(text.into()));
                } else if *field == "transformedContent" {
                    parts.push(ObservationPart::Structured(
                        serde_json::json!({"transformed_text": text}),
                    ));
                } else {
                    parts.push(ObservationPart::Text(text.into()));
                }
            } else {
                omitted = true;
            }
        }
    }
    if omitted {
        parts.push(ObservationPart::Unavailable(
            AvailabilityCode::SensitiveOmitted,
        ));
        issues.push(issue(
            source_id,
            Some(FieldId::Parts),
            Some(offset),
            AvailabilityCode::SensitiveOmitted,
        ));
    }
    if data
        .get("attachments")
        .is_some_and(|value| !value.is_null())
    {
        parts.push(ObservationPart::Unavailable(AvailabilityCode::NotSupported));
        issues.push(issue(
            source_id,
            Some(FieldId::Parts),
            Some(offset),
            AvailabilityCode::NotSupported,
        ));
    }
    parts
}

fn push_tool_call(
    data: &Map<String, Value>,
    name_field: &str,
    access: &ContentAccess,
    (source_id, offset): (unisphere_core::query::SourceId, u64),
    turn_id: Option<String>,
    facets: &mut Vec<ObservationFacet>,
    issues: &mut Vec<AvailabilityIssue>,
) {
    let Some(native_call_id) = nonempty_string(data, "toolCallId") else {
        issues.push(issue(
            source_id,
            Some(FieldId::CallId),
            Some(offset),
            AvailabilityCode::NotCaptured,
        ));
        return;
    };
    let Some(native_name) = nonempty_string(data, name_field) else {
        issues.push(issue(
            source_id,
            Some(FieldId::ToolName),
            Some(offset),
            AvailabilityCode::NotCaptured,
        ));
        return;
    };
    facets.push(ObservationFacet::ToolCall {
        family: tool_family(&native_name).map(str::to_owned),
        native_call_id,
        native_name,
        input: gated_value(
            data.get("arguments"),
            access,
            FieldId::Input,
            source_id,
            Some(offset),
            issues,
        ),
        turn_id,
    });
}

fn tool_output(
    data: &Map<String, Value>,
    access: &ContentAccess,
    source_id: unisphere_core::query::SourceId,
    offset: u64,
    issues: &mut Vec<AvailabilityIssue>,
) -> Vec<ObservationPart> {
    let mut output = gated_value(
        data.get("result"),
        access,
        FieldId::Output,
        source_id,
        Some(offset),
        issues,
    );
    if let Some(error) = data.get("error") {
        output.extend(gated_value(
            Some(error),
            access,
            FieldId::Output,
            source_id,
            Some(offset),
            issues,
        ));
    }
    output
}

fn progress_parts(
    data: &Map<String, Value>,
    access: &ContentAccess,
    field: FieldId,
    source_id: unisphere_core::query::SourceId,
    offset: u64,
    issues: &mut Vec<AvailabilityIssue>,
) -> Vec<ObservationPart> {
    let keys = ["partialOutput", "progressMessage", "inputDelta"];
    let allowed = access.permits_payload(field);
    let mut parts = Vec::new();
    let mut found = false;
    for key in keys {
        if let Some(value) = data.get(key) {
            found = true;
            if allowed {
                parts.push(match value {
                    Value::String(text) => ObservationPart::Text(text.clone()),
                    other => ObservationPart::Structured(other.clone()),
                });
            }
        }
    }
    if found && !allowed {
        parts.push(ObservationPart::Unavailable(
            AvailabilityCode::SensitiveOmitted,
        ));
        issues.push(issue(
            source_id,
            Some(field),
            Some(offset),
            AvailabilityCode::SensitiveOmitted,
        ));
    }
    parts
}

fn gated_value(
    value: Option<&Value>,
    access: &ContentAccess,
    field: FieldId,
    source_id: unisphere_core::query::SourceId,
    offset: Option<u64>,
    issues: &mut Vec<AvailabilityIssue>,
) -> Vec<ObservationPart> {
    let Some(value) = value else {
        return Vec::new();
    };
    if !access.permits_payload(field) {
        issues.push(issue(
            source_id,
            Some(field),
            offset,
            AvailabilityCode::SensitiveOmitted,
        ));
        return vec![ObservationPart::Unavailable(
            AvailabilityCode::SensitiveOmitted,
        )];
    }
    vec![match value {
        Value::String(text) => ObservationPart::Text(text.clone()),
        value => ObservationPart::Structured(value.clone()),
    }]
}

fn legacy_text_parts(
    data: &Map<String, Value>,
    field: &str,
    access: &ContentAccess,
    query_field: FieldId,
    source_id: unisphere_core::query::SourceId,
    issues: &mut Vec<AvailabilityIssue>,
) -> Vec<ObservationPart> {
    gated_value(
        data.get(field),
        access,
        query_field,
        source_id,
        None,
        issues,
    )
}

fn legacy_timeline_parts(
    data: &Map<String, Value>,
    access: &ContentAccess,
    source_id: unisphere_core::query::SourceId,
    issues: &mut Vec<AvailabilityIssue>,
) -> Vec<ObservationPart> {
    let mut parts = legacy_text_parts(data, "text", access, FieldId::Text, source_id, issues);
    if let Some(expanded) = data.get("expandedText") {
        if access.permits_payload(FieldId::Text) {
            parts.push(ObservationPart::Structured(
                serde_json::json!({"transformed_text": expanded}),
            ));
        } else {
            parts.push(ObservationPart::Unavailable(
                AvailabilityCode::SensitiveOmitted,
            ));
            issues.push(issue(
                source_id,
                Some(FieldId::Text),
                None,
                AvailabilityCode::SensitiveOmitted,
            ));
        }
    }
    parts
}

fn tool_outcome(data: &Map<String, Value>) -> Outcome {
    if data.get("cancelled").and_then(Value::as_bool) == Some(true) {
        Outcome::Cancelled
    } else {
        match data.get("success").and_then(Value::as_bool) {
            Some(true) => Outcome::Succeeded,
            Some(false) => Outcome::Failed,
            None => Outcome::Unknown,
        }
    }
}

fn tool_family(name: &str) -> Option<&'static str> {
    match name {
        "bash" | "shell" | "run_terminal_command" => Some("shell"),
        "read_file" | "view" => Some("file-read"),
        "write_file" | "create_file" | "edit" | "apply_patch" => Some("file-write"),
        _ => None,
    }
}

fn event_links(kind: &str, data: Option<&Map<String, Value>>) -> Vec<BranchLink> {
    let Some(data) = data else { return Vec::new() };
    let mut links = Vec::new();
    if let Some(target) = nonempty_string(data, "detachedFromSpawningParentSessionId") {
        links.push(BranchLink {
            kind: LineageKind::Parent,
            target,
        });
    }
    if kind.starts_with("subagent.") {
        if let Some(target) = nonempty_string(data, "parentId") {
            links.push(BranchLink {
                kind: LineageKind::Parent,
                target,
            });
        }
        if let Some(target) = nonempty_string(data, "agentId") {
            links.push(BranchLink {
                kind: LineageKind::Subagent,
                target,
            });
        }
    }
    links
}

fn usage_counters(
    data: &Map<String, Value>,
    source_id: unisphere_core::query::SourceId,
    offset: u64,
    issues: &mut Vec<AvailabilityIssue>,
) -> UsageCounters {
    UsageCounters {
        input_tokens: token(data, "inputTokens", source_id, offset, issues),
        output_tokens: token(data, "outputTokens", source_id, offset, issues),
        cache_read_tokens: token(data, "cacheReadTokens", source_id, offset, issues),
        cache_write_tokens: token(data, "cacheWriteTokens", source_id, offset, issues),
    }
}

fn has_usage(counters: &UsageCounters) -> bool {
    counters.input_tokens.is_some()
        || counters.output_tokens.is_some()
        || counters.cache_read_tokens.is_some()
        || counters.cache_write_tokens.is_some()
}

fn token(
    data: &Map<String, Value>,
    field: &str,
    source_id: unisphere_core::query::SourceId,
    offset: u64,
    issues: &mut Vec<AvailabilityIssue>,
) -> Option<u64> {
    let value = data.get(field)?;
    let token = value.as_u64();
    if token.is_none() {
        issues.push(issue(
            source_id,
            Some(token_field(field)),
            Some(offset),
            AvailabilityCode::ProjectionMissing,
        ));
    }
    token
}

fn token_field(field: &str) -> FieldId {
    match field {
        "inputTokens" => FieldId::InputTokens,
        "outputTokens" => FieldId::OutputTokens,
        "cacheReadTokens" => FieldId::CacheReadTokens,
        _ => FieldId::CacheWriteTokens,
    }
}

fn event_timestamp(
    value: Option<&Value>,
    source_id: unisphere_core::query::SourceId,
    offset: Option<u64>,
    issues: &mut Vec<AvailabilityIssue>,
) -> Option<Timestamp> {
    if value.is_none() {
        issues.push(issue(
            source_id,
            Some(FieldId::Timestamp),
            offset,
            AvailabilityCode::NotCaptured,
        ));
        return None;
    }
    optional_timestamp(value, source_id, offset, issues)
}

fn optional_timestamp(
    value: Option<&Value>,
    source_id: unisphere_core::query::SourceId,
    offset: Option<u64>,
    issues: &mut Vec<AvailabilityIssue>,
) -> Option<Timestamp> {
    let value = value?;
    let parsed = value
        .as_str()
        .and_then(|value| Timestamp::parse(value, TimestampBasis::Native).ok());
    if parsed.is_none() {
        issues.push(issue(
            source_id,
            Some(FieldId::Timestamp),
            offset,
            AvailabilityCode::InvalidClock,
        ));
    }
    parsed
}

fn snapshot_ref(source: &SourceEvidence, key: &str, subrecord: &str) -> SourceRef {
    SourceRef {
        source_id: source.id,
        revision: source.revision.clone(),
        locator: NativeLocator::Snapshot { key: key.into() },
        subrecord: subrecord.into(),
    }
}

fn snapshot_sequence(key: &str) -> NativeSequence {
    NativeSequence {
        version: 1,
        key: key.as_bytes().to_vec(),
    }
}

fn issue(
    source: unisphere_core::query::SourceId,
    field: Option<FieldId>,
    offset: Option<u64>,
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

fn nonempty_string(data: &Map<String, Value>, field: &str) -> Option<String> {
    data.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}
fn permits_model(access: &ContentAccess) -> bool {
    access.permits_payload(FieldId::Model) || access.permits_payload(FieldId::Models)
}

fn common_fields() -> BTreeSet<FieldId> {
    BTreeSet::from([
        FieldId::Id,
        FieldId::SourceRefs,
        FieldId::NativeId,
        FieldId::Harness,
        FieldId::Adapter,
        FieldId::Availability,
        FieldId::Format,
        FieldId::ReadStatus,
        FieldId::Revision,
        FieldId::Association,
        FieldId::SessionId,
        FieldId::BranchIds,
        FieldId::Kind,
        FieldId::Timestamp,
        FieldId::Role,
        FieldId::Text,
        FieldId::Parts,
        FieldId::ToolName,
        FieldId::ToolFamily,
        FieldId::Input,
        FieldId::Output,
        FieldId::Status,
        FieldId::StatusReason,
        FieldId::CallId,
        FieldId::MessageId,
        FieldId::TurnId,
        FieldId::InputTokens,
        FieldId::OutputTokens,
        FieldId::CacheReadTokens,
        FieldId::CacheWriteTokens,
    ])
}

fn current_fields() -> BTreeSet<FieldId> {
    let mut fields = common_fields();
    fields.extend([
        FieldId::Name,
        FieldId::Models,
        FieldId::Model,
        FieldId::ProjectPath,
        FieldId::StartedAt,
        FieldId::FirstEventAt,
        FieldId::ParentIds,
    ]);
    fields
}

fn legacy_fields() -> BTreeSet<FieldId> {
    let mut fields = common_fields();
    fields.extend([FieldId::StartedAt, FieldId::FirstEventAt]);
    fields
}
