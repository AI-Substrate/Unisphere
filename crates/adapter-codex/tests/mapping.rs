use serde_json::{Value, json};
use unisphere_adapter_codex::CodexAdapter;
use unisphere_core::{
    MappedBatch, MappingDiagnosticCode as Code, MappingOptions, NativeRecord, PipelineErrorKind,
    SessionAdapter, SessionRef,
};
use unisphere_testkit::collection::{assert_adapter_conformance, fixture_records};

const ROLLOUT: &[u8] = include_bytes!("../fixtures/rollout.jsonl");

fn source() -> SessionRef {
    SessionRef {
        path: "/synthetic/not-opened/rollout.jsonl".into(),
    }
}

fn map(records: &[NativeRecord], include_content: bool) -> MappedBatch {
    CodexAdapter
        .map(&source(), records, MappingOptions { include_content })
        .unwrap()
}

fn native(value: Value) -> NativeRecord {
    NativeRecord {
        offset: 73,
        bytes: serde_json::to_vec(&value).unwrap(),
    }
}

fn response(payload: Value) -> NativeRecord {
    native(json!({"type": "response_item", "payload": payload}))
}

fn has_diagnostic(batch: &MappedBatch, code: Code) -> bool {
    batch
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == code)
}

#[test]
fn shared_conformance_and_batch_partition_preserve_physical_identity() {
    let records = fixture_records(ROLLOUT);
    assert_adapter_conformance(&CodexAdapter, &source(), &records);
    for include_content in [false, true] {
        let whole = map(&records, include_content);
        for split_at in 0..=records.len() {
            let mut split = map(&records[..split_at], include_content);
            let rest = map(&records[split_at..], include_content);
            split.records.extend(rest.records);
            split.diagnostics.extend(rest.diagnostics);
            assert_eq!(whole, split);
        }
        assert_eq!(
            whole.records[0].attributes["gen_ai.conversation.id"],
            "synthetic-thread"
        );
        assert_eq!(
            whole.records[0].attributes["unisphere.source.session.id"],
            "synthetic-root"
        );
        assert_eq!(
            whole.records[1].attributes["gen_ai.request.model"],
            "synthetic-model"
        );
        assert_eq!(
            whole.records[1].attributes["unisphere.source.turn.id"],
            "synthetic-turn"
        );
        for later in &whole.records[2..] {
            assert!(!later.attributes.contains_key("gen_ai.conversation.id"));
            assert!(!later.attributes.contains_key("gen_ai.request.model"));
            assert!(!later.attributes.contains_key("unisphere.source.session.id"));
        }
    }
    assert_eq!(map(&[], false), MappedBatch::default());
}

#[test]
fn function_and_custom_tools_keep_pairing_and_native_argument_encoding() {
    let batch = map(&fixture_records(ROLLOUT), true);
    let call = batch.records[4].body.as_ref().unwrap();
    assert_eq!(call["parts"][0]["type"], "tool_call");
    assert_eq!(call["parts"][0]["id"], "synthetic-call");
    assert_eq!(call["parts"][0]["name"], "read_file");
    assert_eq!(
        call["parts"][0]["arguments"],
        "{\"path\":\"SENSITIVE-ARGUMENT\"}"
    );
    let output = batch.records[5].body.as_ref().unwrap();
    assert_eq!(output["parts"][0]["id"], call["parts"][0]["id"]);
    assert_eq!(output["parts"][0]["response"], "SENSITIVE-OUTPUT");
    assert!(output["parts"][0].get("name").is_none());
    let custom = batch.records[6].body.as_ref().unwrap();
    assert_eq!(custom["parts"][0]["arguments"], "SENSITIVE-PATCH");
    let custom_output = batch.records[7].body.as_ref().unwrap();
    assert_eq!(custom_output["parts"][0]["id"], custom["parts"][0]["id"]);
    assert_eq!(
        custom_output["parts"][0]["response"][0]["content"],
        "SENSITIVE-RESULT"
    );
    assert_eq!(
        custom_output["parts"][0]["response"][1]["native_type"],
        "encrypted_content"
    );
    assert!(
        !serde_json::to_string(&batch)
            .unwrap()
            .contains("SENSITIVE-OPAQUE")
    );
}

