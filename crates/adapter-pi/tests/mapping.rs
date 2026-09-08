use serde_json::{Value, json};
use unisphere_adapter_pi::{DESCRIPTOR, PiAdapter};
use unisphere_core::{
    MappedBatch, MappingDiagnosticCode as Code, MappingOptions, NativeRecord, PipelineErrorKind,
    SessionAdapter, SessionRef,
};
use unisphere_testkit::collection::{assert_adapter_conformance, fixture_records};

const TREE: &[u8] = include_bytes!("fixtures/v3-tree.jsonl");

fn source() -> SessionRef {
    SessionRef {
        path: "/synthetic/not-opened/pi.jsonl".into(),
    }
}

fn map(records: &[NativeRecord], include_content: bool) -> MappedBatch {
    PiAdapter
        .map(&source(), records, MappingOptions { include_content })
        .unwrap()
}

fn native(value: Value) -> NativeRecord {
    NativeRecord {
        offset: 73,
        bytes: serde_json::to_vec(&value).unwrap(),
    }
}

fn message(message: Value) -> Value {
    json!({"type": "message", "id": "entry-a", "parentId": null, "message": message})
}

fn diagnostic(batch: &MappedBatch, offset: u64, code: Code) -> bool {
    batch
        .diagnostics
        .iter()
        .any(|item| item.offset == offset && item.code == code)
}

#[test]
fn shared_conformance_and_partition_independence() {
    let records = fixture_records(TREE);
    assert_adapter_conformance(&PiAdapter, &source(), &records);
    for include_content in [false, true] {
        let whole = map(&records, include_content);
        for split_at in 0..=records.len() {
            let mut split = map(&records[..split_at], include_content);
            let rest = map(&records[split_at..], include_content);
            split.records.extend(rest.records);
            split.diagnostics.extend(rest.diagnostics);
            assert_eq!(split, whole);
        }
        for record in whole.records {
            assert_eq!(record.attributes["unisphere.source.adapter"], DESCRIPTOR.id);
        }
    }
}

#[test]
fn physical_tree_identity_and_native_usage_scopes_are_not_reconstructed() {
    let records = fixture_records(TREE);
    let batch = map(&records, false);
    let first = &batch.records[4];
    let duplicate = &batch.records[6];
    assert_eq!(
        first.attributes["unisphere.source.record.id"],
        "assistant-a"
    );
    assert_eq!(
        duplicate.attributes["unisphere.source.record.id"],
        "assistant-a"
    );
    assert_eq!(
        first.attributes["unisphere.source.offset"],
        records[4].offset
    );
    assert_eq!(
        duplicate.attributes["unisphere.source.offset"],
        records[6].offset
    );
    assert_eq!(
        batch.records[8].attributes["unisphere.source.parent.id"],
        "user-a"
    );
    assert_eq!(
        batch.records[8].attributes["unisphere.pi.branch.from.id"],
        "tool-a"
    );
    assert_eq!(
        batch.records[0].attributes["gen_ai.conversation.id"],
        "session-a"
    );
    for record in &batch.records[1..] {
        assert!(!record.attributes.contains_key("gen_ai.conversation.id"));
        assert!(!record.attributes.contains_key("gen_ai.usage.input_tokens"));
        assert!(!record.attributes.contains_key("gen_ai.usage.output_tokens"));
    }
    for (index, scope, input) in [
        (4, "assistant_message", 11),
        (5, "tool_execution", 2),
        (7, "compaction", 20),
        (8, "branch_summary", 10),
    ] {
        assert_eq!(
            batch.records[index].attributes["unisphere.usage.scope"],
            scope
        );
        assert_eq!(
            batch.records[index].attributes["unisphere.usage.input_tokens"],
            input
        );
    }
    assert_eq!(first.attributes["unisphere.usage.output_tokens"], 7);
    assert_eq!(
        first.attributes["unisphere.usage.cache_read_input_tokens"],
        13
    );
    assert_eq!(
        first.attributes["unisphere.usage.cache_creation_input_tokens"],
        5
    );
    assert_eq!(first.attributes["unisphere.pi.usage.reasoning_tokens"], 3);
    assert_eq!(
        first.attributes["unisphere.pi.usage.cache_write_1h_tokens"],
        2
    );
    assert_eq!(first.attributes["unisphere.pi.usage.total_tokens"], 36);
    assert_eq!(first.attributes["unisphere.pi.usage.cost.total"], 0.037);
    assert_eq!(first.attributes["gen_ai.request.model"], "model-request");
    assert_eq!(first.attributes["gen_ai.response.model"], "model-response");
    assert!(
        !duplicate
            .attributes
            .contains_key("unisphere.pi.usage.total_tokens")
    );
    assert!(!duplicate.attributes.contains_key("gen_ai.response.model"));
    assert_eq!(
        batch.records[7].attributes["unisphere.pi.compaction.tokens_before"],
        42
    );
}

