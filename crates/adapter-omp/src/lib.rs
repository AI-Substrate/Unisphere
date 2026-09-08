//! Pure physical-record projection of Oh My Pi JSONL, including its mutable title slot.
//! No source, sidecar, environment, clock, or output access occurs here.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedBatch, MappingDiagnostic,
    MappingDiagnosticCode, MappingOptions, NativeRecord, PipelineError, PipelineErrorKind,
    SessionAdapter, SessionRef, TelemetryRecord,
};

/// Metadata for the Oh My Pi JSONL mapper, not the distinct Pi dialect.
pub const DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "oh-my-pi",
    application: "Oh My Pi",
    description: "Physical Oh My Pi JSONL records; no branch reconstruction or sidecar resolution",
    locations: &[LocationHint {
        platforms: &["unix"],
        base: "home",
        path: ".omp/agent/sessions",
        session_glob: "*/*.jsonl",
        storage_format: "jsonl",
    }],
    capabilities: AdapterCapabilities {
        export_platforms: &["unix"],
        output_formats: &["otlp-jsonl"],
        sdk_caller_owned_cursor: true,
        cursor_source_assumption: "Append-only entries in the same source generation; in-place title-slot changes before the cursor are not refreshed",
        cli_persisted_resume: false,
        delayed_revision_reconciliation: false,
        lossless_archive: false,
    },
};

/// Stateless mapper; malformed JSON/UTF-8 fails the supplied batch atomically.
#[derive(Debug, Clone, Copy, Default)]
pub struct OmpAdapter;

impl SessionAdapter for OmpAdapter {
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
            let mut mapping = Mapping {
                offset: native.offset,
                include_content: options.include_content,
                diagnostics: &mut batch.diagnostics,
            };
            batch
                .records
                .push(mapping.record(value, path, native.bytes.len()));
        }
        Ok(batch)
    }
}

struct Mapping<'a> {
    offset: u64,
    include_content: bool,
    diagnostics: &'a mut Vec<MappingDiagnostic>,
}

