use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{BufRead, BufReader},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
};

use serde::{Deserialize, de::IgnoredAny};
use sha2::{Digest, Sha256};
use unisphere_core::{PipelineErrorKind, SnapshotFormat, SnapshotRecord};

use super::*;

mod sqlite;

fn error(kind: PipelineErrorKind) -> PipelineError {
    PipelineError::new(kind, None)
}

pub(super) fn read_snapshot(
    source: &SnapshotRef,
    limits: SnapshotLimits,
) -> Result<NativeSnapshot, PipelineError> {
    let mut file = SourceFile::open(source)?;
    let records = match &source.format {
        SnapshotFormat::SqliteKeyValue { table } => {
            let records = sqlite::read(source, table, limits)?;
            // A committed WAL write does not invalidate a transaction snapshot.
            file.verify(source, false)?;
            records
        }
        format => {
            let result = read_json(&mut file.file, format, limits);
            // Prefer SourceChanged even when mutation produced invalid JSON.
            file.verify(source, true)?;
            result?
        }
    };
    let snapshot = NativeSnapshot {
        source: source.clone(),
        revision: revision(&source.format, &records),
        records,
    };
    snapshot.validate(limits)?;
    Ok(snapshot)
}

struct SourceFile {
    file: File,
    metadata: Metadata,
}

impl SourceFile {
    fn open(source: &SnapshotRef) -> Result<Self, PipelineError> {
        // NOFOLLOW closes the final symlink check/open race; NONBLOCK avoids
        // waiting on a FIFO before we can reject its descriptor.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&source.path)
            .map_err(|_| error(PipelineErrorKind::Read))?;
        let metadata = file.metadata().map_err(|_| error(PipelineErrorKind::Read))?;
        if !metadata.is_file() {
            return Err(error(PipelineErrorKind::Read));
        }
        Ok(Self { file, metadata })
    }

    fn verify(&self, source: &SnapshotRef, unchanged: bool) -> Result<(), PipelineError> {
        let changed = || error(PipelineErrorKind::SourceChanged);
        let path = fs::symlink_metadata(&source.path).map_err(|_| changed())?;
        let descriptor = self.file.metadata().map_err(|_| changed())?;
        for current in [&path, &descriptor] {
            if !current.is_file()
                || current.dev() != self.metadata.dev()
                || current.ino() != self.metadata.ino()
                || (unchanged && stamp(current) != stamp(&self.metadata))
            {
                return Err(changed());
            }
        }
        Ok(())
    }
}

fn stamp(metadata: &Metadata) -> (u64, i64, i64, i64, i64) {
    (
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}

/// Checks sizes before allocating source payloads, for both file and DB readers.
struct Budget {
    limits: SnapshotLimits,
    records: usize,
    bytes: usize,
}

impl Budget {
    fn new(limits: SnapshotLimits) -> Self {
        Self { limits, records: 0, bytes: 0 }
    }

    fn value_capacity(&self, key_bytes: usize) -> Result<usize, PipelineError> {
        if self.records >= self.limits.max_records {
            return Err(error(PipelineErrorKind::BatchLimit));
        }
        if key_bytes == 0 {
            return Err(error(PipelineErrorKind::InvalidData));
        }
        self.limits.max_snapshot_bytes
            .checked_sub(self.bytes)
            .and_then(|remaining| remaining.checked_sub(key_bytes))
            .ok_or_else(|| error(PipelineErrorKind::BatchLimit))
    }

    fn charge(&mut self, key_bytes: usize, value_bytes: usize) -> Result<(), PipelineError> {
        let capacity = self.value_capacity(key_bytes)?;
        if value_bytes > self.limits.max_record_bytes {
            return Err(error(PipelineErrorKind::RecordLimit));
        }
        if value_bytes > capacity {
            return Err(error(PipelineErrorKind::BatchLimit));
        }
        // Proven by value_capacity and the value check, including usize overflow.
        self.bytes += key_bytes + value_bytes;
        self.records += 1;
        Ok(())
    }
}

fn read_json(
    file: &mut File,
    format: &SnapshotFormat,
    limits: SnapshotLimits,
) -> Result<Vec<SnapshotRecord>, PipelineError> {
    let mut reader = BufReader::with_capacity(8192.min(limits.max_snapshot_bytes), file);
    let mut budget = Budget::new(limits);
    let mut records = Vec::new();
    let journal = matches!(format, SnapshotFormat::JsonJournal);
    loop {
        if journal && reader.fill_buf().map_err(|_| error(PipelineErrorKind::Read))?.is_empty() {
            break;
        }
        let key = if journal { format!("journal:{}", records.len()) } else { "document".into() };
        let available = budget.value_capacity(key.len())?;
        let capacity = limits.max_record_bytes.min(available);
        let limit_error = if limits.max_record_bytes <= available {
            PipelineErrorKind::RecordLimit
        } else {
            PipelineErrorKind::BatchLimit
        };
        let bytes = read_value(&mut reader, journal, capacity, limit_error)?;
        validate_json(&bytes)?;
        budget.charge(key.len(), bytes.len())?;
        records.push(SnapshotRecord { key, bytes });
        if !journal {
            break;
        }
    }
    Ok(records)
}

fn read_value(
    reader: &mut impl BufRead,
    journal: bool,
    capacity: usize,
    limit_error: PipelineErrorKind,
) -> Result<Vec<u8>, PipelineError> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|_| error(PipelineErrorKind::Read))?;
        if available.is_empty() {
            return if journal {
                Err(error(PipelineErrorKind::InvalidData))
            } else {
                Ok(bytes)
            };
        }
        let newline = if journal { available.iter().position(|byte| *byte == b'\n') } else { None };
        let count = newline.unwrap_or(available.len());
        if count > capacity - bytes.len() {
            return Err(error(limit_error));
        }
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(bytes);
        }
    }
}

