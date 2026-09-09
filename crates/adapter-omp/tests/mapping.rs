use serde_json::{Value, json};
use unisphere_adapter_omp::{DESCRIPTOR, OmpAdapter};
use unisphere_core::{
    MappedBatch, MappingDiagnosticCode, MappingOptions, NativeRecord, PipelineErrorKind,
    SessionAdapter, SessionRef,
};
use unisphere_testkit::collection::{assert_adapter_conformance, fixture_records};

const FIXTURE: &[u8] = include_bytes!("fixtures/native.jsonl");

fn source() -> SessionRef {
    SessionRef {
        path: "/synthetic/omp/session.jsonl".into(),
    }
}

fn record(offset: u64, value: Value) -> NativeRecord {
    NativeRecord {
        offset,
        bytes: serde_json::to_vec(&value).unwrap(),
    }
}

fn map(records: &[NativeRecord], include_content: bool) -> MappedBatch {
    OmpAdapter
        .map(&source(), records, MappingOptions { include_content })
        .unwrap()
}

fn entry(message: Value) -> NativeRecord {
    record(
        4096,
        json!({"type":"message", "id":"entry-1", "parentId":"parent-1", "timestamp":"2026-01-02T03:04:05Z", "message":message}),
    )
}

#[test]
fn metadata_is_content_free_and_mapping_is_partition_independent() {
    let records = fixture_records(FIXTURE);
    assert_adapter_conformance(&OmpAdapter, &source(), &records);
    for include_content in [false, true] {
        let whole = map(&records, include_content);
        let mut split = MappedBatch::default();
        for native in &records {
            let batch = map(std::slice::from_ref(native), include_content);
            split.records.extend(batch.records);
            split.diagnostics.extend(batch.diagnostics);
        }
        assert_eq!(
            whole, split,
            "a cursor boundary must not change enrichment or provenance"
        );
    }
    let metadata = map(&records, false);
    assert_eq!(
        metadata.records[1].attributes["gen_ai.conversation.id"],
        "session-1"
    );
    assert!(
        !metadata.records[2]
            .attributes
            .contains_key("gen_ai.conversation.id")
    );
    assert_eq!(
        metadata.records[3].attributes["unisphere.source.parent.id"],
        "u1"
    );
    assert_eq!(
        metadata.records[3].attributes["unisphere.source.adapter"],
        DESCRIPTOR.id
    );
}

#[test]
fn mutable_title_slot_retains_physical_offsets_and_audit_events() {
    let records = fixture_records(FIXTURE);
    assert_eq!(records[0].bytes.len(), 255);
    assert_eq!(
        records[1].offset, 256,
        "UTF-8 bytes, not title character count"
    );
    let batch = map(&records, true);
    let title = &batch.records[0];
    assert_eq!(title.attributes["unisphere.source.kind"], "title");
    assert_eq!(title.attributes["unisphere.source.mutable"], true);
    assert_eq!(title.attributes["unisphere.source.offset"], 0);
    assert_eq!(title.body.as_ref().unwrap()["title"], "SENSITIVE-title-λ");
    assert!(title.body.as_ref().unwrap().get("pad").is_none());
    assert_eq!(
        batch.records[8].attributes["unisphere.source.kind"],
        "title_change"
    );
    assert_eq!(
        batch.records[8].body.as_ref().unwrap()["previousTitle"],
        "SENSITIVE-old-title"
    );
    assert!(
        !batch
            .diagnostics
            .iter()
            .any(|d| d.offset == 0 && d.code == MappingDiagnosticCode::InvalidField)
    );

    let shifted = NativeRecord {
        offset: 500,
        bytes: records[0].bytes.clone(),
    };
    let invalid = map(&[shifted], false);
    assert!(
        invalid
            .diagnostics
            .iter()
            .any(|d| d.offset == 500 && d.code == MappingDiagnosticCode::InvalidField)
    );
    let mut shorter = records[0].clone();
    shorter.bytes.pop();
    // The malformed truncated JSON fails, rather than treating a title as free-form text.
    assert_eq!(
        OmpAdapter
            .map(&source(), &[shorter], MappingOptions::default())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidData
    );
}

