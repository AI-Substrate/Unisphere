//! Canonical OTLP LogsData JSONL encoding over a caller-supplied destination.
//!
//! This crate does not open files, inspect source data, read ambient state, or
//! contact a collector. Source semantics belong to adapters; encoding lives here.
#![forbid(unsafe_code)]

use std::io::{self, Write};

use serde_json::Value;
use unisphere_core::{MAX_OUTPUT_BATCH_BYTES, PipelineError, PipelineErrorKind, RecordWriter, TelemetryRecord};

/// Stateless encoder for one LF-terminated OTLP LogsData object per nonempty batch.
///
/// Serialization completes within [`MAX_OUTPUT_BATCH_BYTES`] (including LF)
/// before the destination is touched. Integers outside OTLP's signed 64-bit
/// AnyValue range are rejected rather than rounded or converted to strings.
/// Successful nonempty writes include a destination flush. Empty batches perform
/// no destination operations. A destination error may leave partial output;
/// callers must not accept a checkpoint or assume rollback after an error.
#[derive(Debug, Default, Clone, Copy)]
pub struct OtlpJsonlWriter;

impl RecordWriter for OtlpJsonlWriter {
    fn write_batch(
        &self,
        records: &[TelemetryRecord],
        destination: &mut dyn Write,
    ) -> Result<(), PipelineError> {
        if records.is_empty() {
            return Ok(());
        }
        let mut buffer = BatchBuffer::default();
        encode_batch(records, &mut buffer).map_err(|error| {
            let kind = if error.kind() == io::ErrorKind::FileTooLarge {
                PipelineErrorKind::OutputLimit
            } else {
                PipelineErrorKind::InvalidData
            };
            PipelineError::new(kind, None)
        })?;
        destination
            .write_all(&buffer.bytes)
            .and_then(|()| destination.flush())
            .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))
    }
}

/// Only this bounded staging sink is reachable during serialization.
#[derive(Default)]
struct BatchBuffer {
    bytes: Vec<u8>,
}

impl Write for BatchBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_OUTPUT_BATCH_BYTES - self.bytes.len() {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        let required = self.bytes.len() + bytes.len();
        if required > self.bytes.capacity() {
            let capacity = required
                .max(self.bytes.capacity().saturating_mul(2))
                .min(MAX_OUTPUT_BATCH_BYTES);
            self.bytes.reserve_exact(capacity - self.bytes.len());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encode_batch(records: &[TelemetryRecord], buffer: &mut BatchBuffer) -> io::Result<()> {
    buffer.write_all(
        br#"{"resourceLogs":[{"scopeLogs":[{"scope":{"name":"unisphere","version":"0.1.0"},"logRecords":["#,
    )?;
    for (index, record) in records.iter().enumerate() {
        if index != 0 {
            buffer.write_all(b",")?;
        }
        buffer.write_all(br#"{"eventName":"#)?;
        encode_string(&record.event_name, buffer)?;
        if let Some(timestamp) = record.timestamp_unix_nano {
            write!(buffer, r#","timeUnixNano":"{timestamp}""#)?;
        }
        buffer.write_all(br#","attributes":"#)?;
        encode_key_values(record.attributes.iter(), buffer)?;
        if let Some(body) = &record.body {
            buffer.write_all(br#","body":"#)?;
            encode_value(body, buffer)?;
        }
        buffer.write_all(b"}")?;
    }
    buffer.write_all(b"]}]}]}\n")
}

fn encode_string(value: &str, buffer: &mut BatchBuffer) -> io::Result<()> {
    serde_json::to_writer(buffer, value).map_err(io::Error::from)
}

fn encode_key_values<'a>(
    entries: impl Iterator<Item = (&'a String, &'a Value)>,
    buffer: &mut BatchBuffer,
) -> io::Result<()> {
    buffer.write_all(b"[")?;
    for (index, (key, value)) in entries.enumerate() {
        if index != 0 {
            buffer.write_all(b",")?;
        }
        buffer.write_all(br#"{"key":"#)?;
        encode_string(key, buffer)?;
        buffer.write_all(br#","value":"#)?;
        encode_value(value, buffer)?;
        buffer.write_all(b"}")?;
    }
    buffer.write_all(b"]")
}

fn encode_value(value: &Value, buffer: &mut BatchBuffer) -> io::Result<()> {
    match value {
        Value::Null => buffer.write_all(b"{}"),
        Value::Bool(value) => write!(buffer, "{{\"boolValue\":{value}}}"),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                write!(buffer, "{{\"intValue\":\"{integer}\"}}")
            } else if value.is_f64() {
                write!(buffer, "{{\"doubleValue\":{value}}}")
            } else {
                Err(io::ErrorKind::InvalidData.into())
            }
        }
        Value::String(value) => {
            buffer.write_all(br#"{"stringValue":"#)?;
            encode_string(value, buffer)?;
            buffer.write_all(b"}")
        }
        Value::Array(values) => {
            buffer.write_all(br#"{"arrayValue":{"values":["#)?;
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    buffer.write_all(b",")?;
                }
                encode_value(value, buffer)?;
            }
            buffer.write_all(b"]}}")
        }
        Value::Object(values) => {
            buffer.write_all(br#"{"kvlistValue":{"values":"#)?;
            encode_key_values(values.iter(), buffer)?;
            buffer.write_all(b"}}")
        }
    }
}
