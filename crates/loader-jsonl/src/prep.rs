//! Recursive discovery, stat and prefix anchors for incremental prep.
//! Symlinks are never followed; hidden entries are skipped like a `**` glob.

use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::Path,
};

use sha2::{Digest as _, Sha256};
use unisphere_core::{PipelineError, PipelineErrorKind, SourceIdentity, prep::PrepSourceStat};

/// Upper bound on discovered candidates; exceeding it fails instead of truncating.
pub const MAX_PREP_SOURCES: usize = 100_000;
const ANCHOR_BYTES: u64 = 4096;

pub(crate) fn discover(root: &Path) -> Result<Vec<PrepSourceStat>, PipelineError> {
    let read = |_| PipelineError::new(PipelineErrorKind::Read, None);
    if !root.is_absolute() || !fs::symlink_metadata(root).map_err(read)?.is_dir() {
        return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
    }
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        // The root must be readable; a subdirectory vanishing mid-walk is skipped.
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(failure) if dir == root => return Err(read(failure)),
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_str().is_none_or(|name| name.starts_with('.')) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
                let Ok(metadata) = fs::symlink_metadata(&path) else {
                    continue;
                };
                let Some(file) = path
                    .strip_prefix(root)
                    .ok()
                    .and_then(Path::to_str)
                    .map(str::to_owned)
                else {
                    continue;
                };
                if found.len() == MAX_PREP_SOURCES {
                    return Err(PipelineError::new(PipelineErrorKind::ListingLimit, None));
                }
                found.push(PrepSourceStat {
                    path,
                    file,
                    identity: SourceIdentity::Unix {
                        device: metadata.dev(),
                        inode: metadata.ino(),
                    },
                    size: metadata.len(),
                    mtime_ns: i128::from(metadata.mtime()) * 1_000_000_000
                        + i128::from(metadata.mtime_nsec()),
                });
            }
        }
    }
    // Byte order of the relative path, like a sorted glob; also the table order.
    found.sort_unstable_by(|a, b| a.file.cmp(&b.file));
    Ok(found)
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
