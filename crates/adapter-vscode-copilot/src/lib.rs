//! Pure VS Code chat-session projection from complete caller-supplied snapshots.
//! Journal mutations are reduced before projection. No source, sidecar, clock,
//! environment, network or output is accessed. This is not a lossless archive.
#![forbid(unsafe_code)]

mod journal;
mod prep;

pub use prep::{DOCUMENT_PREP_POLICY_VERSION, JOURNAL_PREP_POLICY_VERSION, VsCodeCopilotPrepFold};

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde_json::{Map, Value, json};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedSnapshot, MappingDiagnosticCode,
    MappingOptions, NativeSnapshot, PipelineError, PipelineErrorKind, SnapshotAdapter,
    SnapshotDiagnostic, SnapshotFormat, TelemetryRecord,
    query::{
        AssociationBasis, AssociationExtent, AssociationObservation, AvailabilityCode,
        AvailabilityIssue, BranchEvidence, ContentAccess, FieldId, InspectedSource, LimitKind,
        MembershipPolicy, MessageRole, NativeLocator, NativeQueryInput, NativeSequence,
        Observation, ObservationFacet, ObservationPart, Outcome, PartitionId, QueryAdapter,
        QueryFailure, QueryFailureCode, QueryLimits, RecoveryAction, RequestMarker,
        SessionEvidenceKey, SourceEvidence, SourceLocator, SourcePartition, SourceProblem,
        SourceReadStatus, SourceRef, SourceViewKind, Timestamp, TimestampBasis,
    },
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
        cli_persisted_resume: true,
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

/// Source-local reconstruction policy used by query registrations for both
/// whole documents and reduced journals.
pub const QUERY_POLICY_VERSION: &str = "vscode-request-containment-v1";

impl QueryAdapter for VsCodeCopilotAdapter {
    fn inspect(
        &self,
        input: NativeQueryInput<'_>,
        access: ContentAccess,
        limits: &QueryLimits,
    ) -> Result<InspectedSource, QueryFailure> {
        limits.validate()?;
        let NativeQueryInput::Snapshot { source, snapshot } = input else {
            return Err(unsupported_query_source(None));
        };
        source.validate()?;
        if source.adapter.as_str() != DESCRIPTOR.id
            || source.query_policy_version != QUERY_POLICY_VERSION
            || snapshot.source.validate().is_err()
            || snapshot.revision != source.revision
            || matches!(&source.locator, SourceLocator::LocalPath(path) if path != &snapshot.source.path)
        {
            return Err(QueryFailure::invalid_data());
        }
        let input_bytes = snapshot.records.iter().try_fold(0usize, |total, record| {
            total
                .checked_add(record.key.len())
                .and_then(|total| total.checked_add(record.bytes.len()))
                .ok_or_else(|| QueryFailure::limit(LimitKind::SourceBytes))
        })?;
        if input_bytes > limits.max_source_bytes || input_bytes > limits.max_total_input_bytes {
            return Err(QueryFailure::limit(LimitKind::SourceBytes));
        }
        if access.emit_content && input_bytes > limits.max_retained_bytes {
            return Err(QueryFailure::limit(LimitKind::RetainedBytes));
        }

        let (value, root_key) = match snapshot.source.format {
            SnapshotFormat::JsonDocument => {
                let [record] = snapshot.records.as_slice() else {
                    return Err(QueryFailure::invalid_data());
                };
                if record.key != "document" {
                    return Err(QueryFailure::invalid_data());
                }
                let value = serde_json::from_slice(&record.bytes)
                    .map_err(|_| QueryFailure::invalid_data())?;
                (value, "document")
            }
            SnapshotFormat::JsonJournal => (
                journal::reduce(snapshot).map_err(query_reduction_failure)?,
                "journal:reduced",
            ),
            SnapshotFormat::SqliteKeyValue { .. } => {
                return Err(unsupported_query_source(Some(source.id)));
            }
        };
        let mapping = QueryMapping {
            source: source.clone(),
            access: &access,
            limits,
            root_key,
            partitions: Vec::new(),
            observations: Vec::new(),
            issues: Vec::new(),
        };
        mapping.inspect(&value, snapshot.source.session_id.as_deref())
    }
}