#[test]
fn opt_in_preserves_native_tool_and_content_structure_without_opaque_payloads() {
    let batch = map(&fixture_records(TREE), true);
    assert_eq!(
        batch.records[3].body.as_ref().unwrap()["parts"][1],
        json!({"type": "image", "mime_type": "image/png", "data": "SENSITIVE-image"})
    );
    let assistant = batch.records[4].body.as_ref().unwrap();
    assert_eq!(
        assistant["parts"][0],
        json!({"type": "reasoning", "content": "SENSITIVE-reasoning"})
    );
    assert_eq!(
        assistant["parts"][2],
        json!({
            "type": "tool_call", "id": "call-a", "name": "read",
            "arguments": {"path": "SENSITIVE-tool-argument"}
        })
    );
    let tool = batch.records[5].body.as_ref().unwrap();
    assert_eq!(tool["role"], "tool");
    assert_eq!(tool["parts"][0]["id"], "call-a");
    assert_eq!(tool["parts"][0]["unisphere.is_error"], true);
    assert_eq!(
        tool["parts"][0]["response"][0]["content"],
        "SENSITIVE-tool-result"
    );
    let redacted = batch.records[6].body.as_ref().unwrap();
    assert_eq!(
        redacted["parts"][0],
        json!({"type": "unisphere.redacted_reasoning"})
    );
    assert_eq!(redacted["error"], "SENSITIVE-error");
    let encoded = serde_json::to_string(&batch).unwrap();
    for omitted in [
        "SENSITIVE-signature",
        "SENSITIVE-text-signature",
        "SENSITIVE-tool-signature",
        "SENSITIVE-redacted",
        "SENSITIVE-encrypted",
        "SENSITIVE-tool-details",
        "SENSITIVE-extension-state",
        "SENSITIVE-compaction-details",
        "SENSITIVE-branch-details",
        "SENSITIVE-custom-details",
        "SENSITIVE-role-details",
        "SENSITIVE-diagnostic",
    ] {
        assert!(!encoded.contains(omitted), "opaque field leaked: {omitted}");
    }
    let metadata = map(&fixture_records(TREE), false);
    assert_eq!(
        metadata.records[4].attributes["unisphere.pi.tool.calls"],
        json!([{"id": "call-a", "name": "read"}])
    );
    assert_eq!(
        metadata.records[5].attributes["gen_ai.tool.call.id"],
        "call-a"
    );
    assert_eq!(
        metadata.records[5].attributes["unisphere.tool.is_error"],
        true
    );
}

