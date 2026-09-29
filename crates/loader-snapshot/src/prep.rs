//! Incremental-prep port for whole-source snapshots, backed by
//! [`FileSnapshotLoader`]. Discovery and stat are metadata-only; SQLite
//! `-wal`/`-shm` sidecars are folded into size and mtime so committed WAL
//! growth is seen before any checkpoint. Native stores are opened read-only
//! and are never locked, renamed or written.

use std::path::Path;

use unisphere_core::{
    PipelineError, PipelineErrorKind, SnapshotFormat, SnapshotLimits, SnapshotLoader, SnapshotRef,
    prep::{
        NativeAddress, PrepBatch, PrepDiscovery, PrepInput, PrepLoader, PrepReadLimits,
        PrepSourceKind, PrepSourceStat,
    },
};

use crate::FileSnapshotLoader;

/// Upper bound on discovered candidates; exceeding it fails instead of truncating.
#[cfg(unix)]
const MAX_PREP_SOURCES: usize = 100_000;
/// SQLite files that belong to a database rather than being sources themselves.
#[cfg(unix)]
const SQLITE_SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

fn error(kind: PipelineErrorKind) -> PipelineError {
    PipelineError::new(kind, None)
}

/// [`PrepLoader`] for one [`SnapshotFormat`]; every discovered source is read
/// as a complete bounded snapshot and any change is a new revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPrepLoader {
    format: SnapshotFormat,
}

impl SnapshotPrepLoader {
    pub fn new(format: SnapshotFormat) -> Self {
        Self { format }
    }

    fn source(&self, path: &Path) -> SnapshotRef {
        SnapshotRef {
            path: path.to_path_buf(),
            format: self.format.clone(),
            session_id: None,
        }
    }

    #[cfg(unix)]
    fn sqlite(&self) -> bool {
        matches!(self.format, SnapshotFormat::SqliteKeyValue { .. })
    }
}

impl PrepLoader for SnapshotPrepLoader {
    fn kind(&self) -> PrepSourceKind {
        PrepSourceKind::Snapshot
    }

    fn discover(
        &self,
        root: &Path,
        accept: &dyn Fn(&str) -> bool,
    ) -> Result<PrepDiscovery, PipelineError> {
        #[cfg(unix)]
        return unix::discover(root, accept, self.sqlite());
        #[cfg(not(unix))]
        {
            let _ = (root, accept);
            Err(error(PipelineErrorKind::Unsupported))
        }
    }

    fn stat(&self, root: &Path, path: &Path) -> Result<PrepSourceStat, PipelineError> {
        #[cfg(unix)]
        return unix::stat(root, path, self.sqlite());
        #[cfg(not(unix))]
        {
            let _ = (root, path);
            Err(error(PipelineErrorKind::Unsupported))
        }
    }

    fn read(
        &self,
        stat: &PrepSourceStat,
        from: Option<&unisphere_core::ReadCursor>,
        limits: PrepReadLimits,
    ) -> Result<PrepBatch, PipelineError> {
        if from.is_some() || stat.kind != PrepSourceKind::Snapshot {
            return Err(error(PipelineErrorKind::InvalidInput));
        }
        #[cfg(unix)]
        unix::same_identity(stat)?;
        let snapshot =
            FileSnapshotLoader.read_snapshot(&self.source(&stat.path), limits.snapshot)?;
        let bytes_read = snapshot
            .records
            .iter()
            .map(|record| (record.key.len() + record.bytes.len()) as u64)
            .sum();
        Ok(PrepBatch {
            input: PrepInput::Snapshot(snapshot),
            next_cursor: None,
            more: false,
            incomplete_tail: false,
            bytes_read,
        })
    }

    fn anchor(&self, _stat: &PrepSourceStat, _offset: u64) -> Result<String, PipelineError> {
        Err(error(PipelineErrorKind::Unsupported))
    }

    /// The value of the snapshot record whose key is `address.key`, read from a
    /// fresh bounded snapshot with the default prep limits.
    fn record_at(
        &self,
        path: &Path,
        address: &NativeAddress,
        max_bytes: usize,
    ) -> Result<Vec<u8>, PipelineError> {
        let key = address
            .key
            .as_deref()
            .filter(|key| !key.is_empty())
            .ok_or_else(|| error(PipelineErrorKind::InvalidInput))?;
        let snapshot =
            FileSnapshotLoader.read_snapshot(&self.source(path), SnapshotLimits::default())?;
        let record = snapshot
            .records
            .into_iter()
            .find(|record| record.key == key)
            .ok_or_else(|| error(PipelineErrorKind::InvalidInput))?;
        if record.bytes.len() > max_bytes {
            return Err(error(PipelineErrorKind::RecordLimit));
        }
        Ok(record.bytes)
    }
}

#[cfg(unix)]
mod unix {
    use std::{
        ffi::OsString,
        fs::{self, Metadata},
        os::unix::fs::MetadataExt,
        path::{Path, PathBuf},
    };

