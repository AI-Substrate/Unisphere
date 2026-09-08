use serde_json::{Value, json};
use unisphere_adapter_copilot_cli::{CopilotCliAdapter, DESCRIPTOR};
use unisphere_core::{
    MappedBatch, MappingDiagnosticCode as Code, MappingOptions, NativeRecord, PipelineErrorKind,
    SessionAdapter, SessionRef,
};
use unisphere_testkit::collection::{assert_adapter_conformance, fixture_records};

const EVENTS: &[u8] = include_bytes!("fixtures/events.jsonl");

fn source() -> SessionRef {
    SessionRef { path: "/synthetic/copilot/events.jsonl".into() }
}

fn map(records: &[NativeRecord], include_content: bool) -> MappedBatch {
    CopilotCliAdapter.map(&source(), records, MappingOptions { include_content }).unwrap()
}

fn native(offset: u64, value: Value) -> NativeRecord {
    NativeRecord { offset, bytes: serde_json::to_vec(&value).unwrap() }
}

fn has(batch: &MappedBatch, offset: u64, code: Code) -> bool {
    batch.diagnostics.iter().any(|diagnostic| diagnostic.offset == offset && diagnostic.code == code)
}

#[test]
fn full_synthetic_dialect_obeys_shared_metadata_and_provenance_contract() {
    assert_adapter_conformance(&CopilotCliAdapter, &source(), &fixture_records(EVENTS));
}

#[test]
fn native_lifecycle_content_is_structured_not_a_reconstructed_conversation() {
    let records = fixture_records(EVENTS);
    let batch = map(&records, true);
    let message = batch.records[3].body.as_ref().unwrap();
    assert_eq!(message["role"], "assistant");
    assert_eq!(message["parts"][0], json!({"type":"text","content":"SENSITIVE-answer"}));
    assert_eq!(message["parts"][1], json!({"type":"reasoning","content":"SENSITIVE-reasoning"}));
    assert_eq!(message["parts"][2], json!({
        "type":"tool_call", "id":"tool-1", "name":"read_file",
        "arguments":{"path":"/SENSITIVE-input","line":2}
    }));
    let response = &batch.records[5].body.as_ref().unwrap()["parts"][0];
    assert_eq!(response["type"], "tool_call_response");
    assert_eq!(response["id"], "tool-1");
    assert_eq!(response["unisphere.is_error"], false);
    assert_eq!(response["response"]["structuredContent"], json!({"text":"SENSITIVE-structured","ok":true}));
    assert_eq!(response["response"]["contents"][1], json!({"type":"unisphere.unknown","native_type":"image"}));
    assert!(has(&batch, records[5].offset, Code::UnsupportedPart));
    let user = batch.records[1].body.as_ref().unwrap();
    assert_eq!(user["parts"][1]["type"], "unisphere.transformed_text");
    assert_eq!(user["parts"][2]["reference"]["path"], "/SENSITIVE-attachment");
    assert!(!serde_json::to_string(&batch).unwrap().contains("SENSITIVE-binary"));
    assert!(batch.records[14].body.is_none());
    assert!(has(&batch, records[14].offset, Code::UnsupportedRecord));
}

#[test]
fn physical_records_and_all_usage_scopes_survive_replay_and_batch_splitting() {
    let mut records = fixture_records(EVENTS);
    let mut repeated = records[3].clone();
    repeated.offset = 90_000;
    records.push(repeated);
    let full = map(&records, true);
    let mut split = map(&records[..8], true);
    let remainder = map(&records[8..], true);
    split.records.extend(remainder.records);
    split.diagnostics.extend(remainder.diagnostics);
    assert_eq!(full, split);
    assert_eq!(full, map(&records, true));
    assert_eq!(full.records[3].attributes["unisphere.message.id"], full.records[15].attributes["unisphere.message.id"]);
    assert_eq!(full.records[15].attributes["unisphere.source.offset"], 90_000);
    assert_eq!(full.records[3].attributes["unisphere.usage.scope"], "assistant_message");
    assert_eq!(full.records[6].attributes["unisphere.usage.scope"], "api_call");
    assert_eq!(full.records[6].attributes["unisphere.usage.input_tokens"], 15);
    assert_eq!(full.records[6].attributes["unisphere.usage.cache_read_input_tokens"], 4);
    assert_eq!(full.records[7].attributes["unisphere.usage.scope"], "session_checkpoint");
    assert_eq!(full.records[7].attributes["unisphere.copilot.usage.total_premium_requests"], 0.5);
    assert_eq!(full.records[8].attributes["unisphere.usage.scope"], "session_shutdown");
    assert_eq!(full.records[8].attributes["unisphere.copilot.model_metrics"]["model-a"]["usage"]["inputTokens"], 15);
    assert_eq!(full.records[9].attributes["unisphere.usage.scope"], "compaction_api_call");
    for record in &full.records {
        assert!(!record.attributes.keys().any(|key| key.starts_with("gen_ai.usage.")));
        assert!(!record.attributes.contains_key("trace_id"));
        assert_eq!(record.attributes["unisphere.source.adapter"], DESCRIPTOR.id);
    }
}

