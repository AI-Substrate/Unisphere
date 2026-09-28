//! Explicit, nonrecursive session discovery and bounded native LF framing.
//! Filesystem support is Unix-only. Ancestor directories are caller-trusted;
//! final symlinks are never followed. No content interpretation or cursor storage.
#![forbid(unsafe_code)]

use sha2::{Digest as _, Sha256};
use unisphere_core::{
    LoadedBatch, PipelineError, ReadCursor, ReadLimits, SessionLoader, SessionRef, SourceScope,
    query::{NativeLocator, NativeRecord as QueryNativeRecord},
};

/// Stateless filesystem implementation of [`SessionLoader`].
#[derive(Debug, Default, Clone, Copy)]
pub struct FileSessionLoader;

impl FileSessionLoader {
    pub const fn new() -> Self {
        Self
    }
}

/// One complete, bounded JSONL generation for immutable query inspection.
///
/// Unlike [`ReadCursor`], `revision` identifies every observed physical byte,
/// including blank records and LF framing. It is not an append checkpoint.
#[derive(Clone, PartialEq)]
pub struct QueryJsonlSnapshot {
    pub source: SessionRef,
    pub revision: String,
    pub input_bytes: usize,
    pub records: Vec<QueryNativeRecord>,
}

impl FileSessionLoader {
    /// Read exactly one current JSONL generation. Capacity stops and incomplete
    /// tails fail instead of publishing a partial query view.
    pub fn read_query_snapshot(
        &self,
        session: &SessionRef,
        limits: ReadLimits,
    ) -> Result<QueryJsonlSnapshot, PipelineError> {
        #[cfg(unix)]
        {
            unix::read_query_snapshot(session, limits)
        }
        #[cfg(not(unix))]
        {
            let _ = (session, limits);
            Err(PipelineError::new(
                unisphere_core::PipelineErrorKind::Unsupported,
                None,
            ))
        }
    }
}

impl SessionLoader for FileSessionLoader {
    fn list_sessions(&self, scope: &SourceScope) -> Result<Vec<SessionRef>, PipelineError> {
        #[cfg(unix)]
        {
            unix::list_sessions(scope)
        }
        #[cfg(not(unix))]
        {
            let _ = scope;
            Err(PipelineError::new(
                unisphere_core::PipelineErrorKind::Unsupported,
                None,
            ))
        }
    }

    fn read_batch(
        &self,
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
    ) -> Result<LoadedBatch, PipelineError> {
        #[cfg(unix)]
        {
            unix::read_batch(session, cursor, limits)
        }
        #[cfg(not(unix))]
        {
            let _ = (session, cursor, limits);
            Err(PipelineError::new(
                unisphere_core::PipelineErrorKind::Unsupported,
                None,
            ))
        }
    }
}

#[cfg(unix)]
mod unix {
    use std::{
        ffi::OsStr,
        fs::{self, File, Metadata, OpenOptions},
        io::{self, BufRead, BufReader, Read, Seek, SeekFrom},
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };

    use super::*;
    use unisphere_core::{PipelineErrorKind, SourceIdentity};

    fn error(kind: PipelineErrorKind, offset: Option<u64>) -> PipelineError {
        PipelineError::new(kind, offset)
    }

    pub(super) fn list_sessions(scope: &SourceScope) -> Result<Vec<SessionRef>, PipelineError> {
        scope.validate()?;
        let read_error = |_| error(PipelineErrorKind::Read, None);
        // Ancestors are trusted, but the selected leaf must itself be a directory.
        if !fs::symlink_metadata(&scope.root)
            .map_err(read_error)?
            .is_dir()
        {
            return Err(error(PipelineErrorKind::Read, None));
        }
        let mut sessions = Vec::new();
        for entry in fs::read_dir(&scope.root).map_err(read_error)? {
            let entry = entry.map_err(read_error)?;
            if !entry.file_type().map_err(read_error)?.is_file()
                || entry.path().extension() != Some(OsStr::new("jsonl"))
            {
                continue;
            }
            let session = SessionRef { path: entry.path() };
            session.validate()?;
            if sessions.len() == scope.max_sessions {
                return Err(error(PipelineErrorKind::ListingLimit, None));
            }
            sessions.push(session);
        }
        sessions.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        Ok(sessions)
    }

