use serde_json::{Value, json};
use unisphere_adapter_vscode_copilot::{DESCRIPTOR, VsCodeCopilotAdapter};
use unisphere_core::{
    MappedSnapshot, MappingDiagnosticCode as Code, MappingOptions, NativeSnapshot,
    PipelineErrorKind, SnapshotAdapter, SnapshotFormat, SnapshotRecord, SnapshotRef,
};

const V1: &[u8] = include_bytes!("fixtures/session-v1.json");
const V2: &[u8] = include_bytes!("fixtures/session-v2.json");
const V3: &[u8] = include_bytes!("fixtures/session-v3.json");
const JOURNAL: &[u8] = include_bytes!("fixtures/session-journal.jsonl");

fn snapshot(format: SnapshotFormat, records: Vec<SnapshotRecord>) -> NativeSnapshot {
    NativeSnapshot {
        source: SnapshotRef {
            path: "/synthetic/not-opened/session".into(), format, session_id: None,
        },
        revision: "synthetic-loader-revision".into(), records,
    }
}

fn document(bytes: &[u8]) -> NativeSnapshot {
    snapshot(SnapshotFormat::JsonDocument, vec![SnapshotRecord {
        key: "document".into(), bytes: bytes.to_vec(),
    }])
}

fn native(value: Value) -> NativeSnapshot {
    document(&serde_json::to_vec(&value).unwrap())
}

fn journal(operations: &[Value]) -> NativeSnapshot {
    snapshot(SnapshotFormat::JsonJournal, operations.iter().enumerate().map(|(index, operation)| SnapshotRecord {
        key: format!("journal:{index}"), bytes: serde_json::to_vec(operation).unwrap(),
    }).collect())
}

fn journal_fixture() -> NativeSnapshot {
    snapshot(SnapshotFormat::JsonJournal, JOURNAL.split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty()).enumerate().map(|(index, line)| SnapshotRecord {
            key: format!("journal:{index}"), bytes: line.to_vec(),
        }).collect())
}

fn map(source: &NativeSnapshot, include_content: bool) -> MappedSnapshot {
    VsCodeCopilotAdapter.map_snapshot(source, MappingOptions { include_content }).unwrap()
}

fn has(batch: &MappedSnapshot, key: &str, code: Code) -> bool {
    batch.diagnostics.iter().any(|diagnostic| diagnostic.key == key && diagnostic.code == code)
}

#[test]
fn native_versions_preserve_requests_and_responses_without_synthetic_ids_or_times() {
    for (bytes, version, session) in [(V1, 1, "synthetic-vscode-v1"), (V2, 2, "synthetic-vscode-v2"), (V3, 3, "synthetic-vscode-v3")] {
        let mapped = map(&document(bytes), true);
        assert_eq!(mapped.records.len(), 3);
        assert_eq!(mapped.records[0].attributes["unisphere.vscode.schema.version"], version);
        assert_eq!(mapped.records[1].attributes["unisphere.message.role"], "user");
        assert_eq!(mapped.records[2].attributes["unisphere.message.role"], "assistant");
        assert_eq!(mapped.records[2].attributes["unisphere.source.parent.id"], mapped.records[1].attributes["unisphere.message.id"]);
        assert_eq!(mapped.records[2].attributes["gen_ai.conversation.id"], session);
    }
    let mapped = map(&document(V1), true);
    assert_eq!(mapped.records[0].timestamp_unix_nano, Some(1_700_000_000_000_000_000));
    assert_eq!(mapped.records[1].timestamp_unix_nano, Some(1_700_000_000_001_000_000));
    assert_eq!(mapped.records[2].timestamp_unix_nano, None);
    assert_eq!(mapped.records[1].body, Some(json!({"role":"user", "parts":[{"type":"text", "content":"SENSITIVE-LEGACY-REQUEST"}]})));
    assert_eq!(mapped.records[2].body, Some(json!({"role":"assistant", "parts":[{"type":"text", "content":"SENSITIVE-LEGACY-ANSWER"}]})));
    assert_eq!(map(&document(V2), true).records[0].body.as_ref().unwrap()["title"], "SENSITIVE-COMPUTED-TITLE");
    let missing = map(&native(json!({"requests":[{"message":"text"}]})), true);
    assert_eq!(missing.records.len(), 2); // no invented assistant response
    assert!(!missing.records[1].attributes.contains_key("unisphere.message.id"));
    assert!(!missing.records[1].attributes.contains_key("gen_ai.conversation.id"));
    assert_eq!(missing.records[0].timestamp_unix_nano, None);
}