#[test]
fn summaries_compaction_and_encrypted_parts_are_not_ordinary_turns() {
    let batch = map(&fixture_records(ROLLOUT), true);
    let reasoning = batch.records[3].body.as_ref().unwrap();
    assert_eq!(reasoning["parts"][0]["content"], "SENSITIVE-SUMMARY");
    assert_eq!(
        reasoning["parts"][0]["unisphere.codex.native_type"],
        "summary_text"
    );
    assert_eq!(reasoning["parts"][1]["content"], "SENSITIVE-REASONING");
    assert_eq!(
        reasoning["parts"][1]["unisphere.codex.native_type"],
        "reasoning_text"
    );
    assert_eq!(batch.records[8].body.as_ref().unwrap()["role"], "assistant");
    let summary = &batch.records[9];
    assert_eq!(
        summary.attributes["unisphere.codex.representation"],
        "native_event_summary"
    );
    assert!(!summary.attributes.contains_key("unisphere.message.role"));
    assert_eq!(
        summary.body.as_ref().unwrap()["parts"][0]["type"],
        "unisphere.codex.message_summary"
    );
    assert!(summary.body.as_ref().unwrap().get("role").is_none());
    assert!(
        batch.records[12].body.is_none(),
        "execution event must not duplicate tool output"
    );
    let compaction = &batch.records[13];
    assert_eq!(
        compaction.attributes["unisphere.codex.representation"],
        "compaction"
    );
    assert_eq!(
        compaction.body.as_ref().unwrap()["parts"][0]["type"],
        "unisphere.compaction"
    );
    assert!(compaction.body.as_ref().unwrap().get("role").is_none());
    assert!(batch.records[14].body.is_none());
    let encoded = serde_json::to_string(&batch).unwrap();
    for marker in [
        "SENSITIVE-ENCRYPTED",
        "SENSITIVE-REPLACEMENT",
        "SENSITIVE-STATE",
        "SENSITIVE-IMAGE",
        "SENSITIVE-INSTRUCTIONS",
    ] {
        assert!(!encoded.contains(marker));
    }
    assert!(has_diagnostic(&batch, Code::UnsupportedPart));
    assert!(has_diagnostic(&batch, Code::UnsupportedRecord));
}

#[test]
fn repeated_last_and_cumulative_usage_remain_independent_snapshots() {
    let batch = map(&fixture_records(ROLLOUT), false);
    for record in &batch.records[10..12] {
        let attributes = &record.attributes;
        assert_eq!(
            attributes["unisphere.usage.last_token_usage.input_tokens"],
            10
        );
        assert_eq!(
            attributes["unisphere.usage.last_token_usage.cached_input_tokens"],
            4
        );
        assert_eq!(
            attributes["unisphere.usage.last_token_usage.reasoning_output_tokens"],
            2
        );
        assert_eq!(
            attributes["unisphere.usage.last_token_usage.total_tokens"],
            13
        );
        assert_eq!(
            attributes["unisphere.usage.total_token_usage.input_tokens"],
            20
        );
        assert_eq!(
            attributes["unisphere.usage.total_token_usage.total_tokens"],
            26
        );
        assert_eq!(
            attributes["unisphere.usage.last_token_usage.scope"],
            "native_last_token_usage"
        );
        assert_eq!(
            attributes["unisphere.usage.total_token_usage.scope"],
            "native_cumulative_token_usage"
        );
        assert!(
            !attributes
                .keys()
                .any(|key| key.starts_with("gen_ai.usage."))
        );
        assert!(!attributes.contains_key("unisphere.usage.input_tokens"));
    }
    assert_ne!(
        batch.records[10].attributes["unisphere.source.offset"],
        batch.records[11].attributes["unisphere.source.offset"]
    );
    assert_eq!(
        batch.records[10].attributes["unisphere.codex.model_context_window"],
        200000
    );
}