type Attributes = BTreeMap<String, Value>;

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

    fn strings(
        &mut self,
        object: &Map<String, Value>,
        attributes: &mut Attributes,
        fields: &[(&str, &str)],
    ) {
        for &(native, profile) in fields {
            if let Some(value) = self.string(object, native) {
                attributes.insert(profile.into(), json!(value));
            }
        }
    }

    fn scalar(
        &mut self,
        object: &Map<String, Value>,
        native: &str,
        profile: &str,
        attributes: &mut Attributes,
        valid: fn(&Value) -> bool,
    ) {
        if let Some(value) = object.get(native) {
            if valid(value) {
                attributes.insert(profile.into(), value.clone());
            } else {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
            }
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

    fn message_timestamp(&mut self, object: &Map<String, Value>, attributes: &mut Attributes) {
        if let Some(value) = object.get("timestamp") {
            match value
                .as_u64()
                .and_then(|value| value.checked_mul(1_000_000))
            {
                Some(value) => {
                    attributes.insert("unisphere.message.timestamp_unix_nano".into(), json!(value));
                }
                None => self.diagnostic(MappingDiagnosticCode::InvalidTimestamp),
            }
        }
    }

    fn omitted(&mut self, attributes: &mut Attributes) {
        if !self.include_content {
            attributes.insert("unisphere.content.omitted".into(), json!(true));
            self.diagnostic(MappingDiagnosticCode::ContentOmitted);
        }
    }

    fn opaque(&mut self, object: &Map<String, Value>, keys: &[&str]) {
        for key in keys {
            if object.get(*key).is_some_and(|value| !value.is_null()) {
                self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
            }
        }
    }

    fn record(&mut self, value: Value, path: &str, byte_len: usize) -> TelemetryRecord {
        let mut attributes = BTreeMap::from([
            ("unisphere.profile.version".into(), json!(1)),
            ("unisphere.source.adapter".into(), json!(DESCRIPTOR.id)),
            ("unisphere.source.path".into(), json!(path)),
            ("unisphere.source.offset".into(), json!(self.offset)),
        ]);
        let mut object = match value {
            Value::Object(value) => value,
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                Map::new()
            }
        };
        let kind = match object.remove("type") {
            Some(Value::String(value)) => value,
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                "unknown".into()
            }
        };
        attributes.insert("unisphere.source.kind".into(), json!(kind));
        self.strings(
            &object,
            &mut attributes,
            &[
                ("id", "unisphere.source.record.id"),
                ("parentId", "unisphere.source.parent.id"),
            ],
        );
        let timestamp_unix_nano = self.timestamp(object.get(if kind == "title" {
            "updatedAt"
        } else {
            "timestamp"
        }));
        let body = match kind.as_str() {
            "message" => self.message(object.remove("message"), &mut attributes),
            "title" => {
                attributes.insert("unisphere.source.mutable".into(), json!(true));
                if self.offset != 0
                    || byte_len != 255
                    || !object.get("pad").is_some_and(Value::is_string)
                {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                }
                if object.get("v") != Some(&json!(1)) {
                    self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
                    None
                } else {
                    self.strings(
                        &object,
                        &mut attributes,
                        &[("source", "unisphere.title.source")],
                    );
                    self.source_event(&kind, &mut object, &mut attributes, &["title"])
                }
            }
            "session" => {
                self.strings(
                    &object,
                    &mut attributes,
                    &[("id", "gen_ai.conversation.id")],
                );
                self.scalar(
                    &object,
                    "version",
                    "unisphere.source.version",
                    &mut attributes,
                    count,
                );
                if object.get("version") != Some(&json!(3)) {
                    self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
                    None
                } else {
                    self.source_event(
                        &kind,
                        &mut object,
                        &mut attributes,
                        &["title", "cwd", "parentSession"],
                    )
                }
            }
            "model_change" => {
                self.strings(
                    &object,
                    &mut attributes,
                    &[
                        ("model", "unisphere.model.selection"),
                        ("role", "unisphere.model.role"),
                    ],
                );
                None
            }
            "thinking_level_change" => {
                for (native, profile) in [
                    ("thinkingLevel", "unisphere.thinking.level"),
                    ("configured", "unisphere.thinking.configured"),
                ] {
                    self.scalar(&object, native, profile, &mut attributes, |value| {
                        value.is_null() || value.is_string()
                    });
                }
                None
            }
            "service_tier_change" => {
                match object.get("serviceTier") {
                    Some(Value::Null) => {
                        attributes.insert("unisphere.service_tier.cleared".into(), json!(true));
                    }
                    Some(Value::Object(tiers)) => {
                        for family in ["openai", "anthropic", "google"] {
                            self.scalar(
                                tiers,
                                family,
                                &format!("unisphere.service_tier.{family}"),
                                &mut attributes,
                                |value| {
                                    matches!(
                                        value.as_str(),
                                        Some("auto" | "default" | "flex" | "scale" | "priority")
                                    )
                                },
                            );
                        }
                    }
                    Some(Value::String(tier)) => {
                        attributes.insert("unisphere.service_tier.legacy".into(), json!(tier));
                    }
                    _ => self.diagnostic(MappingDiagnosticCode::InvalidField),
                }
                None
            }
            "compaction" | "branch_summary" => {
                self.strings(
                    &object,
                    &mut attributes,
                    &[
                        (
                            "firstKeptEntryId",
                            "unisphere.compaction.first_kept_entry.id",
                        ),
                        ("fromId", "unisphere.branch.from.id"),
                    ],
                );
                self.scalar(
                    &object,
                    "tokensBefore",
                    "unisphere.compaction.tokens_before",
                    &mut attributes,
                    count,
                );
                self.scalar(
                    &object,
                    "fromExtension",
                    "unisphere.source.from_extension",
                    &mut attributes,
                    Value::is_boolean,
                );
                self.opaque(&object, &["details", "preserveData"]);
                self.source_event(
                    &kind,
                    &mut object,
                    &mut attributes,
                    &["summary", "shortSummary", "warning"],
                )
            }
            "custom" | "mode_change" => {
                self.strings(
                    &object,
                    &mut attributes,
                    &[
                        ("customType", "unisphere.custom.type"),
                        ("mode", "unisphere.mode"),
                    ],
                );
                self.opaque(&object, &["data"]);
                None
            }
            "custom_message" => {
                self.strings(
                    &object,
                    &mut attributes,
                    &[("customType", "unisphere.custom.type")],
                );
                self.scalar(
                    &object,
                    "display",
                    "unisphere.message.display",
                    &mut attributes,
                    Value::is_boolean,
                );
                self.opaque(&object, &["details", "attribution"]);
                self.content_event(&kind, object.remove("content"), &mut attributes)
            }
            "label" => {
                self.strings(
                    &object,
                    &mut attributes,
                    &[("targetId", "unisphere.label.target.id")],
                );
                self.source_event(&kind, &mut object, &mut attributes, &["label"])
            }
            "title_change" => {
                self.strings(
                    &object,
                    &mut attributes,
                    &[("source", "unisphere.title.source")],
                );
                self.source_event(
                    &kind,
                    &mut object,
                    &mut attributes,
                    &["title", "previousTitle", "trigger"],
                )
            }
            "session_init" => {
                self.opaque(&object, &["outputSchema"]);
                self.strings(
                    &object,
                    &mut attributes,
                    &[("outputSchemaMode", "unisphere.session_init.schema_mode")],
                );
                for key in ["restrictToolNames", "readSummarize"] {
                    self.scalar(
                        &object,
                        key,
                        &format!("unisphere.session_init.{key}"),
                        &mut attributes,
                        Value::is_boolean,
                    );
                }
                let mut body = self.source_event(
                    &kind,
                    &mut object,
                    &mut attributes,
                    &["systemPrompt", "task", "spawns"],
                );
                self.body_field(&mut object, "tools", &mut body, string_array);
                body
            }
            "ttsr_injection" => {
                let mut body = self.source_event(&kind, &mut object, &mut attributes, &[]);
                self.body_field(&mut object, "injectedRules", &mut body, string_array);
                body
            }
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
                None
            }
        };
        TelemetryRecord {
            event_name: "unisphere.session.record".into(),
            timestamp_unix_nano,
            attributes,
            body,
        }
    }

    fn source_event(
        &mut self,
        kind: &str,
        object: &mut Map<String, Value>,
        attributes: &mut Attributes,
        fields: &[&str],
    ) -> Option<Value> {
        self.omitted(attributes);
        let mut body = self
            .include_content
            .then(|| json!({"type": "unisphere.source_event", "native_type": kind}));
        for key in fields {
            self.body_field(object, key, &mut body, Value::is_string);
        }
        body
    }

    fn body_field(
        &mut self,
        object: &mut Map<String, Value>,
        key: &str,
        body: &mut Option<Value>,
        valid: fn(&Value) -> bool,
    ) {
        if let Some(value) = object.remove(key) {
            if value.is_null() {
                return;
            }
            if !valid(&value) {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
            } else if let Some(body) = body {
                body[key] = value;
            }
        }
    }

    fn content_event(
        &mut self,
        kind: &str,
        content: Option<Value>,
        attributes: &mut Attributes,
    ) -> Option<Value> {
        self.omitted(attributes);
        let parts = self.parts(content);
        self.include_content
            .then(|| json!({"type": "unisphere.source_event", "native_type": kind, "parts": parts}))
    }

    fn message(&mut self, value: Option<Value>, attributes: &mut Attributes) -> Option<Value> {
        let Some(Value::Object(mut message)) = value else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let Some(Value::String(role)) = message.remove("role") else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        attributes.insert("unisphere.message.role".into(), json!(role));
        self.message_timestamp(&message, attributes);
        self.opaque(
            &message,
            &[
                "providerPayload",
                "attribution",
                "retryRecovery",
                "contextSnapshot",
                "stopDetails",
                "toolCallAbortMessages",
            ],
        );
        match role.as_str() {
            "assistant" | "user" | "developer" | "toolResult" => {
                if role == "assistant" {
                    self.strings(
                        &message,
                        attributes,
                        &[
                            ("model", "gen_ai.response.model"),
                            ("provider", "gen_ai.provider.name"),
                            ("api", "unisphere.model.api"),
                            ("upstreamProvider", "unisphere.model.upstream_provider"),
                            ("responseId", "gen_ai.response.id"),
                            ("stopReason", "unisphere.message.stop_reason"),
                        ],
                    );
                    self.usage(message.remove("usage"), attributes);
                    for (native, profile) in [
                        ("duration", "unisphere.message.duration_ms"),
                        ("ttft", "unisphere.message.ttft_ms"),
                        ("errorStatus", "unisphere.message.error_status"),
                        ("errorId", "unisphere.message.error_id"),
                    ] {
                        self.scalar(&message, native, profile, attributes, count);
                    }
                }
                for (native, profile) in [
                    ("synthetic", "unisphere.message.synthetic"),
                    ("steering", "unisphere.message.steering"),
                    ("useless", "unisphere.message.useless"),
                ] {
                    self.scalar(&message, native, profile, attributes, Value::is_boolean);
                }
                self.scalar(
                    &message,
                    "prunedAt",
                    "unisphere.message.pruned_at_ms",
                    attributes,
                    count,
                );
                self.omitted(attributes);
                let parts = self.parts(message.remove("content"));
                let mut body = self.include_content.then(|| json!({"role": if role == "toolResult" { "tool" } else { &role }, "parts": parts}));
                if role == "toolResult" {
                    self.strings(
                        &message,
                        attributes,
                        &[
                            ("toolCallId", "gen_ai.tool.call.id"),
                            ("toolName", "gen_ai.tool.name"),
                        ],
                    );
                    self.scalar(
                        &message,
                        "isError",
                        "unisphere.tool.is_error",
                        attributes,
                        Value::is_boolean,
                    );
                    if let Some(body) = &mut body {
                        let response = body["parts"].take();
                        body["parts"] =
                            json!([{"type": "tool_call_response", "response": response}]);
                        for (native, key) in [
                            ("toolCallId", "id"),
                            ("toolName", "name"),
                            ("isError", "unisphere.is_error"),
                        ] {
                            if let Some(value) = message.remove(native).filter(|value| {
                                if native == "isError" {
                                    value.is_boolean()
                                } else {
                                    value.is_string()
                                }
                            }) {
                                body["parts"][0][key] = value;
                            }
                        }
                    }
                    self.references(message.remove("details"), &mut body);
                }
                self.body_field(&mut message, "errorMessage", &mut body, Value::is_string);
                body
            }
            "bashExecution" | "pythonExecution" => {
                for key in ["cancelled", "truncated", "excludeFromContext"] {
                    self.scalar(
                        &message,
                        key,
                        &format!("unisphere.execution.{key}"),
                        attributes,
                        Value::is_boolean,
                    );
                }
                self.scalar(
                    &message,
                    "exitCode",
                    "unisphere.execution.exit_code",
                    attributes,
                    |value| value.is_null() || value.as_i64().is_some(),
                );
                let mut body = self.source_event(
                    &role,
                    &mut message,
                    attributes,
                    &["command", "code", "output"],
                );
                self.references(
                    message.remove("meta").map(|meta| json!({"meta": meta})),
                    &mut body,
                );
                body
            }
            "custom" | "hookMessage" => {
                self.strings(
                    &message,
                    attributes,
                    &[("customType", "unisphere.custom.type")],
                );
                self.scalar(
                    &message,
                    "display",
                    "unisphere.message.display",
                    attributes,
                    Value::is_boolean,
                );
                self.opaque(&message, &["details"]);
                self.content_event(&role, message.remove("content"), attributes)
            }
            "compactionSummary" | "branchSummary" => {
                self.strings(
                    &message,
                    attributes,
                    &[("fromId", "unisphere.branch.from.id")],
                );
                self.scalar(
                    &message,
                    "tokensBefore",
                    "unisphere.compaction.tokens_before",
                    attributes,
                    count,
                );
                self.opaque(&message, &["blocks", "images"]);
                self.source_event(
                    &role,
                    &mut message,
                    attributes,
                    &["summary", "shortSummary", "warning"],
                )
            }
            "fileMention" => {
                let mut body = self.source_event(&role, &mut message, attributes, &[]);
                let Some(Value::Array(files)) = message.remove("files") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return body;
                };
                let mut retained = Vec::new();
                for file in files {
                    let Value::Object(mut file) = file else {
                        self.diagnostic(MappingDiagnosticCode::InvalidField);
                        continue;
                    };
                    let mut item = self.include_content.then(|| json!({}));
                    for key in ["path", "content", "skippedReason"] {
                        self.body_field(&mut file, key, &mut item, Value::is_string);
                    }
                    for key in ["lineCount", "byteSize"] {
                        self.body_field(&mut file, key, &mut item, count);
                    }
                    if let Some(image) = file.remove("image").and_then(|image| self.part(image))
                        && let Some(item) = &mut item
                    {
                        item["image"] = image;
                    }
                    if let Some(item) = item {
                        retained.push(item);
                    }
                }
                if let Some(body) = &mut body {
                    body["files"] = Value::Array(retained);
                }
                body
            }
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
                None
            }
        }
    }

    fn usage(&mut self, value: Option<Value>, attributes: &mut Attributes) {
        let Some(value) = value else { return };
        let Value::Object(mut usage) = value else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return;
        };
        let mut retained = false;
        for (native, profile) in [
            ("input", "input_tokens"),
            ("output", "output_tokens"),
            ("cacheRead", "cache_read_input_tokens"),
            ("cacheWrite", "cache_creation_input_tokens"),
            ("totalTokens", "total_tokens"),
            ("reasoningTokens", "reasoning_tokens"),
            ("premiumRequests", "premium_requests"),
        ] {
            if let Some(value) = usage.remove(native) {
                let valid = if native == "premiumRequests" {
                    measured(&value)
                } else {
                    count(&value)
                };
                if valid {
                    attributes.insert(format!("unisphere.usage.{profile}"), value);
                    retained = true;
                } else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                }
            }
        }
        for (native, prefix, fields) in [
            (
                "orchestration",
                "orchestration",
                &["input", "output", "cacheRead"][..],
            ),
            (
                "cttl",
                "cache_write_ttl",
                &["ephemeral5m", "ephemeral1h"][..],
            ),
            ("server", "server_requests", &["webSearch", "webFetch"][..]),
            (
                "cost",
                "cost",
                &["input", "output", "cacheRead", "cacheWrite", "total"][..],
            ),
        ] {
            if let Some(value) = usage.remove(native) {
                let Value::Object(mut components) = value else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    continue;
                };
                for key in fields {
                    if let Some(value) = components.remove(*key) {
                        let valid = if native == "cost" {
                            measured(&value)
                        } else {
                            count(&value)
                        };
                        if valid {
                            attributes.insert(format!("unisphere.usage.{prefix}.{key}"), value);
                            retained = true;
                        } else {
                            self.diagnostic(MappingDiagnosticCode::InvalidField);
                        }
                    }
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

    fn parts(&mut self, content: Option<Value>) -> Vec<Value> {
        match content {
            Some(Value::String(text)) => {
                if self.include_content {
                    vec![json!({"type": "text", "content": text})]
                } else {
                    Vec::new()
                }
            }
            Some(Value::Array(parts)) => parts
                .into_iter()
                .filter_map(|part| self.part(part))
                .collect(),
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                Vec::new()
            }
        }
    }

    fn part(&mut self, value: Value) -> Option<Value> {
        let Value::Object(mut part) = value else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let Some(Value::String(kind)) = part.remove("type") else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        self.opaque(
            &part,
            &["textSignature", "thinkingSignature", "thoughtSignature"],
        );
        match kind.as_str() {
            "text" | "thinking" => {
                let Some(Value::String(text)) =
                    part.remove(if kind == "text" { "text" } else { "thinking" })
                else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                self.include_content.then(|| json!({"type": if kind == "text" { "text" } else { "reasoning" }, "content": text}))
            }
            "toolCall" => {
                let Some(Value::String(id)) = part.remove("id") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                let Some(Value::String(name)) = part.remove("name") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                let Some(arguments @ Value::Object(_)) = part.remove("arguments") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                let mut body = self.include_content.then(
                    || json!({"type": "tool_call", "id": id, "name": name, "arguments": arguments}),
                );
                for key in ["intent", "rawBlock", "customWireName"] {
                    self.body_field(&mut part, key, &mut body, Value::is_string);
                }
                body
            }
            "image" => {
                let Some(Value::String(data)) = part.remove("data") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                let Some(Value::String(mime)) = part.remove("mimeType") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                let reference = data.starts_with("blob:sha256:");
                if reference {
                    self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                }
                let mut body = self.include_content.then(|| if reference {
                    json!({"type": "unisphere.reference", "uri": data, "mime_type": mime, "resolved": false})
                } else {
                    json!({"type": "image", "data": data, "mime_type": mime})
                });
                self.body_field(&mut part, "detail", &mut body, Value::is_string);
                body
            }
            "fallback" => {
                let from = part
                    .get("from")
                    .and_then(Value::as_object)
                    .and_then(|value| value.get("model"))
                    .and_then(Value::as_str);
                let to = part
                    .get("to")
                    .and_then(Value::as_object)
                    .and_then(|value| value.get("model"))
                    .and_then(Value::as_str);
                match (from, to) {
                    (Some(from), Some(to)) => self.include_content.then(
                        || json!({"type": "unisphere.model_fallback", "from": from, "to": to}),
                    ),
                    _ => {
                        self.diagnostic(MappingDiagnosticCode::InvalidField);
                        None
                    }
                }
            }
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                self.include_content
                    .then(|| json!({"type": "unisphere.unknown", "native_type": kind}))
            }
        }
    }

    // Retain documented artifact/source locators, never arbitrary tool details.
    fn references(&mut self, value: Option<Value>, body: &mut Option<Value>) {
        let Some(value) = value else { return };
        self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
        let Some(meta) = value.get("meta").and_then(Value::as_object) else {
            return;
        };
        let mut references = Vec::new();
        if let Some(value) = meta
            .get("truncation")
            .and_then(|value| value.get("artifactId"))
        {
            if let Some(id) = value.as_str() {
                if self.include_content {
                    references.push(json!({"type": "unisphere.reference", "artifact_id": id, "resolved": false}));
                }
            } else {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
            }
        }
        if let Some(source) = meta.get("source") {
            match (
                source.get("type").and_then(Value::as_str),
                source.get("value").and_then(Value::as_str),
            ) {
                (Some(kind @ ("path" | "url" | "internal")), Some(value)) => {
                    if self.include_content {
                        references.push(json!({"type": "unisphere.reference", "native_type": kind, "value": value, "resolved": false}));
                    }
                }
                _ => self.diagnostic(MappingDiagnosticCode::InvalidField),
            }
        }
        if !references.is_empty()
            && let Some(body) = body
        {
            body["unisphere.references"] = Value::Array(references);
        }
    }
}

fn count(value: &Value) -> bool {
    value.as_i64().is_some_and(|value| value >= 0)
}

fn measured(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|value| value.is_finite() && value >= 0.0)
}

fn string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.iter().all(Value::is_string))
}