#[test]
fn tools_reasoning_and_references_remain_structured_and_unresolved() {
    let batch = map(&fixture_records(FIXTURE), true);
    let assistant = &batch.records[3];
    assert_eq!(assistant.attributes["gen_ai.response.model"], "model-1");
    assert_eq!(assistant.attributes["gen_ai.provider.name"], "anthropic");
    let parts = &assistant.body.as_ref().unwrap()["parts"];
    assert_eq!(
        parts[0],
        json!({"type":"reasoning", "content":"SENSITIVE-thought"})
    );
    assert_eq!(parts[2]["type"], "tool_call");
    assert_eq!(parts[2]["arguments"], json!({"path":"/SENSITIVE-input"}));
    let tool = &batch.records[4];
    assert_eq!(tool.attributes["gen_ai.tool.call.id"], "call-1");
    let body = tool.body.as_ref().unwrap();
    assert_eq!(body["role"], "tool");
    assert_eq!(body["parts"][0]["id"], "call-1");
    assert_eq!(body["parts"][0]["unisphere.is_error"], false);
    assert_eq!(body["parts"][0]["response"][1]["resolved"], false);
    assert_eq!(
        body["parts"][0]["response"][1]["uri"],
        format!("blob:sha256:{}", "a".repeat(64))
    );
    assert_eq!(
        body["unisphere.references"][0]["artifact_id"],
        "SENSITIVE-artifact"
    );
    let serialized = serde_json::to_string(&batch).unwrap();
    assert!(!serialized.contains("SENSITIVE-signature"));
    assert!(!serialized.contains("SENSITIVE-opaque"));
    assert!(
        batch
            .diagnostics
            .iter()
            .any(|d| d.offset == fixture_records(FIXTURE)[4].offset
                && d.code == MappingDiagnosticCode::UnsupportedPart)
    );
}

#[test]
fn measured_usage_preserves_native_components_without_inventing_totals() {
    let native = entry(json!({"role":"assistant", "content":[], "usage":{
        "input":0,"output":12,"cacheRead":8,"cacheWrite":4,"totalTokens":77,
        "reasoningTokens":3,"premiumRequests":0.25,
        "orchestration":{"input":7,"output":2,"cacheRead":1},
        "cttl":{"ephemeral5m":3,"ephemeral1h":1},
        "server":{"webSearch":2,"webFetch":0},
        "cost":{"input":0.01,"output":0.2,"cacheRead":0.001,"cacheWrite":0.02,"total":0.231}
    }}));
    let batch = map(&[native], false);
    let attrs = &batch.records[0].attributes;
    assert_eq!(attrs["unisphere.usage.input_tokens"], 0);
    assert_eq!(
        attrs["unisphere.usage.total_tokens"], 77,
        "keep reported total, never recalculate"
    );
    assert_eq!(attrs["unisphere.usage.reasoning_tokens"], 3);
    assert_eq!(attrs["unisphere.usage.premium_requests"], 0.25);
    assert_eq!(attrs["unisphere.usage.orchestration.input"], 7);
    assert_eq!(attrs["unisphere.usage.cache_write_ttl.ephemeral1h"], 1);
    assert_eq!(attrs["unisphere.usage.server_requests.webFetch"], 0);
    assert_eq!(attrs["unisphere.usage.cost.total"], 0.231);
    assert_eq!(attrs["unisphere.usage.scope"], "native_record_snapshot");
    assert!(!attrs.keys().any(|key| key.starts_with("gen_ai.usage.")));

    let missing = map(
        &[entry(
            json!({"role":"assistant","content":[],"usage":{"input":5}}),
        )],
        false,
    );
    assert!(
        !missing.records[0]
            .attributes
            .contains_key("unisphere.usage.total_tokens")
    );
    assert!(
        !missing.records[0]
            .attributes
            .contains_key("unisphere.usage.output_tokens")
    );
}

#[test]
fn invalid_usage_is_not_coerced_or_leaked_into_diagnostics() {
    let batch = map(
        &[entry(json!({"role":"assistant","content":[],"usage":{
            "input":-1,"output":1.5,"cacheRead":"SENSITIVE-not-a-number", "totalTokens":null,
            "cost":{"total":-0.1},"orchestration":false
        }}))],
        false,
    );
    assert!(
        !batch.records[0]
            .attributes
            .keys()
            .any(|key| key.starts_with("unisphere.usage."))
    );
    assert_eq!(
        batch
            .diagnostics
            .iter()
            .filter(|d| d.code == MappingDiagnosticCode::InvalidField)
            .count(),
        6
    );
    assert!(
        !serde_json::to_string(&batch)
            .unwrap()
            .contains("SENSITIVE-")
    );
}