#[test]
fn usage_rejects_invalid_components_without_guessing_missing_totals() {
    let records = [native(json!({"type": "event_msg", "payload": {
        "type": "token_count", "info": {
            "last_token_usage": {"input_tokens": 7, "cached_input_tokens": -1, "output_tokens": 2.5, "reasoning_output_tokens": "3"},
            "total_token_usage": {"input_tokens": 9223372036854775808_u64, "output_tokens": 0, "total_tokens": 9223372036854775807_i64},
            "model_context_window": false
        }
    }}))];
    let batch = map(&records, false);
    let attributes = &batch.records[0].attributes;
    assert_eq!(
        attributes["unisphere.usage.last_token_usage.input_tokens"],
        7
    );
    assert!(!attributes.contains_key("unisphere.usage.last_token_usage.total_tokens"));
    assert!(!attributes.contains_key("unisphere.usage.last_token_usage.output_tokens"));
    assert!(!attributes.contains_key("unisphere.usage.last_token_usage.cached_input_tokens"));
    assert!(!attributes.contains_key("unisphere.usage.last_token_usage.reasoning_output_tokens"));
    assert!(!attributes.contains_key("unisphere.usage.total_token_usage.input_tokens"));
    assert_eq!(
        attributes["unisphere.usage.total_token_usage.output_tokens"],
        0
    );
    assert_eq!(
        attributes["unisphere.usage.total_token_usage.total_tokens"],
        json!(i64::MAX)
    );
    assert!(!attributes.contains_key("unisphere.codex.model_context_window"));
    assert!(has_diagnostic(&batch, Code::InvalidField));
    for info in [Value::Null, json!({})] {
        let batch = map(
            &[native(
                json!({"type": "event_msg", "payload": {"type": "token_count", "info": info}}),
            )],
            false,
        );
        assert!(
            !batch.records[0]
                .attributes
                .keys()
                .any(|key| key.starts_with("unisphere.usage."))
        );
    }
}

#[test]
fn timestamps_preserve_nanoseconds_and_never_fall_back_to_header_or_clock() {
    let fixture = map(&fixture_records(ROLLOUT), false);
    assert_eq!(
        fixture.records[0].timestamp_unix_nano,
        Some(1767323045123456789)
    );
    let same_instant = native(
        json!({"timestamp": "2026-01-02T04:04:05.123456789+01:00", "type": "session_meta", "payload": {}}),
    );
    assert_eq!(
        map(&[same_instant], false).records[0].timestamp_unix_nano,
        fixture.records[0].timestamp_unix_nano
    );
    for timestamp in [
        json!("bad"),
        json!("1969-12-31T23:59:59Z"),
        json!("9999-01-01T00:00:00Z"),
        json!(123),
    ] {
        let batch = map(
            &[native(
                json!({"timestamp": timestamp, "type": "session_meta", "payload": {"timestamp": "2026-01-02T03:04:05Z"}}),
            )],
            false,
        );
        assert_eq!(batch.records[0].timestamp_unix_nano, None);
        assert!(has_diagnostic(&batch, Code::InvalidTimestamp));
    }
    let absent = map(
        &[native(
            json!({"type": "session_meta", "payload": {"timestamp": "2026-01-02T03:04:05Z"}}),
        )],
        false,
    );
    assert_eq!(absent.records[0].timestamp_unix_nano, None);
    assert!(!has_diagnostic(&absent, Code::InvalidTimestamp));
}

