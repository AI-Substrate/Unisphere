//! Bounded legacy monolithic JSON projection, not an events.jsonl replay.

use super::{DESCRIPTOR, Mapping};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedSnapshot, MappingDiagnosticCode,
    MappingOptions, NativeSnapshot, PipelineError, PipelineErrorKind, SnapshotAdapter,
    SnapshotDiagnostic, SnapshotFormat, TelemetryRecord,
};

/// Legacy JSON has revision replacement semantics, never an LF cursor.
pub const SNAPSHOT_DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "copilot-cli-snapshot",
    application: "GitHub Copilot CLI",
    description: "Legacy Copilot CLI JSON session projection with separate chat and timeline views; metadata by default.",
    locations: &[LocationHint {
        platforms: &["macos", "linux", "windows"],
        base: "home",
        path: ".copilot/history-session-state",
        session_glob: "*.json",
        storage_format: "json_document",
    }],
    capabilities: AdapterCapabilities {
        sdk_caller_owned_cursor: false,
        cursor_source_assumption: "whole_source_revision",
        ..DESCRIPTOR.capabilities
    },
};

/// Pure mapping of a supplied legacy document; chat and timeline are distinct views.
#[derive(Debug, Clone, Copy, Default)]
pub struct CopilotCliAdapterSnapshot;

impl SnapshotAdapter for CopilotCliAdapterSnapshot {
    fn name(&self) -> &'static str {
        SNAPSHOT_DESCRIPTOR.id
    }

    fn map_snapshot(
        &self,
        snapshot: &NativeSnapshot,
        options: MappingOptions,
    ) -> Result<MappedSnapshot, PipelineError> {
        snapshot.source.validate()?;
        if snapshot.source.format != SnapshotFormat::JsonDocument {
            return Err(PipelineError::new(PipelineErrorKind::Unsupported, None));
        }
        let [record] = snapshot.records.as_slice() else {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        };
        if record.key != "document" || snapshot.revision.is_empty() {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        }
        let value: Value = serde_json::from_slice(&record.bytes)
            .map_err(|_| PipelineError::new(PipelineErrorKind::InvalidData, None))?;
        let document = value.as_object()
            .ok_or_else(|| PipelineError::new(PipelineErrorKind::InvalidData, None))?;
        let session_id = document.get("sessionId").and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        if let Some(selected) = snapshot.source.session_id.as_deref()
            && session_id != Some(selected)
        {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        let mut output = MappedSnapshot::default();
        let projection = Projection { snapshot, options, session_id };
        projection.emit("document", "session", &mut output, |mapping| {
            // The document has no native discriminator; do not invent an event type.
            mapping.attributes.insert("unisphere.source.kind".into(), json!("unknown"));
            if session_id.is_none() {
                mapping.diagnostic(MappingDiagnosticCode::InvalidField);
            }
            mapping.strings(document, &[("startTime", "unisphere.copilot.session.start_time")]);
            let timestamp = timestamp(document.get("startTime"), mapping);
            if !document.contains_key("chatMessages") && !document.contains_key("timeline") {
                mapping.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
            }
            (timestamp, None)
        });
        for view in ["chatMessages", "timeline"] {
            let Some(value) = document.get(view) else { continue };
            let Some(items) = value.as_array() else {
                output.diagnostics.push(SnapshotDiagnostic {
                    key: format!("document#/{view}"),
                    code: MappingDiagnosticCode::InvalidField,
                });
                continue;
            };
            for (index, value) in items.iter().enumerate() {
                let key = format!("document#/{view}/{index}");
                projection.emit(&key, view, &mut output, |mapping| {
                    let Some(data) = mapping.object(value) else {
                        mapping.attributes.insert("unisphere.source.kind".into(), json!("unknown"));
                        return (None, None);
                    };
                    if view == "chatMessages" {
                        (None, chat(data, mapping))
                    } else {
                        let time = timestamp(data.get("timestamp"), mapping);
                        (time, timeline(data, mapping))
                    }
                });
            }
        }
        Ok(output)
    }
}

struct Projection<'a> {
    snapshot: &'a NativeSnapshot,
    options: MappingOptions,
    session_id: Option<&'a str>,
}

impl Projection<'_> {
    fn emit(
        &self,
        key: &str,
        view: &str,
        output: &mut MappedSnapshot,
        project: impl FnOnce(&mut Mapping<'_>) -> (Option<u64>, Option<Value>),
    ) {
        let mut diagnostic = |code| output.diagnostics.push(SnapshotDiagnostic {
            key: key.into(),
            code,
        });
        let mut mapping = Mapping {
            include_content: self.options.include_content,
            omitted: false,
            diagnostics: &mut diagnostic,
            attributes: BTreeMap::from([
                ("unisphere.profile.version".into(), json!(1)),
                ("unisphere.source.adapter".into(), json!(SNAPSHOT_DESCRIPTOR.id)),
                ("unisphere.source.path".into(), json!(self.snapshot.source.path.to_str())),
                ("unisphere.source.key".into(), json!(key)),
                ("unisphere.source.revision".into(), json!(self.snapshot.revision)),
                ("unisphere.source.format".into(), json!("json_document")),
                ("unisphere.copilot.view".into(), json!(view)),
            ]),
        };
        if let Some(id) = self.session_id {
            mapping.attributes.insert("unisphere.source.session.id".into(), json!(id));
            mapping.attributes.insert("gen_ai.conversation.id".into(), json!(id));
        }
        let (timestamp_unix_nano, body) = project(&mut mapping);
        output.records.push(TelemetryRecord {
            event_name: "unisphere.session.record".into(),
            timestamp_unix_nano,
            attributes: mapping.attributes,
            body,
        });
    }
}

