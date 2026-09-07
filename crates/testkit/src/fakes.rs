use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use unisphere_core::{
    ConfigReader, Failure, InspectionApi, InspectionReport, InspectionRequest, ReadFailure,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadCall {
    pub path: PathBuf,
    pub max_bytes: usize,
}

/// In-memory reader. Unknown paths behave like absent explicit files.
#[derive(Debug)]
pub struct FakeReader {
    entries: BTreeMap<PathBuf, Result<Vec<u8>, ReadFailure>>,
    calls: Mutex<Vec<ReadCall>>,
}

impl FakeReader {
    /// Panics on a relative fixture path: fake inputs must use absolute keys.
    pub fn new(entries: impl IntoIterator<Item = (PathBuf, Result<Vec<u8>, ReadFailure>)>) -> Self {
        let entries: BTreeMap<_, _> = entries.into_iter().collect();
        assert!(
            entries.keys().all(|path| path.is_absolute()),
            "FakeReader paths must be absolute"
        );
        Self {
            entries,
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<ReadCall> {
        self.calls
            .lock()
            .expect("fake reader call log poisoned")
            .clone()
    }
}

impl ConfigReader for FakeReader {
    fn read(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, ReadFailure> {
        self.calls
            .lock()
            .expect("fake reader call log poisoned")
            .push(ReadCall {
                path: path.to_owned(),
                max_bytes,
            });
        match self.entries.get(path) {
            Some(Ok(bytes)) if bytes.len() > max_bytes => Err(ReadFailure::TooLarge),
            Some(result) => result.clone(),
            None => Err(ReadFailure::NotFound),
        }
    }
}

#[derive(Debug)]
pub struct FakeInspector {
    result: Result<InspectionReport, Failure>,
    requests: Mutex<Vec<InspectionRequest>>,
}

impl FakeInspector {
    pub fn new(result: Result<InspectionReport, Failure>) -> Self {
        Self {
            result,
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn requests(&self) -> Vec<InspectionRequest> {
        self.requests
            .lock()
            .expect("fake inspector request log poisoned")
            .clone()
    }
}

impl InspectionApi for FakeInspector {
    fn inspect(&self, request: &InspectionRequest) -> Result<InspectionReport, Failure> {
        self.requests
            .lock()
            .expect("fake inspector request log poisoned")
            .push(request.clone());
        self.result.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unisphere_core::{ConfigOverrides, ConfigSource, Configuration};

    #[test]
    fn reader_records_limits_and_returns_isolated_values_or_typed_failures() {
        let path = PathBuf::from("/fixture/config.json");
        let denied = PathBuf::from("/fixture/denied.json");
        let reader = FakeReader::new([
            (path.clone(), Ok(b"{}".to_vec())),
            (denied.clone(), Err(ReadFailure::PermissionDenied)),
        ]);
        assert_eq!(reader.read(&path, 1), Err(ReadFailure::TooLarge));
        let mut bytes = reader.read(&path, 2).unwrap();
        bytes[0] = b'!';
        assert_eq!(reader.read(&path, 2), Ok(b"{}".to_vec()));
        assert_eq!(reader.read(&denied, 2), Err(ReadFailure::PermissionDenied));
        assert_eq!(
            reader.read(Path::new("/fixture/missing.json"), 2),
            Err(ReadFailure::NotFound)
        );
        let calls = reader.calls();
        assert_eq!(calls.len(), 5);
        assert_eq!(calls[0], ReadCall { path, max_bytes: 1 });
    }

    #[test]
    fn inspector_records_requests_for_both_success_and_error() {
        let request = InspectionRequest {
            source: ConfigSource::Defaults,
            overrides: ConfigOverrides {
                source_roots: Some(Vec::new()),
            },
        };
        let report = InspectionReport {
            configuration: Configuration::default(),
        };
        let success = FakeInspector::new(Ok(report.clone()));
        assert_eq!(success.inspect(&request), Ok(report));
        assert_eq!(success.requests(), vec![request.clone()]);
        let failure = Failure::configuration_read(ReadFailure::PermissionDenied, None);
        let error = FakeInspector::new(Err(failure.clone()));
        assert_eq!(error.inspect(&request), Err(failure));
        assert_eq!(error.requests(), vec![request]);
    }

    #[test]
    fn fakes_are_parallel_safe_without_shared_environment() {
        let inspector = FakeInspector::new(Ok(InspectionReport {
            configuration: Configuration::default(),
        }));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| inspector.inspect(&InspectionRequest::default()).unwrap());
            }
        });
        assert_eq!(inspector.requests().len(), 8);
    }

    #[test]
    #[should_panic(expected = "FakeReader paths must be absolute")]
    fn relative_fixture_paths_are_rejected() {
        let _ = FakeReader::new([(PathBuf::from("relative.json"), Ok(Vec::new()))]);
    }
}
