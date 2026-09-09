//! Application contract for exporting one complete revision projection.
use crate::{MappingOptions, PipelineError, SnapshotDiagnostic, SnapshotLimits, SnapshotRef};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Clone)]
pub struct SnapshotRequest {
    pub source: SnapshotRef,
    pub limits: SnapshotLimits,
    pub options: MappingOptions,
}

/// Returned only after the destination accepts the entire projection. This is
/// revision evidence, not an append cursor, persisted CLI state or sink transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCheckpoint {
    pub source: SnapshotRef,
    pub revision: String,
    pub adapter: String,
    pub include_content: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCollection {
    pub checkpoint: SnapshotCheckpoint,
    /// Includes the closing scoped replacement manifest, even for an empty projection.
    pub records_written: usize,
    pub diagnostics: Vec<SnapshotDiagnostic>,
}

pub trait SnapshotCollectionApi: Send + Sync {
    /// Every invocation exports a full projection and closing manifest. The caller
    /// decides how to replace its prior view; revisions are never implicitly skipped.
    fn collect_snapshot(
        &self,
        request: &SnapshotRequest,
        destination: &mut dyn Write,
    ) -> Result<SnapshotCollection, PipelineError>;
}
