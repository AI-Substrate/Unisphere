use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedSnapshot,
    MappingDiagnosticCode as Code, MappingOptions, NativeSnapshot, PipelineError,
    PipelineErrorKind, SnapshotAdapter, SnapshotDiagnostic, SnapshotFormat, SnapshotRecord,
    TelemetryRecord,
};

/// IDE state is revision-based, never an append-only byte stream.
pub const IDE_DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "cursor-ide",
    application: "Cursor",
    description: "Cursor IDE cursorDiskKV snapshot projection in composer-declared order; native counters are not inference totals. Opaque CLI blobs are unsupported.",
    locations: &[
        LocationHint {
            platforms: &["macos"],
            base: "home",
            path: "Library/Application Support/Cursor/User/globalStorage",
            session_glob: "state.vscdb",
            storage_format: "sqlite_key_value",
        },
        LocationHint {
            platforms: &["linux"],
            base: "home",
            path: ".config/Cursor/User/globalStorage",
            session_glob: "state.vscdb",
            storage_format: "sqlite_key_value",
        },
        LocationHint {
            platforms: &["windows"],
            base: "appdata",
            path: "Cursor/User/globalStorage",
            session_glob: "state.vscdb",
            storage_format: "sqlite_key_value",
        },
    ],
    capabilities: AdapterCapabilities {
        export_platforms: &["unix"],
        output_formats: &["otlp-jsonl"],
        sdk_caller_owned_cursor: false,
        cursor_source_assumption: "whole_source_revision",
        cli_persisted_resume: false,
        delayed_revision_reconciliation: false,
        lossless_archive: false,
    },
};

/// Maps validated bounded `cursorDiskKV` snapshots without consulting storage.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorIdeAdapter;

impl SnapshotAdapter for CursorIdeAdapter {
    fn name(&self) -> &'static str {
        IDE_DESCRIPTOR.id
    }

    fn map_snapshot(
        &self,
        snapshot: &NativeSnapshot,
        options: MappingOptions,
    ) -> Result<MappedSnapshot, PipelineError> {
        snapshot.source.validate()?;
        if !matches!(&snapshot.source.format, SnapshotFormat::SqliteKeyValue { table } if table == "cursorDiskKV")
        {
            return Err(PipelineError::new(PipelineErrorKind::Unsupported, None));
        }
        if snapshot.revision.is_empty() {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        }
        let mut rows = BTreeMap::new();
        for row in &snapshot.records {
            if row.key.is_empty() || rows.insert(row.key.as_str(), row).is_some() {
                return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
            }
        }
        let mut mapper = Mapper {
            snapshot,
            content: options.include_content,
            output: MappedSnapshot::default(),
        };
        let selected = snapshot.source.session_id.as_deref();
        let mut found = false;
        let mut referenced = BTreeSet::new();
        for (&key, &row) in &rows {
            if let Some(id) = key.strip_prefix("composerData:") {
                if selected.is_none_or(|selected| selected == id) {
                    found = true;
                    mapper.composer(row, id, &rows, &mut referenced);
                }
            } else if !key.starts_with("bubbleId:") && selected.is_none() {
                mapper.diagnostic(key, Code::UnsupportedRecord);
            }
        }
        if !found && let Some(selected) = selected {
            mapper.diagnostic(&format!("composerData:{selected}"), Code::UnsupportedRecord);
        }
        // Orphan/alternate-branch rows are not part of the verified ordered spine.
        let selected_prefix = selected.map(|id| format!("bubbleId:{id}:"));
        for &key in rows.keys() {
            if key.starts_with("bubbleId:")
                && selected_prefix
                    .as_ref()
                    .is_none_or(|prefix| key.starts_with(prefix))
                && !referenced.contains(key)
            {
                mapper.diagnostic(key, Code::UnsupportedRecord);
            }
        }
        Ok(mapper.output)
    }
}

struct Mapper<'a> {
    snapshot: &'a NativeSnapshot,
    content: bool,
    output: MappedSnapshot,
}