#[test]
fn metadata_only_never_exposes_content_and_mapping_is_repeatable() {
    for source in [document(V1), document(V2), document(V3), journal_fixture()] {
        let before = source.clone();
        let metadata = map(&source, false);
        assert_eq!(metadata, map(&source, false));
        assert_eq!(before, source);
        assert!(metadata.records.iter().all(|record| record.body.is_none()));
        let serialized = serde_json::to_string(&metadata.records).unwrap();
        assert!(!serialized.contains("SENSITIVE-"));
        assert!(metadata.diagnostics.iter().any(|diagnostic| diagnostic.code == Code::ContentOmitted));
        for record in metadata.records {
            assert_eq!(record.event_name, "unisphere.session.record");
            assert_eq!(record.attributes["unisphere.source.adapter"], DESCRIPTOR.id);
            assert_eq!(record.attributes["unisphere.profile.version"], 1);
            assert_eq!(record.attributes["unisphere.source.revision"], source.revision);
            assert_eq!(record.attributes["unisphere.source.path"], "/synthetic/not-opened/session");
            assert!(record.attributes["unisphere.source.key"].as_str().unwrap().starts_with(
                if matches!(source.source.format, SnapshotFormat::JsonJournal) { "journal:reduced" } else { "document" }
            ));
            assert!(!record.attributes.contains_key("unisphere.source.offset"));
            assert!(!record.attributes.contains_key("gen_ai.response.model"));
            assert!(!record.attributes.keys().any(|key| key.starts_with("gen_ai.usage.") || key.contains("trace_id") || key.contains("span_id")));
        }
    }
}

#[test]
fn tool_identity_survives_privacy_filter_but_native_details_require_opt_in() {
    let metadata = map(&document(V3), false);
    assert_eq!(metadata.records[2].attributes["unisphere.vscode.tools"], json!([
        {"name":"read_file", "id":"tool-call-v3", "is_complete":true, "confirmation_kind":1}
    ]));
    let content = map(&document(V3), true);
    let parts = &content.records[2].body.as_ref().unwrap()["parts"];
    assert_eq!(parts[0], json!({"type":"text", "content":"SENSITIVE-ASSISTANT-TEXT"}));
    assert_eq!(parts[1], json!({"type":"reasoning", "content":["SENSITIVE-THINKING-A", "SENSITIVE-THINKING-B"]}));
    assert_eq!(parts[2]["id"], "tool-call-v3");
    assert_eq!(parts[2]["resultDetails"]["output"], "SENSITIVE-NATIVE-OUTPUT-DISPLAY");
    assert!(parts[2].get("arguments").is_none()); // serialized data lacks LM arguments
    assert_eq!(parts[3], json!({"type":"unisphere.unknown", "native_type":"textEditGroup"}));
    assert!(has(&content, "document#/requests/0/response/3", Code::UnsupportedPart));
    assert!(has(&content, "document#/requests/0/response/4", Code::UnsupportedPart));
    assert!(has(&content, "document", Code::UnsupportedPart)); // unsent draft is not a user message
    let serialized = serde_json::to_string(&content.records).unwrap();
    for omitted in ["SENSITIVE-UNSENT-DRAFT", "SENSITIVE-ATTACHMENT", "SENSITIVE-EDIT", "SENSITIVE-UNKNOWN"] {
        assert!(!serialized.contains(omitted));
    }
}