#[test]
fn compaction_custom_bash_and_label_operations_never_become_ordinary_turns() {
    let batch = map(&fixture_records(TREE), true);
    for index in [0, 1, 2, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17] {
        let record = &batch.records[index];
        assert!(!record.attributes.contains_key("unisphere.message.role"));
        assert!(
            record
                .body
                .as_ref()
                .is_none_or(|body| body.get("role").is_none())
        );
    }
    assert_eq!(
        batch.records[7].body.as_ref().unwrap(),
        &json!({"kind": "compaction", "summary": "SENSITIVE-compaction"})
    );
    assert_eq!(
        batch.records[10].body.as_ref().unwrap()["kind"],
        "custom_message"
    );
    assert_eq!(
        batch.records[10].attributes["unisphere.pi.custom.display"],
        false
    );
    assert!(batch.records[9].body.is_none());
    assert!(
        !batch.records[9]
            .attributes
            .contains_key("unisphere.usage.scope")
    );
    assert_eq!(
        batch.records[12].attributes["unisphere.pi.label.target.id"],
        "user-a"
    );
    assert!(batch.records[12].body.is_none());
    let bash = &batch.records[14];
    assert_eq!(
        bash.attributes["unisphere.pi.bash.exclude_from_context"],
        true
    );
    assert_eq!(bash.attributes["unisphere.pi.bash.exit_code"], 1);
    assert_eq!(
        bash.body.as_ref().unwrap()["fullOutputPath"],
        "/SENSITIVE-output.txt"
    );
}

#[test]
fn timestamps_use_explicit_units_without_fallback_or_saturation() {
    let record = native(json!({"type": "message", "id": "a", "parentId": null,
        "timestamp": "1970-01-01T01:00:01.123456789+01:00",
        "message": {"role": "user", "timestamp": 2, "content": "text"}}));
    let batch = map(&[record], false);
    assert_eq!(batch.records[0].timestamp_unix_nano, Some(1_123_456_789));
    assert_eq!(
        batch.records[0].attributes["unisphere.pi.message.timestamp_unix_nano"],
        2_000_000
    );
    for bad in [
        json!("1969-12-31T23:59:59Z"),
        json!("9999-01-01T00:00:00Z"),
        json!(3),
        json!("SENSITIVE-bad-time"),
    ] {
        let mut value = message(json!({"role": "user", "timestamp": 42, "content": "ok"}));
        value["timestamp"] = bad;
        let batch = map(&[native(value)], false);
        assert_eq!(batch.records[0].timestamp_unix_nano, None);
        assert!(diagnostic(&batch, 73, Code::InvalidTimestamp));
        assert_eq!(
            batch.records[0].attributes["unisphere.pi.message.timestamp_unix_nano"],
            42_000_000
        );
    }
    for bad in [json!(-1), json!(0.5), json!(u64::MAX), json!("42")] {
        let batch = map(
            &[native(message(
                json!({"role": "user", "timestamp": bad, "content": "ok"}),
            ))],
            false,
        );
        assert!(
            !batch.records[0]
                .attributes
                .contains_key("unisphere.pi.message.timestamp_unix_nano")
        );
        assert!(diagnostic(&batch, 73, Code::InvalidTimestamp));
    }
    let batch = map(
        &[native(message(
            json!({"role": "user", "timestamp": 0, "content": "ok"}),
        ))],
        false,
    );
    assert_eq!(batch.records[0].timestamp_unix_nano, None);
    assert_eq!(
        batch.records[0].attributes["unisphere.pi.message.timestamp_unix_nano"],
        0
    );
}

#[test]
fn usage_rejects_invalid_components_independently_and_does_not_invent_totals() {
    let batch = map(
        &[native(message(
            json!({"role": "assistant", "content": [], "usage": {
                "input": -1, "output": 0, "cacheRead": "13", "cacheWrite": 0.5,
                "cacheWrite1h": u64::MAX, "reasoning": null,
                "cost": {"input": -0.01, "output": 0, "total": "SENSITIVE-invalid-cost"}
            }}),
        ))],
        false,
    );
    let attributes = &batch.records[0].attributes;
    assert_eq!(attributes["unisphere.usage.output_tokens"], 0);
    assert_eq!(attributes["unisphere.pi.usage.cost.output"], 0);
    assert_eq!(attributes["unisphere.usage.scope"], "assistant_message");
    for absent in [
        "unisphere.usage.input_tokens",
        "unisphere.usage.cache_read_input_tokens",
        "unisphere.usage.cache_creation_input_tokens",
        "unisphere.pi.usage.cache_write_1h_tokens",
        "unisphere.pi.usage.reasoning_tokens",
        "unisphere.pi.usage.total_tokens",
        "unisphere.pi.usage.cost.input",
        "unisphere.pi.usage.cost.total",
    ] {
        assert!(!attributes.contains_key(absent));
    }
    assert!(diagnostic(&batch, 73, Code::InvalidField));
    let batch = map(
        &[native(message(
            json!({"role": "assistant", "content": [], "usage": {"input": -1}}),
        ))],
        false,
    );
    assert!(
        !batch.records[0]
            .attributes
            .contains_key("unisphere.usage.scope")
    );
}

