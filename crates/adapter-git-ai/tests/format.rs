use serde_json::json;
use unisphere_adapter_git_ai::GitAiAdapter;
use unisphere_core::{
    GitNoteAdapter, GitNoteRef, GitNotesError, GitNotesLimits, LoadedGitNote, MappingOptions,
    PipelineErrorKind,
};

fn note(bytes: &[u8]) -> LoadedGitNote {
    LoadedGitNote {
        source: GitNoteRef {
            repository: "/synthetic".into(),
            repository_id: "/synthetic/.git".into(),
            notes_ref: "refs/notes/ai".into(),
            notes_tip: "a".repeat(40),
            target_commit: "b".repeat(40),
            note_blob: "c".repeat(40),
        },
        bytes: bytes.into(),
    }
}
#[test]
fn mixed_keys_preserve_ranges_unresolved_evidence_and_content_policy() {
    let source = note(include_bytes!("../fixtures/mixed.notes"));
    let metadata = GitAiAdapter
        .map_note(
            &source,
            MappingOptions::default(),
            GitNotesLimits::default(),
        )
        .unwrap();
    assert!(
        !serde_json::to_string(&metadata)
            .unwrap()
            .contains("SENSITIVE")
    );
    let attribution: Vec<_> = metadata
        .iter()
        .filter(|r| r.event_name == "unisphere.git_ai.attribution")
        .collect();
    let ranges: Vec<_> = attribution
        .iter()
        .map(|r| {
            (
                r.attributes["unisphere.git_ai.line.start"]
                    .as_u64()
                    .unwrap(),
                r.attributes["unisphere.git_ai.line.end"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        ranges,
        [(1, 3), (7, 7), (4, 6), (8, 8), (9, 9), (10, 10), (2, 3)]
    );
    let unresolved = attribution
        .iter()
        .find(|r| r.attributes["unisphere.git_ai.identity_resolution"] == "unresolved")
        .unwrap();
    assert_eq!(
        unresolved.attributes["unisphere.git_ai.attestation.key"],
        "s_aaaaaaaaaaaaaa::t_bbbbbbbbbbbbbb"
    );
    assert!(
        !unresolved
            .attributes
            .contains_key("unisphere.git_ai.agent.id")
    );
    assert!(
        metadata
            .iter()
            .any(|r| r.attributes.get("unisphere.git_ai.agent.id")
                == Some(&json!("synthetic-unreferenced")))
    );
    assert!(
        attribution
            .iter()
            .any(|r| r.attributes["unisphere.git_ai.attestation.key"] == "abc1234")
    );
    let legacy = metadata
        .iter()
        .find(|r| {
            r.event_name == "unisphere.git_ai.identity"
                && r.attributes["unisphere.git_ai.identity.key"] == "0123456789abcdef"
        })
        .unwrap();
    assert_eq!(
        legacy.attributes["unisphere.git_ai.total_additions"],
        json!(null)
    );
    assert_eq!(
        legacy.attributes["unisphere.git_ai.agent.model"],
        json!(null)
    );
    assert!(
        !legacy
            .attributes
            .contains_key("unisphere.git_ai.total_deletions")
    );
    for record in &metadata {
        assert!(record.timestamp_unix_nano.is_none());
        assert!(!record.attributes.contains_key("unisphere.source.offset"));
        assert_eq!(record.attributes["unisphere.git.note.blob"], "c".repeat(40));
        assert_eq!(record.attributes["unisphere.profile.version"], 1);
    }
    let content = GitAiAdapter
        .map_note(
            &source,
            MappingOptions {
                include_content: true,
            },
            GitNotesLimits::default(),
        )
        .unwrap();
    assert!(
        serde_json::to_string(&content)
            .unwrap()
            .contains("SENSITIVE-message")
    );
    assert!(
        content
            .iter()
            .filter(|r| r.event_name == "unisphere.git_ai.attribution")
            .all(|r| !r.attributes.contains_key("unisphere.git_ai.messages"))
    );
}
#[test]
fn malformed_or_unsupported_notes_do_not_become_partial_success() {
    let metadata = r#"{"schema_version":"authorship/3.0.0","prompts":{}}"#;
    for prefix in [
        "source.rs\n  abc1234 0",
        "source.rs\n  abc1234 4-2",
        "source.rs\n  abc1234 1-4,3",
        "file with spaces\n  abc1234 1",
        "source.rs",
    ] {
        let source = note(format!("{prefix}\n---\n{metadata}").as_bytes());
        assert_eq!(
            GitAiAdapter.map_note(
                &source,
                MappingOptions::default(),
                GitNotesLimits::default()
            ),
            Err(GitNotesError::InvalidData)
        );
    }
    for (json, expected) in [
        (
            r#"{"schema_version":"authorship/4.0.0"}"#,
            GitNotesError::UnsupportedFormat,
        ),
        (
            r#"{"schema_version":"authorship/3.0.0","prompts":{},"prompts":{}}"#,
            GitNotesError::InvalidData,
        ),
        (
            r#"{"schema_version":"authorship/3.0.0","unknown_map":{}}"#,
            GitNotesError::UnsupportedFormat,
        ),
    ] {
        assert_eq!(
            GitAiAdapter.map_note(
                &note(format!("---\n{json}").as_bytes()),
                MappingOptions::default(),
                GitNotesLimits::default()
            ),
            Err(expected)
        );
    }
}
#[test]
fn repeated_long_paths_hit_output_bound_before_unbounded_amplification() {
    let ranges = (1..=600)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let source = note(
        format!(
            "{}\n  abc1234 {ranges}\n---\n{{\"schema_version\":\"authorship/3.0.0\"}}",
            "x".repeat(65_536)
        )
        .as_bytes(),
    );
    let error = GitAiAdapter
        .map_note(
            &source,
            MappingOptions::default(),
            GitNotesLimits::default(),
        )
        .unwrap_err();
    assert!(
        matches!(error, GitNotesError::Output(e) if e.kind() == PipelineErrorKind::OutputLimit)
    );
}
