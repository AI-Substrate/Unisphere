//! Pure projection of Cursor's agent-transcript JSONL, not IDE SQLite or CLI blobs.
//!
//! The native transcript writer has already discarded IDs, timing, model, usage
//! and tool-result data. This adapter preserves supplied physical records without
//! reconstructing those missing facts or interpreting text as structured events.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint, MappedBatch, MappingDiagnostic,
    MappingDiagnosticCode, MappingOptions, NativeRecord, PipelineError, PipelineErrorKind,
    SessionAdapter, SessionRef, TelemetryRecord,
};

/// Symbolic locations and capabilities; no discovery or installation detection.
pub const DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "cursor-transcript",
    application: "Cursor",
    description: "Cursor agent-transcript JSONL projection; native timing, model, usage and tool results are absent. Not IDE SQLite or CLI blob storage.",
    locations: &[LocationHint {
        platforms: &["macos", "linux", "windows"],
        base: "home",
        path: ".cursor/projects",
        session_glob: "*/agent-transcripts/**/*.jsonl",
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

/// Stateless mapper over caller-supplied, LF-framed native records.
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorAdapter;

impl SessionAdapter for CursorAdapter {
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

    fn omitted(&mut self, attributes: &mut BTreeMap<String, Value>) {
        if !self.include_content {
            attributes.insert("unisphere.content.omitted".into(), json!(true));
            self.diagnostic(MappingDiagnosticCode::ContentOmitted);
        }
    }

    fn record(&mut self, value: Value, path: &str) -> TelemetryRecord {
        let mut attributes = BTreeMap::from([
            ("unisphere.profile.version".into(), json!(1)),
            ("unisphere.source.adapter".into(), json!(DESCRIPTOR.id)),
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
        // A native control record must never become a turn just because it also
        // carries a role/message-looking payload. Cursor messages have no type.
        let control = object.contains_key("type");
        let kind = match object.remove(if control { "type" } else { "role" }) {
            Some(Value::String(kind)) => kind,
            _ => {
                self.diagnostic(MappingDiagnosticCode::InvalidField);
                "unknown".into()
            }
        };
        let body = match (control, kind.as_str()) {
            (false, "user" | "assistant" | "tool") => {
                attributes.insert("unisphere.message.role".into(), json!(kind));
                self.message(object.remove("message"), &kind, &mut attributes)
            }
            (true, "metadata") => self.overview(object.remove("metadata"), &mut attributes),
            (true, "turn_ended") => self.turn_ended(object, &mut attributes),
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedRecord);
                None
            }
        };
        attributes.insert("unisphere.source.kind".into(), Value::String(kind));
        TelemetryRecord {
            event_name: "unisphere.session.record".into(),
            timestamp_unix_nano: None,
            attributes,
            body,
        }
    }

    fn message(
        &mut self,
        value: Option<Value>,
        role: &str,
        attributes: &mut BTreeMap<String, Value>,
    ) -> Option<Value> {
        let Some(Value::Object(mut message)) = value else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let content = message.remove("content");
        if content.is_some() {
            self.omitted(attributes);
        }
        let Some(Value::Array(content)) = content else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let parts: Vec<_> = content.into_iter().filter_map(|part| self.part(part)).collect();
        self.include_content.then(|| json!({"role": role, "parts": parts}))
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
        match kind.as_str() {
            "text" => {
                let Some(Value::String(content)) = part.remove("text") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                self.include_content.then(|| json!({"type": "text", "content": content}))
            }
            "tool_use" => {
                let Some(Value::String(name)) = part.remove("name") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                let Some(arguments) = part.remove("input") else {
                    self.diagnostic(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                self.include_content.then(|| {
                    json!({"type": "tool_call", "name": name, "arguments": arguments})
                })
            }
            _ => {
                self.diagnostic(MappingDiagnosticCode::UnsupportedPart);
                self.include_content.then(|| {
                    json!({"type": "unisphere.unknown", "native_type": kind})
                })
            }
        }
    }

    fn overview(
        &mut self,
        value: Option<Value>,
        attributes: &mut BTreeMap<String, Value>,
    ) -> Option<Value> {
        let Some(Value::Object(mut metadata)) = value else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let overview = metadata.remove("overview");
        if overview.is_some() {
            self.omitted(attributes);
        }
        let Some(Value::String(overview)) = overview else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        self.include_content.then(|| {
            json!({"type": "unisphere.cursor.metadata", "overview": overview})
        })
    }

    fn turn_ended(
        &mut self,
        mut object: Map<String, Value>,
        attributes: &mut BTreeMap<String, Value>,
    ) -> Option<Value> {
        match object.remove("status") {
            Some(Value::String(status))
                if matches!(status.as_str(), "success" | "error" | "aborted") =>
            {
                attributes.insert("unisphere.cursor.turn.status".into(), Value::String(status));
            }
            _ => self.diagnostic(MappingDiagnosticCode::InvalidField),
        }
        let error = object.remove("error")?;
        self.omitted(attributes);
        let Value::String(error) = error else {
            self.diagnostic(MappingDiagnosticCode::InvalidField);
            return None;
        };
        self.include_content.then(|| {
            json!({"type": "unisphere.cursor.turn_ended", "error": error})
        })
    }
}