    use unisphere_core::{
        PipelineError, PipelineErrorKind, SourceIdentity,
        prep::{PrepDiscovery, PrepSourceKind, PrepSourceStat},
    };

    use super::{MAX_PREP_SOURCES, SQLITE_SIDECARS, error};

    fn mtime_ns(metadata: &Metadata) -> i128 {
        i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec())
    }

    fn identity(metadata: &Metadata) -> SourceIdentity {
        SourceIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    fn sidecar(path: &Path, suffix: &str) -> PathBuf {
        let mut name = OsString::from(path.as_os_str());
        name.push(suffix);
        PathBuf::from(name)
    }

    /// Size and mtime of `path`, plus its regular (never followed) `-wal` and
    /// `-shm` sidecars for SQLite: the sum of sizes and the latest mtime.
    fn stat_of(path: &Path, file: String, metadata: &Metadata, sqlite: bool) -> PrepSourceStat {
        let mut size = metadata.len();
        let mut mtime = mtime_ns(metadata);
        if sqlite {
            for suffix in ["-wal", "-shm"] {
                if let Ok(extra) = fs::symlink_metadata(sidecar(path, suffix))
                    && extra.is_file()
                {
                    size = size.saturating_add(extra.len());
                    mtime = mtime.max(mtime_ns(&extra));
                }
            }
        }
        PrepSourceStat {
            path: path.to_path_buf(),
            file,
            kind: PrepSourceKind::Snapshot,
            identity: identity(metadata),
            size,
            mtime_ns: mtime,
        }
    }

    pub(super) fn discover(
        root: &Path,
        accept: &dyn Fn(&str) -> bool,
        sqlite: bool,
    ) -> Result<PrepDiscovery, PipelineError> {
        let read = |_| error(PipelineErrorKind::Read);
        if !root.is_absolute() || !fs::symlink_metadata(root).map_err(read)?.is_dir() {
            return Err(error(PipelineErrorKind::InvalidInput));
        }
        let mut found = PrepDiscovery::default();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            // The root must be readable; a subdirectory failing mid-walk is counted.
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(failure) if dir == root => return Err(read(failure)),
                Err(_) => {
                    found.skipped.unreadable_entries += 1;
                    continue;
                }
            };
            for entry in entries {
                let Ok(entry) = entry else {
                    found.skipped.unreadable_entries += 1;
                    continue;
                };
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    // Not keyable as a `/`-separated UTF-8 relative path.
                    found.skipped.unreadable_entries += 1;
                    continue;
                };
                if name.starts_with('.') {
                    found.skipped.hidden += 1;
                    continue;
                }
                let Ok(kind) = entry.file_type() else {
                    found.skipped.unreadable_entries += 1;
                    continue;
                };
                let path = entry.path();
                if kind.is_symlink() {
                    found.skipped.symlinks += 1;
                } else if kind.is_dir() {
                    stack.push(path);
                } else if kind.is_file() {
                    // Sidecars are folded into their database's stat, not sources.
                    if sqlite && SQLITE_SIDECARS.iter().any(|s| name.ends_with(s)) {
                        continue;
                    }
                    let Some(file) = path
                        .strip_prefix(root)
                        .ok()
                        .and_then(Path::to_str)
                        .map(str::to_owned)
                    else {
                        found.skipped.unreadable_entries += 1;
                        continue;
                    };
                    if !accept(&file) {
                        continue;
                    }
                    let Ok(metadata) = fs::symlink_metadata(&path) else {
                        found.skipped.unreadable_entries += 1;
                        continue;
                    };
                    if found.sources.len() == MAX_PREP_SOURCES {
                        return Err(error(PipelineErrorKind::ListingLimit));
                    }
                    found.sources.push(stat_of(&path, file, &metadata, sqlite));
                }
            }
        }
        found.sources.sort_unstable_by(|a, b| a.file.cmp(&b.file));
        Ok(found)
    }

    pub(super) fn stat(
        root: &Path,
        path: &Path,
        sqlite: bool,
    ) -> Result<PrepSourceStat, PipelineError> {
        let file = path
            .strip_prefix(root)
            .ok()
            .and_then(Path::to_str)
            .filter(|file| !file.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| error(PipelineErrorKind::InvalidInput))?;
        let metadata = fs::symlink_metadata(path).map_err(|_| error(PipelineErrorKind::Read))?;
        if !metadata.is_file() {
            return Err(error(PipelineErrorKind::InvalidInput));
        }
        Ok(stat_of(path, file, &metadata, sqlite))
    }

    /// The path still names the file that was stat'ed; a replacement between
    /// stat and read is a new generation, not this one.
    pub(super) fn same_identity(stat: &PrepSourceStat) -> Result<(), PipelineError> {
        let metadata = fs::symlink_metadata(&stat.path)
            .map_err(|_| error(PipelineErrorKind::SourceChanged))?;
        if !metadata.is_file() || identity(&metadata) != stat.identity {
            return Err(error(PipelineErrorKind::SourceChanged));
        }
        Ok(())
    }
}
