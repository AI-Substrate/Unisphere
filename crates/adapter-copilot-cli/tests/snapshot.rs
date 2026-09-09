use serde_json::{Value, json};
use unisphere_adapter_copilot_cli::{CopilotCliAdapterSnapshot, SNAPSHOT_DESCRIPTOR};
use unisphere_core::{
    MappedSnapshot, MappingDiagnosticCode as Code, MappingOptions, NativeSnapshot,
    PipelineErrorKind, SnapshotAdapter, SnapshotFormat, SnapshotRecord, SnapshotRef,
};

const LEGACY: &[u8] = include_bytes!("fixtures/legacy.json");

fn snapshot(bytes: Vec<u8>) -> NativeSnapshot {
    NativeSnapshot {
        source: SnapshotRef {
            path: "/synthetic/copilot/legacy.json".into(),
            format: SnapshotFormat::JsonDocument,
            session_id: None,
        },
        revision: "synthetic-revision-a".into(),
        records: vec![SnapshotRecord {
            key: "document".into(),
            bytes,
        }],
    }
}

fn document(value: Value) -> NativeSnapshot {
    snapshot(serde_json::to_vec(&value).unwrap())
}

fn map(snapshot: &NativeSnapshot, include_content: bool) -> MappedSnapshot {
    CopilotCliAdapterSnapshot
        .map_snapshot(snapshot, MappingOptions { include_content })
        .unwrap()
}

fn has(batch: &MappedSnapshot, key: &str, code: Code) -> bool {
    batch
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.key == key && diagnostic.code == code)
}

#[test]
fn metadata_preserves_revision_qualified_structural_identity_without_content_or_offsets() {
    let source = snapshot(LEGACY.to_vec());
    let batch = map(&source, false);
    assert_eq!(batch, map(&source, false));
    let keys: Vec<_> = batch
        .records
        .iter()
        .map(|record| record.attributes["unisphere.source.key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "document",
            "document#/chatMessages/0",
            "document#/chatMessages/1",
            "document#/chatMessages/2",
            "document#/timeline/0",
            "document#/timeline/1",
            "document#/timeline/2",
            "document#/timeline/3",
            "document#/timeline/4",
        ]
    );
    for record in &batch.records {
        assert_eq!(record.event_name, "unisphere.session.record");
        assert_eq!(record.attributes["unisphere.profile.version"], json!(1));
        assert_eq!(
            record.attributes["unisphere.source.adapter"],
            SNAPSHOT_DESCRIPTOR.id
        );
        assert_eq!(
            record.attributes["unisphere.source.path"],
            "/synthetic/copilot/legacy.json"
        );
        assert_eq!(
            record.attributes["unisphere.source.revision"],
            "synthetic-revision-a"
        );
        assert_eq!(
            record.attributes["unisphere.source.format"],
            "json_document"
        );
        assert_eq!(
            record.attributes["unisphere.source.session.id"],
            "legacy-session"
        );
        assert_eq!(
            record.attributes["gen_ai.conversation.id"],
            "legacy-session"
        );
        assert!(record.attributes["unisphere.source.kind"].is_string());
        assert!(!record.attributes.contains_key("unisphere.source.offset"));
        assert!(record.body.is_none());
    }
    let serialized =
        serde_json::to_string(&json!({"records":batch.records,"diagnostics":batch.diagnostics}))
            .unwrap();
    assert!(!serialized.contains("SENSITIVE-"));
}

#[test]
fn chat_and_timeline_are_distinct_views_not_deduplicated_turns_or_usage() {
    let batch = map(&snapshot(LEGACY.to_vec()), true);
    assert_eq!(
        batch.records[1].attributes["unisphere.copilot.view"],
        "chatMessages"
    );
    assert_eq!(
        batch.records[5].attributes["unisphere.copilot.view"],
        "timeline"
    );
    assert_eq!(
        batch.records[1].body.as_ref().unwrap()["parts"][0]["content"],
        "SENSITIVE-legacy-user"
    );
    assert_eq!(
        batch.records[5].body.as_ref().unwrap()["parts"][0]["content"],
        "SENSITIVE-legacy-user"
    );
    assert_eq!(
        batch.records[6].attributes["unisphere.source.kind"],
        "copilot"
    );
    assert_eq!(
        batch.records[6].attributes["unisphere.message.role"],
        "assistant"
    );
    assert_eq!(batch.records[4].attributes["unisphere.source.kind"], "info");
    assert!(
        !batch.records[4]
            .attributes
            .contains_key("unisphere.message.role")
    );
    assert_eq!(batch.records[4].body.as_ref().unwrap()["type"], "info");
    for record in &batch.records {
        assert!(
            !record
                .attributes
                .keys()
                .any(|key| key.contains("usage") || key.contains("model"))
        );
    }
    assert!(has(&batch, "document#/timeline/1", Code::UnsupportedPart));
    assert!(
        !serde_json::to_string(&batch.records)
            .unwrap()
            .contains("SENSITIVE-mention")
    );
}