#[test]
fn usage_scopes_do_not_confuse_latest_call_turn_and_session_counters() {
    let mapped = map(&document(V3), false);
    let response = &mapped.records[2];
    assert_eq!(response.attributes["gen_ai.request.model"], "auto");
    assert!(!response.attributes.contains_key("gen_ai.response.model"));
    assert_eq!(response.attributes["unisphere.vscode.usage"], json!({
        "promptTokens":{"value":120,"scope":"latest_model_call"},
        "completionTokens":{"value":31,"scope":"native_response_counter"},
        "copilotCredits":{"value":0.5,"scope":"response_cost"},
        "sessionCopilotCredits":{"value":1.75,"scope":"session_cumulative"},
        "modelTotals":{"value":[{"model":"synthetic-served-model","inputTokens":250,"cachedTokens":40,"outputTokens":65}],"scope":"whole_turn_including_subagents"}
    }));
    let invalid = map(&native(json!({"version":3,"requests":[{
        "message":"x", "response":[], "promptTokens":-1, "completionTokens":0.5,
        "copilotCredits":-0.25, "sessionCopilotCredits":18446744073709551615u64,
        "modelTotals":[{"model":"bad","inputTokens":1,"cachedTokens":-1,"outputTokens":2}]
    }]})), false);
    let usage = &invalid.records[2].attributes["unisphere.vscode.usage"];
    assert!(usage.get("promptTokens").is_none());
    assert!(usage.get("completionTokens").is_none());
    assert!(usage.get("copilotCredits").is_none());
    assert!(usage.get("sessionCopilotCredits").is_none());
    assert_eq!(usage["modelTotals"]["value"], json!([]));
    assert!(has(&invalid, "document#/requests/0/response", Code::InvalidField));
}

#[test]
fn native_journal_emits_only_final_logical_revision() {
    let mapped = map(&journal_fixture(), true);
    assert_eq!(mapped.records.len(), 3);
    assert!(mapped.records[0].body.is_none()); // Delete, not stale title
    assert_eq!(mapped.records[1].attributes["unisphere.message.id"], "request-journal");
    assert_eq!(mapped.records[2].attributes["unisphere.source.format"], "json_journal");
    assert_eq!(mapped.records[2].body.as_ref().unwrap()["parts"][0]["content"], "SENSITIVE-FINAL-TEXT");
    assert_eq!(mapped.records[2].attributes["unisphere.vscode.tools"][0]["is_complete"], true);
    assert_eq!(mapped.records[2].attributes["unisphere.vscode.usage"]["promptTokens"]["value"], 13);
    let serialized = serde_json::to_string(&mapped.records).unwrap();
    assert!(!serialized.contains("SENSITIVE-OBSOLETE"));
    assert!(!serialized.contains("SENSITIVE-REMOVED"));
}

#[test]
fn suffix_replacement_and_later_initial_follow_native_replay_order() {
    let mapped = map(&journal(&[
        json!({"kind":0,"v":{"version":3,"requests":[{"message":"discarded"}]}}),
        json!({"kind":0,"v":{"version":2,"computedTitle":"kept", "requests":[]}}),
        json!({"kind":2,"k":["requests"],"v":[{"message":"first"},{"message":"old suffix"}]}),
        json!({"kind":2,"k":["requests"],"i":1,"v":[{"message":"new suffix"}]}),
        json!({"kind":1,"k":["requests","0","requestId"],"v":"first-id"}),
        json!({"kind":1,"k":[],"v":{"requests":[]}}),
        json!({"kind":3,"k":[]}),
    ]), true);
    assert_eq!(mapped.records.len(), 3);
    assert_eq!(mapped.records[0].body.as_ref().unwrap()["title"], "kept");
    assert_eq!(mapped.records[1].attributes["unisphere.message.id"], "first-id");
    assert_eq!(mapped.records[2].body.as_ref().unwrap()["parts"][0]["content"], "new suffix");
}

