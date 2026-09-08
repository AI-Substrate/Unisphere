use serde_json::{Value, json};
use unisphere_adapter_cursor::{CursorIdeAdapter, IDE_DESCRIPTOR};
use unisphere_core::{
    MappedSnapshot, MappingDiagnosticCode as Code, MappingOptions, NativeSnapshot, PipelineErrorKind,
    SnapshotAdapter, SnapshotFormat, SnapshotRecord, SnapshotRef,
};

fn fixture() -> NativeSnapshot {
    let rows: Vec<Value> = serde_json::from_str(include_str!("../fixtures/ide.json")).unwrap();
    NativeSnapshot {
        source: SnapshotRef {
            path: "/synthetic/not-opened/state.vscdb".into(),
            format: SnapshotFormat::SqliteKeyValue { table: "cursorDiskKV".into() },
            session_id: Some("alpha".into()),
        },
        revision: "synthetic-revision-one".into(),
        records: rows.into_iter().map(|row| SnapshotRecord {
            key: row["key"].as_str().unwrap().into(),
            bytes: serde_json::to_vec(&row["value"]).unwrap(),
        }).collect(),
    }
}

fn map(snapshot: &NativeSnapshot, content: bool) -> MappedSnapshot {
    CursorIdeAdapter.map_snapshot(snapshot, MappingOptions { include_content: content }).unwrap()
}

fn change(snapshot: &mut NativeSnapshot, key: &str, edit: impl FnOnce(&mut Value)) {
    let row = snapshot.records.iter_mut().find(|row| row.key == key).unwrap();
    let mut value: Value = serde_json::from_slice(&row.bytes).unwrap();
    edit(&mut value);
    row.bytes = serde_json::to_vec(&value).unwrap();
}

fn diagnosed(batch: &MappedSnapshot, key: &str, code: Code) -> bool {
    batch.diagnostics.iter().any(|diagnostic| diagnostic.key == key && diagnostic.code == code)
}

fn keys(batch: &MappedSnapshot) -> Vec<&str> {
    batch.records.iter().map(|record| record.attributes["unisphere.source.key"].as_str().unwrap()).collect()
}

#[test]
fn composer_order_overrides_row_order_lexical_keys_and_timestamps() {
    let mut snapshot = fixture();
    let first = map(&snapshot, true);
    assert_eq!(keys(&first), ["composerData:alpha", "bubbleId:alpha:z", "bubbleId:alpha:a", "bubbleId:alpha:s"]);
    snapshot.records.reverse();
    assert_eq!(first, map(&snapshot, true));
    assert_eq!(first.records[1].attributes["unisphere.message.role"], "user");
    assert_eq!(first.records[2].attributes["unisphere.message.role"], "assistant");
    assert_eq!(first.records[1].timestamp_unix_nano, Some(3_000_000_000));
    assert_eq!(first.records[2].timestamp_unix_nano, Some(2_000_000_007));
    assert_eq!(first.records[0].timestamp_unix_nano, Some(1_000_000_000));
}

#[test]
fn source_key_revision_and_verified_session_replace_fake_offsets() {
    let snapshot = fixture();
    let batch = map(&snapshot, false);
    for record in &batch.records {
        assert_eq!(record.event_name, "unisphere.session.record");
        assert_eq!(record.attributes["unisphere.profile.version"], json!(1));
        assert_eq!(record.attributes["unisphere.source.adapter"], IDE_DESCRIPTOR.id);
        assert_eq!(record.attributes["unisphere.source.path"], "/synthetic/not-opened/state.vscdb");
        assert_eq!(record.attributes["unisphere.source.revision"], "synthetic-revision-one");
        assert_eq!(record.attributes["unisphere.source.format"], "sqlite_key_value");
        assert_eq!(record.attributes["unisphere.source.session.id"], "alpha");
        assert_eq!(record.attributes["gen_ai.conversation.id"], "alpha");
        assert!(!record.attributes.contains_key("unisphere.source.offset"));
    }
    assert_eq!(batch.records[2].attributes["unisphere.message.id"], "a");
    assert_eq!(batch.records[2].attributes["unisphere.cursor.request.id"], "request-a");
    assert_eq!(batch.records[1].attributes["unisphere.cursor.checkpoint.id"], "checkpoint-z");
}

