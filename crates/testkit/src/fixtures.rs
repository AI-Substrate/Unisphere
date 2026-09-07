//! Shared explicit documents and expected categories; no live user data.
use unisphere_core::{FailureKind, ReadFailure};

pub const EMPTY: &[u8] = include_bytes!("../fixtures/config/empty.json");
pub const ROOTS: &[u8] = include_bytes!("../fixtures/config/roots.json");
pub const WRONG_TYPE: &[u8] = include_bytes!("../fixtures/config/wrong-type.json");
pub const BLANK_ROOT: &[u8] = include_bytes!("../fixtures/config/blank-root.json");
pub const MALFORMED: &[u8] = include_bytes!("../fixtures/config/malformed.json");
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
];
pub const READ_FAILURES: &[(ReadFailure, FailureKind)] = &[
    (ReadFailure::NotFound, FailureKind::ConfigurationRead),
    (
        ReadFailure::PermissionDenied,
        FailureKind::ConfigurationRead,
    ),
];

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
}
