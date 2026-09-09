//! Pure projections of caller-supplied Copilot CLI events and legacy snapshots.
//! No discovery, context replay, attachment loading, or ambient state access.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedBatch, MappingDiagnostic,
    MappingDiagnosticCode, MappingOptions, NativeRecord, PipelineError, PipelineErrorKind,
    SessionAdapter, SessionRef, TelemetryRecord,
};

mod snapshot;
pub use snapshot::{CopilotCliAdapterSnapshot, SNAPSHOT_DESCRIPTOR};

/// The JSONL registration only; legacy monolithic JSON needs a separate loader.
pub const DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "copilot-cli",
    application: "GitHub Copilot CLI",
    description: "Physical Copilot CLI event JSONL projection; metadata by default, content by explicit opt-in.",
    locations: &[LocationHint {
        platforms: &["macos", "linux", "windows"],
        base: "home",
        path: ".copilot/session-state",
        session_glob: "*/events.jsonl",
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

/// Stateless mapping of supplied native events, never a reconstructed conversation.
#[derive(Debug, Clone, Copy, Default)]
pub struct CopilotCliAdapter;

impl SessionAdapter for CopilotCliAdapter {
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
            let value: Value = serde_json::from_slice(&native.bytes).map_err(|_| {
                PipelineError::new(PipelineErrorKind::InvalidData, Some(native.offset))
            })?;
            let mut diagnostic = |code| {
                batch.diagnostics.push(MappingDiagnostic {
                    offset: native.offset,
                    code,
                })
            };
            let mut mapping = Mapping {
                include_content: options.include_content,
                omitted: false,
                diagnostics: &mut diagnostic,
                attributes: BTreeMap::from([
                    ("unisphere.profile.version".into(), json!(1)),
                    ("unisphere.source.adapter".into(), json!(DESCRIPTOR.id)),
                    ("unisphere.source.path".into(), json!(path)),
                    ("unisphere.source.offset".into(), json!(native.offset)),
                ]),
            };
            batch.records.push(mapping.record(&value));
        }
        Ok(batch)
    }
}

const TOKENS: &[(&str, &str)] = &[
    ("inputTokens", "unisphere.usage.input_tokens"),
    ("outputTokens", "unisphere.usage.output_tokens"),
    ("cacheReadTokens", "unisphere.usage.cache_read_input_tokens"),
    (
        "cacheWriteTokens",
        "unisphere.usage.cache_creation_input_tokens",
    ),
    (
        "reasoningTokens",
        "unisphere.copilot.usage.reasoning_tokens",
    ),
    (
        "acceptedPredictionTokens",
        "unisphere.copilot.usage.accepted_prediction_tokens",
    ),
    (
        "rejectedPredictionTokens",
        "unisphere.copilot.usage.rejected_prediction_tokens",
    ),
];

struct Mapping<'a> {
    include_content: bool,
    omitted: bool,
    diagnostics: &'a mut dyn FnMut(MappingDiagnosticCode),
    attributes: BTreeMap<String, Value>,
}

