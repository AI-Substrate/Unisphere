//! Incremental-prep port for whole-source snapshots, backed by
//! [`FileSnapshotLoader`]. Discovery and stat are metadata-only; SQLite
//! `-wal`/`-shm` sidecars are folded into size and mtime so committed WAL
//! growth is seen before any checkpoint. Native stores are opened read-only
//! and are never locked, renamed or written.

use std::path::Path;

use unisphere_core::{
    PipelineError, PipelineErrorKind, SnapshotFormat, SnapshotLimits, SnapshotLoader, SnapshotRef,
    prep::{
        NativeAddress, PrepBatch, PrepDirIndex, PrepDiscovery, PrepInput, PrepLoader,
        PrepReadLimits, PrepSourceKind, PrepSourceStat,
    },
};

use crate::FileSnapshotLoader;

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
        previous: &PrepDirIndex,
    ) -> Result<PrepDiscovery, PipelineError> {
        #[cfg(unix)]
        return unix::discover(root, accept, previous, self.sqlite());
        #[cfg(not(unix))]
        {
            let _ = (root, accept, previous);
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

    /// The native record at `address.key`, read from a fresh bounded snapshot
    /// with the default prep limits. A key is a snapshot record key
    /// (`composerData:<id>`), `<record key>#<JSON pointer>` for one value inside
    /// a JSON record (`document#/chatMessages/3`), or a bare JSON pointer into a
    /// JSON document (`/requests/0/response/2`). Pointers into a journal's
    /// reduced document cannot be resolved from raw journal operations.
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
        let (record_key, pointer) = match key.split_once('#') {
            Some((record, pointer)) => (record, Some(pointer)),
            None if key.starts_with('/') => match self.format {
                SnapshotFormat::JsonDocument => ("document", Some(key)),
                _ => return Err(error(PipelineErrorKind::InvalidInput)),
            },
            None => (key, None),
        };
        let snapshot =
            FileSnapshotLoader.read_snapshot(&self.source(path), SnapshotLimits::default())?;
        let record = snapshot
            .records
            .into_iter()
            .find(|record| record.key == record_key)
            .ok_or_else(|| error(PipelineErrorKind::InvalidInput))?;
        let bytes = match pointer {
            None => record.bytes,
            Some(pointer) => {
                let value: serde_json::Value = serde_json::from_slice(&record.bytes)
                    .map_err(|_| error(PipelineErrorKind::InvalidData))?;
                let found = value
                    .pointer(pointer)
                    .ok_or_else(|| error(PipelineErrorKind::InvalidInput))?;
                serde_json::to_vec(found).map_err(|_| error(PipelineErrorKind::InvalidData))?
            }
        };
        if bytes.len() > max_bytes {
            return Err(error(PipelineErrorKind::RecordLimit));
        }
        Ok(bytes)
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
        prep::{
            PrepDirEntry, PrepDirIndex, PrepDiscovery, PrepSourceKind, PrepSourceStat, PrepWalkFs,
            walk_prep_root,
        },
    };

    use super::{SQLITE_SIDECARS, error};

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

    /// The real filesystem for [`walk_prep_root`]; SQLite sidecars are part of
    /// their database, never candidates.
    struct Filesystem {
        sqlite: bool,
    }

    impl PrepWalkFs for Filesystem {
        fn dir(&self, path: &Path) -> std::io::Result<Option<(SourceIdentity, i128)>> {
            let metadata = fs::symlink_metadata(path)?;
            Ok(metadata
                .is_dir()
                .then(|| (identity(&metadata), mtime_ns(&metadata))))
        }

        fn list(&self, path: &Path) -> Option<Vec<PrepDirEntry>> {
            let entries = fs::read_dir(path).ok()?;
            Some(
                entries
                    .filter_map(|entry| self.list_entry(entry.ok()))
                    .collect(),
            )
        }

        fn source(&self, path: &Path, file: String) -> Option<PrepSourceStat> {
            let metadata = fs::symlink_metadata(path).ok()?;
            metadata
                .is_file()
                .then(|| stat_of(path, file, &metadata, self.sqlite))
        }

        fn now_ns(&self) -> i128 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos() as i128)
        }
    }

    impl Filesystem {
        /// Directories, regular files and symlinks by name; other kinds and
        /// (visible) SQLite sidecars are left out.
        fn list_entry(&self, entry: Option<fs::DirEntry>) -> Option<PrepDirEntry> {
            let Some(entry) = entry else {
                return Some(PrepDirEntry::Unreadable);
            };
            let Ok(name) = entry.file_name().into_string() else {
                return Some(PrepDirEntry::Unreadable);
            };
            let Ok(kind) = entry.file_type() else {
                return Some(PrepDirEntry::Untyped(name));
            };
            if kind.is_symlink() {
                Some(PrepDirEntry::Symlink(name))
            } else if kind.is_dir() {
                Some(PrepDirEntry::Dir(name))
            } else {
                let sidecar = self.sqlite
                    && !name.starts_with('.')
                    && SQLITE_SIDECARS.iter().any(|s| name.ends_with(s));
                (kind.is_file() && !sidecar).then_some(PrepDirEntry::File(name))
            }
        }
    }

    pub(super) fn discover(
        root: &Path,
        accept: &dyn Fn(&str) -> bool,
        previous: &PrepDirIndex,
        sqlite: bool,
    ) -> Result<PrepDiscovery, PipelineError> {
        walk_prep_root(&Filesystem { sqlite }, root, accept, previous)
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