fn unsupported_query_source(source: Option<unisphere_core::query::SourceId>) -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::UnsupportedSource,
        RecoveryAction::FixSource {
            reason: SourceProblem::UnsupportedDialect,
            source,
        },
    )
}

fn query_reduction_failure(error: PipelineError) -> QueryFailure {
    if error.kind() == PipelineErrorKind::BatchLimit {
        QueryFailure::limit(LimitKind::RetainedBytes)
    } else {
        QueryFailure::invalid_data()
    }
}

struct QueryMapping<'a> {
    source: SourceEvidence,
    access: &'a ContentAccess,
    limits: &'a QueryLimits,
    root_key: &'static str,
    partitions: Vec<SourcePartition>,
    observations: Vec<Observation>,
    issues: Vec<AvailabilityIssue>,
}

impl QueryMapping<'_> {
    fn inspect(
        mut self,
        value: &Value,
        selected_session: Option<&str>,
    ) -> Result<InspectedSource, QueryFailure> {
        let session = value.as_object().ok_or_else(QueryFailure::invalid_data)?;
        let version = match session.get("version") {
            None => 1,
            Some(value)
                if value
                    .as_u64()
                    .is_some_and(|version| matches!(version, 2 | 3)) =>
            {
                value.as_u64().unwrap_or(1)
            }
            _ => {
                self.source.read_status = SourceReadStatus::Unsupported;
                self.add_issue(AvailabilityCode::NotSupported, None);
                return self.finish();
            }
        };
        let session_id = session
            .get("sessionId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        if selected_session.is_some_and(|selected| session_id.as_deref() != Some(selected)) {
            return Err(QueryFailure::invalid_data());
        }
        let partition = PartitionId::derive(
            self.source.id,
            session_id
                .as_deref()
                .map(str::as_bytes)
                .unwrap_or_else(|| self.source.revision.as_bytes()),
        );
        if self
            .source
            .associations
            .iter()
            .any(|association| association.partition != partition)
        {
            return Err(QueryFailure::invalid_data());
        }
        let mut associations = self.source.associations.clone();
        self.native_workspace_association(session, partition, &mut associations);
        self.source.associations = associations.clone();
        if associations.is_empty() {
            self.add_issue(AvailabilityCode::Unassociated, Some(FieldId::ProjectPath));
        }

        let membership = if session_id.is_some() {
            MembershipPolicy::NativeContainment
        } else {
            MembershipPolicy::Unavailable
        };
        self.partitions.push(SourcePartition {
            id: partition,
            native_session_id: session_id.clone(),
            participant_id: None,
            view: if session_id.is_some() {
                SourceViewKind::Conversation
            } else {
                SourceViewKind::SourceOnly
            },
            membership,
            associations: associations.clone(),
        });
        self.declare_available_fields(version);

        let session_key = session_id.as_ref().map(|id| SessionEvidenceKey {
            namespace: "vscode-chat-session".into(),
            native_id: id.clone(),
            participant_id: None,
            parent_native_id: None,
            fork_native_id: None,
            membership_basis: MembershipPolicy::NativeContainment,
        });
        if let Some(id) = &session_id {
            let mut diagnostics = Vec::new();
            let created_at = self.timestamp(
                session.get("creationDate"),
                FieldId::StartedAt,
                &mut diagnostics,
            );
            let title_field = if version == 2 {
                "computedTitle"
            } else {
                "customTitle"
            };
            let name =
                self.sensitive_string(session.get(title_field), FieldId::Name, &mut diagnostics);
            let models = session
                .get("requests")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_object)
                .filter_map(|request| request.get("modelId").and_then(Value::as_str))
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            self.push_observation(Observation {
                source_ref: self.source_ref(self.root_key),
                native_record_id: Some(id.clone()),
                session: session_key.clone(),
                branch: BranchEvidence::Linear { partition },
                parent_ids: Vec::new(),
                sequence: native_sequence(&[0]),
                timestamp: created_at.clone(),
                facets: vec![ObservationFacet::SessionMetadata {
                    native_id: id.clone(),
                    name,
                    models,
                    created_at,
                    associations: associations.clone(),
                    lineage: Vec::new(),
                }],
                diagnostics,
            })?;
        }

        let Some(requests) = session.get("requests").and_then(Value::as_array) else {
            self.add_issue(AvailabilityCode::NotCaptured, Some(FieldId::Parts));
            return self.finish();
        };
        for (index, request) in requests.iter().enumerate() {
            self.request(request, index, partition, session_key.clone())?;
        }
        if has_any_fields(
            session,
            &[
                "responderUsername",
                "initialLocation",
                "hasPendingEdits",
                "inputState",
                "repoData",
                "pendingRequests",
            ],
        ) {
            self.add_issue(AvailabilityCode::NotSupported, None);
        }

        if has_unknown_fields(
            session,
            &[
                "version",
                "sessionId",
                "creationDate",
                "responderUsername",
                "customTitle",
                "computedTitle",
                "requests",
                "initialLocation",
                "hasPendingEdits",
                "inputState",
                "repoData",
                "pendingRequests",
                "workingDirectory",
            ],
        ) {
            self.add_issue(AvailabilityCode::NotSupported, None);
        }
        self.finish()
    }

    fn request(
        &mut self,
        value: &Value,
        index: usize,
        partition: PartitionId,
        session: Option<SessionEvidenceKey>,
    ) -> Result<(), QueryFailure> {
        let Some(request) = value.as_object() else {
            self.add_issue(AvailabilityCode::NotCaptured, Some(FieldId::Parts));
            return Ok(());
        };
        let request_id = request
            .get("requestId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        let turn_id = request_id
            .clone()
            .unwrap_or_else(|| format!("request-index:{index}"));
        let message_key = format!("{}#/requests/{index}/message", self.root_key);
        let mut diagnostics = Vec::new();
        let parts = self.request_parts(request.get("message"), &mut diagnostics);
        let timestamp = self.timestamp(
            request.get("timestamp"),
            FieldId::Timestamp,
            &mut diagnostics,
        );
        let injected = request.get("isSystemInitiated") == Some(&Value::Bool(true));
        self.push_observation(Observation {
            source_ref: self.source_ref(&message_key),
            native_record_id: request_id.clone(),
            session: session.clone(),
            branch: BranchEvidence::Linear { partition },
            parent_ids: Vec::new(),
            sequence: native_sequence(&[1, index as u64, 0]),
            timestamp,
            facets: vec![ObservationFacet::Message {
                native_id: request_id.clone(),
                role: MessageRole::User,
                parts,
                request_marker: if injected {
                    RequestMarker::Injected
                } else {
                    RequestMarker::Initiating
                },
                turn_id: Some(turn_id.clone()),
            }],
            diagnostics,
        })?;

        if request
            .get("response")
            .is_some_and(|response| !response.is_null())
        {
            self.response(request, index, partition, session, request_id, turn_id)?;
        }
        if has_any_fields(
            request,
            &[
                "agent",
                "isHidden",
                "hiddenFromTranscript",
                "shouldBeRemovedOnSend",
                "isCanceled",
                "modelState",
                "promptTokens",
                "completionTokens",
                "modelTotals",
                "copilotCredits",
                "sessionCopilotCredits",
                "variableData",
                "result",
                "responseMarkdownInfo",
                "followups",
                "vote",
                "slashCommand",
                "usedContext",
                "contentReferences",
                "codeCitations",
                "timeSpentWaiting",
                "outputBuffer",
                "promptTokenDetails",
                "elapsedMs",
                "modeInfo",
                "systemInitiatedLabel",
                "terminalExecutionId",
                "origin",
                "confirmation",
                "editedFileEvents",
            ],
        ) {
            self.add_issue(AvailabilityCode::NotSupported, None);
        }
        if has_unknown_fields(
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
                "shouldBeRemovedOnSend",
                "isSystemInitiated",
                "isCanceled",
                "modelState",
                "promptTokens",
                "completionTokens",
                "modelTotals",
                "copilotCredits",
                "sessionCopilotCredits",
                "variableData",
                "result",
                "responseMarkdownInfo",
                "followups",
                "vote",
                "slashCommand",
                "usedContext",
                "contentReferences",
                "codeCitations",
                "timeSpentWaiting",
                "outputBuffer",
                "promptTokenDetails",
                "elapsedMs",
                "modeInfo",
                "systemInitiatedLabel",
                "terminalExecutionId",
                "origin",
                "confirmation",
                "editedFileEvents",
            ],
        ) {
            self.add_issue(AvailabilityCode::NotSupported, None);
        }
        Ok(())
    }

    fn response(
        &mut self,
        request: &Map<String, Value>,
        request_index: usize,
        partition: PartitionId,
        session: Option<SessionEvidenceKey>,
        request_id: Option<String>,
        turn_id: String,
    ) -> Result<(), QueryFailure> {
        let response_id = request
            .get("responseId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        let response_key = format!("{}#/requests/{request_index}/response", self.root_key);
        let mut diagnostics = Vec::new();
        let timestamp = self.timestamp(
            request.get("responseTimestamp"),
            FieldId::Timestamp,
            &mut diagnostics,
        );
        let mut parts = Vec::new();
        match request.get("response") {
            Some(Value::String(text)) => {
                parts.push(self.sensitive_part(
                    ObservationPart::Text(text.clone()),
                    FieldId::Text,
                    &mut diagnostics,
                ));
            }
            Some(Value::Array(values)) => {
                for (part_index, value) in values.iter().enumerate() {
                    let Some(part) = value.as_object() else {
                        self.issue_into(
                            &mut diagnostics,
                            AvailabilityCode::NotCaptured,
                            Some(FieldId::Parts),
                        );
                        continue;
                    };
                    if part.get("kind").and_then(Value::as_str) == Some("toolInvocationSerialized")
                    {
                        self.tool(
                            part,
                            (request_index, part_index),
                            partition,
                            session.clone(),
                            response_id.as_ref().or(request_id.as_ref()).cloned(),
                            &turn_id,
                        )?;
                        continue;
                    }
                    self.response_part(part, &mut parts, &mut diagnostics);
                }
            }
            _ => self.issue_into(
                &mut diagnostics,
                AvailabilityCode::NotCaptured,
                Some(FieldId::Parts),
            ),
        }
        self.push_observation(Observation {
            source_ref: self.source_ref(&response_key),
            native_record_id: response_id.clone(),
            session,
            branch: BranchEvidence::Linear { partition },
            parent_ids: request_id.into_iter().collect(),
            sequence: native_sequence(&[1, request_index as u64, 1]),
            timestamp,
            facets: vec![ObservationFacet::Message {
                native_id: response_id,
                role: MessageRole::Assistant,
                parts,
                request_marker: RequestMarker::Unknown,
                turn_id: Some(turn_id),
            }],
            diagnostics,
        })
    }

    fn tool(
        &mut self,
        part: &Map<String, Value>,
        (request_index, part_index): (usize, usize),
        partition: PartitionId,
        session: Option<SessionEvidenceKey>,
        parent_id: Option<String>,
        turn_id: &str,
    ) -> Result<(), QueryFailure> {
        let call_id = part
            .get("toolCallId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let name = part
            .get("toolId")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty());
        let (Some(call_id), Some(name)) = (call_id, name) else {
            self.add_issue(AvailabilityCode::NotCaptured, Some(FieldId::CallId));
            return Ok(());
        };
        let key = format!(
            "{}#/requests/{request_index}/response/{part_index}",
            self.root_key
        );
        let mut diagnostics = Vec::new();
        let mut status = Map::new();
        if let Some(is_complete) = part.get("isComplete") {
            if is_complete.is_boolean() {
                status.insert("is_complete".into(), is_complete.clone());
            } else {
                self.issue_into(
                    &mut diagnostics,
                    AvailabilityCode::NotCaptured,
                    Some(FieldId::Status),
                );
            }
        }
        if let Some(confirmed) = part.get("isConfirmed") {
            if confirmed.is_boolean() {
                status.insert("is_confirmed".into(), confirmed.clone());
            } else if let Some(kind) = confirmed
                .get("type")
                .filter(|value| value.is_string() || value.as_i64().is_some())
            {
                status.insert("confirmation_kind".into(), kind.clone());
            } else {
                self.issue_into(
                    &mut diagnostics,
                    AvailabilityCode::NotCaptured,
                    Some(FieldId::Status),
                );
            }
        }
        if let Some(subagent) = part
            .get("subAgentInvocationId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            status.insert("subagent_id".into(), json!(subagent));
        }
        let mut facets = vec![ObservationFacet::ToolCall {
            native_call_id: call_id.into(),
            native_name: name.into(),
            family: tool_family(name).map(str::to_owned),
            input: vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)],
            turn_id: Some(turn_id.into()),
        }];
        self.issue_into(
            &mut diagnostics,
            AvailabilityCode::NotCaptured,
            Some(FieldId::Input),
        );
        if !status.is_empty() {
            facets.push(ObservationFacet::ToolProgress {
                native_call_id: call_id.into(),
                parts: vec![ObservationPart::Structured(Value::Object(status))],
            });
        }
        if let Some(result) = part.get("resultDetails") {
            let output = if self.access.permits_payload(FieldId::Output) {
                vec![ObservationPart::Structured(result.clone())]
            } else {
                self.issue_into(
                    &mut diagnostics,
                    AvailabilityCode::SensitiveOmitted,
                    Some(FieldId::Output),
                );
                vec![ObservationPart::Unavailable(
                    AvailabilityCode::SensitiveOmitted,
                )]
            };
            facets.push(ObservationFacet::ToolResult {
                native_call_id: call_id.into(),
                native_name: Some(name.into()),
                output,
                outcome: Outcome::Unknown,
                exit_code: None,
                reported_duration_ms: None,
                turn_id: Some(turn_id.into()),
            });
            self.issue_into(
                &mut diagnostics,
                AvailabilityCode::NotCaptured,
                Some(FieldId::Status),
            );
        } else {
            self.issue_into(
                &mut diagnostics,
                AvailabilityCode::NotCaptured,
                Some(FieldId::Output),
            );
        }
        self.issue_into(
            &mut diagnostics,
            AvailabilityCode::NotCaptured,
            Some(FieldId::DurationMs),
        );
        self.issue_into(
            &mut diagnostics,
            AvailabilityCode::NotCaptured,
            Some(FieldId::ExitCode),
        );
        if has_any_fields(
            part,
            &[
                "invocationMessage",
                "originMessage",
                "pastTenseMessage",
                "toolSpecificData",
            ],
        ) {
            self.issue_into(
                &mut diagnostics,
                AvailabilityCode::NotSupported,
                Some(FieldId::Parts),
            );
        }
        if has_unknown_fields(
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
        ) {
            self.issue_into(
                &mut diagnostics,
                AvailabilityCode::NotSupported,
                Some(FieldId::Parts),
            );
        }
        self.push_observation(Observation {
            source_ref: self.source_ref(&key),
            native_record_id: Some(call_id.into()),
            session,
            branch: BranchEvidence::Linear { partition },
            parent_ids: parent_id.into_iter().collect(),
            sequence: native_sequence(&[1, request_index as u64, 2, part_index as u64]),
            timestamp: None,
            facets,
            diagnostics,
        })
    }

    fn request_parts(
        &mut self,
        value: Option<&Value>,
        diagnostics: &mut Vec<AvailabilityIssue>,
    ) -> Vec<ObservationPart> {
        let text = match value {
            Some(Value::String(text)) => Some(text.as_str()),
            Some(Value::Object(message)) => message.get("text").and_then(Value::as_str),
            _ => None,
        };
        match text {
            Some(text) => vec![self.sensitive_part(
                ObservationPart::Text(text.into()),
                FieldId::Text,
                diagnostics,
            )],
            None => {
                self.issue_into(
                    diagnostics,
                    AvailabilityCode::NotCaptured,
                    Some(FieldId::Text),
                );
                vec![ObservationPart::Unavailable(AvailabilityCode::NotCaptured)]
            }
        }
    }

    fn response_part(
        &mut self,
        part: &Map<String, Value>,
        parts: &mut Vec<ObservationPart>,
        diagnostics: &mut Vec<AvailabilityIssue>,
    ) {
        match part.get("kind").and_then(Value::as_str) {
            None => {
                if let Some(text) = part.get("value").and_then(Value::as_str) {
                    parts.push(self.sensitive_part(
                        ObservationPart::Text(text.into()),
                        FieldId::Text,
                        diagnostics,
                    ));
                } else {
                    self.issue_into(
                        diagnostics,
                        AvailabilityCode::NotCaptured,
                        Some(FieldId::Parts),
                    );
                }
            }
            Some("markdownContent") => {
                if let Some(text) = part
                    .get("content")
                    .and_then(|content| content.get("value"))
                    .and_then(Value::as_str)
                {
                    parts.push(self.sensitive_part(
                        ObservationPart::Text(text.into()),
                        FieldId::Text,
                        diagnostics,
                    ));
                } else {
                    self.issue_into(
                        diagnostics,
                        AvailabilityCode::NotCaptured,
                        Some(FieldId::Text),
                    );
                }
            }
            Some("thinking") => match part.get("value") {
                Some(Value::String(reasoning)) => parts.push(self.sensitive_part(
                    ObservationPart::Reasoning(reasoning.clone()),
                    FieldId::Parts,
                    diagnostics,
                )),
                Some(Value::Array(values)) if values.iter().all(Value::is_string) => {
                    for reasoning in values.iter().filter_map(Value::as_str) {
                        parts.push(self.sensitive_part(
                            ObservationPart::Reasoning(reasoning.into()),
                            FieldId::Parts,
                            diagnostics,
                        ));
                    }
                }
                _ => self.issue_into(
                    diagnostics,
                    AvailabilityCode::NotCaptured,
                    Some(FieldId::Parts),
                ),
            },
            _ => {
                parts.push(ObservationPart::Unavailable(AvailabilityCode::NotSupported));
                self.issue_into(
                    diagnostics,
                    AvailabilityCode::NotSupported,
                    Some(FieldId::Parts),
                );
            }
        }
    }

    fn sensitive_part(
        &mut self,
        part: ObservationPart,
        field: FieldId,
        diagnostics: &mut Vec<AvailabilityIssue>,
    ) -> ObservationPart {
        if self.access.permits_payload(field) || self.access.permits_payload(FieldId::Parts) {
            part
        } else {
            self.issue_into(diagnostics, AvailabilityCode::SensitiveOmitted, Some(field));
            ObservationPart::Unavailable(AvailabilityCode::SensitiveOmitted)
        }
    }

    fn sensitive_string(
        &mut self,
        value: Option<&Value>,
        field: FieldId,
        diagnostics: &mut Vec<AvailabilityIssue>,
    ) -> Option<String> {
        let value = value?.as_str();
        match value {
            Some(value) if self.access.permits_payload(field) => Some(value.into()),
            Some(_) => {
                self.issue_into(diagnostics, AvailabilityCode::SensitiveOmitted, Some(field));
                None
            }
            None => {
                self.issue_into(diagnostics, AvailabilityCode::NotCaptured, Some(field));
                None
            }
        }
    }

    fn timestamp(
        &mut self,
        value: Option<&Value>,
        field: FieldId,
        diagnostics: &mut Vec<AvailabilityIssue>,
    ) -> Option<Timestamp> {
        let value = value?;
        let timestamp = value
            .as_u64()
            .and_then(|millis| i128::from(millis).checked_mul(1_000_000))
            .and_then(|nanos| Timestamp::new(nanos, TimestampBasis::Native).ok());
        if timestamp.is_none() {
            self.issue_into(diagnostics, AvailabilityCode::InvalidClock, Some(field));
        }
        timestamp
    }

    fn native_workspace_association(
        &mut self,
        session: &Map<String, Value>,
        partition: PartitionId,
        associations: &mut Vec<AssociationObservation>,
    ) {
        match session.get("workingDirectory") {
            Some(Value::String(uri)) => match local_file_uri(uri) {
                Some(path) => {
                    let association = AssociationObservation {
                        basis: AssociationBasis::NativeCwd,
                        path: Some(path),
                        partition,
                        applies_to: AssociationExtent::Partition,
                    };
                    if !associations.contains(&association) {
                        associations.push(association);
                    }
                }
                None => self.add_issue(AvailabilityCode::NotSupported, Some(FieldId::ProjectPath)),
            },
            Some(_) => self.add_issue(AvailabilityCode::NotSupported, Some(FieldId::ProjectPath)),
            None if session.contains_key("repoData") && associations.is_empty() => {
                self.add_issue(AvailabilityCode::Unassociated, Some(FieldId::ProjectPath))
            }
            None => {}
        }
    }

    fn declare_available_fields(&mut self, version: u64) {
        self.source.available_fields.extend([
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
            FieldId::Models,
            FieldId::StartedAt,
            FieldId::FirstEventAt,
            FieldId::ParentIds,
            FieldId::TurnId,
            FieldId::Role,
            FieldId::Timestamp,
            FieldId::Text,
            FieldId::Parts,
            FieldId::Model,
            FieldId::MessageId,
            FieldId::Kind,
        ]);
        if version >= 2 {
            self.source.available_fields.insert(FieldId::Name);
        }
        if version >= 3 {
            self.source.available_fields.extend([
                FieldId::ToolName,
                FieldId::ToolFamily,
                FieldId::Status,
                FieldId::StatusReason,
                FieldId::Input,
                FieldId::Output,
                FieldId::CallId,
            ]);
        }
    }

    fn source_ref(&self, key: &str) -> SourceRef {
        SourceRef {
            source_id: self.source.id,
            revision: self.source.revision.clone(),
            locator: NativeLocator::Snapshot { key: key.into() },
            subrecord: key.into(),
        }
    }

    fn push_observation(&mut self, observation: Observation) -> Result<(), QueryFailure> {
        if self.observations.len() == self.limits.max_observations_and_rows {
            return Err(QueryFailure::limit(LimitKind::ObservationsAndRows));
        }
        self.issues.extend(observation.diagnostics.iter().cloned());
        self.observations.push(observation);
        Ok(())
    }

    fn issue_into(
        &mut self,
        diagnostics: &mut Vec<AvailabilityIssue>,
        code: AvailabilityCode,
        field: Option<FieldId>,
    ) {
        diagnostics.push(self.issue(code, field));
    }

    fn add_issue(&mut self, code: AvailabilityCode, field: Option<FieldId>) {
        let issue = self.issue(code, field);
        if !self.issues.contains(&issue) {
            self.issues.push(issue);
        }
    }

    fn issue(&self, code: AvailabilityCode, field: Option<FieldId>) -> AvailabilityIssue {
        AvailabilityIssue {
            code,
            field,
            source: Some(self.source.id),
            entity: None,
            offset: None,
        }
    }

    fn finish(self) -> Result<InspectedSource, QueryFailure> {
        let inspected = InspectedSource {
            source: self.source,
            partitions: self.partitions,
            observations: self.observations,
            issues: self.issues,
        };
        inspected.validate(self.limits)?;
        Ok(inspected)
    }
}

