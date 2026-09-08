//! Deterministic Claude Code record mapping over caller-supplied bytes.
//!
//! This adapter emits physical source fragments, not inference operations or a
//! reconstructed conversation. Metadata-only is the default. No source, sidecar,
//! attachment, environment, clock, or destination is accessed here.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    MappedBatch, MappingDiagnostic, MappingDiagnosticCode, MappingOptions, NativeRecord,
    PipelineError, PipelineErrorKind, SessionAdapter, SessionRef, TelemetryRecord,
};

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
            let value: Value = serde_json::from_slice(&native.bytes).map_err(|_| {
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
