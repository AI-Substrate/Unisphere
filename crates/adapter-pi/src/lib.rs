//! Pure projection of supplied Pi v3 JSONL tree entries, not a reconstructed chat.
//! Content is opt-in; extension state, opaque signatures and sidecars are omitted.
#![forbid(unsafe_code)]
mod query;

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedBatch, MappingDiagnostic,
    MappingDiagnosticCode as Code, MappingOptions, NativeRecord, PipelineError, PipelineErrorKind,
    SessionAdapter, SessionRef, TelemetryRecord,
};

/// Metadata for the Pi JSONL mapper; location hints never trigger discovery.
pub const DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "pi",
    application: "Pi",
    description: "Physical Pi v3 JSONL tree entries; metadata by default, structured content by opt-in.",
    locations: &[LocationHint {
        platforms: &["macos", "linux", "windows"],
        base: "home",
        path: ".pi/agent/sessions",
        session_glob: "*/*.jsonl",
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

/// Stateless mapper: equal records/options yield equal output across batch splits.
#[derive(Debug, Clone, Copy, Default)]
pub struct PiAdapter;

impl SessionAdapter for PiAdapter {
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
            let value = decode(&native.bytes).map_err(|()| {
                PipelineError::new(PipelineErrorKind::InvalidData, Some(native.offset))
            })?;
            let mut mapping = Mapping {
                offset: native.offset,
                include_content: options.include_content,
                diagnostics: &mut batch.diagnostics,
            };
            batch.records.push(mapping.record(&value, path));
        }
        Ok(batch)
    }
}

struct Mapping<'a> {
    offset: u64,
    include_content: bool,
    diagnostics: &'a mut Vec<MappingDiagnostic>,
}

fn decode(bytes: &[u8]) -> Result<Value, ()> {
    serde_json::from_slice(bytes).map_err(|_| ())
}

