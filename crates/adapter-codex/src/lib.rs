//! Pure, stateless projection of supplied Codex rollout JSONL records.
//!
//! Physical records are not reconstructed turns. Header context is never carried
//! into later records; usage snapshots and event summaries are never aggregated.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedBatch, MappingDiagnostic,
    MappingDiagnosticCode, MappingOptions, NativeRecord, PipelineError, PipelineErrorKind,
    SessionAdapter, SessionRef, TelemetryRecord,
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
            let value = serde_json::from_slice(&native.bytes).map_err(|_| {
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
                self.attributes.insert((*profile).into(), Value::String(value));
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
        let body = self.object(object.remove("payload")).and_then(|mut payload| {
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
                    if ["base_instructions", "instructions"].iter().any(|key| {
                        payload.get(*key).is_some_and(|value| !value.is_null())
                    }) {
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
                    if ["user_instructions", "developer_instructions"].iter().any(|key| {
                        payload.get(*key).is_some_and(|value| !value.is_null())
                    }) {
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
                    self.attributes.insert("unisphere.codex.payload.type".into(), json!(subtype));
                    body
                }
                "compacted" => {
                    self.attributes.insert("unisphere.codex.representation".into(), json!("compaction"));
                    if payload.get("replacement_history").is_some_and(|value| !value.is_null()) {
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
        self.attributes.insert("unisphere.source.kind".into(), json!(kind));
        if self.content_present && !self.include_content {
            self.attributes.insert("unisphere.content.omitted".into(), json!(true));
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
        if let Some(value) = payload.remove("internal_chat_message_metadata_passthrough")
            .filter(|value| !value.is_null())
            && let Some(mut metadata) = self.object(Some(value))
        {
            self.strings(&mut metadata, &[("turn_id", "unisphere.source.turn.id")]);
        }
        match kind {
            "message" => {
                let role = self.string(&mut payload, "role");
                let role = match role {
                    Some(role) if matches!(role.as_str(), "user" | "assistant" | "system" | "developer") => {
                        self.attributes.insert("unisphere.message.role".into(), json!(role));
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
                if payload.get("encrypted_content").is_some_and(|value| !value.is_null()) {
                    self.content_present = true;
                    self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                    if self.include_content {
                        parts.push(json!({"type": "unisphere.unknown", "native_type": "encrypted_content"}));
                    }
                }
                self.body(None, parts)
            }
            "function_call" | "custom_tool_call" => {
                let key = if kind == "function_call" { "arguments" } else { "input" };
                self.content_present |= payload.contains_key(key);
                let arguments = self.string(&mut payload, key);
                if arguments.is_none() || !self.attributes.contains_key("unisphere.tool.name") {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                }
                if payload.get("encrypted_function_args").is_some_and(|value| !value.is_null()) {
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
                    Some(Value::String(text)) => self.include_content.then_some(Value::String(text)),
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
                self.attributes.insert("unisphere.codex.representation".into(), json!("compaction"));
                self.content_present = payload.get("encrypted_content").is_some_and(|value| !value.is_null());
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
        self.attributes.insert("unisphere.codex.representation".into(), json!("native_event_summary"));
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
                    if payload.get(key).is_some_and(|value| !value.is_null() && value.as_array().is_none_or(|items| !items.is_empty())) {
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
                        self.attributes.insert("unisphere.codex.exit_code".into(), value);
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
        let Some(value) = value.filter(|value| !value.is_null()) else { return };
        let Some(mut info) = self.object(Some(value)) else { return };
        for (native, scope) in [
            ("last_token_usage", "native_last_token_usage"),
            ("total_token_usage", "native_cumulative_token_usage"),
        ] {
            let Some(value) = info.remove(native).filter(|value| !value.is_null()) else { continue };
            let Some(mut usage) = self.object(Some(value)) else { continue };
            let mut retained = false;
            for field in [
                "input_tokens", "cached_input_tokens", "cache_write_input_tokens",
                "output_tokens", "reasoning_output_tokens", "total_tokens",
            ] {
                if let Some(value) = usage.remove(field) {
                    if value.as_i64().is_some_and(|value| value >= 0) {
                        self.attributes.insert(format!("unisphere.usage.{native}.{field}"), value);
                        retained = true;
                    } else {
                        self.diagnostic(MappingDiagnosticCode::InvalidField);
                    }
                }
            }
            if retained {
                self.attributes.insert(format!("unisphere.usage.{native}.scope"), json!(scope));
            }
        }
        if let Some(value) = info.remove("model_context_window").filter(|value| !value.is_null()) {
            if value.as_i64().is_some_and(|value| value >= 0) {
                self.attributes.insert("unisphere.codex.model_context_window".into(), value);
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
        parts.into_iter().filter_map(|part| {
            let mut part = self.object(Some(part))?;
            let kind = self.kind(&mut part);
            let supported = if reasoning {
                matches!(kind.as_str(), "summary_text" | "reasoning_text" | "text")
            } else {
                matches!(kind.as_str(), "input_text" | "output_text")
            };
            if supported {
                let mut text = self.text(part.remove("text"), if reasoning { "reasoning" } else { "text" })?;
                text["unisphere.codex.native_type"] = json!(kind);
                Some(text)
            } else {
                self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                self.include_content.then(|| json!({"type": "unisphere.unknown", "native_type": kind}))
            }
        }).collect()
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
        let Some(value) = value.filter(|value| !value.is_null()) else { return None };
        self.content_present = true;
        let mut action = self.object(Some(value))?;
        let kind = self.kind(&mut action);
        let fields: &[(&str, bool)] = match (tool_kind, kind.as_str()) {
            ("local_shell_call", "exec") => &[("command", true), ("working_directory", false), ("user", false)],
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
                    value.as_array().is_some_and(|values| values.iter().all(Value::is_string))
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