#[test]
fn malformed_and_unknown_parts_keep_valid_siblings_with_safe_diagnostics() {
    let record = native(message(json!({"role": "assistant", "content": [
        {"type": "text", "text": "kept"},
        {"type": "toolCall", "id": "call", "name": "read", "arguments": "SENSITIVE-invalid"},
        {"type": "image", "data": "SENSITIVE-wrong-role", "mimeType": "image/png"},
        {"type": "future", "secret": "SENSITIVE-future"},
        {"type": "thinking", "thinking": 3}, null
    ]})));
    for include_content in [false, true] {
        let batch = map(std::slice::from_ref(&record), include_content);
        assert!(diagnostic(&batch, 73, Code::InvalidField));
        assert!(diagnostic(&batch, 73, Code::UnsupportedPart));
        assert!(
            !serde_json::to_string(&batch)
                .unwrap()
                .contains("SENSITIVE-")
        );
        if include_content {
            assert_eq!(
                batch.records[0].body.as_ref().unwrap()["parts"],
                json!([
                    {"type": "text", "content": "kept"},
                    {"type": "unisphere.unknown", "native_type": "image"},
                    {"type": "unisphere.unknown", "native_type": "future"}
                ])
            );
        }
    }
}

#[test]
fn unsupported_records_and_roles_are_provenance_not_guessed_messages() {
    for value in [
        json!({"type": "future", "id": "a", "parentId": null, "payload": "SENSITIVE-future"}),
        message(json!({"role": "hookMessage", "content": "SENSITIVE-legacy"})),
    ] {
        let batch = map(&[native(value)], true);
        assert!(diagnostic(&batch, 73, Code::UnsupportedRecord));
        assert!(batch.records[0].body.is_none());
        assert!(
            !serde_json::to_string(&batch)
                .unwrap()
                .contains("SENSITIVE-")
        );
    }
    let batch = map(&[native(json!(["SENSITIVE-nonobject"]))], true);
    assert_eq!(
        batch.records[0].attributes["unisphere.source.kind"],
        "unknown"
    );
    assert!(diagnostic(&batch, 73, Code::InvalidField));
    for version in [json!(1), json!(2), json!(4), Value::Null] {
        let batch = map(
            &[native(
                json!({"type": "session", "id": "a", "version": version}),
            )],
            false,
        );
        assert!(diagnostic(&batch, 73, Code::UnsupportedRecord));
    }
}

#[test]
fn malformed_bytes_fail_the_batch_at_physical_offset_without_payload_leak() {
    for bytes in [
        b"{\"SENSITIVE-unfinished\":".to_vec(),
        vec![0xff],
        b"{}{}".to_vec(),
    ] {
        let records = [
            native(message(json!({"role": "user", "content": "valid"}))),
            NativeRecord { offset: 801, bytes },
        ];
        let error = PiAdapter
            .map(&source(), &records, MappingOptions::default())
            .unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        assert_eq!(error.offset(), Some(801));
        assert!(
            !format!("{error:?} {error} {} {}", error.message(), error.fix())
                .contains("SENSITIVE-")
        );
    }
}

#[test]
fn explicit_source_validation_precedes_parsing_without_opening_any_path() {
    let invalid_source = SessionRef {
        path: "relative.jsonl".into(),
    };
    let error = PiAdapter
        .map(
            &invalid_source,
            &[NativeRecord {
                offset: 1,
                bytes: vec![0xff],
            }],
            MappingOptions::default(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
    let records = fixture_records(TREE);
    let mapped = map(&records, false);
    assert_eq!(
        mapped.records[0].attributes["unisphere.source.path"],
        "/synthetic/not-opened/pi.jsonl"
    );
}
