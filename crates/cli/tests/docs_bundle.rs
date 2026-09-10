#[path = "../src/docs.rs"]
mod docs;

use std::collections::BTreeSet;

const EXAMPLE_CASES: &str =
    include_str!("../../testkit/fixtures/query-docs/example-cases.json");

#[test]
fn registry_contains_complete_linked_offline_topics() {
    let expected = BTreeSet::from([
        "agents",
        "extract-context",
        "filter-time-and-text",
        "find-sessions",
        "git-ai",
        "inspect-conversations",
        "output-and-schema",
        "privacy-and-coverage",
        "sdk",
        "start",
        "tool-analysis",
        "troubleshooting",
    ]);
    let actual = docs::topics()
        .iter()
        .map(|topic| topic.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);

    for topic in docs::topics() {
        assert!(!topic.title.trim().is_empty(), "{} title", topic.id);
        assert!(!topic.summary.trim().is_empty(), "{} summary", topic.id);
        assert_eq!(docs::get(topic.id), Some(topic));
        for related in topic.related {
            assert_ne!(*related, topic.id, "{} links to itself", topic.id);
            assert!(docs::get(related).is_some(), "missing related topic {related}");
        }
    }
    assert!(docs::get("not-a-topic").is_none());
}

#[test]
fn seven_recipes_and_case_manifest_have_actionable_semantics() {
    let manifest: serde_json::Value =
        serde_json::from_str(EXAMPLE_CASES).expect("valid example case manifest");
    assert_eq!(manifest["schema_version"], 1);
    let cases = manifest["cases"].as_array().expect("cases array");
    assert_eq!(cases.len(), 7);
    let mut ids = BTreeSet::new();
    for case in cases {
        assert!(ids.insert(case["id"].as_str().expect("case id")));
        assert!(case["question"].as_str().is_some_and(|v| !v.is_empty()));
        assert!(manifest["fixtures"].get(case["fixture"].as_str().unwrap()).is_some());
        let argv = case["argv"].as_array().expect("argv array");
        assert_eq!(argv.first().and_then(|v| v.as_str()), Some("unisphere"));
        assert!(case["bindings"].is_object(), "named input bindings");
        assert!(
            case["expect"]["semantic_fields"]
                .as_array()
                .is_some_and(|fields| !fields.is_empty())
        );
        assert!(case["expect"]["exit"].is_number());
        assert!(case["expect"]["privacy"].is_object());
        assert_eq!(case["expect"]["action"]["automatic"], false);
    }

    let context_case = cases
        .iter()
        .find(|case| case["id"] == "extract-error-context")
        .expect("context extraction case");
    let context_fields = context_case["expect"]["semantic_fields"]
        .as_array()
        .expect("context semantic fields");
    assert!(context_fields.iter().any(|field| field == "rows[].is_context"));
    assert!(!context_fields.iter().any(|field| field == "rows[].context_kind"));
    assert!(context_case["argv"]
        .as_array()
        .expect("context argv")
        .iter()
        .all(|arg| arg != "--is-context"));
}

