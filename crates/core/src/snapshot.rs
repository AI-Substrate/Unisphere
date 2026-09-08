//! Whole-source revision snapshots, separate from append-only LF cursors.
use crate::{
    MappingDiagnosticCode, MappingOptions, PipelineError, PipelineErrorKind, TelemetryRecord,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Explicit storage representation; no ambient path or database discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapshotFormat {
    JsonDocument,
    JsonJournal,
    /// Read key/value columns from an explicitly named table in one read transaction.
    SqliteKeyValue {
        table: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRef {
    pub path: PathBuf,
    pub format: SnapshotFormat,
    /// Optional native logical session selection, interpreted by the pure mapper.
    pub session_id: Option<String>,
}
impl SnapshotRef {
    pub fn validate(&self) -> Result<(), PipelineError> {
        if !self.path.is_absolute()
            || self.path.to_str().is_none()
            || self.session_id.as_ref().is_some_and(|id| id.is_empty())
        {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        if let SnapshotFormat::SqliteKeyValue { table } = &self.format {
            if table.is_empty()
                || !table
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotLimits {
    pub max_records: usize,
    pub max_record_bytes: usize,
    pub max_snapshot_bytes: usize,
}
impl Default for SnapshotLimits {
    fn default() -> Self {
        Self {
            max_records: 100_000,
            max_record_bytes: 32 * 1024 * 1024,
            max_snapshot_bytes: 64 * 1024 * 1024,
        }
    }
}
impl SnapshotLimits {
    pub fn validate(self) -> Result<(), PipelineError> {
        if self.max_records == 0
            || self.max_record_bytes == 0
            || self.max_snapshot_bytes < self.max_record_bytes
        {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        Ok(())
    }
}

/// One raw JSON document, journal operation or database value. Keys are native
/// DB keys or documented structural locations, never pretend file byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    pub key: String,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSnapshot {
    pub source: SnapshotRef,
    /// Loader-owned digest of the complete bounded raw snapshot representation.
    pub revision: String,
    pub records: Vec<SnapshotRecord>,
}
impl NativeSnapshot {
    pub fn validate(&self, limits: SnapshotLimits) -> Result<(), PipelineError> {
        limits.validate()?;
        self.source.validate()?;
        if self.revision.is_empty() {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        }
        if self.records.len() > limits.max_records {
            return Err(PipelineError::new(PipelineErrorKind::BatchLimit, None));
        }
        let mut total = 0usize;
        for record in &self.records {
            if record.key.is_empty() {
                return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
            }
            if record.bytes.len() > limits.max_record_bytes {
                return Err(PipelineError::new(PipelineErrorKind::RecordLimit, None));
            }
            total = total
                .checked_add(record.key.len())
                .and_then(|n| n.checked_add(record.bytes.len()))
                .ok_or_else(|| PipelineError::new(PipelineErrorKind::BatchLimit, None))?;
            if total > limits.max_snapshot_bytes {
                return Err(PipelineError::new(PipelineErrorKind::BatchLimit, None));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotDiagnostic {
    pub key: String,
    pub code: MappingDiagnosticCode,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MappedSnapshot {
    pub records: Vec<TelemetryRecord>,
    pub diagnostics: Vec<SnapshotDiagnostic>,
}

pub trait SnapshotLoader: Send + Sync {
    /// Read a complete bounded representation or fail; partial snapshots never succeed.
    fn read_snapshot(
        &self,
        source: &SnapshotRef,
        limits: SnapshotLimits,
    ) -> Result<NativeSnapshot, PipelineError>;
}
pub trait SnapshotAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    /// Pure mapping/reduction. Journal patches must be reduced before projecting messages.
    fn map_snapshot(
        &self,
        snapshot: &NativeSnapshot,
        options: MappingOptions,
    ) -> Result<MappedSnapshot, PipelineError>;
}
