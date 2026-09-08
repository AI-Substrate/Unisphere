//! Pure VS Code chat-session projection from complete caller-supplied snapshots.
//! Journal mutations are reduced before projection. No source, sidecar, clock,
//! environment, network or output is accessed. This is not a lossless archive.
#![forbid(unsafe_code)]

mod journal;

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedSnapshot, MappingDiagnosticCode,
    MappingOptions, NativeSnapshot, PipelineError, PipelineErrorKind, SnapshotAdapter,
    SnapshotDiagnostic, SnapshotFormat, TelemetryRecord,
};

/// Metadata for the snapshot runner registered by the application composition root.
pub const DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "vscode-copilot",
    application: "VS Code Copilot",
    description: "VS Code v1/v2/v3 snapshots with pure native journal replay; current revision projection only, no persistent reconciliation or lossless archive",
    locations: &[
        LocationHint {
            platforms: &["macos"],
            base: "home",
            path: "Library/Application Support/Code/User/workspaceStorage",
            session_glob: "*/chatSessions/*.json",
            storage_format: "json_document",
        },
        LocationHint {
            platforms: &["macos"],
            base: "home",
            path: "Library/Application Support/Code/User/workspaceStorage",
            session_glob: "*/chatSessions/*.jsonl",
            storage_format: "json_journal",
        },
        LocationHint {
            platforms: &["linux"],
            base: "home",
            path: ".config/Code/User/workspaceStorage",
            session_glob: "*/chatSessions/*.json",
            storage_format: "json_document",
        },
        LocationHint {
            platforms: &["linux"],
            base: "home",
            path: ".config/Code/User/workspaceStorage",
            session_glob: "*/chatSessions/*.jsonl",
            storage_format: "json_journal",
        },
        LocationHint {
            platforms: &["windows"],
            base: "appdata",
            path: "Code/User/workspaceStorage",
            session_glob: "*/chatSessions/*.json",
            storage_format: "json_document",
        },
        LocationHint {
            platforms: &["windows"],
            base: "appdata",
            path: "Code/User/workspaceStorage",
            session_glob: "*/chatSessions/*.jsonl",
            storage_format: "json_journal",
        },
    ],
    capabilities: AdapterCapabilities {
        export_platforms: &["linux", "macos"],
        output_formats: &["otlp-jsonl"],
        sdk_caller_owned_cursor: false,
        cursor_source_assumption: "whole_source_revision",
        cli_persisted_resume: false,
        delayed_revision_reconciliation: false,
        lossless_archive: false,
    },
};

/// Stateless mapper for explicitly supplied VS Code native session revisions.
#[derive(Debug, Clone, Copy, Default)]
pub struct VsCodeCopilotAdapter;

impl SnapshotAdapter for VsCodeCopilotAdapter {
    fn name(&self) -> &'static str {
        DESCRIPTOR.id
    }

    fn map_snapshot(
        &self,
        snapshot: &NativeSnapshot,
        options: MappingOptions,
    ) -> Result<MappedSnapshot, PipelineError> {
        snapshot.source.validate()?;
        if snapshot.revision.is_empty() {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        }
        let (value, root_key, format) = match snapshot.source.format {
            SnapshotFormat::JsonDocument => {
                let [record] = snapshot.records.as_slice() else {
                    return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
                };
                if record.key != "document" {
                    return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
                }
                let value = serde_json::from_slice(&record.bytes)
                    .map_err(|_| PipelineError::new(PipelineErrorKind::InvalidData, None))?;
                (value, "document", "json_document")
            }
            SnapshotFormat::JsonJournal => (
                journal::reduce(snapshot)?,
                "journal:reduced",
                "json_journal",
            ),
            SnapshotFormat::SqliteKeyValue { .. } => {
                return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
            }
        };
        let mut mapping = Mapping {
            snapshot,
            options,
            format,
            session_id: None,
            output: MappedSnapshot::default(),
        };
        mapping.session(&value, root_key)?;
        Ok(mapping.output)
    }
}