#[test]
fn context_model_and_subagent_state_do_not_become_response_facts_or_turns() {
    let records = fixture_records(EVENTS);
    let batch = map(&records, true);
    assert_eq!(batch.records[0].attributes["gen_ai.conversation.id"], "session-1");
    assert_eq!(batch.records[0].attributes["unisphere.copilot.selected_model"], "model-a");
    assert_eq!(batch.records[12].attributes["unisphere.copilot.selected_model"], "model-b");
    assert!(!batch.records[1].attributes.contains_key("gen_ai.conversation.id"));
    assert!(!batch.records[12].attributes.contains_key("gen_ai.response.model"));
    assert_eq!(batch.records[10].attributes["unisphere.copilot.agent.id"], "agent-1");
    assert_eq!(batch.records[10].attributes["unisphere.copilot.agent.parent_id"], "agent-parent");
    assert_eq!(batch.records[10].attributes["unisphere.source.parent.id"], "compact");
    for index in [0, 2, 7, 8, 9, 10, 11, 12, 14] {
        assert!(!batch.records[index].attributes.contains_key("unisphere.message.role"));
        assert!(batch.records[index].body.as_ref().is_none_or(|body| body.get("role").is_none()));
    }
    assert_eq!(batch.records[9].body.as_ref().unwrap()["type"], "session.compaction_complete");
    assert_eq!(batch.records[13].attributes["unisphere.copilot.fragment"], true);
    assert_eq!(batch.records[13].attributes["unisphere.copilot.ephemeral"], true);
}

#[test]
fn scoped_agent_metrics_exclude_prompt_labels_and_keep_independent_model_counters() {
    let record = native(5, json!({"type":"session.shutdown", "data":{
        "agentMetrics":{"child":{"agentName":"researcher","agentDisplayName":"SENSITIVE-task prompt",
            "totalNanoAiu":20,"totalApiDurationMs":1.5,
            "modelMetrics":{"model-a":{"usage":{"inputTokens":4,"outputTokens":2},"requests":{"count":1,"cost":0.5}}}}}
    }}));
    let batch = map(&[record], false);
    let metrics = &batch.records[0].attributes["unisphere.copilot.agent_metrics"]["child"];
    assert_eq!(metrics["modelMetrics"]["model-a"]["usage"]["inputTokens"], 4);
    assert_eq!(metrics["totalNanoAiu"], 20);
    assert!(metrics.get("agentDisplayName").is_none());
    assert!(!serde_json::to_string(&batch).unwrap().contains("SENSITIVE-"));
}

#[test]
fn invalid_usage_values_are_not_coerced_summed_or_replaced_with_zero() {
    let record = native(19, json!({"type":"assistant.usage","data":{
        "model":"model-a","inputTokens":-1,"outputTokens":3.5,
        "cacheReadTokens":"12","cacheWriteTokens":9223372036854775808_u64,
        "reasoningTokens":0,"cost":-0.1,"duration":null,
        "totalNanoAiu":9223372036854775807_i64
    }}));
    let batch = map(&[record], false);
    let attrs = &batch.records[0].attributes;
    for key in ["unisphere.usage.input_tokens", "unisphere.usage.output_tokens", "unisphere.usage.cache_read_input_tokens", "unisphere.usage.cache_creation_input_tokens", "unisphere.copilot.usage.cost_multiplier", "unisphere.copilot.usage.duration_ms"] {
        assert!(!attrs.contains_key(key));
    }
    assert_eq!(attrs["unisphere.copilot.usage.reasoning_tokens"], 0);
    assert_eq!(attrs["unisphere.copilot.usage.total_nano_aiu"], json!(i64::MAX));
    assert!(has(&batch, 19, Code::InvalidField));
}