impl Mapper<'_> {
    fn diagnostic(&mut self, key: &str, code: Code) {
        self.output.diagnostics.push(SnapshotDiagnostic {
            key: key.into(),
            code,
        });
    }

    fn object(&mut self, row: &SnapshotRecord) -> Option<Map<String, Value>> {
        match serde_json::from_slice(&row.bytes) {
            Ok(Value::Object(object)) => Some(object),
            _ => {
                self.diagnostic(&row.key, Code::InvalidField);
                None
            }
        }
    }

    fn record(&self, key: &str, kind: &str, session: &str) -> TelemetryRecord {
        TelemetryRecord {
            event_name: "unisphere.session.record".into(),
            timestamp_unix_nano: None,
            attributes: BTreeMap::from([
                ("unisphere.profile.version".into(), json!(1)),
                ("unisphere.source.adapter".into(), json!(IDE_DESCRIPTOR.id)),
                (
                    "unisphere.source.path".into(),
                    json!(self.snapshot.source.path.to_str()),
                ),
                ("unisphere.source.key".into(), json!(key)),
                (
                    "unisphere.source.revision".into(),
                    json!(self.snapshot.revision),
                ),
                ("unisphere.source.format".into(), json!("sqlite_key_value")),
                ("unisphere.source.kind".into(), json!(kind)),
                ("unisphere.source.session.id".into(), json!(session)),
                ("gen_ai.conversation.id".into(), json!(session)),
            ]),
            body: None,
        }
    }

    fn omitted(&mut self, key: &str, record: &mut TelemetryRecord) {
        if !self.content && !record.attributes.contains_key("unisphere.content.omitted") {
            record
                .attributes
                .insert("unisphere.content.omitted".into(), json!(true));
            self.diagnostic(key, Code::ContentOmitted);
        }
    }

    fn string(&mut self, key: &str, object: &mut Map<String, Value>, name: &str) -> Option<String> {
        match object.remove(name) {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) => Some(value),
            Some(_) => {
                self.diagnostic(key, Code::InvalidField);
                None
            }
        }
    }

    fn composer(
        &mut self,
        row: &SnapshotRecord,
        id: &str,
        rows: &BTreeMap<&str, &SnapshotRecord>,
        referenced: &mut BTreeSet<String>,
    ) {
        let key = row.key.as_str();
        let Some(mut object) = self.object(row) else {
            return;
        };
        if id.is_empty() || object.get("composerId").and_then(Value::as_str) != Some(id) {
            self.diagnostic(key, Code::InvalidField);
            return;
        }
        if object
            .get("_v")
            .and_then(Value::as_u64)
            .is_none_or(|version| version < 2)
        {
            self.diagnostic(key, Code::UnsupportedRecord);
            return;
        }
        let Some(Value::Array(headers)) = object.remove("fullConversationHeadersOnly") else {
            self.diagnostic(key, Code::InvalidField);
            return;
        };
        let mut record = self.record(key, "composerData", id);
        if let Some(created) = object.remove("createdAt") {
            record.timestamp_unix_nano = created.as_u64().and_then(|ms| ms.checked_mul(1_000_000));
            if record.timestamp_unix_nano.is_none() {
                self.diagnostic(key, Code::InvalidTimestamp);
            }
        }
        self.model(
            key,
            object.remove("modelConfig"),
            "unisphere.cursor.model_config.model_name",
            &mut record,
        );
        let mut body = Map::new();
        self.native_object(
            key,
            object.remove("usageData"),
            "unisphere.cursor.usage_data",
            &mut body,
            &mut record,
        );
        if !body.is_empty() {
            body.insert("type".into(), json!("unisphere.cursor.composer"));
            record.body = Some(Value::Object(body));
        }
        self.output.records.push(record);
        for header in headers {
            let Value::Object(header) = header else {
                self.diagnostic(key, Code::InvalidField);
                continue;
            };
            let Some(bubble_id) = header
                .get("bubbleId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                self.diagnostic(key, Code::InvalidField);
                continue;
            };
            let bubble_key = format!("bubbleId:{id}:{bubble_id}");
            if !referenced.insert(bubble_key.clone()) {
                self.diagnostic(key, Code::InvalidField);
                continue;
            }
            let Some(row) = rows.get(bubble_key.as_str()) else {
                self.diagnostic(&bubble_key, Code::InvalidField);
                continue;
            };
            self.bubble(
                row,
                id,
                bubble_id,
                header.get("type").and_then(Value::as_i64),
            );
        }
    }

    fn model(
        &mut self,
        key: &str,
        value: Option<Value>,
        attribute: &str,
        record: &mut TelemetryRecord,
    ) {
        let Some(value) = value else { return };
        let Value::Object(mut model) = value else {
            self.diagnostic(key, Code::InvalidField);
            return;
        };
        if let Some(name) = self.string(key, &mut model, "modelName") {
            record
                .attributes
                .insert(attribute.into(), Value::String(name));
        }
    }

    fn native_object(
        &mut self,
        key: &str,
        value: Option<Value>,
        field: &str,
        body: &mut Map<String, Value>,
        record: &mut TelemetryRecord,
    ) {
        let Some(value) = value else { return };
        let Value::Object(object) = value else {
            self.diagnostic(key, Code::InvalidField);
            return;
        };
        if !object.is_empty() {
            self.diagnostic(key, Code::UnsupportedPart);
            self.omitted(key, record);
            if self.content {
                body.insert(field.into(), Value::Object(object));
            }
        }
    }

    fn bubble(&mut self, row: &SnapshotRecord, session: &str, id: &str, header_type: Option<i64>) {
        let key = row.key.as_str();
        let Some(mut object) = self.object(row) else {
            return;
        };
        if object
            .get("bubbleId")
            .is_some_and(|value| value.as_str() != Some(id))
        {
            self.diagnostic(key, Code::InvalidField);
            return;
        }
        let kind = object.get("type").and_then(Value::as_i64);
        if kind.is_none() || kind != header_type {
            self.diagnostic(key, Code::InvalidField);
            return;
        }
        let kind = kind.unwrap();
        let mut record = self.record(key, &kind.to_string(), session);
        record
            .attributes
            .insert("unisphere.message.id".into(), json!(id));
        for (native, attribute) in [
            ("requestId", "unisphere.cursor.request.id"),
            ("checkpointId", "unisphere.cursor.checkpoint.id"),
        ] {
            if let Some(value) = self.string(key, &mut object, native) {
                record
                    .attributes
                    .insert(attribute.into(), Value::String(value));
            }
        }
        if let Some(created) = object.remove("createdAt") {
            record.timestamp_unix_nano = created
                .as_str()
                .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
                .and_then(|value| u64::try_from(value.unix_timestamp_nanos()).ok());
            if record.timestamp_unix_nano.is_none() {
                self.diagnostic(key, Code::InvalidTimestamp);
            }
        }
        for field in [
            "skipRendering",
            "isDisplayOnly",
            "isSimulatedMsg",
            "isPlanExecution",
        ] {
            if let Some(value) = object.get(field) {
                if value.is_boolean() {
                    record
                        .attributes
                        .insert(format!("unisphere.cursor.{field}"), value.clone());
                } else {
                    self.diagnostic(key, Code::InvalidField);
                }
            }
        }
        let control = object.get("capabilityType").and_then(Value::as_i64) == Some(22)
            || object.get("isSimulatedMsg").and_then(Value::as_bool) == Some(true);
        if !matches!(kind, 1 | 2) {
            self.diagnostic(key, Code::UnsupportedRecord);
            self.output.records.push(record);
            return;
        }
        let role = if kind == 1 { "user" } else { "assistant" };
        if !control {
            record
                .attributes
                .insert("unisphere.message.role".into(), json!(role));
        }
        if let Some(value) = object.get("capabilityType") {
            if value.as_i64().is_some() {
                record
                    .attributes
                    .insert("unisphere.cursor.capability_type".into(), value.clone());
            } else {
                self.diagnostic(key, Code::InvalidField);
            }
        }
        self.model(
            key,
            object.remove("modelInfo"),
            "unisphere.cursor.model_info.model_name",
            &mut record,
        );
        self.tokens(key, object.remove("tokenCount"), &mut record);
        let mut parts = Vec::new();
        for (field, part_type) in [("text", "text"), ("richText", "unisphere.cursor.rich_text")] {
            if object.contains_key(field) {
                self.omitted(key, &mut record);
            }
            if let Some(text) = self.string(key, &mut object, field)
                && self.content
            {
                parts.push(json!({"type": part_type, "content": text}));
            }
        }
        if let Some(thinking) = object.remove("thinking") {
            self.omitted(key, &mut record);
            if let Value::Object(mut thinking) = thinking {
                if let Some(text) = self.string(key, &mut thinking, "text")
                    && self.content
                {
                    parts.push(json!({"type":"reasoning", "content":text}));
                }
            } else {
                self.diagnostic(key, Code::InvalidField);
            }
        }
        if let Some(tool) = object.remove("toolFormerData") {
            self.omitted(key, &mut record);
            self.tool(key, tool, &mut parts);
        }
        if let Some(results) = object.remove("toolResults") {
            match results {
                Value::Array(results) => {
                    if !results.is_empty() {
                        self.omitted(key, &mut record);
                    }
                    for result in results {
                        if result.is_object() {
                            self.diagnostic(key, Code::UnsupportedPart);
                            if self.content {
                                parts.push(
                                    json!({"type":"unisphere.cursor.tool_result", "native":result}),
                                );
                            }
                        } else {
                            self.diagnostic(key, Code::InvalidField);
                        }
                    }
                }
                _ => self.diagnostic(key, Code::InvalidField),
            }
        }
        // Attachments and context are not silently dereferenced or treated as text.
        for field in [
            "images",
            "attachedCodeChunks",
            "contextPieces",
            "allThinkingBlocks",
        ] {
            if let Some(value) = object.get(field) {
                match value.as_array() {
                    Some(values) if values.is_empty() => {}
                    Some(_) => {
                        self.diagnostic(key, Code::UnsupportedPart);
                        self.omitted(key, &mut record);
                    }
                    None => self.diagnostic(key, Code::InvalidField),
                }
            }
        }
        if self.content && !parts.is_empty() {
            record.body = Some(if control {
                json!({"type":"unisphere.cursor.control", "parts":parts})
            } else {
                json!({"role":role, "parts":parts})
            });
        }
        self.output.records.push(record);
    }

    fn tokens(&mut self, key: &str, value: Option<Value>, record: &mut TelemetryRecord) {
        let Some(value) = value else { return };
        let Value::Object(object) = value else {
            self.diagnostic(key, Code::InvalidField);
            return;
        };
        let mut retained = false;
        for (field, attribute) in [
            ("inputTokens", "unisphere.cursor.token_count.input_tokens"),
            ("outputTokens", "unisphere.cursor.token_count.output_tokens"),
        ] {
            if let Some(value) = object.get(field) {
                if let Some(count) = value.as_i64().filter(|count| *count >= 0) {
                    record.attributes.insert(attribute.into(), json!(count));
                    retained = true;
                } else {
                    self.diagnostic(key, Code::InvalidField);
                }
            }
        }
        if object
            .keys()
            .any(|field| !matches!(field.as_str(), "inputTokens" | "outputTokens"))
        {
            self.diagnostic(key, Code::UnsupportedPart);
        }
        if retained {
            record.attributes.insert(
                "unisphere.cursor.token_count.scope".into(),
                json!("native_bubble_snapshot"),
            );
        }
    }

    fn tool(&mut self, key: &str, value: Value, parts: &mut Vec<Value>) {
        let Value::Object(mut tool) = value else {
            self.diagnostic(key, Code::InvalidField);
            return;
        };
        let name = self.string(key, &mut tool, "name");
        let id = self.string(key, &mut tool, "toolCallId");
        let arguments = tool.remove("params").or_else(|| tool.remove("rawArgs"));
        let result = tool.remove("result");
        let error = tool.remove("error");
        if name.is_none() {
            self.diagnostic(key, Code::UnsupportedPart);
        }
        if self.content {
            if let (Some(name), Some(arguments)) = (name, arguments) {
                let mut part = json!({"type":"tool_call", "name":name, "arguments":arguments});
                if let Some(id) = &id {
                    part["id"] = json!(id);
                }
                parts.push(part);
            }
            if let Some(response) = result {
                let mut part = json!({"type":"tool_call_response", "response":response});
                if let Some(id) = &id {
                    part["id"] = json!(id);
                }
                parts.push(part);
            }
            if let Some(error) = error {
                let mut part = json!({"type":"unisphere.cursor.tool_error", "error":error});
                if let Some(id) = id {
                    part["id"] = Value::String(id);
                }
                parts.push(part);
            }
        }
    }
}
