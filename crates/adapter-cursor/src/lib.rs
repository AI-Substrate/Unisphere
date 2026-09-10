//! Pure, distinct projections for Cursor transcript JSONL and IDE snapshots.
//!
//! Neither mapper reconstructs facts discarded by its native source, reads
//! storage, or interprets content as executable instructions.
#![forbid(unsafe_code)]

mod ide;
mod query;

pub use ide::{CursorIdeAdapter, IDE_DESCRIPTOR};

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
            let classified = classify_transcript(&native.bytes, native.offset)?;
            batch
                .diagnostics
                .extend(classified.diagnostics.iter().map(|code| MappingDiagnostic {
                    offset: native.offset,
                    code: *code,
                }));
            let mut attributes = BTreeMap::from([
                ("unisphere.profile.version".into(), json!(1)),
                ("unisphere.source.adapter".into(), json!(DESCRIPTOR.id)),
                ("unisphere.source.path".into(), json!(path)),
                ("unisphere.source.offset".into(), json!(native.offset)),
                (
                    "unisphere.source.kind".into(),
                    Value::String(classified.kind.clone()),
                ),
            ]);
            if let Some(role) = &classified.role {
                attributes.insert("unisphere.message.role".into(), json!(role));
            }
            if let Some(status) = &classified.turn_status {
                attributes.insert(
                    "unisphere.cursor.turn.status".into(),
                    Value::String(status.clone()),
                );
            }
            if classified.sensitive_content && !options.include_content {
                attributes.insert("unisphere.content.omitted".into(), json!(true));
                batch.diagnostics.push(MappingDiagnostic {
                    offset: native.offset,
                    code: MappingDiagnosticCode::ContentOmitted,
                });
            }
            batch.records.push(TelemetryRecord {
                event_name: "unisphere.session.record".into(),
                timestamp_unix_nano: None,
                attributes,
                body: options.include_content.then_some(classified.body).flatten(),
            });
        }
        Ok(batch)
    }
}

pub(crate) struct ClassifiedTranscript {
    pub(crate) kind: String,
    pub(crate) role: Option<String>,
    pub(crate) body: Option<Value>,
    pub(crate) turn_status: Option<String>,
    pub(crate) sensitive_content: bool,
    pub(crate) diagnostics: Vec<MappingDiagnosticCode>,
}

pub(crate) fn classify_transcript(
    bytes: &[u8],
    offset: u64,
) -> Result<ClassifiedTranscript, PipelineError> {
    let value = serde_json::from_slice(bytes)
        .map_err(|_| PipelineError::new(PipelineErrorKind::InvalidData, Some(offset)))?;
    let mut classifier = TranscriptClassifier::default();
    let mut object = match value {
        Value::Object(object) => object,
        _ => {
            classifier
                .diagnostics
                .push(MappingDiagnosticCode::InvalidField);
            Map::new()
        }
    };
    // A native control record must never become a turn just because it also
    // carries a role/message-looking payload. Cursor messages have no type.
    let control = object.contains_key("type");
    let kind = match object.remove(if control { "type" } else { "role" }) {
        Some(Value::String(kind)) => kind,
        _ => {
            classifier
                .diagnostics
                .push(MappingDiagnosticCode::InvalidField);
            "unknown".into()
        }
    };
    let mut role = None;
    let mut turn_status = None;
    let body = match (control, kind.as_str()) {
        (false, "user" | "assistant" | "tool") => {
            role = Some(kind.clone());
            classifier.message(object.remove("message"), &kind)
        }
        (true, "metadata") => classifier.overview(object.remove("metadata")),
        (true, "turn_ended") => {
            turn_status = match object.remove("status") {
                Some(Value::String(status))
                    if matches!(status.as_str(), "success" | "error" | "aborted") =>
                {
                    Some(status)
                }
                _ => {
                    classifier
                        .diagnostics
                        .push(MappingDiagnosticCode::InvalidField);
                    None
                }
            };
            classifier.turn_ended(object)
        }
        _ => {
            classifier
                .diagnostics
                .push(MappingDiagnosticCode::UnsupportedRecord);
            None
        }
    };
    Ok(ClassifiedTranscript {
        kind,
        role,
        body,
        turn_status,
        sensitive_content: classifier.sensitive_content,
        diagnostics: classifier.diagnostics,
    })
}

#[derive(Default)]
struct TranscriptClassifier {
    sensitive_content: bool,
    diagnostics: Vec<MappingDiagnosticCode>,
}

impl TranscriptClassifier {
    fn message(&mut self, value: Option<Value>, role: &str) -> Option<Value> {
        let Some(Value::Object(mut message)) = value else {
            self.diagnostics.push(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let content = message.remove("content");
        self.sensitive_content |= content.is_some();
        let Some(Value::Array(content)) = content else {
            self.diagnostics.push(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let parts: Vec<_> = content
            .into_iter()
            .filter_map(|part| self.part(part))
            .collect();
        Some(json!({"role": role, "parts": parts}))
    }

    fn part(&mut self, value: Value) -> Option<Value> {
        let Value::Object(mut part) = value else {
            self.diagnostics.push(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let Some(Value::String(kind)) = part.remove("type") else {
            self.diagnostics.push(MappingDiagnosticCode::InvalidField);
            return None;
        };
        match kind.as_str() {
            "text" => {
                let Some(Value::String(content)) = part.remove("text") else {
                    self.diagnostics.push(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                Some(json!({"type": "text", "content": content}))
            }
            "tool_use" => {
                let Some(Value::String(name)) = part.remove("name") else {
                    self.diagnostics.push(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                let Some(arguments) = part.remove("input") else {
                    self.diagnostics.push(MappingDiagnosticCode::InvalidField);
                    return None;
                };
                Some(json!({"type": "tool_call", "name": name, "arguments": arguments}))
            }
            _ => {
                self.diagnostics
                    .push(MappingDiagnosticCode::UnsupportedPart);
                Some(json!({"type": "unisphere.unknown", "native_type": kind}))
            }
        }
    }

    fn overview(&mut self, value: Option<Value>) -> Option<Value> {
        let Some(Value::Object(mut metadata)) = value else {
            self.diagnostics.push(MappingDiagnosticCode::InvalidField);
            return None;
        };
        let overview = metadata.remove("overview");
        self.sensitive_content |= overview.is_some();
        let Some(Value::String(overview)) = overview else {
            self.diagnostics.push(MappingDiagnosticCode::InvalidField);
            return None;
        };
        Some(json!({"type": "unisphere.cursor.metadata", "overview": overview}))
    }

    fn turn_ended(&mut self, mut object: Map<String, Value>) -> Option<Value> {
        let error = object.remove("error")?;
        self.sensitive_content = true;
        let Value::String(error) = error else {
            self.diagnostics.push(MappingDiagnosticCode::InvalidField);
            return None;
        };
        Some(json!({"type": "unisphere.cursor.turn_ended", "error": error}))
    }
}