#[test]
fn physical_and_message_timestamps_are_distinct_and_never_guessed() {
    let native = entry(json!({"role":"user","content":"hello","timestamp":1}));
    let batch = map(&[native], false);
    assert_eq!(
        batch.records[0].timestamp_unix_nano,
        Some(1_767_323_045_000_000_000)
    );
    assert_eq!(
        batch.records[0].attributes["unisphere.message.timestamp_unix_nano"],
        1_000_000
    );
    let bad = record(
        90,
        json!({"type":"message", "timestamp":"SENSITIVE-invalid", "message":{"role":"user", "content":[], "timestamp":18446744073709551615_u64}}),
    );
    let invalid = map(&[bad], false);
    assert_eq!(invalid.records[0].timestamp_unix_nano, None);
    assert!(
        !invalid.records[0]
            .attributes
            .contains_key("unisphere.message.timestamp_unix_nano")
    );
    assert_eq!(
        invalid
            .diagnostics
            .iter()
            .filter(|d| d.code == MappingDiagnosticCode::InvalidTimestamp)
            .count(),
        2
    );
    let absent = map(
        &[record(
            91,
            json!({"type":"model_change","model":"provider/model"}),
        )],
        false,
    );
    assert_eq!(absent.records[0].timestamp_unix_nano, None);
}

#[test]
fn control_custom_and_compaction_records_do_not_become_model_turns() {
    let batch = map(&fixture_records(FIXTURE), true);
    let compaction = &batch.records[5];
    assert_eq!(
        compaction.body.as_ref().unwrap()["type"],
        "unisphere.source_event"
    );
    assert_eq!(
        compaction.attributes["unisphere.compaction.tokens_before"],
        420
    );
    assert!(
        !compaction
            .attributes
            .keys()
            .any(|key| key.starts_with("unisphere.usage."))
    );
    assert!(!compaction.attributes.contains_key("unisphere.message.role"));
    assert_eq!(
        batch.records[6].body, None,
        "custom data is not an ordinary message"
    );
    assert_eq!(
        batch.records[7].body.as_ref().unwrap()["native_type"],
        "custom_message"
    );
    let selection = map(
        &[
            record(
                1,
                json!({"type":"model_change", "model":"provider/model", "role":"smol"}),
            ),
            record(
                2,
                json!({"type":"thinking_level_change", "thinkingLevel":null, "configured":"auto"}),
            ),
            record(
                3,
                json!({"type":"service_tier_change", "serviceTier":{"openai":"flex","anthropic":"priority"}}),
            ),
            record(4, json!({"type":"service_tier_change", "serviceTier":null})),
            entry(json!({"role":"assistant","content":[]})),
        ],
        false,
    );
    assert_eq!(
        selection.records[0].attributes["unisphere.model.selection"],
        "provider/model"
    );
    assert!(
        !selection.records[0]
            .attributes
            .contains_key("gen_ai.response.model")
    );
    assert!(
        !selection.records[4]
            .attributes
            .contains_key("gen_ai.response.model")
    );
    assert_eq!(
        selection.records[1].attributes["unisphere.thinking.level"],
        Value::Null
    );
    assert_eq!(
        selection.records[2].attributes["unisphere.service_tier.openai"],
        "flex"
    );
    assert_eq!(
        selection.records[3].attributes["unisphere.service_tier.cleared"],
        true
    );
}

