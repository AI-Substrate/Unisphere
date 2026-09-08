use serde_json::{Value, json};
use unisphere_adapter_cursor::{CursorAdapter, DESCRIPTOR};
use unisphere_core::{
    MappedBatch, MappingDiagnosticCode as Code, MappingOptions, NativeRecord, PipelineErrorKind,
    SessionAdapter, SessionRef,
};
use unisphere_testkit::collection::{assert_adapter_conformance, fixture_records};

const TRANSCRIPT: &[u8] = include_bytes!("../fixtures/transcript.jsonl");

fn source() -> SessionRef {
    SessionRef { path: "/synthetic/not-opened/parent/subagents/child.jsonl".into() }
}

fn native(value: Value) -> NativeRecord {
    NativeRecord { offset: 73, bytes: serde_json::to_vec(&value).unwrap() }
}

fn map(records: &[NativeRecord], include_content: bool) -> MappedBatch {
    CursorAdapter.map(&source(), records, MappingOptions { include_content }).unwrap()
}

fn diagnosed(batch: &MappedBatch, code: Code) -> bool {
    batch.diagnostics.iter().any(|diagnostic| diagnostic.code == code)
}

#[test]
fn shared_conformance_and_content_privacy_include_control_records() {
    let records = fixture_records(TRANSCRIPT);
    assert_adapter_conformance(&CursorAdapter, &source(), &records);
    let batch = map(&records, false);
    assert_eq!(CursorAdapter.name(), DESCRIPTOR.id);
    assert_eq!(batch.records[0].attributes["unisphere.content.omitted"], true);
    assert_eq!(batch.records[4].attributes["unisphere.content.omitted"], true);
    assert_eq!(batch.records[4].attributes["unisphere.cursor.turn.status"], "error");
    assert!(!batch.records[5].attributes.contains_key("unisphere.content.omitted"));
}

#[test]
fn actual_outer_role_and_idless_structured_tools_are_preserved() {
    let records = fixture_records(TRANSCRIPT);
    let batch = map(&records, true);
    assert_eq!(batch.records[1].body, Some(json!({
        "role": "user", "parts": [{"type": "text", "content": "SENSITIVE-PROMPT: explain λ\nwithout guessing"}]
    })));
    let body = batch.records[2].body.as_ref().unwrap();
    assert_eq!(body["role"], "assistant");
    assert_eq!(body["parts"][0], json!({
        "type": "text", "content": "SENSITIVE-ANSWER\n\nSENSITIVE-NATIVE-MERGED-REASONING"
    }));
    assert_eq!(body["parts"][1], json!({
        "type": "tool_call", "name": "read_file", "arguments": {
            "path": "SENSITIVE-FILE", "lines": [1, 2], "options": {"exact": true, "fallback": null}
        }
    }));
    assert_eq!(batch.records[2].body, batch.records[3].body);
    assert_ne!(batch.records[2].attributes["unisphere.source.offset"], batch.records[3].attributes["unisphere.source.offset"]);
    assert!(batch.diagnostics.is_empty());
}

#[test]
fn overview_and_turn_end_are_not_message_turns_or_session_finality() {
    let batch = map(&fixture_records(TRANSCRIPT), true);
    assert_eq!(batch.records[0].body, Some(json!({
        "type": "unisphere.cursor.metadata", "overview": "SENSITIVE-OVERVIEW: inspect a synthetic project"
    })));
    assert_eq!(batch.records[4].body, Some(json!({
        "type": "unisphere.cursor.turn_ended", "error": "SENSITIVE-ERROR"
    })));
    for index in [0, 4, 5] {
        assert!(!batch.records[index].attributes.contains_key("unisphere.message.role"));
    }
    assert!(batch.records[5].body.is_none());
    assert_eq!(batch.records[5].attributes["unisphere.cursor.turn.status"], "success");
}

#[test]
fn facts_discarded_by_native_writer_are_never_reconstructed() {
    let batch = map(&fixture_records(TRANSCRIPT), true);
    for record in batch.records {
        assert!(record.timestamp_unix_nano.is_none());
        for key in ["gen_ai.conversation.id", "gen_ai.response.model", "unisphere.source.record.id", "unisphere.source.parent.id", "unisphere.message.id", "unisphere.usage.scope"] {
            assert!(!record.attributes.contains_key(key), "invented {key}");
        }
        assert!(!record.attributes.keys().any(|key| key.contains("tokens") || key.contains("trace") || key.contains("span")));
    }
}