impl Mapping<'_> {
    fn diagnostic(&mut self, code: MappingDiagnosticCode) {
        (self.diagnostics)(code);
    }

    fn object<'a>(&mut self, value: &'a Value) -> Option<&'a Map<String, Value>> {
        let object = value.as_object();
        if object.is_none() {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
        }
        object
    }

    fn string<'a>(&mut self, data: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
        match data.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text),
            Some(_) => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                None
            }
        }
    }

    fn strings(&mut self, data: &Map<String, Value>, fields: &[(&str, &str)]) {
        for &(native, profile) in fields {
            if let Some(text) = self.string(data, native) {
                self.attributes.insert(profile.into(), json!(text));
            }
        }
    }

    fn number(&mut self, value: &Value, fractional: bool) -> Option<Value> {
        if value.as_i64().is_some_and(|number| number >= 0)
            || (fractional
                && value.is_f64()
                && value.as_f64().is_some_and(|n| n.is_finite() && n >= 0.0))
        {
            Some(value.clone())
        } else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            None
        }
    }

    fn numbers(&mut self, data: &Map<String, Value>, fields: &[(&str, &str)], fractional: bool) {
        for &(native, profile) in fields {
            if let Some(value) = data.get(native)
                && let Some(value) = self.number(value, fractional)
            {
                self.attributes.insert(profile.into(), value);
            }
        }
    }

    fn boolean(&mut self, data: &Map<String, Value>, native: &str, profile: &str) {
        if let Some(value) = data.get(native) {
            if value.is_boolean() {
                self.attributes.insert(profile.into(), value.clone());
            } else {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
            }
        }
    }

    fn content(&mut self) -> bool {
        if !self.include_content && !self.omitted {
            self.omitted = true;
            self.attributes
                .insert("unisphere.content.omitted".into(), json!(true));
            self.diagnostic(MappingDiagnosticCode::ContentOmitted);
        }
        self.include_content
    }

    fn unsupported_fields(&mut self, data: &Map<String, Value>, fields: &[&str]) {
        for field in fields {
            if data.get(*field).is_some_and(|value| !value.is_null()) {
                self.content();
                self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
            }
        }
    }

    fn record(&mut self, value: &Value) -> TelemetryRecord {
        let mut timestamp_unix_nano = None;
        let mut body = None;
        if let Some(event) = self.object(value) {
            let kind = self.string(event, "type").unwrap_or("unknown");
            if !event.get("type").is_some_and(Value::is_string) {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
            }
            self.attributes
                .insert("unisphere.source.kind".into(), json!(kind));
            self.strings(
                event,
                &[
                    ("id", "unisphere.source.record.id"),
                    ("parentId", "unisphere.source.parent.id"),
                    ("agentId", "unisphere.copilot.agent.id"),
                ],
            );
            self.boolean(event, "ephemeral", "unisphere.copilot.ephemeral");
            if let Some(timestamp) = event.get("timestamp") {
                timestamp_unix_nano = timestamp
                    .as_str()
                    .and_then(|text| OffsetDateTime::parse(text, &Rfc3339).ok())
                    .and_then(|time| u64::try_from(time.unix_timestamp_nanos()).ok());
                if timestamp_unix_nano.is_none() {
                    self.diagnostic(MappingDiagnosticCode::InvalidTimestamp);
                }
            }
            if let Some(data) = event.get("data").and_then(|value| self.object(value)) {
                body = self.data(kind, data);
            } else if !event.contains_key("data") {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
            }
        } else {
            self.attributes
                .insert("unisphere.source.kind".into(), json!("unknown"));
            self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
        }
        TelemetryRecord {
            event_name: "unisphere.session.record".into(),
            timestamp_unix_nano,
            attributes: std::mem::take(&mut self.attributes),
            body,
        }
    }

    fn data(&mut self, kind: &str, data: &Map<String, Value>) -> Option<Value> {
        // These identities are record-local. Never populate them from earlier events.
        self.strings(
            data,
            &[
                ("sessionId", "gen_ai.conversation.id"),
                ("messageId", "unisphere.message.id"),
                ("turnId", "unisphere.copilot.turn.id"),
                ("toolCallId", "unisphere.copilot.tool_call.id"),
                ("parentToolCallId", "unisphere.copilot.parent_tool_call.id"),
                ("interactionId", "unisphere.copilot.interaction.id"),
                ("apiCallId", "unisphere.copilot.api_call.id"),
                ("reasoningId", "unisphere.copilot.reasoning.id"),
            ],
        );
        match kind {
            "user.message"
            | "assistant.message"
            | "assistant.reasoning"
            | "assistant.message_delta"
            | "assistant.reasoning_delta"
            | "system.message" => self.message(kind, data),
            "tool.execution_start" => {
                self.strings(data, &[("model", "unisphere.copilot.model")]);
                let part = self.tool_call(data, "toolName");
                self.message_body("assistant", part.into_iter().collect())
            }
            "tool.execution_complete" => self.tool_result(data),
            "tool.execution_partial_result"
            | "tool.execution_progress"
            | "assistant.tool_call_delta" => {
                self.attributes
                    .insert("unisphere.copilot.fragment".into(), json!(true));
                self.native_text(
                    data,
                    kind,
                    &["partialOutput", "progressMessage", "inputDelta", "toolName"],
                )
            }
            "assistant.usage" => {
                self.strings(
                    data,
                    &[
                        ("model", "gen_ai.response.model"),
                        ("providerCallId", "unisphere.copilot.provider_call.id"),
                    ],
                );
                self.usage(data, "api_call");
                None
            }
            "session.start" | "session.resume" => {
                self.strings(
                    data,
                    &[
                        ("selectedModel", "unisphere.copilot.selected_model"),
                        ("copilotVersion", "unisphere.copilot.version"),
                        ("producer", "unisphere.copilot.producer"),
                        ("startTime", "unisphere.copilot.session.start_time"),
                        ("resumeTime", "unisphere.copilot.session.resume_time"),
                        (
                            "detachedFromSpawningParentSessionId",
                            "unisphere.copilot.parent_session.id",
                        ),
                    ],
                );
                self.numbers(
                    data,
                    &[
                        ("version", "unisphere.copilot.schema_version"),
                        ("eventCount", "unisphere.copilot.event_count"),
                    ],
                    false,
                );
                data.get("context")
                    .and_then(|value| self.object(value))
                    .and_then(|context| self.context(context))
            }
            "session.context_changed" => self.context(data),
            "session.model_change" => {
                self.strings(
                    data,
                    &[
                        ("newModel", "unisphere.copilot.selected_model"),
                        ("previousModel", "unisphere.copilot.previous_model"),
                        ("reasoningEffort", "unisphere.copilot.reasoning_effort"),
                    ],
                );
                None
            }
            "session.usage_checkpoint" | "session.shutdown" => {
                self.strings(
                    data,
                    &[
                        ("currentModel", "unisphere.copilot.selected_model"),
                        ("shutdownType", "unisphere.copilot.shutdown_type"),
                    ],
                );
                self.usage(
                    data,
                    if kind == "session.shutdown" {
                        "session_shutdown"
                    } else {
                        "session_checkpoint"
                    },
                );
                self.context_usage(data);
                if let Some(value) = data.get("modelMetrics")
                    && let Some(metrics) = self.model_metrics(value)
                {
                    self.attributes
                        .insert("unisphere.copilot.model_metrics".into(), metrics);
                }
                if let Some(value) = data.get("agentMetrics")
                    && let Some(metrics) = self.agent_metrics(value)
                {
                    self.attributes
                        .insert("unisphere.copilot.agent_metrics".into(), metrics);
                }
                self.unsupported_fields(
                    data,
                    &["tokenDetails", "modelCacheState", "promptCacheBreakState"],
                );
                self.native_text(data, kind, &["errorReason"])
            }
            "session.usage_info" => {
                self.context_usage(data);
                None
            }
            "session.compaction_start" => None,
            "session.compaction_complete" => {
                self.boolean(data, "success", "unisphere.copilot.success");
                self.numbers(
                    data,
                    &[
                        (
                            "preCompactionTokens",
                            "unisphere.copilot.compaction.tokens_before",
                        ),
                        (
                            "postCompactionTokens",
                            "unisphere.copilot.compaction.tokens_after",
                        ),
                        (
                            "tokensRemoved",
                            "unisphere.copilot.compaction.tokens_removed",
                        ),
                        (
                            "messagesRemoved",
                            "unisphere.copilot.compaction.messages_removed",
                        ),
                        (
                            "checkpointNumber",
                            "unisphere.copilot.compaction.checkpoint",
                        ),
                    ],
                    false,
                );
                if let Some(value) = data.get("compactionTokensUsed")
                    && let Some(usage) = self.object(value)
                {
                    self.usage(usage, "compaction_api_call");
                    self.strings(usage, &[("model", "unisphere.copilot.compaction.model")]);
                }
                self.native_text(
                    data,
                    kind,
                    &[
                        "summaryContent",
                        "customInstructions",
                        "checkpointPath",
                        "error",
                    ],
                )
            }
            "assistant.turn_start" | "assistant.turn_end" | "assistant.message_start" => {
                self.strings(data, &[("model", "unisphere.copilot.model")]);
                None
            }
            "subagent.started"
            | "subagent.configured"
            | "subagent.completed"
            | "subagent.failed"
            | "subagent.selected"
            | "subagent.deselected" => {
                self.strings(
                    data,
                    &[
                        ("agentName", "unisphere.copilot.agent.name"),
                        ("parentId", "unisphere.copilot.agent.parent_id"),
                        ("model", "unisphere.copilot.model"),
                        (
                            "firstDispatchedModel",
                            "unisphere.copilot.agent.first_dispatched_model",
                        ),
                        (
                            "configuredModelPreference",
                            "unisphere.copilot.agent.configured_model",
                        ),
                    ],
                );
                self.boolean(data, "cancelled", "unisphere.copilot.cancelled");
                if data.contains_key("totalTokens") {
                    self.numbers(
                        data,
                        &[("totalTokens", "unisphere.copilot.usage.total_tokens")],
                        false,
                    );
                    self.attributes.insert(
                        "unisphere.usage.scope".into(),
                        json!("subagent_reported_total"),
                    );
                }
                self.native_text(
                    data,
                    kind,
                    &["agentDisplayName", "agentDescription", "error"],
                )
            }
            "session.error"
            | "session.info"
            | "session.warning"
            | "session.title_changed"
            | "assistant.intent"
            | "abort" => self.native_text(data, kind, &["message", "title", "intent", "reason"]),
            "session.idle" | "assistant.idle" => None,
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
                if !data.is_empty() {
                    self.content();
                }
                None
            }
        }
    }

    fn usage(&mut self, data: &Map<String, Value>, scope: &str) {
        self.attributes
            .insert("unisphere.usage.scope".into(), json!(scope));
        self.numbers(data, TOKENS, false);
        self.numbers(
            data,
            &[("totalNanoAiu", "unisphere.copilot.usage.total_nano_aiu")],
            false,
        );
        self.numbers(
            data,
            &[
                ("cost", "unisphere.copilot.usage.cost_multiplier"),
                ("duration", "unisphere.copilot.usage.duration_ms"),
                (
                    "totalApiDurationMs",
                    "unisphere.copilot.usage.total_api_duration_ms",
                ),
                (
                    "totalPremiumRequests",
                    "unisphere.copilot.usage.total_premium_requests",
                ),
            ],
            true,
        );
        if let Some(value) = data.get("copilotUsage")
            && let Some(usage) = self.object(value)
        {
            self.numbers(
                usage,
                &[("totalNanoAiu", "unisphere.copilot.usage.total_nano_aiu")],
                false,
            );
            self.unsupported_fields(usage, &["tokenDetails"]);
        }
    }

    fn context_usage(&mut self, data: &Map<String, Value>) {
        self.numbers(
            data,
            &[
                ("currentTokens", "unisphere.copilot.context.current_tokens"),
                (
                    "conversationTokens",
                    "unisphere.copilot.context.conversation_tokens",
                ),
                ("systemTokens", "unisphere.copilot.context.system_tokens"),
                (
                    "toolDefinitionsTokens",
                    "unisphere.copilot.context.tool_definition_tokens",
                ),
                ("tokenLimit", "unisphere.copilot.context.token_limit"),
                ("messagesLength", "unisphere.copilot.context.message_count"),
            ],
            false,
        );
    }

    fn model_metrics(&mut self, value: &Value) -> Option<Value> {
        let metrics = self.object(value)?;
        let mut models = Map::new();
        for (model, value) in metrics {
            let Some(metric) = self.object(value) else {
                continue;
            };
            let mut retained = Map::new();
            for (field, allowed, fractional) in [
                (
                    "usage",
                    &[
                        "inputTokens",
                        "outputTokens",
                        "cacheReadTokens",
                        "cacheWriteTokens",
                        "reasoningTokens",
                    ][..],
                    false,
                ),
                ("requests", &["count", "cost"][..], true),
            ] {
                if let Some(value) = metric.get(field)
                    && let Some(values) = self.object(value)
                {
                    let mut selected = Map::new();
                    for key in allowed {
                        if let Some(value) = values.get(*key)
                            && let Some(value) = self.number(value, fractional && *key == "cost")
                        {
                            selected.insert((*key).into(), value);
                        }
                    }
                    if !selected.is_empty() {
                        retained.insert(field.into(), Value::Object(selected));
                    }
                }
            }
            if let Some(value) = metric.get("totalNanoAiu")
                && let Some(value) = self.number(value, false)
            {
                retained.insert("totalNanoAiu".into(), value);
            }
            self.unsupported_fields(metric, &["tokenDetails"]);
            if !retained.is_empty() {
                models.insert(model.clone(), Value::Object(retained));
            }
        }
        Some(Value::Object(models))
    }

    fn agent_metrics(&mut self, value: &Value) -> Option<Value> {
        let metrics = self.object(value)?;
        let mut agents = Map::new();
        for (agent, value) in metrics {
            let Some(metric) = self.object(value) else {
                continue;
            };
            let mut retained = Map::new();
            if let Some(name) = self.string(metric, "agentName") {
                retained.insert("agentName".into(), json!(name));
            }
            for key in ["totalNanoAiu", "totalApiDurationMs"] {
                if let Some(value) = metric.get(key)
                    && let Some(value) = self.number(value, key == "totalApiDurationMs")
                {
                    retained.insert(key.into(), value);
                }
            }
            if let Some(value) = metric.get("modelMetrics")
                && let Some(models) = self.model_metrics(value)
            {
                retained.insert("modelMetrics".into(), models);
            }
            // Display names can contain the full delegated prompt, not a stable name.
            if metric.contains_key("agentDisplayName") {
                self.content();
                self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
            }
            agents.insert(agent.clone(), Value::Object(retained));
        }
        Some(Value::Object(agents))
    }

    fn context(&mut self, data: &Map<String, Value>) -> Option<Value> {
        self.boolean(
            data,
            "pendingGitContext",
            "unisphere.copilot.context.pending_git",
        );
        self.native_text(
            data,
            "session.context",
            &[
                "cwd",
                "gitRoot",
                "branch",
                "repository",
                "repositoryHost",
                "headCommit",
                "baseCommit",
            ],
        )
    }

    fn native_text(
        &mut self,
        data: &Map<String, Value>,
        kind: &str,
        fields: &[&str],
    ) -> Option<Value> {
        let mut selected = Map::new();
        for key in fields {
            if let Some(text) = self.string(data, key)
                && self.content()
            {
                selected.insert((*key).into(), json!(text));
            }
        }
        (!selected.is_empty()).then(|| json!({"type": kind, "data": selected}))
    }

    fn text_part(
        &mut self,
        data: &Map<String, Value>,
        key: &str,
        kind: &str,
        parts: &mut Vec<Value>,
    ) {
        if let Some(text) = self.string(data, key)
            && self.content()
        {
            parts.push(json!({"type": kind, "content": text}));
        }
    }

    fn message_body(&mut self, role: &str, parts: Vec<Value>) -> Option<Value> {
        self.attributes
            .insert("unisphere.message.role".into(), json!(role));
        (self.include_content && !parts.is_empty()).then(|| json!({"role": role, "parts": parts}))
    }

    fn message(&mut self, kind: &str, data: &Map<String, Value>) -> Option<Value> {
        let role = match kind {
            "user.message" => "user",
            "system.message" => match self.string(data, "role") {
                Some(role @ ("system" | "developer")) => role,
                _ => {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                }
            },
            _ => "assistant",
        };
        let delta = matches!(
            kind,
            "assistant.message_delta" | "assistant.reasoning_delta"
        );
        let reasoning = matches!(kind, "assistant.reasoning" | "assistant.reasoning_delta");
        if delta {
            self.attributes
                .insert("unisphere.copilot.fragment".into(), json!(true));
        }
        if kind == "assistant.message" {
            self.strings(data, &[("model", "gen_ai.response.model")]);
            self.numbers(
                data,
                &[
                    ("chunkIndex", "unisphere.copilot.chunk.index"),
                    ("chunkCount", "unisphere.copilot.chunk.count"),
                ],
                false,
            );
            if data.contains_key("outputTokens") {
                self.numbers(
                    data,
                    &[("outputTokens", "unisphere.usage.output_tokens")],
                    false,
                );
                self.attributes
                    .insert("unisphere.usage.scope".into(), json!("assistant_message"));
            }
        }
        let mut parts = Vec::new();
        let content_key = if delta { "deltaContent" } else { "content" };
        if !data.contains_key(content_key) {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
        }
        self.text_part(
            data,
            content_key,
            if reasoning { "reasoning" } else { "text" },
            &mut parts,
        );
        self.text_part(data, "reasoningText", "reasoning", &mut parts);
        self.text_part(
            data,
            "transformedContent",
            "unisphere.transformed_text",
            &mut parts,
        );
        if let Some(value) = data.get("toolRequests") {
            if let Some(requests) = value.as_array() {
                for request in requests {
                    if let Some(request) = self.object(request)
                        && let Some(part) = self.tool_call(request, "name")
                    {
                        parts.push(part);
                    }
                }
            } else {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
            }
        }
        if let Some(value) = data.get("attachments") {
            self.attachments(value, &mut parts);
        }
        self.unsupported_fields(
            data,
            &[
                "encryptedContent",
                "reasoningOpaque",
                "reasoningBlocks",
                "serverTools",
                "citations",
            ],
        );
        self.message_body(role, parts)
    }

    fn tool_call(&mut self, data: &Map<String, Value>, name_key: &str) -> Option<Value> {
        let id = self.string(data, "toolCallId");
        let name = self.string(data, name_key);
        if id.is_none() || name.is_none() {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
        }
        if !self.content() {
            return None;
        }
        let mut part = json!({"type": "tool_call", "id": id?, "name": name?});
        if let Some(arguments) = data.get("arguments") {
            part["arguments"] = arguments.clone();
        }
        Some(part)
    }

    fn tool_result(&mut self, data: &Map<String, Value>) -> Option<Value> {
        self.strings(data, &[("model", "unisphere.copilot.model")]);
        self.boolean(data, "success", "unisphere.copilot.success");
        let id = self.string(data, "toolCallId");
        let mut response = Map::new();
        if let Some(value) = data.get("result")
            && let Some(result) = self.object(value)
        {
            for key in ["content", "detailedContent"] {
                if let Some(text) = self.string(result, key)
                    && self.content()
                {
                    response.insert(key.into(), json!(text));
                }
            }
            if let Some(value) = result.get("structuredContent")
                && self.content()
            {
                response.insert("structuredContent".into(), value.clone());
            }
            if let Some(value) = result.get("contents") {
                let parts = self.result_parts(value);
                if self.include_content {
                    response.insert("contents".into(), Value::Array(parts));
                }
            }
            self.unsupported_fields(
                result,
                &[
                    "binaryResultsForLlm",
                    "citableSources",
                    "uiResource",
                    "mcpMeta",
                ],
            );
        }
        if let Some(value) = data.get("error")
            && let Some(error) = self.object(value)
            && let Some(body) = self.native_text(error, "error", &["message", "code"])
        {
            response.insert("error".into(), body["data"].clone());
        }
        if id.is_none() || !data.get("success").is_some_and(Value::is_boolean) {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
        }
        let mut parts = Vec::new();
        if self.include_content && !response.is_empty() {
            let mut part = json!({"type": "tool_call_response", "response": response});
            if let Some(id) = id {
                part["id"] = json!(id);
            }
            if let Some(success) = data.get("success").and_then(Value::as_bool) {
                part["unisphere.is_error"] = json!(!success);
            }
            parts.push(part);
        }
        self.message_body("tool", parts)
    }

    fn result_parts(&mut self, value: &Value) -> Vec<Value> {
        self.content();
        let Some(parts) = value.as_array() else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return Vec::new();
        };
        let mut retained = Vec::new();
        for part in parts {
            let Some(part) = self.object(part) else {
                continue;
            };
            match self.string(part, "type") {
                Some("text") => self.text_part(part, "text", "text", &mut retained),
                Some(kind) => {
                    self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                    if self.include_content {
                        retained.push(json!({"type": "unisphere.unknown", "native_type": kind}));
                    }
                }
                None => self.diagnostic(MappingDiagnosticCode::InvalidField),
            }
        }
        retained
    }

    fn attachments(&mut self, value: &Value, parts: &mut Vec<Value>) {
        self.content();
        let Some(attachments) = value.as_array() else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return;
        };
        for value in attachments {
            let Some(attachment) = self.object(value) else {
                continue;
            };
            match self.string(attachment, "type") {
                Some(kind @ ("file" | "directory" | "selection")) => {
                    if let Some(body) = self.native_text(
                        attachment,
                        "unisphere.attachment_reference",
                        &[
                            "path",
                            "filePath",
                            "displayName",
                            "text",
                            "assetId",
                            "mimeType",
                            "taggedFilesEntry",
                        ],
                    ) {
                        parts.push(json!({"type": "unisphere.attachment_reference", "native_type": kind, "reference": body["data"]}));
                    }
                }
                Some(kind) => {
                    self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                    if self.include_content {
                        parts.push(json!({"type": "unisphere.unknown", "native_type": kind}));
                    }
                }
                None => self.diagnostic(MappingDiagnosticCode::InvalidField),
            }
        }
    }
}
