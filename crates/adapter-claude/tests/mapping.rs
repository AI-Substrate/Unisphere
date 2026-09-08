use serde_json::{Value, json};
use unisphere_adapter_claude::ClaudeCodeAdapter;
use unisphere_core::{
    MappedBatch, MappingDiagnosticCode as Code, MappingOptions, NativeRecord,
    PipelineErrorKind, SessionAdapter, SessionRef,
};
use unisphere_testkit::collection::{
    CLAUDE_BASIC, CLAUDE_PARTS, assert_adapter_conformance, fixture_records,
};

fn source() -> SessionRef {
    SessionRef { path: "/synthetic/not-opened/session.jsonl".into() }
}

fn map(records: &[NativeRecord], include_content: bool) -> MappedBatch {
    ClaudeCodeAdapter.map(&source(), records, MappingOptions { include_content }).unwrap()
}

fn native(value: Value) -> NativeRecord {
    NativeRecord { offset: 73, bytes: serde_json::to_vec(&value).unwrap() }
}

fn message(content: Value) -> Value {
    json!({"type": "assistant", "message": {"role": "assistant", "content": content}})
}

fn has_diagnostic(batch: &MappedBatch, offset: u64, code: Code) -> bool {
    batch.diagnostics.iter().any(|entry| entry.offset == offset && entry.code == code)
}

#[test]
fn shared_conformance_over_both_native_fixtures() {
    for fixture in [CLAUDE_BASIC, CLAUDE_PARTS] {
        assert_adapter_conformance(&ClaudeCodeAdapter, &source(), &fixture_records(fixture));
    }
}

#[test]
fn repeated_logical_ids_retain_physical_records_and_usage_snapshots() {
    let records = fixture_records(CLAUDE_BASIC);
    let batch = map(&records, true);
    assert!(batch.diagnostics.is_empty());
    assert_eq!(batch.records[0].body, Some(json!({
        "role": "user", "parts": [{"type": "text", "content": "SENSITIVE-USER-CONTENT"}],
    })));
    let first = &batch.records[1];
    let second = &batch.records[2];
    assert_eq!(first.attributes["unisphere.message.id"], "response-shared");
    assert_eq!(second.attributes["unisphere.message.id"], "response-shared");
    assert_eq!(first.attributes["unisphere.source.record.id"], "record-answer-1");
    assert_eq!(second.attributes["unisphere.source.record.id"], "record-answer-2");
    assert_eq!(first.attributes["unisphere.source.parent.id"], "record-user");
    assert_eq!(second.attributes["unisphere.source.parent.id"], "record-answer-1");
    assert_eq!(first.attributes["gen_ai.conversation.id"], "fixture-session");
    assert_eq!(first.attributes["gen_ai.response.model"], "claude-fixture");
    assert_eq!(first.attributes["unisphere.usage.input_tokens"], 10);
    assert_eq!(second.attributes["unisphere.usage.input_tokens"], 10);
    assert_eq!(first.attributes["unisphere.usage.output_tokens"], 2);
    assert_eq!(second.attributes["unisphere.usage.output_tokens"], 3);
    assert_eq!(first.attributes["unisphere.usage.cache_read_input_tokens"], 4);
    assert_eq!(first.attributes["unisphere.usage.cache_creation_input_tokens"], 1);
    assert!(!second.attributes.contains_key("unisphere.usage.cache_read_input_tokens"));
    assert_eq!(first.attributes["unisphere.usage.scope"], "native_record_snapshot");
    for (record, supplied) in batch.records.iter().zip(&records) {
        assert_eq!(record.event_name, "unisphere.session.record");
        assert_eq!(record.attributes["unisphere.source.offset"], supplied.offset);
        assert!(!record.attributes.keys().any(|key| key.starts_with("gen_ai.usage.")));
        assert!(!record.attributes.contains_key("gen_ai.provider.name"));
        assert!(!record.attributes.contains_key("trace_id"));
        assert!(!record.attributes.contains_key("span_id"));
    }
    assert_eq!(first.body.as_ref().unwrap()["parts"][0]["content"], "SENSITIVE-ANSWER-ONE");
    assert_eq!(second.body.as_ref().unwrap()["parts"][0]["content"], "SENSITIVE-ANSWER-TWO");
}

