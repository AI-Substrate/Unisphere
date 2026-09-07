//! Shared explicit documents and expected categories; no live user data.
use unisphere_core::{FailureKind, MAX_CONFIG_BYTES, ReadFailure};

pub const EMPTY: &[u8] = include_bytes!("../fixtures/config/empty.json");
pub const ROOTS: &[u8] = include_bytes!("../fixtures/config/roots.json");
pub const WRONG_TYPE: &[u8] = include_bytes!("../fixtures/config/wrong-type.json");
pub const BLANK_ROOT: &[u8] = include_bytes!("../fixtures/config/blank-root.json");
pub const MALFORMED: &[u8] = include_bytes!("../fixtures/config/malformed.json");
pub const NON_OBJECT: &[u8] = b"[]";
pub const UNKNOWN_KEY: &[u8] = br#"{"unknown":"SENSITIVE-CONFIG-MARKER"}"#;
pub const DUPLICATE_KEY: &[u8] = br#"{"source_roots":[],"source_roots":["duplicate"]}"#;
pub const NON_STRING_ROOT: &[u8] = br#"{"source_roots":[42]}"#;
pub const INVALID_UTF8: &[u8] = b"{\"source_roots\":[\"SENSITIVE-CONFIG-MARKER\xff\"]}";
pub const DEFAULT_ROOTS: &[&str] = &["default-root"];
pub const DOCUMENT_ROOTS: &[&str] = &[
    " relative-root ",
    "~/literal",
    "relative-root",
    "relative-root",
];
pub const OVERRIDE_ROOTS: &[&str] = &["override-root"];
pub const INVALID_DOCUMENTS: &[(&str, &[u8], FailureKind)] = &[
    ("wrong-type", WRONG_TYPE, FailureKind::InvalidConfiguration),
    ("blank-root", BLANK_ROOT, FailureKind::InvalidConfiguration),
    ("malformed", MALFORMED, FailureKind::InvalidConfiguration),
    ("non-object", NON_OBJECT, FailureKind::InvalidConfiguration),
    (
        "unknown-key",
        UNKNOWN_KEY,
        FailureKind::InvalidConfiguration,
    ),
    (
        "duplicate-key",
        DUPLICATE_KEY,
        FailureKind::InvalidConfiguration,
    ),
    (
        "non-string-root",
        NON_STRING_ROOT,
        FailureKind::InvalidConfiguration,
    ),
    (
        "invalid-utf8",
        INVALID_UTF8,
        FailureKind::InvalidConfiguration,
    ),
];
pub const READ_FAILURES: &[(ReadFailure, FailureKind)] = &[
    (ReadFailure::NotFound, FailureKind::ConfigurationRead),
    (
        ReadFailure::PermissionDenied,
        FailureKind::ConfigurationRead,
    ),
];

/// A valid empty JSON object padded one byte beyond the configured size limit.
/// Generate it only when needed, rather than embedding a megabyte fixture.
pub fn oversized_document() -> Vec<u8> {
    let mut bytes = vec![b' '; MAX_CONFIG_BYTES + 1];
    bytes[0] = b'{';
    bytes[1] = b'}';
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn valid_documents_express_empty_and_uninterpreted_roots() {
        assert_eq!(
            serde_json::from_slice::<Value>(EMPTY).unwrap(),
            serde_json::json!({})
        );
        let document: Value = serde_json::from_slice(ROOTS).unwrap();
        assert_eq!(document["source_roots"], serde_json::json!(DOCUMENT_ROOTS));
        assert_ne!(DEFAULT_ROOTS, OVERRIDE_ROOTS);
    }

    #[test]
    fn invalid_documents_and_read_failures_have_explicit_expected_categories() {
        assert!(serde_json::from_slice::<Value>(MALFORMED).is_err());
        assert!(serde_json::from_slice::<Value>(INVALID_UTF8).is_err());
        assert!(
            serde_json::from_slice::<Value>(NON_OBJECT)
                .unwrap()
                .is_array()
        );
        assert!(
            serde_json::from_slice::<Value>(UNKNOWN_KEY)
                .unwrap()
                .get("unknown")
                .is_some()
        );
        assert!(
            serde_json::from_slice::<Value>(NON_STRING_ROOT).unwrap()["source_roots"][0]
                .is_number()
        );
        assert!(serde_json::from_slice::<Value>(WRONG_TYPE).unwrap()["source_roots"].is_number());
        assert!(
            serde_json::from_slice::<Value>(BLANK_ROOT).unwrap()["source_roots"][0]
                .as_str()
                .unwrap()
                .trim()
                .is_empty()
        );
        assert!(
            INVALID_DOCUMENTS
                .iter()
                .all(|(_, _, kind)| *kind == FailureKind::InvalidConfiguration)
        );
        assert!(
            READ_FAILURES
                .iter()
                .all(|(_, kind)| *kind == FailureKind::ConfigurationRead)
        );
    }

    #[test]
    fn duplicate_fixture_retains_both_keys_in_the_input_stream() {
        // A Value collapses duplicate keys, so inspect the known fixture's
        // two key occurrences before checking that its JSON is otherwise valid.
        let text = std::str::from_utf8(DUPLICATE_KEY).unwrap();
        assert_eq!(text.matches("\"source_roots\":").count(), 2);
        assert!(
            serde_json::from_slice::<Value>(DUPLICATE_KEY)
                .unwrap()
                .is_object()
        );
    }

    #[test]
    fn oversized_fixture_is_valid_json_but_exceeds_the_limit() {
        let bytes = oversized_document();
        assert_eq!(bytes.len(), MAX_CONFIG_BYTES + 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            serde_json::json!({})
        );
    }
}
