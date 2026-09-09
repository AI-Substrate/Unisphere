//! Run with `cargo run -p unisphere-output-otlp --example encode`.
//! Synthetic input exercises the public writer without any source-file access.
use serde_json::json;
use unisphere_core::{RecordWriter, TelemetryRecord};
use unisphere_output_otlp::OtlpJsonlWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let record = TelemetryRecord {
        event_name: "unisphere.example".into(),
        timestamp_unix_nano: Some(1_700_000_000_123_456_789),
        attributes: [("example.count".into(), json!(9_007_199_254_740_993_i64))].into(),
        body: Some(json!({"role": "user", "parts": [{"type": "text", "content": "hello\n世界"}]})),
    };
    OtlpJsonlWriter.write_batch(&[record], &mut std::io::stdout().lock())?;
    Ok(())
}