fn native_sequence(parts: &[u64]) -> NativeSequence {
    let mut key = Vec::with_capacity(std::mem::size_of_val(parts));
    for part in parts {
        key.extend_from_slice(&part.to_be_bytes());
    }
    NativeSequence { version: 1, key }
}

fn has_any_fields(object: &Map<String, Value>, fields: &[&str]) -> bool {
    fields.iter().any(|field| object.contains_key(*field))
}

fn has_unknown_fields(object: &Map<String, Value>, supported: &[&str]) -> bool {
    object
        .keys()
        .any(|field| !supported.contains(&field.as_str()))
}

fn tool_family(name: &str) -> Option<&'static str> {
    match name {
        "read_file" => Some("file-read"),
        "create_file" | "insert_edit_into_file" | "replace_string_in_file" => Some("file-write"),
        "run_in_terminal" | "run_terminal_command" => Some("shell"),
        "file_search" | "grep_search" | "semantic_search" => Some("search"),
        _ => None,
    }
}

fn local_file_uri(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    if !encoded.starts_with('/') || encoded.contains('?') || encoded.contains('#') {
        return None;
    }
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    if decoded.contains(&0) {
        return None;
    }
    let decoded = String::from_utf8(decoded).ok()?;
    let path = Path::new(&decoded);
    path.is_absolute().then(|| path.to_path_buf())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