#[test]
fn metadata_never_leaks_overviews_usage_objects_tools_or_message_content() {
    let mut snapshot = fixture();
    change(&mut snapshot, "bubbleId:alpha:a", |value| {
        value["toolResults"] = json!([{"content":"SENSITIVE-LEGACY-RESULT"}]);
        value["images"] = json!([{"data":"SENSITIVE-IMAGE"}]);
        value["toolFormerData"]["error"] = json!({"detail":"SENSITIVE-TOOL-ERROR"});
    });
    let batch = map(&snapshot, false);
    assert!(batch.records.iter().all(|record| record.body.is_none()));
    let serialized = serde_json::to_string(&batch.records).unwrap();
    assert!(!serialized.contains("SENSITIVE-"));
    assert!(!format!("{:?}", batch.diagnostics).contains("SENSITIVE-"));
    assert!(diagnosed(&batch, "composerData:alpha", Code::ContentOmitted));
    assert!(diagnosed(&batch, "bubbleId:alpha:a", Code::UnsupportedPart));
    assert!(diagnosed(&batch, "bubbleId:alpha:a", Code::ContentOmitted));
}

#[test]
fn opt_in_tools_keep_arguments_and_results_structured_without_aggregation() {
    let batch = map(&fixture(), true);
    let body = batch.records[2].body.as_ref().unwrap();
    assert_eq!(body["parts"][0], json!({"type":"text","content":"SENSITIVE-ANSWER"}));
    assert_eq!(body["parts"][1], json!({"type":"reasoning","content":"SENSITIVE-REASONING"}));
    assert_eq!(body["parts"][2], json!({"type":"tool_call","name":"read_file","id":"call-a",
        "arguments":{"path":"SENSITIVE-PATH","nested":[true,null,3]}}));
    assert_eq!(body["parts"][3], json!({"type":"tool_call_response","id":"call-a",
        "response":{"contents":"SENSITIVE-RESULT"}}));
    assert_eq!(batch.records[0].body.as_ref().unwrap()["unisphere.cursor.usage_data"],
        json!({"nativeCounter":{"amount":3,"unprovenScope":"SENSITIVE-NATIVE-USAGE"}}));
    for index in [2, 3] {
        let attributes = &batch.records[index].attributes;
        assert_eq!(attributes["unisphere.cursor.token_count.input_tokens"], 11);
        assert_eq!(attributes["unisphere.cursor.token_count.output_tokens"], 7);
        assert_eq!(attributes["unisphere.cursor.token_count.scope"], "native_bubble_snapshot");
        assert!(!attributes.keys().any(|key| key.starts_with("gen_ai.usage.")));
    }
    assert_eq!(batch.records[2].attributes["unisphere.cursor.model_info.model_name"], "observed-model");
    assert_eq!(batch.records[0].attributes["unisphere.cursor.model_config.model_name"], "configured-model");
    assert!(batch.records.iter().all(|record| !record.attributes.contains_key("gen_ai.response.model")));
}

#[test]
fn summary_and_simulated_bubbles_remain_control_not_ordinary_turns() {
    let mut snapshot = fixture();
    change(&mut snapshot, "bubbleId:alpha:z", |value| { value["isSimulatedMsg"] = json!(true); });
    let batch = map(&snapshot, true);
    for index in [1, 3] {
        let record = &batch.records[index];
        assert!(!record.attributes.contains_key("unisphere.message.role"));
        assert_eq!(record.body.as_ref().unwrap()["type"], "unisphere.cursor.control");
        assert!(record.body.as_ref().unwrap().get("role").is_none());
    }
    assert_eq!(batch.records[1].attributes["unisphere.cursor.isDisplayOnly"], true);
    assert_eq!(batch.records[2].attributes["unisphere.cursor.skipRendering"], true);
}

