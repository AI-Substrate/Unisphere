use std::path::Path;

use crate::{Failure, InspectionReport, InspectionRequest, ReadFailure};

/// Reads only the explicitly supplied path, rejecting oversized input before
/// copying an unbounded document. Implementations own their I/O policy.
pub trait ConfigReader: Send + Sync {
    fn read(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, ReadFailure>;
}

/// Synchronous inspection over explicit inputs; no background work is implied.
pub trait InspectionApi: Send + Sync {
    fn inspect(&self, request: &InspectionRequest) -> Result<InspectionReport, Failure>;
}