fn timestamp(value: Option<&Value>, mapping: &mut Mapping<'_>) -> Option<u64> {
    let value = value?;
    let parsed = value.as_str()
        .and_then(|text| OffsetDateTime::parse(text, &Rfc3339).ok())
        .and_then(|time| u64::try_from(time.unix_timestamp_nanos()).ok());
    if parsed.is_none() {
        mapping.diagnostic(MappingDiagnosticCode::InvalidTimestamp);
    }
    parsed
}

fn chat(data: &Map<String, Value>, mapping: &mut Mapping<'_>) -> Option<Value> {
    let role = mapping.string(data, "role").unwrap_or("unknown");
    mapping.attributes.insert("unisphere.source.kind".into(), json!(role));
    if !matches!(role, "user" | "assistant" | "tool") {
        mapping.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
        if !data.is_empty() {
            mapping.content();
        }
        return None;
    }
    let mut parts = Vec::new();
    if role == "tool" {
        mapping.strings(data, &[("tool_call_id", "unisphere.copilot.tool_call.id")]);
        let id = mapping.string(data, "tool_call_id");
        let content = mapping.string(data, "content");
        if id.is_none() || content.is_none() {
            mapping.diagnostic(MappingDiagnosticCode::InvalidField);
        }
        if content.is_some() && mapping.content() {
            let mut part = json!({"type": "tool_call_response", "response": content});
            if let Some(id) = id {
                part["id"] = json!(id);
            }
            parts.push(part);
        }
    } else {
        if !data.get("content").is_some_and(Value::is_string) {
            mapping.diagnostic(MappingDiagnosticCode::InvalidField);
        }
        mapping.text_part(data, "content", "text", &mut parts);
        if let Some(value) = data.get("tool_calls") {
            if let Some(calls) = value.as_array() {
                for value in calls {
                    if let Some(call) = mapping.object(value)
                        && let Some(part) = function_call(call, mapping)
                    {
                        parts.push(part);
                    }
                }
            } else {
                mapping.diagnostic(MappingDiagnosticCode::InvalidField);
            }
        }
    }
    // No model/usage/occurrence-time fields were observed in this legacy view.
    mapping.unsupported_fields(data, &["usage", "model", "timestamp"]);
    mapping.message_body(role, parts)
}

fn function_call(data: &Map<String, Value>, mapping: &mut Mapping<'_>) -> Option<Value> {
    if mapping.string(data, "type") != Some("function") {
        mapping.content();
        mapping.diagnostic(MappingDiagnosticCode::UnsupportedPart);
        return None;
    }
    let id = mapping.string(data, "id");
    let Some(function) = data.get("function").and_then(|value| mapping.object(value)) else {
        if !data.contains_key("function") {
            mapping.diagnostic(MappingDiagnosticCode::InvalidField);
        }
        return None;
    };
    let name = mapping.string(function, "name");
    let arguments = mapping.string(function, "arguments");
    if id.is_none() || name.is_none() || arguments.is_none() {
        mapping.diagnostic(MappingDiagnosticCode::InvalidField);
    }
    if !mapping.content() {
        return None;
    }
    // Legacy arguments are strings, even when they happen to contain valid JSON.
    Some(json!({"type": "tool_call", "id": id?, "name": name?, "arguments": arguments?}))
}

fn timeline(data: &Map<String, Value>, mapping: &mut Mapping<'_>) -> Option<Value> {
    let kind = mapping.string(data, "type").unwrap_or("unknown");
    mapping.attributes.insert("unisphere.source.kind".into(), json!(kind));
    mapping.strings(data, &[
        ("id", "unisphere.source.record.id"),
        ("callId", "unisphere.copilot.tool_call.id"),
    ]);
    mapping.unsupported_fields(data, &["mentions", "usage", "model"]);
    match kind {
        "user" | "copilot" => {
            let mut parts = Vec::new();
            mapping.text_part(data, "text", "text", &mut parts);
            mapping.text_part(data, "expandedText", "unisphere.transformed_text", &mut parts);
            mapping.message_body(if kind == "user" { "user" } else { "assistant" }, parts)
        }
        "tool_call_requested" | "tool_call_completed" => {
            let id = mapping.string(data, "callId");
            if id.is_none() {
                mapping.diagnostic(MappingDiagnosticCode::InvalidField);
            }
            let requested = kind == "tool_call_requested";
            let name = mapping.string(data, "name");
            if requested && name.is_none() {
                mapping.diagnostic(MappingDiagnosticCode::InvalidField);
            }
            let field = if requested { "arguments" } else { "result" };
            let payload = data.get(field).and_then(|value| {
                if value.is_object() {
                    Some(value)
                } else {
                    mapping.diagnostic(MappingDiagnosticCode::InvalidField);
                    None
                }
            });
            let mut parts = Vec::new();
            if mapping.content()
                && let Some(id) = id
                && (!requested || name.is_some())
            {
                let mut part = json!({
                    "type": if requested { "tool_call" } else { "tool_call_response" },
                    "id": id,
                });
                if let Some(name) = name {
                    part["name"] = json!(name);
                }
                if let Some(payload) = payload {
                    part[if requested { "arguments" } else { "response" }] = payload.clone();
                }
                parts.push(part);
            }
            for (field, part_type) in [
                ("text", "text"),
                ("expandedText", "unisphere.transformed_text"),
                ("toolTitle", "unisphere.tool_title"),
                ("intentionSummary", "unisphere.tool_intention"),
            ] {
                mapping.text_part(data, field, part_type, &mut parts);
            }
            mapping.message_body(if kind == "tool_call_requested" { "assistant" } else { "tool" }, parts)
        }
        "info" => mapping.native_text(data, kind, &["text", "expandedText"]),
        _ => {
            mapping.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
            if !data.is_empty() {
                mapping.content();
            }
            None
        }
    }
}