#[test]
fn tool_arguments_keep_the_native_string_or_object_and_results_stay_structured() {
    let batch = map(&snapshot(LEGACY.to_vec()), true);
    assert_eq!(
        batch.records[2].body.as_ref().unwrap()["parts"][1],
        json!({
            "type":"tool_call","id":"call-1","name":"read_file",
            "arguments":"{\"path\":\"/SENSITIVE-input\"}"
        })
    );
    assert_eq!(
        batch.records[3].body.as_ref().unwrap()["parts"][0],
        json!({
            "type":"tool_call_response","id":"call-1","response":"SENSITIVE-legacy-result"
        })
    );
    assert_eq!(
        batch.records[7].body.as_ref().unwrap()["parts"][0]["arguments"],
        json!({"path":"/SENSITIVE-input"})
    );
    let response = &batch.records[8].body.as_ref().unwrap()["parts"][0];
    assert_eq!(response["id"], "call-1");
    assert_eq!(
        response["response"],
        json!({"content":"SENSITIVE-legacy-result","nested":{"count":2,"ok":true}})
    );
    assert!(response.get("unisphere.is_error").is_none());
    let source = document(json!({"sessionId":"s","chatMessages":[{
        "role":"assistant","content":"","tool_calls":[{"id":"c","type":"function","function":{"name":"f","arguments":"not JSON {"}}]
    }],"timeline":[]}));
    let result = map(&source, true);
    assert_eq!(
        result.records[1].body.as_ref().unwrap()["parts"][1]["arguments"],
        "not JSON {"
    );
    assert!(!has(
        &result,
        "document#/chatMessages/0",
        Code::InvalidField
    ));
}

#[test]
fn session_start_time_never_becomes_a_message_occurrence_time() {
    let source = document(json!({"sessionId":"s","startTime":"1970-01-01T00:00:10Z",
        "chatMessages":[{"role":"user","content":"a"}],
        "timeline":[
            {"id":"a","type":"user","text":"a"},
            {"id":"b","type":"copilot","text":"b","timestamp":"1970-01-01T01:00:11.123456789+01:00"},
            {"id":"c","type":"info","timestamp":"1969-12-31T23:59:59Z"},
            {"id":"d","type":"info","timestamp":"invalid"}
        ]
    }));
    let batch = map(&source, true);
    assert_eq!(batch.records[0].timestamp_unix_nano, Some(10_000_000_000));
    assert_eq!(batch.records[1].timestamp_unix_nano, None);
    assert_eq!(batch.records[2].timestamp_unix_nano, None);
    assert_eq!(batch.records[3].timestamp_unix_nano, Some(11_123_456_789));
    assert_eq!(batch.records[4].timestamp_unix_nano, None);
    assert_eq!(batch.records[5].timestamp_unix_nano, None);
    assert!(has(&batch, "document#/timeline/2", Code::InvalidTimestamp));
    assert!(has(&batch, "document#/timeline/3", Code::InvalidTimestamp));
}

#[test]
fn empty_replacement_does_not_retain_messages_from_the_previous_revision() {
    let old = snapshot(LEGACY.to_vec());
    let before = map(&old, true);
    let mut replacement =
        document(json!({"sessionId":"legacy-session","chatMessages":[],"timeline":[]}));
    replacement.revision = "synthetic-revision-b".into();
    let after = map(&replacement, true);
    assert_eq!(after.records.len(), 1);
    assert_eq!(
        after.records[0].attributes["unisphere.source.key"],
        "document"
    );
    assert_eq!(
        after.records[0].attributes["unisphere.source.revision"],
        "synthetic-revision-b"
    );
    assert!(after.records[0].body.is_none());
    assert_eq!(before, map(&old, true));
    assert_eq!(after, map(&replacement, true));
}

