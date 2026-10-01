//! Bounded explicit-source snapshots; storage only, with no native semantic mapping.
//! Unix filesystem support rejects final symlinks and nonregular files. Ancestor
//! directories are caller-trusted, as with the shared JSONL loader.
#![forbid(unsafe_code)]

use unisphere_core::{NativeSnapshot, PipelineError, SnapshotLimits, SnapshotLoader, SnapshotRef};

mod prep;
#[cfg(unix)]
mod unix;

pub use prep::SnapshotPrepLoader;

/// Stateless JSON document, LF journal and read-only SQLite snapshot loader.
#[derive(Debug, Default, Clone, Copy)]
pub struct FileSnapshotLoader;

impl FileSnapshotLoader {
    pub const fn new() -> Self {
        Self
    }
}

impl SnapshotLoader for FileSnapshotLoader {
    fn read_snapshot(
        &self,
        source: &SnapshotRef,
        limits: SnapshotLimits,
    ) -> Result<NativeSnapshot, PipelineError> {
        source.validate()?;
        limits.validate()?;
        #[cfg(unix)]
        {
            unix::read_snapshot(source, limits)
        }
        #[cfg(not(unix))]
        {
            Err(PipelineError::new(
                unisphere_core::PipelineErrorKind::Unsupported,
                None,
            ))
        }
    }
}