#[test]
fn sparse_array_extension_and_delete_do_not_shift_following_parts() {
    let mapped = map(&journal(&[
        json!({"kind":0,"v":{"version":3,"requests":[{"message":"x","response":[]}]}}),
        json!({"kind":2,"k":["requests",0,"response"],"i":1,"v":[{"value":"second"}]}),
        json!({"kind":1,"k":["requests",0,"response",0],"v":{"value":"first"}}),
        json!({"kind":3,"k":["requests",0,"response",0]}),
    ]), true);
    assert_eq!(mapped.records[2].body, Some(json!({"role":"assistant","parts":[{"type":"text","content":"second"}]})));
    assert!(has(&mapped, "journal:reduced#/requests/0/response/0", Code::InvalidField));
}

#[test]
fn push_creates_missing_or_falsy_leaf_but_not_missing_intermediate() {
    for leaf in [Value::Null, json!(false), json!(0), json!("")] {
        let mapped = map(&journal(&[
            json!({"kind":0,"v":{"version":3,"requests":leaf}}),
            json!({"kind":2,"k":["requests"],"v":[{"message":"created"}]}),
        ]), true);
        assert_eq!(mapped.records[1].body.as_ref().unwrap()["parts"][0]["content"], "created");
    }
    let missing = map(&journal(&[
        json!({"kind":0,"v":{"version":3}}),
        json!({"kind":2,"k":["requests"],"v":[{"message":"created"}]}),
    ]), true);
    assert_eq!(missing.records.len(), 2);
    let error = VsCodeCopilotAdapter.map_snapshot(&journal(&[
        json!({"kind":0,"v":{"version":3}}),
        json!({"kind":1,"k":["missing","child"],"v":"SENSITIVE-NO-PARENT"}),
    ]), MappingOptions::default()).unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
    assert_eq!(error.offset(), None);
}

#[test]
fn malformed_journal_never_salvages_an_earlier_session_or_leaks_payloads() {
    let initial = json!({"kind":0,"v":{"version":3,"requests":[{"message":"SENSITIVE-PREFIX"}]}});
    let cases = [
        vec![],
        vec![json!({"kind":1,"k":["requests"],"v":[]})],
        vec![initial.clone(), json!({"kind":42,"payload":"SENSITIVE-UNKNOWN-OP"})],
        vec![initial.clone(), json!({"kind":1,"k":["requests",9,"message"],"v":"SENSITIVE-MISSING"})],
        vec![initial.clone(), json!({"kind":2,"k":["requests"],"i":-1})],
        vec![initial.clone(), json!({"kind":2,"k":["requests"],"i":0.5})],
        vec![initial.clone(), json!({"kind":2,"k":["requests"],"v":{}})],
        vec![initial.clone(), json!({"kind":2,"k":[],"v":[]})],
        vec![initial.clone(), json!({"kind":1,"k":[true],"v":1})],
        vec![initial.clone(), json!({"kind":0})],
    ];
    for operations in cases {
        let error = VsCodeCopilotAdapter.map_snapshot(&journal(&operations), MappingOptions { include_content:true }).unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        assert_eq!(error.offset(), None);
        assert!(!format!("{error:?} {error}").contains("SENSITIVE-"));
    }
    let mut broken = journal(&[initial]);
    broken.records.push(SnapshotRecord { key:"journal:1".into(), bytes:b"{\"SENSITIVE-TRUNCATED".to_vec() });
    assert_eq!(VsCodeCopilotAdapter.map_snapshot(&broken, MappingOptions::default()).unwrap_err().kind(), PipelineErrorKind::InvalidData);
}