fn validate_json(bytes: &[u8]) -> Result<(), PipelineError> {
    // No Value tree or normalized copy: syntax/completeness only. Preserve raw
    // whitespace, CR, field order and native bytes for the pure adapter/revision.
    std::str::from_utf8(bytes).map_err(|_| error(PipelineErrorKind::InvalidData))?;
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    IgnoredAny::deserialize(&mut deserializer).map_err(|_| error(PipelineErrorKind::InvalidData))?;
    deserializer.end().map_err(|_| error(PipelineErrorKind::InvalidData))
}

fn revision(format: &SnapshotFormat, records: &[SnapshotRecord]) -> String {
    fn field(hash: &mut Sha256, bytes: &[u8]) {
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    let mut hash = Sha256::new();
    hash.update(b"unisphere.snapshot.v1\0");
    match format {
        SnapshotFormat::JsonDocument => field(&mut hash, b"json_document"),
        SnapshotFormat::JsonJournal => field(&mut hash, b"json_journal"),
        SnapshotFormat::SqliteKeyValue { table } => {
            field(&mut hash, b"sqlite_key_value");
            field(&mut hash, table.as_bytes());
        }
    }
    hash.update((records.len() as u64).to_be_bytes());
    for record in records {
        field(&mut hash, record.key.as_bytes());
        field(&mut hash, &record.bytes);
    }
    format!("sha256:{:x}", hash.finalize())
}

#[cfg(test)]
mod tests {
    use std::{io::Write, os::unix::fs::symlink};
    use tempfile::TempDir;
    use super::*;

    #[test]
    fn file_snapshot_rejects_changes_between_read_and_publication() {
        for mutation in ["append", "truncate", "rewrite", "replace", "unlink", "symlink"] {
            let directory = TempDir::new().unwrap();
            let path = directory.path().join("source.json");
            fs::write(&path, b"{}").unwrap();
            let source = SnapshotRef { path: path.clone(), format: SnapshotFormat::JsonDocument, session_id: None };
            let mut opened = SourceFile::open(&source).unwrap();
            let original_time = opened.metadata.modified().unwrap();
            let records = read_json(&mut opened.file, &source.format, SnapshotLimits::default()).unwrap();
            assert_eq!(records[0].bytes, b"{}");
            match mutation {
                "append" => OpenOptions::new().append(true).open(&path).unwrap().write_all(b" ").unwrap(),
                "truncate" => File::create(&path).unwrap().set_len(0).unwrap(),
                "rewrite" => {
                    fs::write(&path, b"[]").unwrap();
                    // Restoring mtime must not defeat ctime-based mutation detection.
                    File::options().write(true).open(&path).unwrap().set_modified(original_time).unwrap();
                }
                "replace" => {
                    let replacement = directory.path().join("replacement");
                    fs::write(&replacement, b"{}").unwrap();
                    fs::rename(replacement, &path).unwrap();
                }
                "unlink" => fs::remove_file(&path).unwrap(),
                "symlink" => {
                    let replacement = directory.path().join("replacement");
                    fs::write(&replacement, b"{}").unwrap();
                    fs::remove_file(&path).unwrap();
                    symlink(replacement, &path).unwrap();
                }
                _ => unreachable!(),
            }
            assert_eq!(opened.verify(&source, true).unwrap_err().kind(), PipelineErrorKind::SourceChanged, "{mutation}");
        }
    }
}