#[test]
fn identity_mismatches_and_missing_rows_never_rebind_another_conversation() {
    let mut snapshot = fixture();
    change(&mut snapshot, "bubbleId:alpha:a", |value| { value["bubbleId"] = json!("foreign"); });
    snapshot.records.retain(|row| row.key != "bubbleId:alpha:z");
    let batch = map(&snapshot, true);
    assert_eq!(keys(&batch), ["composerData:alpha", "bubbleId:alpha:s"]);
    assert!(diagnosed(&batch, "bubbleId:alpha:a", Code::InvalidField));
    assert!(diagnosed(&batch, "bubbleId:alpha:z", Code::InvalidField));
    assert!(!serde_json::to_string(&batch.records).unwrap().contains("OTHER-SESSION"));
    change(&mut snapshot, "composerData:alpha", |value| { value["composerId"] = json!("beta"); });
    let batch = map(&snapshot, true);
    assert!(batch.records.is_empty());
    assert!(diagnosed(&batch, "composerData:alpha", Code::InvalidField));
}

#[test]
fn duplicate_or_mismatched_headers_and_orphans_are_not_extra_turns() {
    let mut snapshot = fixture();
    change(&mut snapshot, "composerData:alpha", |value| {
        value["fullConversationHeadersOnly"] = json!([
            {"bubbleId":"a","type":1}, {"bubbleId":"z","type":1}, {"bubbleId":"z","type":1}
        ]);
    });
    let batch = map(&snapshot, true);
    assert_eq!(keys(&batch), ["composerData:alpha", "bubbleId:alpha:z"]);
    assert!(diagnosed(&batch, "bubbleId:alpha:a", Code::InvalidField));
    assert!(diagnosed(&batch, "composerData:alpha", Code::InvalidField));
    assert!(diagnosed(&batch, "bubbleId:alpha:s", Code::UnsupportedRecord));
}

#[test]
fn complete_revision_selection_and_empty_deletion_do_not_use_previous_state() {
    let mut snapshot = fixture();
    snapshot.source.session_id = None;
    let all = map(&snapshot, true);
    assert_eq!(keys(&all), ["composerData:alpha", "bubbleId:alpha:z", "bubbleId:alpha:a", "bubbleId:alpha:s", "composerData:beta", "bubbleId:beta:z"]);
    snapshot.source.session_id = Some("beta".into());
    assert_eq!(keys(&map(&snapshot, true)), ["composerData:beta", "bubbleId:beta:z"]);
    snapshot.revision = "synthetic-revision-two".into();
    snapshot.records.clear();
    let deleted = map(&snapshot, true);
    assert!(deleted.records.is_empty());
    assert!(diagnosed(&deleted, "composerData:beta", Code::UnsupportedRecord));
    snapshot.source.session_id = None;
    assert_eq!(map(&snapshot, true), MappedSnapshot::default());
}

#[test]
fn native_timestamp_and_counter_errors_do_not_invent_fallbacks() {
    let mut snapshot = fixture();
    change(&mut snapshot, "composerData:alpha", |value| { value["createdAt"] = json!(u64::MAX); });
    change(&mut snapshot, "bubbleId:alpha:a", |value| {
        value["createdAt"] = json!(1234);
        value["tokenCount"] = json!({"inputTokens":-1,"outputTokens":1.5,"unknownTotal":100});
    });
    change(&mut snapshot, "bubbleId:alpha:z", |value| {
        value["createdAt"] = json!("1969-12-31T23:59:59Z");
        value.as_object_mut().unwrap().remove("tokenCount");
    });
    let batch = map(&snapshot, false);
    for index in [0, 1, 2] { assert!(batch.records[index].timestamp_unix_nano.is_none()); }
    for index in [1, 2] { assert!(!batch.records[index].attributes.contains_key("unisphere.cursor.token_count.scope")); }
    assert!(diagnosed(&batch, "composerData:alpha", Code::InvalidTimestamp));
    assert!(diagnosed(&batch, "bubbleId:alpha:a", Code::InvalidField));
    assert!(diagnosed(&batch, "bubbleId:alpha:a", Code::UnsupportedPart));
}

