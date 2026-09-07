//! Compile the actual service without its facade or filesystem adapter.
#![forbid(unsafe_code)]

#[path = "../src/service.rs"]
mod service;

use std::path::{Path, PathBuf};

use service::Inspector;
use unisphere_core::{
    ConfigOverrides, ConfigReader, ConfigSource, Configuration, FailureKind, InspectionApi,
    InspectionRequest, MAX_CONFIG_BYTES, ReadFailure,
};
use unisphere_testkit::{FakeReader, ReadCall, fixtures};

struct BorrowedReader<'a>(&'a FakeReader);

impl ConfigReader for BorrowedReader<'_> {
    fn read(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, ReadFailure> {
        self.0.read(path, max_bytes)
    }
}

fn path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\unisphere-fixtures\config.json")
    } else {
        PathBuf::from("/unisphere-fixtures/config.json")
    }
}

fn roots(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn request(bytes: &[u8]) -> InspectionRequest {
    InspectionRequest {
        source: ConfigSource::Inline(bytes.to_vec()),
        ..InspectionRequest::default()
    }
}

#[test]
fn precedence_preserves_omission_and_explicit_empty_at_each_layer() {
    let reader = FakeReader::new([]);
    let inspector = Inspector::with_defaults(
        BorrowedReader(&reader),
        Configuration { source_roots: roots(fixtures::DEFAULT_ROOTS) },
    );
    for (source, inherited) in [
        (ConfigSource::Defaults, roots(fixtures::DEFAULT_ROOTS)),
        (ConfigSource::Inline(fixtures::EMPTY.to_vec()), roots(fixtures::DEFAULT_ROOTS)),
        (ConfigSource::Inline(fixtures::ROOTS.to_vec()), roots(fixtures::DOCUMENT_ROOTS)),
        (ConfigSource::Inline(br#"{"source_roots":[]}"#.to_vec()), vec![]),
    ] {
        for overrides in [None, Some(vec![]), Some(roots(fixtures::OVERRIDE_ROOTS))] {
            let expected = overrides.as_ref().unwrap_or(&inherited).clone();
            let report = inspector.inspect(&InspectionRequest {
                source: source.clone(),
                overrides: ConfigOverrides { source_roots: overrides },
            }).unwrap();
            assert_eq!(report.configuration.source_roots, expected);
        }
    }
    assert!(reader.calls().is_empty());
}

#[test]
fn built_in_defaults_and_explicit_empty_are_valid() {
    let inspector = Inspector::new(FakeReader::new([]));
    for input in [InspectionRequest::default(), request(fixtures::EMPTY), request(br#"{"source_roots":[]}"#)] {
        assert_eq!(inspector.inspect(&input).unwrap().configuration, Configuration::default());
    }
}

#[test]
fn roots_are_literal_in_all_supplied_layers() {
    let literal = roots(&["  relative  ", "~/literal", "$HOME/root", "same", "same", "\u{2003}nonblank\u{2003}"]);
    let inspector = Inspector::with_defaults(
        FakeReader::new([]),
        Configuration { source_roots: literal.clone() },
    );
    let document = serde_json::to_vec(&serde_json::json!({"source_roots": literal})).unwrap();
    for input in [
        InspectionRequest::default(),
        request(&document),
        InspectionRequest {
            overrides: ConfigOverrides { source_roots: Some(literal.clone()) },
            ..InspectionRequest::default()
        },
    ] {
        assert_eq!(inspector.inspect(&input).unwrap().configuration.source_roots, literal);
    }
}

#[test]
fn shared_invalid_documents_fail_inline_and_file_without_secret_diagnostics() {
    for (name, bytes, expected_kind) in fixtures::INVALID_DOCUMENTS {
        let file = path();
        let reader = FakeReader::new([(file.clone(), Ok(bytes.to_vec()))]);
        let inspector = Inspector::new(BorrowedReader(&reader));
        for source in [ConfigSource::Inline(bytes.to_vec()), ConfigSource::File(file.clone())] {
            let is_file = matches!(source, ConfigSource::File(_));
            let failure = inspector.inspect(&InspectionRequest {
                source,
                // A valid replacement may not conceal an invalid document.
                overrides: ConfigOverrides { source_roots: Some(vec![]) },
            }).unwrap_err();
            assert_eq!(failure.kind(), *expected_kind, "{name}");
            assert_eq!(failure.code(), "UNI-CONFIG-INVALID", "{name}");
            assert_eq!(failure.read_failure(), None, "{name}");
            assert!(!failure.retryable());
            assert!(!format!("{failure:?} {failure}").contains("SENSITIVE-CONFIG-MARKER"));
            if is_file {
                assert_eq!(failure.location().unwrap().path.as_deref(), file.to_str());
            }
        }
        assert_eq!(reader.calls(), [ReadCall { path: file, max_bytes: MAX_CONFIG_BYTES }]);
    }
}

#[test]
fn invalid_defaults_and_overrides_are_not_hidden_by_precedence() {
    for blank in ["", " \t\r\n", "\u{2003}"] {
        let reader = FakeReader::new([]);
        let invalid_defaults = Inspector::with_defaults(
            BorrowedReader(&reader),
            Configuration { source_roots: vec![blank.into()] },
        );
        let failure = invalid_defaults.inspect(&InspectionRequest {
            source: ConfigSource::Inline(fixtures::ROOTS.to_vec()),
            overrides: ConfigOverrides { source_roots: Some(roots(fixtures::OVERRIDE_ROOTS)) },
        }).unwrap_err();
        assert_eq!(failure.kind(), FailureKind::InvalidConfiguration);
        assert_eq!(failure.location().unwrap().field.as_deref(), Some("source_roots[0]"));
        let failure = Inspector::new(BorrowedReader(&reader)).inspect(&InspectionRequest {
            source: ConfigSource::Inline(fixtures::ROOTS.to_vec()),
            overrides: ConfigOverrides { source_roots: Some(vec!["valid".into(), blank.into()]) },
        }).unwrap_err();
        assert_eq!(failure.location().unwrap().field.as_deref(), Some("source_roots[1]"));
        assert!(reader.calls().is_empty());
    }
}

#[test]
fn explicit_file_reads_once_with_the_bound_and_preserves_document_roots() {
    let file = path();
    let reader = FakeReader::new([(file.clone(), Ok(fixtures::ROOTS.to_vec()))]);
    let result = Inspector::new(BorrowedReader(&reader)).inspect(&InspectionRequest {
        source: ConfigSource::File(file.clone()),
        ..InspectionRequest::default()
    }).unwrap();
    assert_eq!(result.configuration.source_roots, roots(fixtures::DOCUMENT_ROOTS));
    assert_eq!(reader.calls(), [ReadCall { path: file, max_bytes: MAX_CONFIG_BYTES }]);
}

#[test]
fn file_failures_preserve_typed_reason_and_supplied_path() {
    for kind in fixtures::READ_FAILURES.iter().map(|(kind, _)| *kind)
        .chain([ReadFailure::TooLarge, ReadFailure::Other])
    {
        let file = path();
        let reader = FakeReader::new([(file.clone(), Err(kind))]);
        let failure = Inspector::new(BorrowedReader(&reader)).inspect(&InspectionRequest {
            source: ConfigSource::File(file.clone()),
            overrides: ConfigOverrides { source_roots: Some(vec![]) },
        }).unwrap_err();
        assert_eq!(failure.kind(), FailureKind::ConfigurationRead);
        assert_eq!(failure.read_failure(), Some(kind));
        assert_eq!(failure.code(), "UNI-CONFIG-READ");
        assert_eq!(failure.location().unwrap().path.as_deref(), file.to_str());
        assert_eq!(reader.calls(), [ReadCall { path: file, max_bytes: MAX_CONFIG_BYTES }]);
    }
}

#[test]
fn relative_file_paths_fail_before_reader_calls() {
    let reader = FakeReader::new([]);
    for relative in ["config.json", "../config.json", ""] {
        let failure = Inspector::new(BorrowedReader(&reader)).inspect(&InspectionRequest {
            source: ConfigSource::File(PathBuf::from(relative)),
            ..InspectionRequest::default()
        }).unwrap_err();
        assert_eq!(failure.kind(), FailureKind::InvalidConfiguration);
        assert_eq!(failure.location().unwrap().path.as_deref(), Some(relative));
    }
    assert!(reader.calls().is_empty());
}

#[test]
fn inline_size_boundary_accepts_exact_limit_and_never_calls_reader() {
    let reader = FakeReader::new([]);
    let inspector = Inspector::new(BorrowedReader(&reader));
    for size in [MAX_CONFIG_BYTES - 1, MAX_CONFIG_BYTES] {
        let mut bytes = vec![b' '; size];
        bytes[..2].copy_from_slice(b"{}");
        assert_eq!(inspector.inspect(&request(&bytes)).unwrap().configuration, Configuration::default());
    }
    let failure = inspector.inspect(&request(&fixtures::oversized_document())).unwrap_err();
    assert_eq!(failure.kind(), FailureKind::InvalidConfiguration);
    assert_eq!(failure.location(), None);
    assert!(reader.calls().is_empty());
}

#[test]
fn oversized_bytes_from_a_nonconforming_reader_are_still_rejected() {
    struct OversizedReader;
    impl ConfigReader for OversizedReader {
        fn read(&self, _path: &Path, _max_bytes: usize) -> Result<Vec<u8>, ReadFailure> {
            Ok(fixtures::oversized_document())
        }
    }
    let failure = Inspector::new(OversizedReader).inspect(&InspectionRequest {
        source: ConfigSource::File(path()),
        ..InspectionRequest::default()
    }).unwrap_err();
    assert_eq!(failure.kind(), FailureKind::ConfigurationRead);
    assert_eq!(failure.read_failure(), Some(ReadFailure::TooLarge));
}

#[test]
fn json_edges_reject_duplicates_escaped_aliases_unknowns_and_trailing_input() {
    let inspector = Inspector::new(FakeReader::new([]));
    for bytes in [
        &br#"{"source_roots":[],"source_\u0072oots":["hidden"]}"#[..],
        br#"{"source_\u0072oots":[],"source_roots":[]}"#,
        br#"{"source_roots":null}"#,
        br#"{"source_roots":[null]}"#,
        br#"{"source_roots":[{}]}"#,
        br#"{"source_roots":[true]}"#,
        br#"{"source_roots":[],"SENSITIVE-CONFIG-MARKER":0}"#,
        br#"{"SENSITIVE-CONFIG-MARKER":0}"#,
        br#"{"source_roots":[],}"#,
        br#"{"source_roots":["x",]}"#,
        br#"{} {}"#,
        br#"{} trailing"#,
        b"\xef\xbb\xbf{}",
        b"\x0b{}",
        b"null",
        b"\"string\"",
        b"",
    ] {
        let failure = inspector.inspect(&request(bytes)).unwrap_err();
        assert_eq!(failure.kind(), FailureKind::InvalidConfiguration);
        assert!(!format!("{failure:?} {failure}").contains("SENSITIVE-CONFIG-MARKER"));
    }
}

#[test]
fn escaped_keys_and_structural_characters_inside_roots_remain_valid() {
    let inspector = Inspector::new(FakeReader::new([]));
    let bytes = br#" { "source_\u0072oots" : ["}:,[\"source_roots\"]", "slash\\quote\"", "snowman \u2603"] } "#;
    let report = inspector.inspect(&request(bytes)).unwrap();
    assert_eq!(report.configuration.source_roots, ["}:,[\"source_roots\"]", "slash\\quote\"", "snowman \u{2603}"]);
}

#[test]
fn invalid_root_locations_are_structural_and_parser_coordinates_are_safe() {
    let inspector = Inspector::new(FakeReader::new([]));
    for bytes in [br#"{"source_roots":["valid",42]}"#.as_slice(), br#"{"source_roots":["valid"," "]}"#] {
        let failure = inspector.inspect(&request(bytes)).unwrap_err();
        assert_eq!(failure.location().unwrap().field.as_deref(), Some("source_roots[1]"));
    }
    let failure = inspector.inspect(&request(b"{\n  \"source_roots\": [\n  \"SENSITIVE-CONFIG-MARKER\",\n}")).unwrap_err();
    let location = failure.location().unwrap();
    assert!(location.line.is_some_and(|line| line >= 2));
    assert!(location.column.is_some());
    assert!(!format!("{failure:?} {failure}").contains("SENSITIVE-CONFIG-MARKER"));
}

#[test]
fn independent_readers_and_defaults_are_parallel_safe() {
    std::thread::scope(|scope| {
        for index in 0..8 {
            scope.spawn(move || {
                let expected = vec![format!("root-{index}")];
                let inspector = Inspector::with_defaults(
                    FakeReader::new([(path(), Ok(fixtures::EMPTY.to_vec()))]),
                    Configuration { source_roots: expected.clone() },
                );
                let result = inspector.inspect(&InspectionRequest {
                    source: ConfigSource::File(path()),
                    ..InspectionRequest::default()
                }).unwrap();
                assert_eq!(result.configuration.source_roots, expected);
            });
        }
    });
}