#[test]
fn native_parts_remain_structured_and_sidechain_session_is_not_inferred() {
    let records = fixture_records(CLAUDE_PARTS);
    let batch = map(&records, true);
    assert_eq!(batch.records[0].body, Some(json!({
        "role": "assistant", "parts": [
            {"type": "reasoning", "content": "SENSITIVE-REASONING"},
            {"type": "tool_call", "id": "tool-1", "name": "Read", "arguments": {"path": "SENSITIVE-ARGUMENT"}},
            {"type": "unisphere.unknown", "native_type": "future_part"},
        ],
    })));
    assert_eq!(batch.records[1].body, Some(json!({
        "role": "user", "parts": [{
            "type": "tool_call_response", "id": "tool-1",
            "response": {"nested": ["SENSITIVE-TOOL-RESULT", null, true]},
            "unisphere.is_error": true,
        }],
    })));
    assert_eq!(batch.records[1].attributes["gen_ai.conversation.id"], "fixture-parent");
    assert_eq!(batch.records[1].attributes["unisphere.source.is_sidechain"], true);
    assert_eq!(batch.records[1].attributes["unisphere.source.parent.id"], "record-tools");
    assert_eq!(batch.records[2].attributes["unisphere.source.kind"], "future_record");
    assert!(batch.records[2].body.is_none());
    assert!(has_diagnostic(&batch, records[0].offset, Code::UnsupportedPart));
    assert!(has_diagnostic(&batch, records[2].offset, Code::UnsupportedRecord));
    let public = serde_json::to_string(&batch).unwrap();
    for omitted in ["SENSITIVE-UNKNOWN-PART", "SENSITIVE-UNKNOWN-RECORD", "DO-NOT-PROMOTE-SIGNATURE", "private-spill"] {
        assert!(!public.contains(omitted));
    }
}

#[test]
fn default_policy_omits_every_content_category_but_retains_structural_diagnostics() {
    let records = fixture_records(CLAUDE_PARTS);
    let batch = ClaudeCodeAdapter.map(&source(), &records, MappingOptions::default()).unwrap();
    let public = serde_json::to_string(&batch).unwrap();
    assert!(!public.contains("SENSITIVE-"));
    assert!(!public.contains("private-spill"));
    for record in &batch.records { assert!(record.body.is_none()); }
    for native in &records[..2] {
        assert!(has_diagnostic(&batch, native.offset, Code::ContentOmitted));
    }
    assert_eq!(batch.records[0].attributes["unisphere.content.omitted"], true);
    assert_eq!(batch.records[1].attributes["unisphere.content.omitted"], true);
    assert!(has_diagnostic(&batch, records[0].offset, Code::UnsupportedPart));
    assert!(has_diagnostic(&batch, records[2].offset, Code::UnsupportedRecord));
}

#[test]
fn redacted_and_attachment_parts_are_markers_not_payloads_or_dereferences() {
    let record = native(message(json!([
        {"type": "redacted_thinking", "data": "SENSITIVE-REDACTED"},
        {"type": "image", "source": {"type": "url", "url": "file:///SENSITIVE-ATTACHMENT"}},
        {"type": "text", "text": "visible"},
    ])));
    let batch = map(std::slice::from_ref(&record), true);
    assert_eq!(batch.records[0].body, Some(json!({"role": "assistant", "parts": [
        {"type": "unisphere.unknown", "native_type": "redacted_thinking"},
        {"type": "unisphere.unknown", "native_type": "image"},
        {"type": "text", "content": "visible"},
    ]})));
    assert!(!serde_json::to_string(&batch).unwrap().contains("SENSITIVE-"));
    assert_eq!(batch.diagnostics.iter().filter(|d| d.code == Code::UnsupportedPart).count(), 2);
    let metadata = map(&[record], false);
    assert!(metadata.records[0].body.is_none());
    assert!(has_diagnostic(&metadata, 73, Code::ContentOmitted));
    assert!(has_diagnostic(&metadata, 73, Code::UnsupportedPart));
}

#[test]
fn missing_optional_facts_remain_absent_and_empty_content_is_not_invented() {
    let batch = map(&[native(message(json!([])))], true);
    let attributes = &batch.records[0].attributes;
    assert!(batch.diagnostics.is_empty());
    for key in ["gen_ai.conversation.id", "gen_ai.response.model", "unisphere.message.id", "unisphere.source.record.id", "unisphere.source.parent.id", "unisphere.source.is_sidechain", "unisphere.usage.scope"] {
        assert!(!attributes.contains_key(key));
    }
    assert!(batch.records[0].timestamp_unix_nano.is_none());
    assert_eq!(batch.records[0].body, Some(json!({"role": "assistant", "parts": []})));
}

