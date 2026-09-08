use std::io::{self, Write};

use serde_json::{Value, json};
use unisphere_core::{MAX_OUTPUT_BATCH_BYTES, PipelineError, PipelineErrorKind, RecordWriter, TelemetryRecord};
use unisphere_output_otlp::OtlpJsonlWriter;

fn record(body: Option<Value>) -> TelemetryRecord {
    TelemetryRecord {
        event_name: "unisphere.session.record".into(),
        timestamp_unix_nano: None,
        attributes: Default::default(),
        body,
    }
}

fn encode(records: &[TelemetryRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    OtlpJsonlWriter.write_batch(records, &mut bytes).unwrap();
    bytes
}

#[test]
fn encodes_recursive_any_values_and_only_legal_otlp_fields() {
    let mut input = record(Some(json!({
        "role": "assistant",
        "parts": [{"type": "tool_call", "arguments": {"enabled": true, "items": [null, 1.25]}}]
    })));
    input.event_name = "event\n\"雪".into();
    input.timestamp_unix_nano = Some(u64::MAX);
    input.attributes = [
        ("array".into(), json!([])),
        ("double".into(), json!(1.0)),
        ("integer".into(), json!(9_007_199_254_740_993_i64)),
        ("maximum".into(), json!(i64::MAX)),
        ("minimum".into(), json!(i64::MIN)),
        ("null".into(), Value::Null),
        ("object".into(), json!({})),
        ("quote\"\n".into(), json!("λ\n\t\u{0000}\\\"")),
    ].into();

    let encoded = encode(&[input]);
    let text = std::str::from_utf8(&encoded).unwrap();
    assert!(text.ends_with('\n'));
    assert_eq!(text.bytes().filter(|byte| *byte == b'\n').count(), 1);
    let document: Value = serde_json::from_str(text).unwrap();
    let expected: Value = serde_json::from_str(r#"{
        "resourceLogs": [{"scopeLogs": [{
            "scope": {"name": "unisphere", "version": "0.1.0"},
            "logRecords": [{
                "eventName": "event\n\"雪",
                "timeUnixNano": "18446744073709551615",
                "attributes": [
                    {"key": "array", "value": {"arrayValue": {"values": []}}},
                    {"key": "double", "value": {"doubleValue": 1.0}},
                    {"key": "integer", "value": {"intValue": "9007199254740993"}},
                    {"key": "maximum", "value": {"intValue": "9223372036854775807"}},
                    {"key": "minimum", "value": {"intValue": "-9223372036854775808"}},
                    {"key": "null", "value": {}},
                    {"key": "object", "value": {"kvlistValue": {"values": []}}},
                    {"key": "quote\"\n", "value": {"stringValue": "λ\n\t\u0000\\\""}}
                ],
                "body": {"kvlistValue": {"values": [
                    {"key": "parts", "value": {"arrayValue": {"values": [
                        {"kvlistValue": {"values": [
                            {"key": "arguments", "value": {"kvlistValue": {"values": [
                                {"key": "enabled", "value": {"boolValue": true}},
                                {"key": "items", "value": {"arrayValue": {"values": [{}, {"doubleValue": 1.25}]}}}
                            ]}}},
                            {"key": "type", "value": {"stringValue": "tool_call"}}
                        ]}}
                    ]}}},
                    {"key": "role", "value": {"stringValue": "assistant"}}
                ]}}
            }]
        }]}]
    }"#).unwrap();
    assert_eq!(document, expected);
}

#[test]
fn preserves_record_order_batch_framing_and_absent_vs_empty_values() {
    let absent = record(None);
    let mut explicit = record(Some(Value::Null));
    explicit.timestamp_unix_nano = Some(0);
    let mut bytes = Vec::new();
    OtlpJsonlWriter.write_batch(&[absent, explicit], &mut bytes).unwrap();
    OtlpJsonlWriter.write_batch(&[], &mut bytes).unwrap();
    OtlpJsonlWriter.write_batch(&[record(Some(json!(false)))], &mut bytes).unwrap();
    let lines: Vec<Value> = std::str::from_utf8(&bytes).unwrap().lines()
        .map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"], json!([
        {"eventName": "unisphere.session.record", "attributes": []},
        {"eventName": "unisphere.session.record", "attributes": [], "timeUnixNano": "0", "body": {}}
    ]));
    assert_eq!(lines[1]["resourceLogs"][0]["scopeLogs"][0]["logRecords"], json!([
        {"eventName": "unisphere.session.record", "attributes": [], "body": {"boolValue": false}}
    ]));
}

#[derive(Clone, Copy)]
enum Fault {
    None,
    WriteAfter(usize),
    WriteZero,
    Flush,
    Interrupted,
}

struct Sink {
    bytes: Vec<u8>,
    fault: Fault,
    chunk: usize,
    writes: usize,
    flushes: usize,
}

impl Sink {
    fn new(fault: Fault) -> Self {
        Self { bytes: Vec::new(), fault, chunk: usize::MAX, writes: 0, flushes: 0 }
    }
}

impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        let remaining = match self.fault {
            Fault::WriteAfter(limit) if self.bytes.len() >= limit => {
                return Err(io::Error::other("SENSITIVE-output-path-and-data"));
            }
            Fault::WriteAfter(limit) => limit - self.bytes.len(),
            Fault::WriteZero => return Ok(0),
            Fault::Interrupted => {
                self.fault = Fault::None;
                return Err(io::ErrorKind::Interrupted.into());
            }
            _ => usize::MAX,
        };
        let written = bytes.len().min(self.chunk).min(remaining);
        self.bytes.extend_from_slice(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        if matches!(self.fault, Fault::Flush) {
            Err(io::Error::other("SENSITIVE-flush-path-and-data"))
        } else {
            Ok(())
        }
    }
}

fn assert_safe(error: &PipelineError, kind: PipelineErrorKind) {
    assert_eq!(error.kind(), kind);
    assert_eq!(error.offset(), None);
    let diagnostics = format!("{error:?} {error} {} {} {}", error.code(), error.message(), error.fix());
    assert!(!diagnostics.contains("SENSITIVE"));
    assert!(std::error::Error::source(error).is_none());
}

#[test]
fn empty_batches_never_write_or_flush() {
    let mut sink = Sink::new(Fault::Flush);
    OtlpJsonlWriter.write_batch(&[], &mut sink).unwrap();
    assert_eq!((sink.writes, sink.flushes), (0, 0));
}

#[test]
fn invalid_late_values_reject_entire_batch_before_destination_access() {
    let too_large = json!(i64::MAX as u64 + 1);
    let mut invalid_attribute = record(None);
    invalid_attribute.attributes.insert("SENSITIVE-key".into(), json!({"nested": [too_large]}));
    let invalid_body = record(Some(json!(["SENSITIVE-body", u64::MAX])));
    for invalid in [invalid_attribute, invalid_body] {
        let mut sink = Sink::new(Fault::None);
        let error = OtlpJsonlWriter.write_batch(&[record(None), invalid], &mut sink).unwrap_err();
        assert_safe(&error, PipelineErrorKind::InvalidData);
        assert_eq!((sink.writes, sink.flushes), (0, 0));
    }
}

#[test]
fn write_all_retries_interruptions_and_short_writes_before_flushing() {
    let records = [record(Some(json!("several writes: λ\n雪")))];
    let expected = encode(&records);
    let mut sink = Sink::new(Fault::Interrupted);
    sink.chunk = 3;
    OtlpJsonlWriter.write_batch(&records, &mut sink).unwrap();
    assert_eq!(sink.bytes, expected);
    assert_eq!(sink.flushes, 1);
}

#[test]
fn failed_or_zero_writes_report_safe_errors_without_flushing() {
    let records = [record(Some(json!("SENSITIVE-body")))];
    let expected = encode(&records);
    for (fault, accepted) in [(Fault::WriteAfter(0), 0), (Fault::WriteAfter(17), 17), (Fault::WriteZero, 0)] {
        let mut sink = Sink::new(fault);
        let error = OtlpJsonlWriter.write_batch(&records, &mut sink).unwrap_err();
        assert_safe(&error, PipelineErrorKind::Write);
        assert_eq!(sink.bytes, expected[..accepted]);
        assert_eq!(sink.flushes, 0);
    }
}

#[test]
fn flush_failure_reports_failure_despite_complete_written_bytes() {
    let records = [record(None)];
    let mut sink = Sink::new(Fault::Flush);
    let error = OtlpJsonlWriter.write_batch(&records, &mut sink).unwrap_err();
    assert_safe(&error, PipelineErrorKind::Write);
    assert_eq!(sink.bytes, encode(&records));
    assert_eq!(sink.flushes, 1);
}

#[test]
fn encoded_limit_includes_newline_and_rejects_before_destination_access() {
    let overhead = encode(&[record(Some(json!("")))]).len();
    let content = "x".repeat(MAX_OUTPUT_BATCH_BYTES - overhead);
    let mut input = record(Some(Value::String(content)));
    let mut exact = Sink::new(Fault::None);
    OtlpJsonlWriter.write_batch(std::slice::from_ref(&input), &mut exact).unwrap();
    assert_eq!(exact.bytes.len(), MAX_OUTPUT_BATCH_BYTES);
    assert_eq!(exact.bytes.last(), Some(&b'\n'));
    assert_eq!(exact.flushes, 1);
    // One additional input byte now makes the final LF exceed the budget.
    if let Some(Value::String(content)) = &mut input.body {
        content.push('x');
    }
    let mut rejected = Sink::new(Fault::None);
    let error = OtlpJsonlWriter.write_batch(&[input], &mut rejected).unwrap_err();
    assert_safe(&error, PipelineErrorKind::OutputLimit);
    assert_eq!((rejected.writes, rejected.flushes), (0, 0));
}

#[test]
fn output_budget_counts_json_escape_expansion_not_input_length() {
    let input = record(Some(Value::String("\0".repeat(MAX_OUTPUT_BATCH_BYTES / 6))));
    let mut sink = Sink::new(Fault::None);
    let error = OtlpJsonlWriter.write_batch(&[input], &mut sink).unwrap_err();
    assert_safe(&error, PipelineErrorKind::OutputLimit);
    assert_eq!((sink.writes, sink.flushes), (0, 0));
}