#[test]
fn tiny_journal_cannot_allocate_a_giant_sparse_array() {
    for operation in [
        json!({"kind":2,"k":["requests"],"i":4294967295u64}),
        json!({"kind":1,"k":["requests",4000000000u64],"v":null}),
    ] {
        let source = journal(&[json!({"kind":0,"v":{"version":3,"requests":[]}}), operation]);
        let error = VsCodeCopilotAdapter.map_snapshot(&source, MappingOptions::default()).unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::BatchLimit);
        assert_eq!(error.offset(), None);
    }
}

#[test]
fn session_selection_is_verified_against_native_identity() {
    let mut source = document(V3);
    source.source.session_id = Some("synthetic-vscode-v3".into());
    assert_eq!(map(&source, false).records[0].attributes["unisphere.source.session.id"], "synthetic-vscode-v3");
    source.source.session_id = Some("different-session".into());
    assert_eq!(VsCodeCopilotAdapter.map_snapshot(&source, MappingOptions::default()).unwrap_err().kind(), PipelineErrorKind::InvalidInput);
    let mut missing = native(json!({"version":3,"requests":[]}));
    missing.source.session_id = Some("not-in-native-data".into());
    assert_eq!(VsCodeCopilotAdapter.map_snapshot(&missing, MappingOptions::default()).unwrap_err().kind(), PipelineErrorKind::InvalidInput);
}

#[test]
fn empty_replacement_and_future_schema_are_observable_without_guessing() {
    let empty = map(&native(json!({"version":3,"sessionId":"empty","requests":[]})), true);
    assert_eq!(empty.records.len(), 1);
    assert_eq!(empty.records[0].attributes["gen_ai.conversation.id"], "empty");
    let future = map(&native(json!({"version":4,"sessionId":"future","requests":[{"message":"SENSITIVE-FUTURE"}]})), true);
    assert_eq!(future.records.len(), 1);
    assert!(has(&future, "document", Code::UnsupportedRecord));
    assert!(future.records[0].body.is_none());
}

#[test]
fn invalid_optional_timestamps_and_unknown_fields_do_not_become_guessed_data() {
    let mapped = map(&native(json!({"version":3,"creationDate":-1,"requests":[{
        "message":"text", "timestamp":"SENSITIVE-NOT-TIME", "responseTimestamp":18446744073709551615u64,
        "response":[{"kind":"markdownContent","content":{"value":"explicit markdown"}},{"kind":"thinking","value":17}],
        "SENSITIVE-UNKNOWN-FIELD":"SENSITIVE-UNKNOWN-VALUE"
    }]})), true);
    assert!(mapped.records.iter().all(|record| record.timestamp_unix_nano.is_none()));
    assert!(has(&mapped, "document", Code::InvalidTimestamp));
    assert!(has(&mapped, "document#/requests/0/message", Code::InvalidTimestamp));
    assert!(has(&mapped, "document#/requests/0/response", Code::InvalidTimestamp));
    assert!(has(&mapped, "document#/requests/0/response/1", Code::InvalidField));
    assert_eq!(mapped.records[2].body.as_ref().unwrap()["parts"][0]["content"], "explicit markdown");
    assert!(!serde_json::to_string(&mapped.diagnostics).unwrap().contains("SENSITIVE-"));
}

#[test]
fn wrong_representation_and_invalid_document_fail_with_fixed_errors() {
    for bytes in [b"{\"SENSITIVE-MALFORMED".as_slice(), b"\xff", b"null"] {
        let error = VsCodeCopilotAdapter.map_snapshot(&document(bytes), MappingOptions::default()).unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        assert_eq!(error.offset(), None);
        assert!(!format!("{error:?}").contains("SENSITIVE-"));
    }
    let database = snapshot(SnapshotFormat::SqliteKeyValue { table:"ItemTable".into() }, vec![]);
    assert_eq!(VsCodeCopilotAdapter.map_snapshot(&database, MappingOptions::default()).unwrap_err().kind(), PipelineErrorKind::InvalidInput);
}
