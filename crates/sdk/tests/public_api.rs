#![forbid(unsafe_code)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use unisphere_sdk::{
    ConfigOverrides, ConfigReader, ConfigSource, Configuration, FailureKind, InspectionApi,
    InspectionRequest, Inspector, MAX_CONFIG_BYTES, ReadFailure, StdConfigReader, inspect,
};
use unisphere_testkit::{FakeReader, fixtures};

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "unisphere-sdk-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create isolated SDK fixture: {error}"),
            }
        }
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        // Only remove the exact directory this test successfully created.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn file_request(path: PathBuf) -> InspectionRequest {
    InspectionRequest {
        source: ConfigSource::File(path),
        ..InspectionRequest::default()
    }
}

#[test]
fn public_facade_and_injected_service_agree_on_explicit_file() {
    let temp = TemporaryDirectory::new();
    let path = temp.file("config.json", fixtures::ROOTS);
    let reader = FakeReader::new([(path.clone(), Ok(fixtures::ROOTS.to_vec()))]);
    let request = file_request(path.clone());
    let expected = Inspector::new(reader).inspect(&request).unwrap();
    assert_eq!(inspect(&request).unwrap(), expected);
    assert_eq!(
        StdConfigReader.read(&path, MAX_CONFIG_BYTES).unwrap(),
        fixtures::ROOTS
    );
    assert_eq!(fs::read(path).unwrap(), fixtures::ROOTS);
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 1);
}

#[test]
fn public_facade_defaults_inline_overrides_and_diagnostics_are_usable() {
    assert_eq!(
        inspect(&InspectionRequest::default())
            .unwrap()
            .configuration,
        Configuration::default()
    );
    let report = inspect(&InspectionRequest {
        source: ConfigSource::Inline(fixtures::ROOTS.to_vec()),
        overrides: ConfigOverrides {
            source_roots: Some(vec![]),
        },
    })
    .unwrap();
    assert!(report.configuration.source_roots.is_empty());
    let failure = inspect(&InspectionRequest {
        source: ConfigSource::Inline(fixtures::BLANK_ROOT.to_vec()),
        ..InspectionRequest::default()
    })
    .unwrap_err();
    assert_eq!(failure.kind(), FailureKind::InvalidConfiguration);
    assert_eq!(failure.code(), "UNI-CONFIG-INVALID");
    assert_eq!(
        failure.location().unwrap().field.as_deref(),
        Some("source_roots[0]")
    );
}

#[test]
fn adapter_enforces_byte_limits_including_zero_and_exact_length() {
    let temp = TemporaryDirectory::new();
    let empty = temp.file("empty", b"");
    let bytes = temp.file("bytes", b"abc");
    assert_eq!(StdConfigReader.read(&empty, 0), Ok(vec![]));
    assert_eq!(StdConfigReader.read(&bytes, 0), Err(ReadFailure::TooLarge));
    assert_eq!(StdConfigReader.read(&bytes, 2), Err(ReadFailure::TooLarge));
    assert_eq!(StdConfigReader.read(&bytes, 3), Ok(b"abc".to_vec()));
    assert_eq!(StdConfigReader.read(&bytes, 4), Ok(b"abc".to_vec()));
    assert_eq!(
        StdConfigReader.read(&bytes, usize::MAX),
        Ok(b"abc".to_vec())
    );
}

#[test]
fn facade_file_limit_distinguishes_exact_size_from_one_byte_over() {
    let temp = TemporaryDirectory::new();
    let oversized = fixtures::oversized_document();
    let exact = temp.file("exact.json", &oversized[..MAX_CONFIG_BYTES]);
    assert_eq!(
        inspect(&file_request(exact)).unwrap().configuration,
        Configuration::default()
    );
    let path = temp.file("oversized.json", &oversized);
    let failure = inspect(&file_request(path.clone())).unwrap_err();
    assert_eq!(failure.kind(), FailureKind::ConfigurationRead);
    assert_eq!(failure.read_failure(), Some(ReadFailure::TooLarge));
    assert_eq!(failure.location().unwrap().path.as_deref(), path.to_str());
}

#[test]
fn missing_and_non_file_inputs_are_typed_read_failures_not_defaults() {
    let temp = TemporaryDirectory::new();
    let missing = temp.0.join("missing.json");
    assert_eq!(
        StdConfigReader.read(&missing, MAX_CONFIG_BYTES),
        Err(ReadFailure::NotFound)
    );
    let failure = inspect(&file_request(missing.clone())).unwrap_err();
    assert_eq!(failure.kind(), FailureKind::ConfigurationRead);
    assert_eq!(failure.read_failure(), Some(ReadFailure::NotFound));
    assert_eq!(
        failure.location().unwrap().path.as_deref(),
        missing.to_str()
    );
    assert!(StdConfigReader.read(&temp.0, MAX_CONFIG_BYTES).is_err());
    assert_eq!(
        inspect(&file_request(temp.0.clone())).unwrap_err().kind(),
        FailureKind::ConfigurationRead
    );
}

#[test]
fn direct_adapter_rejects_relative_paths_without_opening_them() {
    assert_eq!(
        StdConfigReader.read(Path::new("config.json"), MAX_CONFIG_BYTES),
        Err(ReadFailure::Other)
    );
    assert_eq!(
        inspect(&file_request(PathBuf::from("config.json")))
            .unwrap_err()
            .kind(),
        FailureKind::InvalidConfiguration
    );
}

#[test]
fn real_file_parser_failures_retain_path_but_not_sensitive_document_values() {
    let temp = TemporaryDirectory::new();
    for (name, bytes, kind) in fixtures::INVALID_DOCUMENTS {
        let path = temp.file(name, bytes);
        let failure = inspect(&file_request(path.clone())).unwrap_err();
        assert_eq!(failure.kind(), *kind, "{name}");
        assert_eq!(failure.location().unwrap().path.as_deref(), path.to_str());
        assert!(!format!("{failure:?} {failure}").contains("SENSITIVE-CONFIG-MARKER"));
    }
}

#[test]
fn filesystem_calls_are_parallel_safe_with_private_fixture_roots() {
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                let temp = TemporaryDirectory::new();
                let path = temp.file("explicit.json", fixtures::ROOTS);
                let report = inspect(&file_request(path)).unwrap();
                assert_eq!(report.configuration.source_roots, fixtures::DOCUMENT_ROOTS);
            });
        }
    });
}
