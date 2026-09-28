//! Storage, pure mapping and output contracts for source-derived session records.
//! No operation here opens files, observes ambient state, or persists checkpoints.

use std::{collections::BTreeMap, error::Error, fmt, io::Write, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum serialized OTLP batch, including its terminating newline.
pub const MAX_OUTPUT_BATCH_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceScope {
    pub root: PathBuf,
    pub max_sessions: usize,
}

impl SourceScope {
    pub fn validate(&self) -> Result<(), PipelineError> {
        if self.max_sessions == 0 || !self.root.is_absolute() || self.root.to_str().is_none() {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRef {
    pub path: PathBuf,
}

impl SessionRef {
    pub fn validate(&self) -> Result<(), PipelineError> {
        if !self.path.is_absolute() || self.path.to_str().is_none() {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceIdentity {
    Unix { device: u64, inode: u64 },
    Unavailable,
}

/// Position immediately after a complete LF in one particular source generation.
/// The caller persists it only after successfully accepting the collection result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadCursor {
    pub source: PathBuf,
    pub identity: SourceIdentity,
    pub offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadLimits {
    pub max_records: usize,
    pub max_record_bytes: usize,
    pub max_batch_bytes: usize,
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self {
            max_records: 128,
            // Real Claude transcripts carry image tool results up to ~1.4 MB per line.
            max_record_bytes: 3 * 1024 * 1024,
            max_batch_bytes: 4_194_304,
        }
    }
}

impl ReadLimits {
    /// Validate before any loader or service I/O, including fake-loader calls.
    pub fn validate(self) -> Result<(), PipelineError> {
        if self.max_records == 0
            || self.max_record_bytes == 0
            || self.max_batch_bytes == 0
            || self.max_batch_bytes < self.max_record_bytes
        {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRecord {
    pub offset: u64,
    /// Native bytes excluding the final LF; CR is retained.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedBatch {
    pub source: SessionRef,
    pub records: Vec<NativeRecord>,
    pub next_cursor: ReadCursor,
    pub more: bool,
    pub incomplete_tail: bool,
}

impl LoadedBatch {
    /// Check observable bounds at the service boundary even for injected loaders.
    /// The loader additionally accounts for blank physical lines not returned here.
    pub fn validate(&self, limits: ReadLimits) -> Result<(), PipelineError> {
        limits.validate()?;
        self.source.validate()?;
        if self.next_cursor.source != self.source.path {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        }
        if self.records.len() > limits.max_records {
            return Err(PipelineError::new(PipelineErrorKind::BatchLimit, None));
        }
        let mut total = 0usize;
        let mut prior_end = None;
        for record in &self.records {
            let physical_len = record.bytes.len().checked_add(1).ok_or_else(|| {
                PipelineError::new(PipelineErrorKind::RecordLimit, Some(record.offset))
            })?;
            if physical_len > limits.max_record_bytes {
                return Err(PipelineError::new(
                    PipelineErrorKind::RecordLimit,
                    Some(record.offset),
                ));
            }
            total = total.checked_add(physical_len).ok_or_else(|| {
                PipelineError::new(PipelineErrorKind::BatchLimit, Some(record.offset))
            })?;
            if total > limits.max_batch_bytes {
                return Err(PipelineError::new(
                    PipelineErrorKind::BatchLimit,
                    Some(record.offset),
                ));
            }
            let length = u64::try_from(physical_len).map_err(|_| {
                PipelineError::new(PipelineErrorKind::InvalidData, Some(record.offset))
            })?;
            let end = record.offset.checked_add(length).ok_or_else(|| {
                PipelineError::new(PipelineErrorKind::InvalidData, Some(record.offset))
            })?;
            if prior_end.is_some_and(|previous| record.offset < previous)
                || end > self.next_cursor.offset
            {
                return Err(PipelineError::new(
                    PipelineErrorKind::InvalidData,
                    Some(record.offset),
                ));
            }
            prior_end = Some(end);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryRecord {
    pub event_name: String,
    pub timestamp_unix_nano: Option<u64>,
    pub attributes: BTreeMap<String, Value>,
    pub body: Option<Value>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingOptions {
    pub include_content: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingDiagnosticCode {
    UnsupportedRecord,
    UnsupportedPart,
    InvalidField,
    InvalidTimestamp,
    ContentOmitted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingDiagnostic {
    pub offset: u64,
    pub code: MappingDiagnosticCode,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MappedBatch {
    pub records: Vec<TelemetryRecord>,
    pub diagnostics: Vec<MappingDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionBatch {
    pub mapped: MappedBatch,
    pub next_cursor: ReadCursor,
    pub more: bool,
    pub incomplete_tail: bool,
}

pub trait SessionLoader: Send + Sync {
    fn list_sessions(&self, scope: &SourceScope) -> Result<Vec<SessionRef>, PipelineError>;
    fn read_batch(
        &self,
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
    ) -> Result<LoadedBatch, PipelineError>;
}

/// Pure deterministic mapping: no loader, filesystem handle, clock, or destination.
pub trait SessionAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn map(
        &self,
        source: &SessionRef,
        records: &[NativeRecord],
        options: MappingOptions,
    ) -> Result<MappedBatch, PipelineError>;
}

pub trait RecordWriter: Send + Sync {
    fn write_batch(
        &self,
        records: &[TelemetryRecord],
        destination: &mut dyn Write,
    ) -> Result<(), PipelineError>;
}

/// Core-owned object-safe application port used by the CLI frontend.
pub trait CollectionApi: Send + Sync {
    fn list_sessions(&self, scope: &SourceScope) -> Result<Vec<SessionRef>, PipelineError>;
    fn collect_batch(
        &self,
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
        options: MappingOptions,
        destination: &mut dyn Write,
    ) -> Result<CollectionBatch, PipelineError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineErrorKind {
    InvalidInput,
    Read,
    SourceChanged,
    RecordLimit,
    BatchLimit,
    ListingLimit,
    OutputLimit,
    InvalidData,
    Write,
    Unsupported,
}

/// Fixed public diagnostics, with optional structural offset but no source contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineError {
    kind: PipelineErrorKind,
    offset: Option<u64>,
}

impl PipelineError {
    pub fn new(kind: PipelineErrorKind, offset: Option<u64>) -> Self {
        Self { kind, offset }
    }
    pub fn kind(&self) -> PipelineErrorKind {
        self.kind
    }
    pub fn offset(&self) -> Option<u64> {
        self.offset
    }
    pub fn code(&self) -> &'static str {
        match self.kind {
            PipelineErrorKind::InvalidInput => "UNI-INPUT",
            PipelineErrorKind::Read => "UNI-READ",
            PipelineErrorKind::SourceChanged => "UNI-SOURCE-CHANGED",
            PipelineErrorKind::RecordLimit => "UNI-LIMIT-RECORD",
            PipelineErrorKind::BatchLimit => "UNI-LIMIT-BATCH",
            PipelineErrorKind::ListingLimit => "UNI-LIMIT-LISTING",
            PipelineErrorKind::OutputLimit => "UNI-LIMIT-OUTPUT",
            PipelineErrorKind::InvalidData => "UNI-DATA",
            PipelineErrorKind::Write => "UNI-WRITE",
            PipelineErrorKind::Unsupported => "UNI-UNSUPPORTED",
        }
    }
    pub fn message(&self) -> &'static str {
        match self.kind {
            PipelineErrorKind::InvalidInput => "The explicit source or limits are invalid.",
            PipelineErrorKind::Read => "The explicit source could not be read.",
            PipelineErrorKind::SourceChanged => "The cursor does not match the current source.",
            PipelineErrorKind::RecordLimit => {
                "A physical source record exceeds the selected limit."
            }
            PipelineErrorKind::BatchLimit => "The native batch exceeds the selected budget.",
            PipelineErrorKind::ListingLimit => {
                "The explicit directory contains too many session candidates."
            }
            PipelineErrorKind::OutputLimit => "The encoded output batch exceeds the output budget.",
            PipelineErrorKind::InvalidData => {
                "The supplied native data or batch structure is invalid."
            }
            PipelineErrorKind::Write => "The destination did not accept the output batch.",
            PipelineErrorKind::Unsupported => {
                "The requested storage operation is unsupported on this platform."
            }
        }
    }
    pub fn fix(&self) -> &'static str {
        match self.kind {
            PipelineErrorKind::InvalidInput => {
                "Use absolute UTF-8 paths and positive limits; max-batch-bytes must be at least max-record-bytes."
            }
            PipelineErrorKind::Read => {
                "Check the explicit file or leaf project directory and its read permissions; symlink files are not followed."
            }
            PipelineErrorKind::SourceChanged => {
                "Inspect the replaced, truncated or mismatched source, then explicitly start with no cursor if appropriate."
            }
            PipelineErrorKind::RecordLimit => {
                "Retry from the previous cursor with larger --max-record-bytes and compatible --max-batch-bytes; no record was skipped."
            }
            PipelineErrorKind::BatchLimit => {
                "Reduce --max-records or increase the compatible --max-batch-bytes budget, then retry the previous cursor."
            }
            PipelineErrorKind::ListingLimit => {
                "Increase --max-sessions or select a narrower leaf project directory."
            }
            PipelineErrorKind::OutputLimit => {
                "Reduce the input batch size or omit content; an encoded batch must fit within 33554432 bytes."
            }
            PipelineErrorKind::InvalidData => {
                "Inspect the record at the reported byte offset or correct the injected loader result; malformed data is not silently skipped."
            }
            PipelineErrorKind::Write => {
                "Check the destination and handle any partial output before retrying; the checkpoint was not accepted."
            }
            PipelineErrorKind::Unsupported => {
                "Use the filesystem session loader on a supported Unix host; pure mapping and output encoding accept supplied data independently."
            }
        }
    }
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())?;
        if let Some(offset) = self.offset {
            write!(f, " (byte {offset})")?;
        }
        write!(f, " {}", self.fix())
    }
}
impl Error for PipelineError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_limits_are_rejected_before_use() {
        for limits in [
            ReadLimits {
                max_records: 0,
                ..ReadLimits::default()
            },
            ReadLimits {
                max_record_bytes: 0,
                ..ReadLimits::default()
            },
            ReadLimits {
                max_batch_bytes: 1,
                ..ReadLimits::default()
            },
        ] {
            assert_eq!(
                limits.validate().unwrap_err().kind(),
                PipelineErrorKind::InvalidInput
            );
        }
        assert!(
            ReadLimits {
                max_records: 1,
                max_record_bytes: 1,
                max_batch_bytes: 1
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn loader_results_cannot_exceed_limits_or_claim_impossible_offsets() {
        let path = PathBuf::from("/explicit/session.jsonl");
        let mut batch = LoadedBatch {
            source: SessionRef { path: path.clone() },
            records: vec![NativeRecord {
                offset: 0,
                bytes: b"{}".to_vec(),
            }],
            next_cursor: ReadCursor {
                source: path,
                identity: SourceIdentity::Unix {
                    device: 1,
                    inode: 2,
                },
                offset: 3,
            },
            more: false,
            incomplete_tail: false,
        };
        let limits = ReadLimits {
            max_records: 1,
            max_record_bytes: 3,
            max_batch_bytes: 3,
        };
        assert!(batch.validate(limits).is_ok());
        batch.next_cursor.offset = 2;
        assert_eq!(
            batch.validate(limits).unwrap_err().kind(),
            PipelineErrorKind::InvalidData
        );
        batch.next_cursor.offset = 3;
        batch.records[0].bytes.push(b' ');
        let error = batch.validate(limits).unwrap_err();
        assert_eq!(
            (error.kind(), error.offset()),
            (PipelineErrorKind::RecordLimit, Some(0))
        );
    }

    #[test]
    fn limit_failures_identify_the_remedy_without_content() {
        let cases = [
            (
                PipelineErrorKind::RecordLimit,
                "UNI-LIMIT-RECORD",
                "--max-record-bytes",
            ),
            (
                PipelineErrorKind::BatchLimit,
                "UNI-LIMIT-BATCH",
                "--max-batch-bytes",
            ),
            (
                PipelineErrorKind::ListingLimit,
                "UNI-LIMIT-LISTING",
                "--max-sessions",
            ),
            (
                PipelineErrorKind::OutputLimit,
                "UNI-LIMIT-OUTPUT",
                "33554432",
            ),
        ];
        for (kind, code, flag) in cases {
            let error = PipelineError::new(kind, None);
            assert_eq!(error.code(), code);
            assert!(error.fix().contains(flag));
        }
    }
}
