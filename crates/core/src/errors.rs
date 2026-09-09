use std::{error::Error, fmt};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadFailure {
    NotFound,
    PermissionDenied,
    TooLarge,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    InvalidConfiguration,
    ConfigurationRead,
    InvalidArguments,
}

/// Safe structural context: supplied file paths or known field names, never
/// raw document values or third-party parser diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Location {
    pub path: Option<String>,
    pub field: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// A typed failure whose diagnostic copy cannot be replaced by input data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    kind: FailureKind,
    location: Option<Location>,
    read_failure: Option<ReadFailure>,
}

impl Failure {
    pub fn invalid_configuration(location: Option<Location>) -> Self {
        Self {
            kind: FailureKind::InvalidConfiguration,
            location,
            read_failure: None,
        }
    }

    pub fn configuration_read(kind: ReadFailure, location: Option<Location>) -> Self {
        Self {
            kind: FailureKind::ConfigurationRead,
            location,
            read_failure: Some(kind),
        }
    }

    pub fn invalid_arguments(location: Option<Location>) -> Self {
        Self {
            kind: FailureKind::InvalidArguments,
            location,
            read_failure: None,
        }
    }

    pub fn kind(&self) -> FailureKind {
        self.kind
    }
    pub fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }
    pub fn read_failure(&self) -> Option<ReadFailure> {
        self.read_failure
    }

    pub fn code(&self) -> &'static str {
        match self.kind {
            FailureKind::InvalidConfiguration => "UNI-CONFIG-INVALID",
            FailureKind::ConfigurationRead => "UNI-CONFIG-READ",
            FailureKind::InvalidArguments => "UNI-ARGS-INVALID",
        }
    }

    pub fn message(&self) -> &'static str {
        match self.kind {
            FailureKind::InvalidConfiguration => "The explicit configuration is invalid.",
            FailureKind::ConfigurationRead => match self.read_failure {
                Some(ReadFailure::NotFound) => "The explicit configuration file was not found.",
                Some(ReadFailure::PermissionDenied) => {
                    "The explicit configuration file is not readable."
                }
                Some(ReadFailure::TooLarge) => {
                    "The explicit configuration file exceeds the size limit."
                }
                _ => "The explicit configuration file could not be read.",
            },
            FailureKind::InvalidArguments => "The command arguments are invalid.",
        }
    }

    pub fn fix(&self) -> &'static str {
        match self.kind {
            FailureKind::InvalidConfiguration => {
                "Supply a JSON object with only source_roots, an array of non-blank strings; use an absolute SDK file path and at most 1048576 bytes."
            }
            FailureKind::ConfigurationRead => match self.read_failure {
                Some(ReadFailure::NotFound) => "Choose an existing configuration file explicitly.",
                Some(ReadFailure::PermissionDenied) => {
                    "Choose a readable configuration file or correct its permissions."
                }
                Some(ReadFailure::TooLarge) => {
                    "Reduce the configuration document to at most 1048576 bytes."
                }
                _ => "Check the explicitly supplied file path and its readability.",
            },
            FailureKind::InvalidArguments => {
                "Use --help for accepted arguments; do not combine conflicting options."
            }
        }
    }

    pub fn retryable(&self) -> bool {
        false
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} {}", self.code(), self.message(), self.fix())
    }
}

impl Error for Failure {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_constructors_preserve_typed_categories_and_location() {
        let location = Location {
            field: Some("source_roots".into()),
            ..Location::default()
        };
        let invalid = Failure::invalid_configuration(Some(location.clone()));
        assert_eq!(invalid.kind(), FailureKind::InvalidConfiguration);
        assert_eq!(invalid.location(), Some(&location));
        assert_eq!(invalid.read_failure(), None);
        assert_eq!(invalid.code(), "UNI-CONFIG-INVALID");
        let args = Failure::invalid_arguments(None);
        assert_eq!(args.kind(), FailureKind::InvalidArguments);
        assert_eq!(args.code(), "UNI-ARGS-INVALID");
        assert_eq!(args.read_failure(), None);
        for kind in [
            ReadFailure::NotFound,
            ReadFailure::PermissionDenied,
            ReadFailure::TooLarge,
            ReadFailure::Other,
        ] {
            let failure = Failure::configuration_read(kind, Some(location.clone()));
            assert_eq!(failure.kind(), FailureKind::ConfigurationRead);
            assert_eq!(failure.read_failure(), Some(kind));
            assert_eq!(failure.code(), "UNI-CONFIG-READ");
            assert!(!failure.retryable());
            assert!(!failure.fix().is_empty());
        }
    }

    #[test]
    fn display_does_not_include_caller_supplied_location_text() {
        let failure = Failure::invalid_configuration(Some(Location {
            path: Some("PRIVATE-MARKER".into()),
            ..Location::default()
        }));
        assert!(!failure.to_string().contains("PRIVATE-MARKER"));
        assert!(failure.to_string().contains(failure.code()));
        assert!(!failure.retryable());
    }
}