#[test]
fn native_type_prevents_custom_or_compaction_payload_promotion() {
    let records = [
        native(json!({"type":"compaction","role":"assistant","message":{"content":[{"type":"text","text":"SENSITIVE-SUMMARY"}]}})),
        native(json!({"type":"custom","role":"user","message":{"content":[]},"data":"SENSITIVE-CUSTOM"})),
        native(json!({"type":null,"role":"assistant","message":{"content":[]}})),
        native(json!({"type":"assistant","message":{"role":"assistant","content":"SENSITIVE-CLAUDE"}})),
    ];
    let batch = map(&records, true);
    assert!(diagnosed(&batch, Code::UnsupportedRecord));
    assert!(diagnosed(&batch, Code::InvalidField));
    for record in &batch.records {
        assert!(record.body.is_none());
        assert!(!record.attributes.contains_key("unisphere.message.role"));
    }
    assert!(!serde_json::to_string(&batch).unwrap().contains("SENSITIVE-"));
    assert_eq!(batch.records[0].attributes["unisphere.source.kind"], "compaction");
    assert_eq!(batch.records[2].attributes["unisphere.source.kind"], "unknown");
}

#[test]
fn malformed_parts_preserve_valid_siblings_and_hide_unsupported_payloads() {
    let record = native(json!({"role":"assistant","message":{"content":[
        {"type":"text","text":42},
        {"type":"tool_use","name":"read_file"},
        {"type":"tool_use","name":false,"input":{}},
        {"type":"image","source":{"data":"SENSITIVE-IMAGE"}},
        {"type":"tool_result","content":"SENSITIVE-RESULT"},
        {"type":"text","text":"supported"},
        {"type":"tool_use","name":"empty","input":null},
        17
    ]}}));
    let content = map(std::slice::from_ref(&record), true);
    assert_eq!(content.records[0].body, Some(json!({"role":"assistant","parts":[
        {"type":"unisphere.unknown","native_type":"image"},
        {"type":"unisphere.unknown","native_type":"tool_result"},
        {"type":"text","content":"supported"},
        {"type":"tool_call","name":"empty","arguments":null}
    ]})));
    assert!(diagnosed(&content, Code::InvalidField));
    assert!(diagnosed(&content, Code::UnsupportedPart));
    assert!(!serde_json::to_string(&content).unwrap().contains("SENSITIVE-"));
    let metadata = map(&[record], false);
    assert!(metadata.records[0].body.is_none());
    assert!(diagnosed(&metadata, Code::InvalidField));
    assert!(diagnosed(&metadata, Code::UnsupportedPart));
    assert!(diagnosed(&metadata, Code::ContentOmitted));
}

#[test]
fn unsupported_envelopes_and_bad_known_fields_stay_observable_without_payloads() {
    let records = [
        native(json!(["SENSITIVE-ARRAY"])),
        native(json!({"message":{"role":"user","content":[]}})),
        native(json!({"role":"assistant","message":{"content":"SENSITIVE-NOT-ARRAY"}})),
        native(json!({"role":"user"})),
        native(json!({"type":"metadata","metadata":{"overview":42}})),
        native(json!({"type":"turn_ended","status":"SENSITIVE-UNKNOWN-STATUS","error":{}})),
    ];
    let batch = map(&records, true);
    assert_eq!(batch.records.len(), records.len());
    assert!(batch.records.iter().all(|record| record.body.is_none()));
    assert!(diagnosed(&batch, Code::InvalidField));
    assert!(!serde_json::to_string(&batch).unwrap().contains("SENSITIVE-"));
    assert!(!batch.records[5].attributes.contains_key("unisphere.cursor.turn.status"));
}

#[test]
fn nested_role_and_unverified_metadata_do_not_override_native_envelope() {
    let batch = map(&[native(json!({
        "role":"user", "timestamp":"2026-01-01T00:00:00Z", "sessionId":"unverified",
        "message":{"role":"assistant","model":"unverified", "id":"unverified",
            "usage":{"input_tokens":100,"output_tokens":20}, "content":[]}
    }))], true);
    assert_eq!(batch.records[0].body, Some(json!({"role":"user","parts":[]})));
    assert!(batch.records[0].timestamp_unix_nano.is_none());
    assert!(!serde_json::to_string(&batch).unwrap().contains("unverified"));
}

#[test]
fn malformed_bytes_fail_the_entire_batch_at_safe_physical_offset() {
    for bytes in [b"{\"secret\":\"SENSITIVE-TRUNCATED".to_vec(), vec![0xff]] {
        let records = [native(json!({"role":"user","message":{"content":[]}})), NativeRecord { offset: 917, bytes }];
        let error = CursorAdapter.map(&source(), &records, MappingOptions::default()).unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        assert_eq!(error.offset(), Some(917));
        assert!(!format!("{error:?} {error}").contains("SENSITIVE-"));
    }
}

#[test]
fn split_batches_replay_and_crlf_preserve_physical_identity() {
    let records = fixture_records(TRANSCRIPT);
    for content in [false, true] {
        let whole = map(&records, content);
        let mut split = map(&records[..3], content);
        let last = map(&records[3..], content);
        split.records.extend(last.records);
        split.diagnostics.extend(last.diagnostics);
        assert_eq!(whole, split);
        assert_eq!(whole, map(&records, content));
    }
    let crlf = fixture_records(b"\r\n{\"role\":\"user\",\"message\":{\"content\":[]}}\r\n");
    assert_eq!(map(&crlf, true).records[0].attributes["unisphere.source.offset"], 2);
}