#[test]
fn unknown_variants_and_invalid_siblings_are_key_diagnosed_without_content_fallback() {
    let source = document(json!({"sessionId":"s","chatMessages":[
        {"role":"future","content":"SENSITIVE-unknown"},
        17,
        {"role":"assistant","content":"SENSITIVE-valid","tool_calls":[
            {"id":"bad","type":"future","function":{"arguments":"SENSITIVE-hidden"}},
            {"id":"good","type":"function","function":{"name":"f","arguments":"{}"}}
        ]}
    ],"timeline":[
        {"type":"future","text":"SENSITIVE-future"},
        {"type":"user","text":"SENSITIVE-timeline-valid"},
        {"type":"tool_call_requested","arguments":{},"text":"SENSITIVE-tool-context"}
    ]}));
    let batch = map(&source, true);
    assert!(has(
        &batch,
        "document#/chatMessages/0",
        Code::UnsupportedRecord
    ));
    assert!(has(&batch, "document#/chatMessages/1", Code::InvalidField));
    assert!(has(
        &batch,
        "document#/chatMessages/2",
        Code::UnsupportedPart
    ));
    assert!(has(&batch, "document#/timeline/0", Code::UnsupportedRecord));
    assert!(has(&batch, "document#/timeline/2", Code::InvalidField));
    assert!(batch.records[1].body.is_none());
    assert!(batch.records[2].body.is_none());
    assert_eq!(
        batch.records[3].body.as_ref().unwrap()["parts"][1]["id"],
        "good"
    );
    assert_eq!(
        batch.records[5].body.as_ref().unwrap()["parts"][0]["content"],
        "SENSITIVE-timeline-valid"
    );
    assert_eq!(
        batch.records[6].body.as_ref().unwrap()["parts"],
        json!([{"type":"text","content":"SENSITIVE-tool-context"}])
    );
    assert!(
        !serde_json::to_string(&batch.records)
            .unwrap()
            .contains("SENSITIVE-unknown")
    );
    assert!(
        !serde_json::to_string(&batch.diagnostics)
            .unwrap()
            .contains("SENSITIVE")
    );
    let malformed = map(
        &document(
            json!({"sessionId":"s","chatMessages":{},"timeline":[{"type":"info","text":"ok"}]}),
        ),
        true,
    );
    assert!(has(
        &malformed,
        "document#/chatMessages",
        Code::InvalidField
    ));
    assert_eq!(
        malformed.records[1].attributes["unisphere.source.key"],
        "document#/timeline/0"
    );
}

#[test]
fn supplied_session_selector_must_match_the_observed_document_identity() {
    let mut source = snapshot(LEGACY.to_vec());
    source.source.session_id = Some("legacy-session".into());
    let selected = map(&source, false);
    source.source.session_id = None;
    assert_eq!(selected, map(&source, false));
    source.source.session_id = Some("other-session".into());
    let error = CopilotCliAdapterSnapshot
        .map_snapshot(&source, MappingOptions::default())
        .unwrap_err();
    assert_eq!(error.kind(), PipelineErrorKind::InvalidInput);
    assert_eq!(error.offset(), None);
    let mut no_identity = document(json!({"chatMessages":[],"timeline":[]}));
    no_identity.source.session_id = Some("unverifiable".into());
    assert_eq!(
        CopilotCliAdapterSnapshot
            .map_snapshot(&no_identity, MappingOptions::default())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidInput
    );
}

#[test]
fn malformed_documents_and_wrong_representations_fail_without_fake_offsets_or_payloads() {
    for bytes in [
        b"{\"SENSITIVE-parser\":}".to_vec(),
        vec![0xff],
        b"[]".to_vec(),
    ] {
        let error = CopilotCliAdapterSnapshot
            .map_snapshot(&snapshot(bytes), MappingOptions::default())
            .unwrap_err();
        assert_eq!(error.kind(), PipelineErrorKind::InvalidData);
        assert_eq!(error.offset(), None);
        assert!(!format!("{error:?} {error}").contains("SENSITIVE"));
    }
    let mut source = snapshot(LEGACY.to_vec());
    source.source.format = SnapshotFormat::JsonJournal;
    assert_eq!(
        CopilotCliAdapterSnapshot
            .map_snapshot(&source, MappingOptions::default())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::Unsupported
    );
    source.source.format = SnapshotFormat::JsonDocument;
    source.records[0].key = "journal:0".into();
    assert_eq!(
        CopilotCliAdapterSnapshot
            .map_snapshot(&source, MappingOptions::default())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidData
    );
    source.records[0].key = "document".into();
    source.revision.clear();
    assert_eq!(
        CopilotCliAdapterSnapshot
            .map_snapshot(&source, MappingOptions::default())
            .unwrap_err()
            .kind(),
        PipelineErrorKind::InvalidData
    );
}