#[test]
fn source_context_and_execution_messages_remain_non_conversational() {
    let records = [
        record(
            0,
            json!({"type":"session_init","systemPrompt":"SENSITIVE-system","task":"SENSITIVE-task","tools":["read"],"restrictToolNames":true}),
        ),
        record(
            200,
            json!({"type":"ttsr_injection","injectedRules":["SENSITIVE-rule"]}),
        ),
        entry(
            json!({"role":"bashExecution","command":"SENSITIVE-command","output":"SENSITIVE-output","exitCode":-1,"cancelled":true,"truncated":true,"meta":{"truncation":{"artifactId":"SENSITIVE-artifact"}}}),
        ),
        record(
            600,
            json!({"type":"message","message":{"role":"fileMention","files":[{"path":"SENSITIVE-file","content":"SENSITIVE-content","lineCount":1,"image":{"type":"image","mimeType":"image/png","data":"SENSITIVE-base64"}}]}}),
        ),
    ];
    assert_adapter_conformance(&OmpAdapter, &source(), &records);
    let batch = map(&records, true);
    assert_eq!(
        batch.records[0].body.as_ref().unwrap()["tools"],
        json!(["read"])
    );
    assert_eq!(
        batch.records[1].body.as_ref().unwrap()["injectedRules"],
        json!(["SENSITIVE-rule"])
    );
    assert_eq!(
        batch.records[2].body.as_ref().unwrap()["native_type"],
        "bashExecution"
    );
    assert_eq!(
        batch.records[2].attributes["unisphere.execution.exit_code"],
        -1
    );
    assert_eq!(
        batch.records[3].body.as_ref().unwrap()["files"][0]["image"]["type"],
        "image"
    );
}

#[test]
fn unsupported_shapes_are_observable_without_raw_content() {
    let records = [
        record(1, json!({"type":"future-kind", "data":"SENSITIVE-unknown"})),
        entry(
            json!({"role":"assistant","content":[{"type":"redactedThinking","data":"SENSITIVE-encrypted"},{"type":"future-part","data":"SENSITIVE-future"}]}),
        ),
        record(
            3,
            json!({"type":"session","version":4,"id":"future","cwd":"SENSITIVE-path"}),
        ),
        record(4, json!(["SENSITIVE-array"])),
    ];
    let batch = map(&records, true);
    assert_eq!(
        batch.records[0].attributes["unisphere.source.kind"],
        "future-kind"
    );
    assert_eq!(batch.records[0].body, None);
    assert_eq!(
        batch.records[1].body.as_ref().unwrap()["parts"][0],
        json!({"type":"unisphere.unknown","native_type":"redactedThinking"})
    );
    assert_eq!(batch.records[2].body, None);
    assert!(
        batch
            .diagnostics
            .iter()
            .any(|d| d.offset == 1 && d.code == MappingDiagnosticCode::UnsupportedRecord)
    );
    assert!(
        batch
            .diagnostics
            .iter()
            .any(|d| d.offset == 4 && d.code == MappingDiagnosticCode::InvalidField)
    );
    assert!(
        !serde_json::to_string(&batch)
            .unwrap()
            .contains("SENSITIVE-")
    );
}

#[test]
fn malformed_json_or_utf8_fails_the_whole_batch_with_safe_offset() {
    for bytes in [b"{\"SENSITIVE-broken\":".to_vec(), vec![0xff, 0xfe]] {
        let records = [
            entry(json!({"role":"user","content":"valid"})),
            NativeRecord { offset: 987, bytes },
        ];
        let error = OmpAdapter
            .map(&source(), &records, MappingOptions::default())
            .unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        assert_eq!(error.offset(), Some(987));
        assert!(!error.to_string().contains("SENSITIVE-"));
    }
    let invalid_source = SessionRef {
        path: "relative.jsonl".into(),
    };
    assert_eq!(
        OmpAdapter
            .map(&invalid_source, &[], MappingOptions::default())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidInput
    );
}

#[test]
fn invalid_parts_are_diagnosed_even_when_content_is_disabled() {
    let native = entry(json!({"role":"assistant","content":[
        {"type":"toolCall","id":"call","name":"read","arguments":"SENSITIVE-wrong-shape"},
        {"type":"image","data":false,"mimeType":"image/png"},
        {"type":"text","text":12}
    ]}));
    for enabled in [false, true] {
        let batch = map(std::slice::from_ref(&native), enabled);
        assert_eq!(
            batch
                .diagnostics
                .iter()
                .filter(|d| d.code == MappingDiagnosticCode::InvalidField)
                .count(),
            3
        );
        assert!(
            !serde_json::to_string(&batch)
                .unwrap()
                .contains("SENSITIVE-")
        );
    }
}