struct Mapping<'a> {
    snapshot: &'a NativeSnapshot,
    options: MappingOptions,
    format: &'static str,
    session_id: Option<String>,
    output: MappedSnapshot,
}

impl Mapping<'_> {
    fn diagnostic(&mut self, key: &str, code: MappingDiagnosticCode) {
        self.output.diagnostics.push(SnapshotDiagnostic {
            key: key.into(),
            code,
        });
    }

    fn record(&self, key: &str, kind: &str) -> TelemetryRecord {
        let mut attributes = BTreeMap::from([
            ("unisphere.profile.version".into(), json!(1)),
            ("unisphere.source.adapter".into(), json!(DESCRIPTOR.id)),
            (
                "unisphere.source.path".into(),
                json!(self.snapshot.source.path.to_str()),
            ),
            ("unisphere.source.key".into(), json!(key)),
            (
                "unisphere.source.revision".into(),
                json!(self.snapshot.revision),
            ),
            ("unisphere.source.format".into(), json!(self.format)),
            ("unisphere.source.kind".into(), json!(kind)),
        ]);
        if let Some(id) = &self.session_id {
            attributes.insert("gen_ai.conversation.id".into(), json!(id));
            attributes.insert("unisphere.source.session.id".into(), json!(id));
        }
        TelemetryRecord {
            event_name: "unisphere.session.record".into(),
            timestamp_unix_nano: None,
            attributes,
            body: None,
        }
    }

    fn string<'a>(
        &mut self,
        object: &'a Map<String, Value>,
        field: &str,
        key: &str,
    ) -> Option<&'a str> {
        match object.get(field) {
            None => None,
            Some(Value::String(text)) => Some(text),
            Some(_) => {
                self.diagnostic(key, MappingDiagnosticCode::InvalidField);
                None
            }
        }
    }

    fn strings(
        &mut self,
        object: &Map<String, Value>,
        fields: &[(&str, &str)],
        key: &str,
        record: &mut TelemetryRecord,
    ) {
        for (field, attribute) in fields {
            if let Some(value) = self.string(object, field, key) {
                record.attributes.insert((*attribute).into(), json!(value));
            }
        }
    }

    fn timestamp(&mut self, value: Option<&Value>, key: &str) -> Option<u64> {
        let value = value?;
        let timestamp = value.as_u64().and_then(|ms| ms.checked_mul(1_000_000));
        if timestamp.is_none() {
            self.diagnostic(key, MappingDiagnosticCode::InvalidTimestamp);
        }
        timestamp
    }

    fn omitted(&mut self, key: &str, record: &mut TelemetryRecord) {
        if !self.options.include_content {
            record
                .attributes
                .insert("unisphere.content.omitted".into(), json!(true));
            self.diagnostic(key, MappingDiagnosticCode::ContentOmitted);
        }
    }

    fn unprojected(&mut self, object: &Map<String, Value>, supported: &[&str], key: &str) {
        if object
            .keys()
            .any(|field| !supported.contains(&field.as_str()))
        {
            // Structural location only: unknown property names may contain data.
            self.diagnostic(key, MappingDiagnosticCode::UnsupportedPart);
        }
    }

    fn session(&mut self, value: &Value, key: &str) -> Result<(), PipelineError> {
        let Some(session) = value.as_object() else {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        };
        self.session_id = self
            .string(session, "sessionId", key)
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        if self
            .snapshot
            .source
            .session_id
            .as_ref()
            .is_some_and(|selected| self.session_id.as_ref() != Some(selected))
        {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        let mut record = self.record(key, "session");
        let version = match session.get("version") {
            None => 1,
            Some(value) if matches!(value.as_u64(), Some(2 | 3)) => value.as_u64().unwrap_or(0),
            _ => {
                self.diagnostic(key, MappingDiagnosticCode::UnsupportedRecord);
                self.output.records.push(record);
                return Ok(());
            }
        };
        record
            .attributes
            .insert("unisphere.vscode.schema.version".into(), json!(version));
        record.timestamp_unix_nano = self.timestamp(session.get("creationDate"), key);
        self.strings(
            session,
            &[("responderUsername", "unisphere.vscode.responder_username")],
            key,
            &mut record,
        );
        let title = if version == 2 {
            "computedTitle"
        } else {
            "customTitle"
        };
        if let Some(title) = self.string(session, title, key) {
            self.omitted(key, &mut record);
            if self.options.include_content {
                record.body = Some(json!({"type": "unisphere.session", "title": title}));
            }
        }
        self.unprojected(
            session,
            &[
                "version",
                "sessionId",
                "creationDate",
                "responderUsername",
                "customTitle",
                "computedTitle",
                "requests",
            ],
            key,
        );
        self.output.records.push(record);
        let Some(requests) = session.get("requests").and_then(Value::as_array) else {
            self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            return Ok(());
        };
        for (index, request) in requests.iter().enumerate() {
            self.request(request, &format!("{key}#/requests/{index}"));
        }
        Ok(())
    }

    fn request(&mut self, value: &Value, key: &str) {
        let Some(request) = value.as_object() else {
            self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            self.output.records.push(self.record(key, "unknown"));
            return;
        };
        let message_key = format!("{key}/message");
        let mut message = self.record(&message_key, "message");
        message
            .attributes
            .insert("unisphere.message.role".into(), json!("user"));
        message.timestamp_unix_nano = self.timestamp(request.get("timestamp"), &message_key);
        self.strings(
            request,
            &[
                ("requestId", "unisphere.message.id"),
                ("modelId", "gen_ai.request.model"),
            ],
            &message_key,
            &mut message,
        );
        for (field, attribute) in [
            ("isHidden", "unisphere.vscode.request.is_hidden"),
            (
                "hiddenFromTranscript",
                "unisphere.vscode.request.hidden_from_transcript",
            ),
            (
                "isSystemInitiated",
                "unisphere.vscode.request.is_system_initiated",
            ),
        ] {
            if let Some(value) = request.get(field) {
                if value.is_boolean() {
                    message.attributes.insert(attribute.into(), value.clone());
                } else {
                    self.diagnostic(&message_key, MappingDiagnosticCode::InvalidField);
                }
            }
        }
        let text = match request.get("message") {
            Some(Value::String(text)) => Some(text.as_str()),
            Some(Value::Object(parsed)) => {
                // Native parser parts describe spans/references; text is the
                // actual request text. Never dereference attached variables.
                self.unprojected(parsed, &["text", "parts"], &message_key);
                self.string(parsed, "text", &message_key)
            }
            _ => None,
        };
        if let Some(text) = text {
            self.omitted(&message_key, &mut message);
            if self.options.include_content {
                message.body =
                    Some(json!({"role":"user", "parts":[{"type":"text", "content":text}]}));
            }
        } else {
            self.diagnostic(&message_key, MappingDiagnosticCode::InvalidField);
        }
        self.output.records.push(message);
        self.unprojected(
            request,
            &[
                "requestId",
                "message",
                "timestamp",
                "modelId",
                "response",
                "responseId",
                "responseTimestamp",
                "agent",
                "isHidden",
                "hiddenFromTranscript",
                "isSystemInitiated",
                "isCanceled",
                "modelState",
                "promptTokens",
                "completionTokens",
                "modelTotals",
                "copilotCredits",
                "sessionCopilotCredits",
            ],
            key,
        );
        // An unanswered request is not an invented empty assistant response.
        if request
            .get("response")
            .is_some_and(|value| !value.is_null())
        {
            self.response(request, &format!("{key}/response"));
        }
    }

    fn response(&mut self, request: &Map<String, Value>, key: &str) {
        let mut record = self.record(key, "response");
        record
            .attributes
            .insert("unisphere.message.role".into(), json!("assistant"));
        record.timestamp_unix_nano = self.timestamp(request.get("responseTimestamp"), key);
        self.strings(
            request,
            &[
                ("responseId", "unisphere.message.id"),
                ("requestId", "unisphere.source.parent.id"),
                ("modelId", "gen_ai.request.model"),
            ],
            key,
            &mut record,
        );
        if let Some(agent) = request.get("agent") {
            if let Some(agent) = agent.as_object() {
                self.strings(
                    agent,
                    &[("id", "unisphere.vscode.agent.id")],
                    key,
                    &mut record,
                );
            } else {
                self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            }
        }
        if let Some(value) = request.get("isCanceled") {
            if value.is_boolean() {
                record.attributes.insert(
                    "unisphere.vscode.response.is_canceled".into(),
                    value.clone(),
                );
            } else {
                self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            }
        }
        if let Some(state) = request.get("modelState") {
            if let Some(value) = state.get("value").and_then(Value::as_i64) {
                record
                    .attributes
                    .insert("unisphere.vscode.response.state".into(), json!(value));
            } else {
                self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            }
        }
        self.usage(request, key, &mut record);
        self.omitted(key, &mut record);
        let mut parts = Vec::new();
        let mut tools = Vec::new();
        match request.get("response") {
            Some(Value::Array(values)) => {
                for (index, value) in values.iter().enumerate() {
                    if let Some(part) = self.part(value, &format!("{key}/{index}"), &mut tools) {
                        parts.push(part);
                    }
                }
            }
            Some(Value::String(text)) => {
                if self.options.include_content {
                    parts.push(json!({"type":"text", "content":text}));
                }
            }
            _ => self.diagnostic(key, MappingDiagnosticCode::InvalidField),
        }
        if !tools.is_empty() {
            record
                .attributes
                .insert("unisphere.vscode.tools".into(), Value::Array(tools));
        }
        if self.options.include_content {
            record.body = Some(json!({"role":"assistant", "parts":parts}));
        }
        self.output.records.push(record);
    }

    fn part(&mut self, value: &Value, key: &str, tools: &mut Vec<Value>) -> Option<Value> {
        let Some(part) = value.as_object() else {
            self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            return None;
        };
        let kind = part.get("kind").and_then(Value::as_str);
        match kind {
            None if part.get("value").is_some_and(Value::is_string) => self
                .options
                .include_content
                .then(|| json!({"type":"text", "content":part["value"]})),
            Some("markdownContent") => {
                let text = part
                    .get("content")
                    .and_then(|content| content.get("value"))
                    .and_then(Value::as_str);
                if text.is_none() {
                    self.diagnostic(key, MappingDiagnosticCode::InvalidField);
                }
                text.filter(|_| self.options.include_content)
                    .map(|text| json!({"type":"text", "content":text}))
            }
            Some("thinking") => {
                let value = part.get("value")?;
                if !value.is_string()
                    && !value
                        .as_array()
                        .is_some_and(|items| items.iter().all(Value::is_string))
                {
                    self.diagnostic(key, MappingDiagnosticCode::InvalidField);
                    return None;
                }
                self.options
                    .include_content
                    .then(|| json!({"type":"reasoning", "content":value}))
            }
            Some("toolInvocationSerialized") => self.tool(part, key, tools),
            _ => {
                self.diagnostic(key, MappingDiagnosticCode::UnsupportedPart);
                self.options.include_content.then(
                    || json!({"type":"unisphere.unknown", "native_type":kind.unwrap_or("unknown")}),
                )
            }
        }
    }

    fn tool(
        &mut self,
        part: &Map<String, Value>,
        key: &str,
        tools: &mut Vec<Value>,
    ) -> Option<Value> {
        let mut metadata = Map::new();
        for (field, output) in [
            ("toolId", "name"),
            ("toolCallId", "id"),
            ("subAgentInvocationId", "subagent_id"),
        ] {
            if let Some(value) = self.string(part, field, key) {
                metadata.insert(output.into(), json!(value));
            }
        }
        for field in ["toolId", "toolCallId"] {
            if !part.contains_key(field) {
                self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            }
        }
        if let Some(value) = part.get("isComplete") {
            if value.is_boolean() {
                metadata.insert("is_complete".into(), value.clone());
            } else {
                self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            }
        }
        if let Some(confirmed) = part.get("isConfirmed") {
            if confirmed.is_boolean() {
                metadata.insert("is_confirmed".into(), confirmed.clone());
            } else if let Some(kind) = confirmed
                .get("type")
                .filter(|value| value.is_string() || value.as_i64().is_some())
            {
                metadata.insert("confirmation_kind".into(), kind.clone());
            } else {
                self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            }
        }
        self.unprojected(
            part,
            &[
                "kind",
                "toolId",
                "toolCallId",
                "subAgentInvocationId",
                "isComplete",
                "isConfirmed",
                "invocationMessage",
                "originMessage",
                "pastTenseMessage",
                "resultDetails",
                "toolSpecificData",
            ],
            key,
        );
        let mut content = self.options.include_content.then(|| metadata.clone());
        tools.push(Value::Object(metadata));
        if let Some(content) = &mut content {
            content.insert("type".into(), json!("unisphere.tool_invocation"));
            // These are native UI details, not reconstructed LM parameters or
            // a complete tool result. Preserve their types only with opt-in.
            for field in [
                "invocationMessage",
                "originMessage",
                "pastTenseMessage",
                "resultDetails",
                "toolSpecificData",
            ] {
                if let Some(value) = part.get(field) {
                    content.insert(field.into(), value.clone());
                }
            }
        }
        content.map(Value::Object)
    }

    fn usage(&mut self, request: &Map<String, Value>, key: &str, record: &mut TelemetryRecord) {
        let mut usage = Map::new();
        for (field, scope) in [
            ("promptTokens", "latest_model_call"),
            ("completionTokens", "native_response_counter"),
            ("copilotCredits", "response_cost"),
            ("sessionCopilotCredits", "session_cumulative"),
        ] {
            if let Some(value) = request.get(field) {
                let valid = if field.ends_with("Credits") {
                    value.as_f64().is_some_and(|n| n.is_finite() && n >= 0.0)
                        && (!value.is_u64() || value.as_i64().is_some())
                } else {
                    value.as_i64().is_some_and(|n| n >= 0)
                };
                if valid {
                    usage.insert(field.into(), json!({"value":value, "scope":scope}));
                } else {
                    self.diagnostic(key, MappingDiagnosticCode::InvalidField);
                }
            }
        }
        if let Some(value) = request.get("modelTotals") {
            if let Some(totals) = value.as_array() {
                let mut retained = Vec::new();
                for total in totals {
                    if let Some(object) = total.as_object().filter(|object| {
                        object.get("model").is_some_and(Value::is_string)
                            && ["inputTokens", "cachedTokens", "outputTokens"]
                                .iter()
                                .all(|field| {
                                    object
                                        .get(*field)
                                        .and_then(Value::as_i64)
                                        .is_some_and(|n| n >= 0)
                                })
                    }) {
                        retained.push(json!({"model":object["model"], "inputTokens":object["inputTokens"],
                            "cachedTokens":object["cachedTokens"], "outputTokens":object["outputTokens"]}));
                        self.unprojected(
                            object,
                            &["model", "inputTokens", "cachedTokens", "outputTokens"],
                            key,
                        );
                    } else {
                        self.diagnostic(key, MappingDiagnosticCode::InvalidField);
                    }
                }
                usage.insert(
                    "modelTotals".into(),
                    json!({"value":retained, "scope":"whole_turn_including_subagents"}),
                );
            } else {
                self.diagnostic(key, MappingDiagnosticCode::InvalidField);
            }
        }
        if !usage.is_empty() {
            record
                .attributes
                .insert("unisphere.vscode.usage".into(), Value::Object(usage));
        }
    }
}
