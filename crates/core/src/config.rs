use std::path::PathBuf;

use serde::Serialize;

/// The maximum accepted explicit configuration document size, in bytes.
pub const MAX_CONFIG_BYTES: usize = 1_048_576;

/// Source-root strings are configuration values, not paths to discover or read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Configuration {
    pub source_roots: Vec<String>,
}

/// `None` retains lower-priority values; `Some([])` explicitly clears them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigOverrides {
    pub source_roots: Option<Vec<String>>,
}

/// Only a caller-selected input is inspected. No ambient source is implied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ConfigSource {
    #[default]
    Defaults,
    Inline(Vec<u8>),
    File(PathBuf),
}

/// Input bytes are deliberately not serializable into diagnostic envelopes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InspectionRequest {
    pub source: ConfigSource,
    pub overrides: ConfigOverrides,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectionReport {
    pub configuration: Configuration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_and_explicit_empty_overrides_remain_distinct() {
        let omitted = ConfigOverrides::default();
        let cleared = ConfigOverrides {
            source_roots: Some(Vec::new()),
        };
        assert_ne!(omitted, cleared);
        assert!(Configuration::default().source_roots.is_empty());
        assert_eq!(InspectionRequest::default().source, ConfigSource::Defaults);
    }
}