#[test]
fn timestamps_use_only_the_event_clock_and_preserve_nanoseconds() {
    let batch = map(&[
        native(1, json!({"type":"session.idle","timestamp":"1970-01-01T01:00:01.123456789+01:00","data":{}})),
        native(2, json!({"type":"session.start","data":{"startTime":"2026-01-01T00:00:00Z"}})),
        native(3, json!({"type":"session.idle","timestamp":"1969-12-31T23:59:59Z","data":{}})),
        native(4, json!({"type":"session.idle","timestamp":"9999-12-31T23:59:59Z","data":{}})),
        native(5, json!({"type":"session.idle","timestamp":123,"data":{}})),
    ], false);
    assert_eq!(batch.records[0].timestamp_unix_nano, Some(1_123_456_789));
    for record in &batch.records[1..] {
        assert_eq!(record.timestamp_unix_nano, None);
    }
    assert!(!has(&batch, 2, Code::InvalidTimestamp));
    for offset in [3, 4, 5] {
        assert!(has(&batch, offset, Code::InvalidTimestamp));
    }
}

#[test]
fn malformed_parts_preserve_valid_siblings_and_diagnostics_never_echo_payloads() {
    let batch = map(&[
        native(71, json!({"type":"assistant.message","data":{
            "content":"SENSITIVE-good","model":{},"toolRequests":[false, {"name":42,"toolCallId":"bad"},
                {"name":"echo","toolCallId":"good","arguments":{"ok":true}}],
            "reasoningOpaque":"SENSITIVE-opaque"
        }})),
        native(72, json!({"type":"user.message","data":"SENSITIVE-bad-data"})),
        native(73, json!(["SENSITIVE-nonobject"])),
    ], true);
    let parts = &batch.records[0].body.as_ref().unwrap()["parts"];
    assert_eq!(parts, &json!([
        {"type":"text","content":"SENSITIVE-good"},
        {"type":"tool_call","id":"good","name":"echo","arguments":{"ok":true}}
    ]));
    assert!(has(&batch, 71, Code::InvalidField));
    assert!(has(&batch, 71, Code::UnsupportedPart));
    assert!(has(&batch, 72, Code::InvalidField));
    assert!(has(&batch, 73, Code::UnsupportedRecord));
    assert!(!serde_json::to_string(&batch.diagnostics).unwrap().contains("SENSITIVE"));
    assert!(batch.records[1].body.is_none());
    assert!(batch.records[2].body.is_none());
}

#[test]
fn malformed_bytes_fail_the_batch_at_the_physical_offset_without_parser_text() {
    for bytes in [b"{\"SENSITIVE-bad\":}".to_vec(), vec![0xff], b"{\"type\":\"user.message\"}\n{}".to_vec()] {
        let records = [native(0, json!({"type":"session.idle","data":{}})), NativeRecord { offset: 73, bytes }];
        let error = CopilotCliAdapter.map(&source(), &records, MappingOptions::default()).unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        assert_eq!(error.offset(), Some(73));
        assert!(!format!("{error:?} {error}").contains("SENSITIVE"));
    }
}

#[test]
fn source_validation_is_explicit_and_does_not_require_a_real_path() {
    let invalid = SessionRef { path: "relative/events.jsonl".into() };
    let error = CopilotCliAdapter.map(&invalid, &[], MappingOptions::default()).unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
    let batch = map(&[native(5, json!({"type":"tool.execution_complete","data":{
        "toolCallId":"tool-x","success":false,"error":{"message":"SENSITIVE-failure","code":"NOT_FOUND"}
    }}))], true);
    assert_eq!(batch.records[0].body.as_ref().unwrap()["parts"][0]["unisphere.is_error"], true);
}