#[test]
fn malformed_rows_report_keys_without_parser_payload_and_do_not_abort_siblings() {
    let mut snapshot = fixture();
    snapshot.records.iter_mut().find(|row| row.key == "bubbleId:alpha:a").unwrap().bytes = b"{\"SENSITIVE-TRUNCATED".to_vec();
    let batch = map(&snapshot, true);
    assert_eq!(keys(&batch), ["composerData:alpha", "bubbleId:alpha:z", "bubbleId:alpha:s"]);
    assert!(diagnosed(&batch, "bubbleId:alpha:a", Code::InvalidField));
    assert!(!format!("{:?}", batch.diagnostics).contains("SENSITIVE-"));
}

#[test]
fn unknown_bubble_kinds_and_legacy_composers_have_explicit_disposition() {
    let mut snapshot = fixture();
    change(&mut snapshot, "composerData:alpha", |value| { value["fullConversationHeadersOnly"][1]["type"] = json!(99); });
    change(&mut snapshot, "bubbleId:alpha:a", |value| { value["type"] = json!(99); });
    let batch = map(&snapshot, true);
    assert!(batch.records[2].body.is_none());
    assert!(!batch.records[2].attributes.contains_key("unisphere.message.role"));
    assert!(diagnosed(&batch, "bubbleId:alpha:a", Code::UnsupportedRecord));
    change(&mut snapshot, "composerData:alpha", |value| { value["_v"] = json!(1); });
    let batch = map(&snapshot, false);
    assert!(batch.records.is_empty());
    assert!(diagnosed(&batch, "composerData:alpha", Code::UnsupportedRecord));
}

#[test]
fn non_native_formats_duplicate_keys_and_invalid_source_are_rejected() {
    let mut snapshot = fixture();
    for format in [SnapshotFormat::JsonDocument, SnapshotFormat::JsonJournal, SnapshotFormat::SqliteKeyValue { table:"blobs".into() }] {
        snapshot.source.format = format;
        let error = CursorIdeAdapter.map_snapshot(&snapshot, MappingOptions::default()).unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::Unsupported);
        assert_eq!(error.offset(), None);
    }
    let mut snapshot = fixture();
    snapshot.records.push(snapshot.records[0].clone());
    assert_eq!(CursorIdeAdapter.map_snapshot(&snapshot, MappingOptions::default()).unwrap_err().kind(), PipelineErrorKind::InvalidData);
    snapshot.source.path = "relative.db".into();
    assert_eq!(CursorIdeAdapter.map_snapshot(&snapshot, MappingOptions::default()).unwrap_err().kind(), PipelineErrorKind::InvalidInput);
}

#[test]
fn raw_argument_strings_and_idless_tool_errors_are_not_repaired_or_linked() {
    let mut snapshot = fixture();
    change(&mut snapshot, "bubbleId:alpha:a", |value| {
        value["toolFormerData"] = json!({
            "name":"pending_tool", "rawArgs":"{\"partial\":",
            "error":{"detail":"SENSITIVE-TOOL-ERROR"}
        });
    });
    let batch = map(&snapshot, true);
    let parts = &batch.records[2].body.as_ref().unwrap()["parts"];
    assert_eq!(parts[2], json!({
        "type":"tool_call", "name":"pending_tool", "arguments":"{\"partial\":"
    }));
    assert_eq!(parts[3], json!({
        "type":"unisphere.cursor.tool_error", "error":{"detail":"SENSITIVE-TOOL-ERROR"}
    }));
}