#[test]
fn malformed_bytes_fail_the_batch_with_only_safe_offset_diagnostics() {
    for bytes in [
        b"{\"SENSITIVE-UNFINISHED\":".to_vec(),
        vec![0xff],
        b"{}{}".to_vec(),
    ] {
        let records = [
            response(json!({"type": "message", "role": "user", "content": []})),
            NativeRecord { offset: 801, bytes },
        ];
        let error = CodexAdapter
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
fn invalid_fields_preserve_supported_siblings_without_promoting_unknown_payloads() {
    let records = [
        response(json!({"type": "message", "role": "assistant", "content": [
            {"type": "output_text", "text": "SENSITIVE-GOOD"},
            {"type": "output_text", "text": {"secret": "SENSITIVE-BAD"}},
            {"type": "future_part", "text": "SENSITIVE-UNKNOWN"}
        ]})),
        response(
            json!({"type": "message", "role": 7, "content": [{"type": "input_text", "text": "SENSITIVE-NOROLE"}]}),
        ),
        response(
            json!({"type": "function_call", "name": "x", "arguments": {"not": "SENSITIVE-STRING"}}),
        ),
        response(json!({"type": "future", "content": "SENSITIVE-FUTURE"})),
        native(json!(["SENSITIVE-NONOBJECT"])),
    ];
    let content = map(&records, true);
    assert_eq!(
        content.records[0].body.as_ref().unwrap()["parts"],
        json!([
            {"type": "text", "content": "SENSITIVE-GOOD", "unisphere.codex.native_type": "output_text"},
            {"type": "unisphere.unknown", "native_type": "future_part"}
        ])
    );
    assert!(
        content.records[1..]
            .iter()
            .all(|record| record.body.is_none())
    );
    assert_eq!(
        content.records[4].attributes["unisphere.source.kind"],
        "unknown"
    );
    let metadata = map(&records, false);
    assert!(metadata.records.iter().all(|record| record.body.is_none()));
    assert!(
        !serde_json::to_string(&metadata)
            .unwrap()
            .contains("SENSITIVE-")
    );
    for batch in [&metadata, &content] {
        assert!(has_diagnostic(batch, Code::InvalidField));
        assert!(has_diagnostic(batch, Code::UnsupportedPart));
        assert!(has_diagnostic(batch, Code::UnsupportedRecord));
        assert!(
            batch
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.offset == 73)
        );
    }
}

#[test]
fn native_shell_and_search_actions_are_opt_in_and_do_not_invent_call_ids() {
    let records = [
        response(
            json!({"type": "local_shell_call", "id": "legacy-item", "action": {"type": "exec", "command": ["echo", "SENSITIVE-SHELL"], "working_directory": "/SENSITIVE-WORKDIR", "env": {"SECRET": "SENSITIVE-ENV"}}}),
        ),
        response(
            json!({"type": "web_search_call", "id": "search-item", "action": {"type": "search", "queries": ["SENSITIVE-QUERY"]}}),
        ),
        response(
            json!({"type": "web_search_call", "action": {"type": "future", "query": "SENSITIVE-FUTURE"}}),
        ),
    ];
    let content = map(&records, true);
    let shell = &content.records[0].body.as_ref().unwrap()["parts"][0];
    assert_eq!(
        shell["arguments"]["command"],
        json!(["echo", "SENSITIVE-SHELL"])
    );
    assert_eq!(shell["unisphere.codex.tool_kind"], "local_shell_call");
    assert!(shell.get("id").is_none(), "item id is not call_id");
    assert!(shell.get("name").is_none());
    assert!(shell["arguments"].get("env").is_none());
    assert_eq!(
        content.records[1].body.as_ref().unwrap()["parts"][0]["arguments"]["queries"],
        json!(["SENSITIVE-QUERY"])
    );
    assert!(content.records[2].body.is_none());
    let metadata = map(&records, false);
    assert!(
        !serde_json::to_string(&metadata)
            .unwrap()
            .contains("SENSITIVE-")
    );
    assert!(has_diagnostic(&metadata, Code::UnsupportedPart));
}

#[test]
fn supplied_native_turn_metadata_and_source_validation_need_no_filesystem() {
    let batch = map(
        &[response(
            json!({"type": "message", "id": "message-id", "role": "developer", "internal_chat_message_metadata_passthrough": {"turn_id": "native-turn"}, "content": []}),
        )],
        true,
    );
    assert_eq!(
        batch.records[0].attributes["unisphere.source.turn.id"],
        "native-turn"
    );
    assert_eq!(
        batch.records[0].attributes["unisphere.source.record.id"],
        "message-id"
    );
    assert_eq!(
        batch.records[0].body,
        Some(json!({"role": "developer", "parts": []}))
    );
    let error = CodexAdapter
        .map(
            &SessionRef {
                path: "relative.jsonl".into(),
            },
            &[],
            MappingOptions::default(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
}