impl Mapping<'_> {
    fn diagnostic(&mut self, code: Code) {
        self.diagnostics.push(MappingDiagnostic {
            offset: self.offset,
            code,
        });
    }

    fn field<'a>(
        &mut self,
        object: &'a Map<String, Value>,
        key: &str,
        valid: fn(&Value) -> bool,
    ) -> Option<&'a Value> {
        match object.get(key) {
            None => None,
            Some(value) if valid(value) => Some(value),
            Some(_) => {
                self.diagnostic(Code::InvalidField);
                None
            }
        }
    }

    fn required_string<'a>(
        &mut self,
        object: &'a Map<String, Value>,
        key: &str,
    ) -> Option<&'a str> {
        if !object.contains_key(key) {
            self.diagnostic(Code::InvalidField);
        }
        self.field(object, key, Value::is_string)?.as_str()
    }

    fn strings(
        &mut self,
        object: &Map<String, Value>,
        attributes: &mut BTreeMap<String, Value>,
        fields: &[(&str, &str)],
    ) {
        for (native, profile) in fields {
            if let Some(value) = self.field(object, native, Value::is_string) {
                attributes.insert((*profile).into(), value.clone());
            }
        }
    }

    fn flags(
        &mut self,
        object: &Map<String, Value>,
        attributes: &mut BTreeMap<String, Value>,
        fields: &[(&str, &str)],
    ) {
        for (native, profile) in fields {
            if let Some(value) = self.field(object, native, Value::is_boolean) {
                attributes.insert((*profile).into(), value.clone());
            }
        }
    }

    fn omitted(&mut self, attributes: &mut BTreeMap<String, Value>) {
        attributes.insert("unisphere.content.omitted".into(), json!(true));
        self.diagnostic(Code::ContentOmitted);
    }

    fn opaque(
        &mut self,
        object: &Map<String, Value>,
        keys: &[&str],
        attributes: &mut BTreeMap<String, Value>,
    ) {
        if keys.iter().any(|key| object.contains_key(*key)) {
            self.omitted(attributes);
        }
    }

    fn record(&mut self, value: &Value, path: &str) -> TelemetryRecord {
        let mut record = TelemetryRecord {
            event_name: "unisphere.session.record".into(),
            timestamp_unix_nano: None,
            attributes: BTreeMap::from([
                ("unisphere.profile.version".into(), json!(1)),
                ("unisphere.source.adapter".into(), json!(DESCRIPTOR.id)),
                ("unisphere.source.path".into(), json!(path)),
                ("unisphere.source.offset".into(), json!(self.offset)),
                ("unisphere.source.kind".into(), json!("unknown")),
            ]),
            body: None,
        };
        let Some(object) = value.as_object() else {
            self.diagnostic(Code::InvalidField);
            return record;
        };
        let kind = self.required_string(object, "type").unwrap_or("unknown");
        let attributes = &mut record.attributes;
        attributes.insert("unisphere.source.kind".into(), json!(kind));
        if let Some(id) = self.required_string(object, "id") {
            attributes.insert("unisphere.source.record.id".into(), json!(id));
            if kind == "session" {
                attributes.insert("gen_ai.conversation.id".into(), json!(id));
            }
        }
        if kind != "session" {
            match object.get("parentId") {
                Some(Value::Null) => {
                    attributes.insert("unisphere.pi.root_entry".into(), json!(true));
                }
                Some(Value::String(parent)) => {
                    attributes.insert("unisphere.source.parent.id".into(), json!(parent));
                }
                _ => self.diagnostic(Code::InvalidField),
            }
        }
        record.timestamp_unix_nano = self.timestamp(object.get("timestamp"), false);
        record.body = match kind {
            "message" => self.message(object.get("message"), attributes),
            "session" => {
                let version = self.field(object, "version", Value::is_u64);
                if let Some(version) = version {
                    attributes.insert("unisphere.pi.session.version".into(), version.clone());
                }
                if version.and_then(Value::as_u64) != Some(3) {
                    self.diagnostic(Code::UnsupportedRecord);
                }
                self.text_fields(object, "session", &["cwd", "parentSession"], attributes)
            }
            "model_change" => {
                self.strings(
                    object,
                    attributes,
                    &[
                        ("provider", "gen_ai.provider.name"),
                        ("modelId", "gen_ai.request.model"),
                    ],
                );
                None
            }
            "thinking_level_change" => {
                self.strings(
                    object,
                    attributes,
                    &[("thinkingLevel", "unisphere.pi.thinking_level")],
                );
                None
            }
            "compaction" | "branch_summary" => {
                self.strings(
                    object,
                    attributes,
                    &[
                        ("firstKeptEntryId", "unisphere.pi.first_kept_entry.id"),
                        ("fromId", "unisphere.pi.branch.from.id"),
                    ],
                );
                self.flags(
                    object,
                    attributes,
                    &[("fromHook", "unisphere.pi.from_hook")],
                );
                self.tokens_before(object, attributes);
                self.usage(object.get("usage"), kind, attributes);
                self.opaque(object, &["details"], attributes);
                self.text_fields(object, kind, &["summary"], attributes)
            }
            "custom" => {
                self.strings(
                    object,
                    attributes,
                    &[("customType", "unisphere.pi.custom_type")],
                );
                self.opaque(object, &["data"], attributes);
                None
            }
            "custom_message" => self.custom_message(object, attributes),
            "label" => {
                self.strings(
                    object,
                    attributes,
                    &[("targetId", "unisphere.pi.label.target.id")],
                );
                self.text_fields(object, "label", &["label"], attributes)
            }
            "session_info" => self.text_fields(object, "session_info", &["name"], attributes),
            _ => {
                self.diagnostic(Code::UnsupportedRecord);
                None
            }
        };
        record
    }

    fn timestamp(&mut self, value: Option<&Value>, milliseconds: bool) -> Option<u64> {
        let value = value?;
        let parsed = if milliseconds {
            value
                .as_u64()
                .and_then(|value| value.checked_mul(1_000_000))
        } else {
            value
                .as_str()
                .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
                .and_then(|value| u64::try_from(value.unix_timestamp_nanos()).ok())
        };
        if parsed.is_none() {
            self.diagnostic(Code::InvalidTimestamp);
        }
        parsed
    }

    fn tokens_before(
        &mut self,
        object: &Map<String, Value>,
        attributes: &mut BTreeMap<String, Value>,
    ) {
        if let Some(value) = self.field(object, "tokensBefore", nonnegative_integer) {
            attributes.insert(
                "unisphere.pi.compaction.tokens_before".into(),
                value.clone(),
            );
        }
    }

    fn usage(
        &mut self,
        value: Option<&Value>,
        scope: &str,
        attributes: &mut BTreeMap<String, Value>,
    ) {
        let Some(value) = value else { return };
        let Some(usage) = value.as_object() else {
            self.diagnostic(Code::InvalidField);
            return;
        };
        let mut retained = false;
        for (native, profile) in [
            ("input", "unisphere.usage.input_tokens"),
            ("output", "unisphere.usage.output_tokens"),
            ("cacheRead", "unisphere.usage.cache_read_input_tokens"),
            ("cacheWrite", "unisphere.usage.cache_creation_input_tokens"),
            ("cacheWrite1h", "unisphere.pi.usage.cache_write_1h_tokens"),
            ("reasoning", "unisphere.pi.usage.reasoning_tokens"),
            ("totalTokens", "unisphere.pi.usage.total_tokens"),
        ] {
            if let Some(value) = self.field(usage, native, nonnegative_integer) {
                attributes.insert(profile.into(), value.clone());
                retained = true;
            }
        }
        if let Some(cost) = self
            .field(usage, "cost", Value::is_object)
            .and_then(Value::as_object)
        {
            for native in ["input", "output", "cacheRead", "cacheWrite", "total"] {
                if let Some(value) = self.field(cost, native, nonnegative_number) {
                    attributes.insert(format!("unisphere.pi.usage.cost.{native}"), value.clone());
                    retained = true;
                }
            }
        }
        if retained {
            attributes.insert("unisphere.usage.scope".into(), json!(scope));
        }
    }

    fn text_fields(
        &mut self,
        object: &Map<String, Value>,
        kind: &str,
        keys: &[&str],
        attributes: &mut BTreeMap<String, Value>,
    ) -> Option<Value> {
        let mut body = Map::new();
        for key in keys {
            if let Some(value) = self.field(object, key, Value::is_string) {
                if self.include_content {
                    body.insert((*key).into(), value.clone());
                } else {
                    self.omitted(attributes);
                }
            }
        }
        if body.is_empty() {
            None
        } else {
            body.insert("kind".into(), json!(kind));
            Some(Value::Object(body))
        }
    }

    fn custom_message(
        &mut self,
        object: &Map<String, Value>,
        attributes: &mut BTreeMap<String, Value>,
    ) -> Option<Value> {
        self.strings(
            object,
            attributes,
            &[("customType", "unisphere.pi.custom_type")],
        );
        self.flags(
            object,
            attributes,
            &[("display", "unisphere.pi.custom.display")],
        );
        self.opaque(object, &["details"], attributes);
        let parts = self.content(object.get("content"), "custom", attributes)?;
        Some(json!({"kind": "custom_message", "parts": parts}))
    }

    fn message(
        &mut self,
        value: Option<&Value>,
        attributes: &mut BTreeMap<String, Value>,
    ) -> Option<Value> {
        let Some(object) = value.and_then(Value::as_object) else {
            self.diagnostic(Code::InvalidField);
            return None;
        };
        let role = self.required_string(object, "role")?;
        attributes.insert("unisphere.pi.message.role".into(), json!(role));
        if let Some(timestamp) = self.timestamp(object.get("timestamp"), true) {
            attributes.insert(
                "unisphere.pi.message.timestamp_unix_nano".into(),
                json!(timestamp),
            );
        }
        match role {
            "custom" => return self.custom_message(object, attributes),
            "bashExecution" => {
                self.flags(
                    object,
                    attributes,
                    &[
                        ("cancelled", "unisphere.pi.bash.cancelled"),
                        ("truncated", "unisphere.pi.bash.truncated"),
                        (
                            "excludeFromContext",
                            "unisphere.pi.bash.exclude_from_context",
                        ),
                    ],
                );
                if let Some(value) = self.field(object, "exitCode", Value::is_i64) {
                    attributes.insert("unisphere.pi.bash.exit_code".into(), value.clone());
                }
                return self.text_fields(
                    object,
                    "bash_execution",
                    &["command", "output", "fullOutputPath"],
                    attributes,
                );
            }
            "branchSummary" | "compactionSummary" => {
                self.strings(
                    object,
                    attributes,
                    &[("fromId", "unisphere.pi.branch.from.id")],
                );
                self.tokens_before(object, attributes);
                return self.text_fields(object, role, &["summary"], attributes);
            }
            "user" | "assistant" | "toolResult" => {}
            _ => {
                self.diagnostic(Code::UnsupportedRecord);
                return None;
            }
        }
        attributes.insert(
            "unisphere.message.role".into(),
            json!(if role == "toolResult" { "tool" } else { role }),
        );
        if role == "assistant" {
            self.strings(
                object,
                attributes,
                &[
                    ("api", "unisphere.pi.api"),
                    ("provider", "gen_ai.provider.name"),
                    ("model", "gen_ai.request.model"),
                    ("responseModel", "gen_ai.response.model"),
                    ("responseId", "gen_ai.response.id"),
                    ("stopReason", "unisphere.pi.stop_reason"),
                    ("rawStopReason", "unisphere.pi.raw_stop_reason"),
                ],
            );
            self.usage(object.get("usage"), "assistant_message", attributes);
            self.opaque(object, &["diagnostics"], attributes);
        } else if role == "toolResult" {
            self.strings(
                object,
                attributes,
                &[
                    ("toolCallId", "gen_ai.tool.call.id"),
                    ("toolName", "gen_ai.tool.name"),
                ],
            );
            self.flags(
                object,
                attributes,
                &[("isError", "unisphere.tool.is_error")],
            );
            self.usage(object.get("usage"), "tool_execution", attributes);
            self.opaque(object, &["details"], attributes);
            if let Some(names) = self.field(object, "addedToolNames", |value| {
                value
                    .as_array()
                    .is_some_and(|names| names.iter().all(Value::is_string))
            }) {
                attributes.insert("unisphere.pi.added_tool_names".into(), names.clone());
            }
        }
        let error = self.field(object, "errorMessage", Value::is_string);
        if error.is_some() && !self.include_content {
            self.omitted(attributes);
        }
        let parts = self.content(object.get("content"), role, attributes)?;
        let mut body = if role == "toolResult" {
            let mut response = json!({"type": "tool_call_response", "response": parts});
            for (key, attribute) in [
                ("id", "gen_ai.tool.call.id"),
                ("name", "gen_ai.tool.name"),
                ("unisphere.is_error", "unisphere.tool.is_error"),
            ] {
                if let Some(value) = attributes.get(attribute) {
                    response[key] = value.clone();
                }
            }
            json!({"role": "tool", "parts": [response]})
        } else {
            json!({"role": role, "parts": parts})
        };
        if let Some(error) = error {
            body["error"] = error.clone();
        }
        Some(body)
    }

    fn content(
        &mut self,
        value: Option<&Value>,
        role: &str,
        attributes: &mut BTreeMap<String, Value>,
    ) -> Option<Value> {
        let mut parts = Vec::new();
        match value {
            Some(Value::String(text)) if matches!(role, "user" | "custom") => {
                if self.include_content {
                    parts.push(json!({"type": "text", "content": text}));
                }
            }
            Some(Value::Array(values)) => {
                for value in values {
                    if let Some(part) = self.part(value, role, attributes) {
                        parts.push(part);
                    }
                }
            }
            _ => {
                self.diagnostic(Code::InvalidField);
                return None;
            }
        }
        if self.include_content {
            Some(Value::Array(parts))
        } else {
            self.omitted(attributes);
            None
        }
    }

    fn part(
        &mut self,
        value: &Value,
        role: &str,
        attributes: &mut BTreeMap<String, Value>,
    ) -> Option<Value> {
        let Some(object) = value.as_object() else {
            self.diagnostic(Code::InvalidField);
            return None;
        };
        let kind = self.required_string(object, "type")?;
        self.opaque(
            object,
            &["textSignature", "thinkingSignature", "thoughtSignature"],
            attributes,
        );
        match kind {
            "text" => {
                let text = self.required_string(object, "text")?;
                self.include_content
                    .then(|| json!({"type": "text", "content": text}))
            }
            "thinking" if role == "assistant" => {
                let redacted = self
                    .field(object, "redacted", Value::is_boolean)
                    .and_then(Value::as_bool);
                if redacted == Some(true) {
                    self.omitted(attributes);
                    return self
                        .include_content
                        .then(|| json!({"type": "unisphere.redacted_reasoning"}));
                }
                let thinking = self.required_string(object, "thinking")?;
                self.include_content
                    .then(|| json!({"type": "reasoning", "content": thinking}))
            }
            "image" if role != "assistant" => {
                let data = self.required_string(object, "data")?;
                let mime_type = self.required_string(object, "mimeType")?;
                self.include_content
                    .then(|| json!({"type": "image", "mime_type": mime_type, "data": data}))
            }
            "toolCall" if role == "assistant" => {
                let id = self.required_string(object, "id");
                let name = self.required_string(object, "name");
                if let (Some(id), Some(name)) = (id, name) {
                    // Call identity remains available without exporting arguments.
                    let calls = attributes
                        .entry("unisphere.pi.tool.calls".into())
                        .or_insert_with(|| Value::Array(Vec::new()));
                    if let Value::Array(calls) = calls {
                        calls.push(json!({"id": id, "name": name}));
                    }
                }
                if !object.contains_key("arguments") {
                    self.diagnostic(Code::InvalidField);
                }
                let arguments = self.field(object, "arguments", Value::is_object)?;
                let (id, name) = (id?, name?);
                self.include_content.then(
                    || json!({"type": "tool_call", "id": id, "name": name, "arguments": arguments}),
                )
            }
            _ => {
                self.diagnostic(Code::UnsupportedPart);
                self.include_content
                    .then(|| json!({"type": "unisphere.unknown", "native_type": kind}))
            }
        }
    }
}

fn nonnegative_integer(value: &Value) -> bool {
    value.as_i64().is_some_and(|value| value >= 0)
}

fn nonnegative_number(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|value| value.is_finite() && value >= 0.0)
}