#[test]
fn malformed_message_fields_are_not_filled_from_record_type() {
    let records = [
        native(json!({"type": "user"})),
        native(json!({"type": "assistant", "message": {"content": "SENSITIVE-NO-ROLE"}})),
        native(json!({"type": "assistant", "message": {"role": "assistant"}})),
        native(message(json!(42))),
    ];
    let batch = map(&records, true);
    assert_eq!(batch.records.len(), records.len());
    for record in &batch.records { assert!(record.body.is_none()); }
    assert!(!batch.records[1].attributes.contains_key("unisphere.message.role"));
    assert!(has_diagnostic(&batch, 73, Code::InvalidField));
    assert!(!serde_json::to_string(&batch).unwrap().contains("SENSITIVE-NO-ROLE"));
}

#[test]
fn unknown_or_nonobject_records_retain_provenance_without_promoting_payloads() {
    let records = [
        native(json!({"type": "future", "uuid": "physical", "payload": "SENSITIVE-PAYLOAD", "message": {"model": "not-a-response"}})),
        native(json!({"type": 9, "uuid": "physical-two"})),
        native(json!(["SENSITIVE-NONOBJECT"])),
    ];
    let batch = map(&records, true);
    assert_eq!(batch.records.len(), 3);
    assert_eq!(batch.records[0].attributes["unisphere.source.kind"], "future");
    assert_eq!(batch.records[0].attributes["unisphere.source.record.id"], "physical");
    assert!(!batch.records[0].attributes.contains_key("gen_ai.response.model"));
    assert_eq!(batch.records[1].attributes["unisphere.source.kind"], "unknown");
    assert_eq!(batch.records[2].attributes["unisphere.source.kind"], "unknown");
    assert!(has_diagnostic(&batch, 73, Code::InvalidField));
    assert!(has_diagnostic(&batch, 73, Code::UnsupportedRecord));
    assert!(!serde_json::to_string(&batch).unwrap().contains("SENSITIVE-"));
}

#[test]
fn invalid_optional_metadata_is_omitted_without_affecting_valid_fields() {
    let batch = map(&[native(json!({
        "type": "assistant", "uuid": 12, "parentUuid": null, "sessionId": [], "isSidechain": "false",
        "message": {"role": "assistant", "id": {}, "model": 99, "content": "visible"},
    }))], true);
    let attributes = &batch.records[0].attributes;
    for key in ["unisphere.source.record.id", "unisphere.source.parent.id", "gen_ai.conversation.id", "unisphere.source.is_sidechain", "unisphere.message.id", "gen_ai.response.model"] {
        assert!(!attributes.contains_key(key));
    }
    assert_eq!(batch.records[0].body.as_ref().unwrap()["parts"][0]["content"], "visible");
    assert!(has_diagnostic(&batch, 73, Code::InvalidField));
}

#[test]
fn usage_accepts_only_independent_nonnegative_i64_components() {
    let mut value = message(json!([]));
    value["message"]["usage"] = json!({
        "input_tokens": 0, "output_tokens": i64::MAX,
        "cache_read_input_tokens": -1, "cache_creation_input_tokens": 9223372036854775808_u64,
        "total_tokens": 777, "cache_creation": {"ephemeral_5m_input_tokens": 999},
    });
    let batch = map(&[native(value)], false);
    let attributes = &batch.records[0].attributes;
    assert_eq!(attributes["unisphere.usage.input_tokens"], 0);
    assert_eq!(attributes["unisphere.usage.output_tokens"], i64::MAX);
    assert_eq!(attributes["unisphere.usage.scope"], "native_record_snapshot");
    assert!(!attributes.contains_key("unisphere.usage.cache_read_input_tokens"));
    assert!(!attributes.contains_key("unisphere.usage.cache_creation_input_tokens"));
    assert_eq!(attributes.keys().filter(|key| key.starts_with("unisphere.usage.")).count(), 3);
    assert!(has_diagnostic(&batch, 73, Code::InvalidField));

    let mut value = message(json!([]));
    value["message"]["usage"] = json!({"input_tokens": 1.0, "output_tokens": "2", "cache_read_input_tokens": null, "cache_creation_input_tokens": true});
    let batch = map(&[native(value)], false);
    assert!(!batch.records[0].attributes.keys().any(|key| key.starts_with("unisphere.usage.")));
    assert!(has_diagnostic(&batch, 73, Code::InvalidField));
}