    fn identity(metadata: &Metadata) -> SourceIdentity {
        SourceIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }

    pub(super) fn read_batch(
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
    ) -> Result<LoadedBatch, PipelineError> {
        read_framed(session, cursor, limits, None, false)
    }

    pub(super) fn read_query_snapshot(
        session: &SessionRef,
        limits: ReadLimits,
    ) -> Result<QueryJsonlSnapshot, PipelineError> {
        let mut digest = Sha256::new();
        digest.update(b"unisphere.query.jsonl.v1\0");
        let batch = read_framed(session, None, limits, Some(&mut digest), true)?;
        let input_bytes = usize::try_from(batch.next_cursor.offset)
            .map_err(|_| error(PipelineErrorKind::BatchLimit, None))?;
        let records = batch
            .records
            .into_iter()
            .map(|record| QueryNativeRecord {
                locator: NativeLocator::Jsonl {
                    offset: record.offset,
                },
                bytes: record.bytes,
            })
            .collect();
        Ok(QueryJsonlSnapshot {
            source: batch.source,
            revision: format!("sha256:{:x}", digest.finalize()),
            input_bytes,
            records,
        })
    }

    fn read_framed(
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
        mut revision: Option<&mut Sha256>,
        require_complete: bool,
    ) -> Result<LoadedBatch, PipelineError> {
        limits.validate()?;
        session.validate()?;
        let offset = cursor.map_or(0, |cursor| cursor.offset);
        let changed = || error(PipelineErrorKind::SourceChanged, Some(offset));
        let read_error = |_| error(PipelineErrorKind::Read, Some(offset));
        if let Some(cursor) = cursor {
            if cursor.source != session.path {
                return Err(changed());
            }
            if cursor.identity == SourceIdentity::Unavailable {
                return Err(error(PipelineErrorKind::Unsupported, Some(offset)));
            }
        }
        // NOFOLLOW closes the check/open symlink race. NONBLOCK prevents a FIFO or
        // other special leaf from blocking before its descriptor is inspected.
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&session.path)
            .map_err(|failure| {
                if cursor.is_some()
                    && (failure.kind() == io::ErrorKind::NotFound
                        || failure.raw_os_error() == Some(libc::ELOOP))
                {
                    changed()
                } else {
                    read_error(failure)
                }
            })?;
        let metadata = file.metadata().map_err(read_error)?;
        if !metadata.is_file() {
            return Err(error(PipelineErrorKind::Read, Some(offset)));
        }
        let source_identity = identity(&metadata);
        let observed_end = metadata.len();
        if offset > observed_end || cursor.is_some_and(|cursor| cursor.identity != source_identity)
        {
            return Err(changed());
        }
        if offset != 0 {
            file.seek(SeekFrom::Start(offset - 1)).map_err(read_error)?;
            let mut previous = [0];
            file.read_exact(&mut previous).map_err(|failure| {
                if failure.kind() == io::ErrorKind::UnexpectedEof {
                    changed()
                } else {
                    read_error(failure)
                }
            })?;
            if previous[0] != b'\n' {
                return Err(changed());
            }
        }
        file.seek(SeekFrom::Start(offset)).map_err(read_error)?;
        // Take freezes the observed boundary: this call never chases an appender.
        let mut reader = BufReader::with_capacity(
            limits.max_record_bytes.min(8192),
            file.take(observed_end - offset),
        );
        let mut records = Vec::new();
        let mut next = offset;
        let mut consumed = 0usize;
        let mut physical_records = 0usize;
        let mut incomplete_tail = false;
        let mut more = false;
        while next < observed_end {
            if physical_records == limits.max_records || consumed == limits.max_batch_bytes {
                if require_complete {
                    return Err(error(PipelineErrorKind::BatchLimit, Some(next)));
                }
                more = true;
                break;
            }
            let capacity = limits
                .max_record_bytes
                .min(limits.max_batch_bytes - consumed);
            let mut bytes = Vec::new();
            match frame(&mut reader, &mut bytes, capacity).map_err(read_error)? {
                Frame::Complete => {
                    consumed = consumed
                        .checked_add(bytes.len())
                        .ok_or_else(|| error(PipelineErrorKind::BatchLimit, Some(next)))?;
                    let length = u64::try_from(bytes.len())
                        .map_err(|_| error(PipelineErrorKind::RecordLimit, Some(next)))?;
                    let end = next.checked_add(length).ok_or_else(changed)?;
                    physical_records += 1;
                    if let Some(digest) = revision.as_mut() {
                        digest.update(&bytes);
                    }
                    if !bytes.iter().all(u8::is_ascii_whitespace) {
                        // Only LF is removed; native CR remains.
                        bytes.pop();
                        records.push(unisphere_core::NativeRecord {
                            offset: next,
                            bytes,
                        });
                    }
                    next = end;
                }
                Frame::Incomplete if require_complete => {
                    return Err(error(PipelineErrorKind::InvalidData, Some(next)));
                }
                Frame::Incomplete => {
                    incomplete_tail = true;
                    break;
                }
                Frame::Capacity if capacity == limits.max_record_bytes => {
                    return Err(error(PipelineErrorKind::RecordLimit, Some(next)));
                }
                Frame::Capacity if require_complete => {
                    return Err(error(PipelineErrorKind::BatchLimit, Some(next)));
                }
                Frame::Capacity => {
                    more = true;
                    break;
                }
            }
        }
        // Rotation/removal and observable truncation invalidate the entire call,
        // not merely its checkpoint. Same-inode rewriting/regrowth is unsupported.
        verify_source(
            reader.get_ref().get_ref(),
            session,
            &source_identity,
            observed_end,
        )
        .map_err(|_| changed())?;
        Ok(LoadedBatch {
            source: session.clone(),
            records,
            next_cursor: ReadCursor {
                source: session.path.clone(),
                identity: source_identity,
                offset: next,
            },
            more,
            incomplete_tail,
        })
    }

    enum Frame {
        Complete,
        Incomplete,
        Capacity,
    }

    fn frame(reader: &mut impl BufRead, bytes: &mut Vec<u8>, capacity: usize) -> io::Result<Frame> {
        loop {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok(Frame::Incomplete);
            }
            if bytes.len() == capacity {
                return Ok(Frame::Capacity);
            }
            let available = &available[..available.len().min(capacity - bytes.len())];
            let newline = available.iter().position(|byte| *byte == b'\n');
            let count = newline.map_or(available.len(), |index| index + 1);
            bytes.extend_from_slice(&available[..count]);
            reader.consume(count);
            if newline.is_some() {
                return Ok(Frame::Complete);
            }
        }
    }

    fn verify_source(
        file: &File,
        session: &SessionRef,
        expected: &SourceIdentity,
        observed_end: u64,
    ) -> io::Result<()> {
        let current = fs::symlink_metadata(&session.path)?;
        if !current.is_file()
            || identity(&current) != *expected
            || current.len() < observed_end
            || file.metadata()?.len() < observed_end
        {
            return Err(io::Error::other("source changed"));
        }
        Ok(())
    }
}

#[cfg(unix)]
mod prep;
#[cfg(unix)]
pub use prep::MAX_PREP_SOURCES;

impl unisphere_core::prep::PrepLoader for FileSessionLoader {
    fn discover(
        &self,
        root: &std::path::Path,
    ) -> Result<Vec<unisphere_core::prep::PrepSourceStat>, PipelineError> {
        #[cfg(unix)]
        {
            prep::discover(root)
        }
        #[cfg(not(unix))]
        {
            let _ = root;
            Err(PipelineError::new(
                unisphere_core::PipelineErrorKind::Unsupported,
                None,
            ))
        }
    }

    fn read_batch(
        &self,
        session: &SessionRef,
        cursor: Option<&ReadCursor>,
        limits: ReadLimits,
    ) -> Result<LoadedBatch, PipelineError> {
        SessionLoader::read_batch(self, session, cursor, limits)
    }

    fn anchor(&self, path: &std::path::Path, offset: u64) -> Result<String, PipelineError> {
        #[cfg(unix)]
        {
            prep::anchor(path, offset)
        }
        #[cfg(not(unix))]
        {
            let _ = (path, offset);
            Err(PipelineError::new(
                unisphere_core::PipelineErrorKind::Unsupported,
                None,
            ))
        }
    }
}
