//! Recursive discovery, stat, prefix anchors and single-record reads for
//! incremental prep. Native files are opened read-only; symlinks are never
//! followed and hidden entries are skipped, both counted.

use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::Path,
};

use sha2::{Digest as _, Sha256};
use unisphere_core::{
    PipelineError, PipelineErrorKind, SourceIdentity,
    prep::{NativeAddress, PrepDiscovery, PrepSourceKind, PrepSourceStat},
};

/// Upper bound on discovered candidates; exceeding it fails instead of truncating.
pub const MAX_PREP_SOURCES: usize = 100_000;
const ANCHOR_BYTES: u64 = 4096;

fn stat_of(path: &Path, file: String, metadata: &fs::Metadata) -> PrepSourceStat {
    PrepSourceStat {
        path: path.to_path_buf(),
        file,
        kind: PrepSourceKind::Append,
        identity: SourceIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
        size: metadata.len(),
        mtime_ns: i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec()),
    }
}

pub(crate) fn discover(
    root: &Path,
    accept: &dyn Fn(&str) -> bool,
) -> Result<PrepDiscovery, PipelineError> {
    let read = |_| PipelineError::new(PipelineErrorKind::Read, None);
    if !root.is_absolute() || !fs::symlink_metadata(root).map_err(read)?.is_dir() {
        return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
    }
    let mut found = PrepDiscovery::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        // The root must be readable; a subdirectory vanishing mid-walk is counted.
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
            if name.to_str().is_none_or(|name| name.starts_with('.')) {
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
                    return Err(PipelineError::new(PipelineErrorKind::ListingLimit, None));
                }
                found.sources.push(stat_of(&path, file, &metadata));
            }
        }
    }
    // Byte order of the relative path, like a sorted glob; also the table order.
    found.sources.sort_unstable_by(|a, b| a.file.cmp(&b.file));
    Ok(found)
}

pub(crate) fn stat(root: &Path, path: &Path) -> Result<PrepSourceStat, PipelineError> {
    let file = path
        .strip_prefix(root)
        .ok()
        .and_then(Path::to_str)
        .map(str::to_owned)
        .ok_or_else(|| PipelineError::new(PipelineErrorKind::InvalidInput, None))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| PipelineError::new(PipelineErrorKind::Read, None))?;
    if !metadata.is_file() {
        return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
    }
    Ok(stat_of(path, file, &metadata))
}

/// Digest of the first and the last (up to) 4 KiB of the prefix `[0, offset)`.
pub(crate) fn anchor(path: &Path, offset: u64) -> Result<String, PipelineError> {
    let read = |_| PipelineError::new(PipelineErrorKind::Read, Some(offset));
    let mut file = File::open(path).map_err(read)?;
    let mut digest = Sha256::new();
    digest.update(b"unisphere.prep.anchor.v1\0");
    digest.update(offset.to_le_bytes());
    let head_len = offset.min(ANCHOR_BYTES);
    let tail_start = offset.saturating_sub(ANCHOR_BYTES).max(head_len);
    let mut buffer = vec![0; usize::try_from(ANCHOR_BYTES).unwrap_or(4096)];
    for (start, len) in [(0, head_len), (tail_start, offset - tail_start)] {
        if len == 0 {
            continue;
        }
        file.seek(SeekFrom::Start(start)).map_err(read)?;
        let slice = &mut buffer[..usize::try_from(len).unwrap_or(0)];
        file.read_exact(slice)
            .map_err(|_| PipelineError::new(PipelineErrorKind::SourceChanged, Some(offset)))?;
        digest.update(&*slice);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

/// The complete line starting at `address.offset` (which must follow an LF or
/// be 0), without its LF, at most `max_bytes`.
pub(crate) fn record_at(
    path: &Path,
    address: &NativeAddress,
    max_bytes: usize,
) -> Result<Vec<u8>, PipelineError> {
    let invalid = || PipelineError::new(PipelineErrorKind::InvalidInput, address.offset);
    let offset = address.offset.ok_or_else(invalid)?;
    let read = |_| PipelineError::new(PipelineErrorKind::Read, Some(offset));
    let mut file = File::open(path).map_err(read)?;
    if offset > 0 {
        let mut previous = [0u8; 1];
        file.seek(SeekFrom::Start(offset - 1)).map_err(read)?;
        file.read_exact(&mut previous).map_err(read)?;
        if previous[0] != b'\n' {
            return Err(invalid());
        }
    }
    file.seek(SeekFrom::Start(offset)).map_err(read)?;
    let limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).map_err(read)?;
    let end = bytes.iter().position(|b| *b == b'\n').ok_or_else(|| {
        if bytes.len() > max_bytes {
            PipelineError::new(PipelineErrorKind::RecordLimit, Some(offset))
        } else {
            invalid()
        }
    })?;
    bytes.truncate(end);
    Ok(bytes)
}