#[test]
fn timestamps_preserve_offsets_and_nanoseconds_and_reject_unrepresentable_values() {
    for (timestamp, expected) in [
        (json!("1970-01-01T00:00:00Z"), Some(0)),
        (json!("1970-01-01T01:00:00.000000001+01:00"), Some(1)),
        (json!("1970-01-01T00:00:01.123456789Z"), Some(1_123_456_789)),
        (json!("1969-12-31T23:59:59.999999999Z"), None),
        (json!("9999-12-31T23:59:59Z"), None),
        (json!("SENSITIVE-BAD-TIME"), None),
        (json!(42), None),
        (Value::Null, None),
    ] {
        let mut value = message(json!([]));
        value["timestamp"] = timestamp;
        let batch = map(&[native(value)], true);
        assert_eq!(batch.records[0].timestamp_unix_nano, expected);
        assert_eq!(has_diagnostic(&batch, 73, Code::InvalidTimestamp), expected.is_none());
        assert!(!serde_json::to_string(&batch).unwrap().contains("SENSITIVE-BAD-TIME"));
    }
}

#[test]
fn malformed_parts_do_not_erase_supported_siblings_or_guess_tool_fields() {
    let record = native(message(json!([
        {"type": "text", "text": 4}, {"type": "thinking"},
        {"type": "tool_use", "input": {}}, {"type": "tool_use", "name": "NoInput"},
        {"type": "tool_result", "tool_use_id": "no-response"},
        99, {"type": false},
        {"type": "tool_use", "id": 17, "name": "Valid", "input": {"values": [null, 1, true]}},
        {"type": "tool_result", "is_error": "yes", "content": ["result"]},
        {"type": "text", "text": "last"},
    ])));
    let batch = map(&[record], true);
    assert!(has_diagnostic(&batch, 73, Code::InvalidField));
    assert_eq!(batch.records[0].body, Some(json!({"role": "assistant", "parts": [
        {"type": "tool_call", "name": "Valid", "arguments": {"values": [null, 1, true]}},
        {"type": "tool_call_response", "response": ["result"]},
        {"type": "text", "content": "last"},
    ]})));
}

#[test]
fn malformed_bytes_fail_batch_at_physical_offset_without_public_content() {
    for bytes in [b"{\"SENSITIVE-UNFINISHED\":".to_vec(), vec![0xff, b'\n'], b"{}{}".to_vec()] {
        let records = [native(message(json!("valid before failure"))), NativeRecord { offset: 801, bytes }];
        let error = ClaudeCodeAdapter.map(&source(), &records, MappingOptions::default()).unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        assert_eq!(error.offset(), Some(801));
        assert!(!format!("{error:?} {error} {} {}", error.message(), error.fix()).contains("SENSITIVE-"));
    }
}

#[test]
fn split_batches_and_replay_are_identical_without_adapter_state() {
    let records = fixture_records(CLAUDE_BASIC);
    for include_content in [false, true] {
        let whole = map(&records, include_content);
        let mut split = map(&records[..1], include_content);
        let remainder = map(&records[1..], include_content);
        split.records.extend(remainder.records);
        split.diagnostics.extend(remainder.diagnostics);
        assert_eq!(whole, split);
        assert_eq!(whole, map(&records, include_content));
    }
    assert_eq!(map(&[], false), MappedBatch::default());
}

#[test]
fn explicit_source_validation_is_independent_of_filesystem_existence() {
    let invalid = SessionRef { path: "relative.jsonl".into() };
    let error = ClaudeCodeAdapter.map(&invalid, &[], MappingOptions::default()).unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
    let records = fixture_records(b"{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"CRLF\"}}\r\n");
    let batch = map(&records, true);
    assert_eq!(batch.records[0].attributes["unisphere.source.path"], "/synthetic/not-opened/session.jsonl");
    assert_eq!(batch.records[0].body.as_ref().unwrap()["parts"][0]["content"], "CRLF");
}
