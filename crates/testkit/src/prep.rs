//! In-memory prep fakes: a store, a loader and a recording fold that let the
//! engine, the CLI and external consumers prove prep behaviour without Parquet
//! or native files. Deterministic; no clock, no filesystem.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde_json::json;
use unisphere_core::{
    NativeRecord, NativeSnapshot, PipelineError, PipelineErrorKind, ReadCursor, SnapshotFormat,
    SnapshotRecord, SnapshotRef, SourceIdentity,
    prep::{
        CacheWriteBasis, CallSighting, NativeAddress, PREP_CHECKPOINT_FORMAT, PrepBatch,
        PrepCallRow, PrepCheckpoint, PrepCommit, PrepCompactReport, PrepDiscovery, PrepFold,
        PrepFoldSession, PrepInput, PrepLoaded, PrepLoader, PrepOptions, PrepReadLimits, PrepRows,
        PrepSkipCounts, PrepSourceKind, PrepSourceMeta, PrepSourceStat, PrepState, PrepStore,
        SessionFacts,
    },
};

fn error(kind: PipelineErrorKind) -> PipelineError {
    PipelineError::new(kind, None)
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StoreInner {
    state: Option<PrepState>,
    runs: Vec<PrepRows>,
    fail_next_commit: bool,
    commits: u64,
}

/// In-memory [`PrepStore`]; commits are all-or-nothing.
#[derive(Default)]
pub struct MemoryPrepStore {
    inner: Mutex<StoreInner>,
}

impl MemoryPrepStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Make the next `commit` fail with `Write` before anything is stored.
    pub fn fail_next_commit(&self) {
        self.lock().fail_next_commit = true;
    }

    /// Successful commits so far.
    pub fn commits(&self) -> u64 {
        self.lock().commits
    }

    /// Every committed row, in commit order.
    pub fn all_rows(&self) -> PrepRows {
        let mut rows = PrepRows::default();
        for run in &self.lock().runs {
            rows.extend(run.clone());
        }
        rows
    }

    /// Rows of each source's current generation only (the canonical filter),
    /// with call sightings merged per (source, generation, msg_id, request_id)
    /// by per-field maximum and the last non-null stop reason.
    pub fn canonical(&self) -> PrepRows {
        let inner = self.lock();
        let Some(state) = inner.state.as_ref() else {
            return PrepRows::default();
        };
        let current = |source: &str, generation: u32| {
            state
                .sources
                .get(source)
                .is_some_and(|s| s.generation == generation)
        };
        let mut rows = PrepRows::default();
        let mut calls: Vec<PrepCallRow> = Vec::new();
        for run in &inner.runs {
            for call in &run.calls {
                if !current(&call.source, call.generation) {
                    continue;
                }
                let same = |c: &&mut PrepCallRow| {
                    c.source == call.source
                        && c.generation == call.generation
                        && c.msg_id == call.msg_id
                        && c.request_id == call.request_id
                };
                match calls.iter_mut().find(same) {
                    Some(existing) => merge_call(existing, call),
                    None => calls.push(call.clone()),
                }
            }
            rows.turns.extend(
                run.turns
                    .iter()
                    .filter(|r| current(&r.source, r.generation))
                    .cloned(),
            );
            rows.triggers.extend(
                run.triggers
                    .iter()
                    .filter(|r| current(&r.source, r.generation))
                    .cloned(),
            );
            rows.events.extend(
                run.events
                    .iter()
                    .filter(|r| current(&r.source, r.generation))
                    .cloned(),
            );
            rows.tool_uses.extend(
                run.tool_uses
                    .iter()
                    .filter(|r| current(&r.source, r.generation))
                    .cloned(),
            );
        }
        rows.calls = calls;
        rows
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StoreInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

fn merge_call(existing: &mut PrepCallRow, update: &PrepCallRow) {
    let max = |a: Option<i64>, b: Option<i64>| match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    existing.input = max(existing.input, update.input);
    existing.cw_1h = max(existing.cw_1h, update.cw_1h);
    existing.cw_5m = max(existing.cw_5m, update.cw_5m);
    existing.cache_read = max(existing.cache_read, update.cache_read);
    existing.output = max(existing.output, update.output);
    if update.stop_reason.is_some() {
        existing.stop_reason.clone_from(&update.stop_reason);
    }
    existing.records += update.records;
}

impl PrepStore for MemoryPrepStore {
    fn state(&self) -> Result<Option<PrepState>, PipelineError> {
        Ok(self.lock().state.clone())
    }

    fn load(&self) -> Result<PrepLoaded, PipelineError> {
        Ok(PrepLoaded {
            state: self.lock().state.clone(),
            orphans_removed: 0,
        })
    }

    fn commit(&self, rows: &PrepRows, state: &PrepState) -> Result<PrepCommit, PipelineError> {
        let mut inner = self.lock();
        if std::mem::take(&mut inner.fail_next_commit) {
            return Err(error(PipelineErrorKind::Write));
        }
        inner.runs.push(rows.clone());
        inner.state = Some(state.clone());
        inner.commits += 1;
        Ok(PrepCommit {
            parts_written: Vec::new(),
            bytes_written: 0,
            snapshot_rows: state.sources.len() as u64,
            orphans_removed: 0,
        })
    }

    fn compact(&self) -> Result<PrepCompactReport, PipelineError> {
        let before = self.all_rows().counts();
        let canonical = self.canonical();
        let after = canonical.counts();
        let mut inner = self.lock();
        let parts_before = inner.runs.len() as u64;
        inner.runs = vec![canonical];
        Ok(PrepCompactReport {
            target: PathBuf::from("memory"),
            parts_before,
            parts_after: 1,
            rows_before: before,
            rows_after: after,
            bytes_before: 0,
            bytes_after: 0,
        })
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MemorySource {
    bytes: Vec<u8>,
    snapshot: Vec<SnapshotRecord>,
    revision: u64,
    inode: u64,
    mtime_ns: i128,
}

#[derive(Default)]
struct LoaderInner {
    sources: BTreeMap<String, MemorySource>,
    skipped: PrepSkipCounts,
    next_inode: u64,
    clock: i128,
    reads: u64,
    unreadable: Vec<String>,
}

/// In-memory [`PrepLoader`] over named sources below one root. Append sources
/// support append (incl. partial lines), truncate, rotate and same-identity
/// rewrite; snapshot sources support revisions.
pub struct MemoryLoader {
    kind: PrepSourceKind,
    root: PathBuf,
    inner: Mutex<LoaderInner>,
}

impl MemoryLoader {
    pub fn new(kind: PrepSourceKind, root: impl Into<PathBuf>) -> Self {
        Self {
            kind,
            root: root.into(),
            inner: Mutex::new(LoaderInner {
                next_inode: 1,
                ..LoaderInner::default()
            }),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Append bytes (possibly without a trailing LF); creates the source if absent.
    pub fn append(&self, file: &str, bytes: &[u8]) {
        let mut inner = self.lock();
        inner.clock += 1;
        let clock = inner.clock;
        if !inner.sources.contains_key(file) {
            let inode = inner.next_inode;
            inner.next_inode += 1;
            inner.sources.insert(
                file.to_owned(),
                MemorySource {
                    bytes: Vec::new(),
                    snapshot: Vec::new(),
                    revision: 0,
                    inode,
                    mtime_ns: clock,
                },
            );
        }
        if let Some(source) = inner.sources.get_mut(file) {
            source.bytes.extend_from_slice(bytes);
            source.mtime_ns = clock;
        }
    }

    /// New identity with the given content (rename-over / rotation).
    pub fn rotate(&self, file: &str, bytes: &[u8]) {
        let mut inner = self.lock();
        inner.clock += 1;
        let (clock, inode) = (inner.clock, inner.next_inode);
        inner.next_inode += 1;
        inner.sources.insert(
            file.to_owned(),
            MemorySource {
                bytes: bytes.to_vec(),
                snapshot: Vec::new(),
                revision: 0,
                inode,
                mtime_ns: clock,
            },
        );
    }

    /// Same identity, new content; `touch` controls whether mtime advances.
    pub fn rewrite(&self, file: &str, bytes: &[u8], touch: bool) {
        let mut inner = self.lock();
        inner.clock += 1;
        let clock = inner.clock;
        if let Some(source) = inner.sources.get_mut(file) {
            source.bytes = bytes.to_vec();
            if touch {
                source.mtime_ns = clock;
            }
        }
    }

    pub fn truncate(&self, file: &str, len: usize) {
        let mut inner = self.lock();
        inner.clock += 1;
        let clock = inner.clock;
        if let Some(source) = inner.sources.get_mut(file) {
            source.bytes.truncate(len);
            source.mtime_ns = clock;
        }
    }

    pub fn remove(&self, file: &str) {
        self.lock().sources.remove(file);
    }

    /// Replace a snapshot source's records; the revision advances only when they differ.
    pub fn put_snapshot(&self, file: &str, records: &[(&str, &[u8])]) {
        let mut inner = self.lock();
        inner.clock += 1;
        let (clock, inode) = (inner.clock, inner.next_inode);
        let records: Vec<SnapshotRecord> = records
            .iter()
            .map(|(key, bytes)| SnapshotRecord {
                key: (*key).to_owned(),
                bytes: bytes.to_vec(),
            })
            .collect();
        match inner.sources.get_mut(file) {
            Some(source) => {
                if source.snapshot != records {
                    source.revision += 1;
                    source.snapshot = records;
                }
                source.mtime_ns = clock;
            }
            None => {
                inner.next_inode += 1;
                inner.sources.insert(
                    file.to_owned(),
                    MemorySource {
                        bytes: Vec::new(),
                        snapshot: records,
                        revision: 1,
                        inode,
                        mtime_ns: clock,
                    },
                );
            }
        }
    }

    /// Counts discovery reports as skipped.
    pub fn set_skipped(&self, skipped: PrepSkipCounts) {
        self.lock().skipped = skipped;
    }

    /// Make reads of `file` fail with `Read`.
    pub fn set_unreadable(&self, file: &str, unreadable: bool) {
        let mut inner = self.lock();
        inner.unreadable.retain(|f| f != file);
        if unreadable {
            inner.unreadable.push(file.to_owned());
        }
    }

    /// Body reads served so far (`read` calls).
    pub fn reads(&self) -> u64 {
        self.lock().reads
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LoaderInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn stat_of(&self, file: &str, source: &MemorySource) -> PrepSourceStat {
        PrepSourceStat {
            path: self.root.join(file),
            file: file.to_owned(),
            kind: self.kind,
            identity: SourceIdentity::Unix {
                device: 1,
                inode: source.inode,
            },
            size: match self.kind {
                PrepSourceKind::Append => source.bytes.len() as u64,
                PrepSourceKind::Snapshot => source
                    .snapshot
                    .iter()
                    .map(|r| (r.key.len() + r.bytes.len()) as u64)
                    .sum(),
            },
            mtime_ns: source.mtime_ns,
        }
    }

    fn file_of(&self, path: &Path) -> Result<String, PipelineError> {
        path.strip_prefix(&self.root)
            .ok()
            .and_then(Path::to_str)
            .map(str::to_owned)
            .ok_or_else(|| error(PipelineErrorKind::InvalidInput))
    }
}

impl PrepLoader for MemoryLoader {
    fn kind(&self) -> PrepSourceKind {
        self.kind
    }

    fn discover(
        &self,
        root: &Path,
        accept: &dyn Fn(&str) -> bool,
    ) -> Result<PrepDiscovery, PipelineError> {
        if root != self.root {
            return Err(error(PipelineErrorKind::Read));
        }
        let inner = self.lock();
        Ok(PrepDiscovery {
            sources: inner
                .sources
                .iter()
                .filter(|(file, _)| accept(file))
                .map(|(file, source)| self.stat_of(file, source))
                .collect(),
            skipped: inner.skipped,
        })
    }

    fn stat(&self, root: &Path, path: &Path) -> Result<PrepSourceStat, PipelineError> {
        if root != self.root {
            return Err(error(PipelineErrorKind::InvalidInput));
        }
        let file = self.file_of(path)?;
        let inner = self.lock();
        let source = inner
            .sources
            .get(&file)
            .ok_or_else(|| error(PipelineErrorKind::Read))?;
        Ok(self.stat_of(&file, source))
    }

    fn read(
        &self,
        stat: &PrepSourceStat,
        from: Option<&ReadCursor>,
        limits: PrepReadLimits,
    ) -> Result<PrepBatch, PipelineError> {
        let mut inner = self.lock();
        inner.reads += 1;
        if inner.unreadable.contains(&stat.file) {
            return Err(error(PipelineErrorKind::Read));
        }
        let source = inner
            .sources
            .get(&stat.file)
            .cloned()
            .ok_or_else(|| error(PipelineErrorKind::Read))?;
        let identity = SourceIdentity::Unix {
            device: 1,
            inode: source.inode,
        };
        match self.kind {
            PrepSourceKind::Snapshot => {
                if from.is_some() {
                    return Err(error(PipelineErrorKind::InvalidInput));
                }
                let bytes_read = source
                    .snapshot
                    .iter()
                    .map(|r| (r.key.len() + r.bytes.len()) as u64)
                    .sum();
                Ok(PrepBatch {
                    input: PrepInput::Snapshot(NativeSnapshot {
                        source: SnapshotRef {
                            path: stat.path.clone(),
                            format: SnapshotFormat::JsonDocument,
                            session_id: None,
                        },
                        revision: format!("memory-rev-{}", source.revision),
                        records: source.snapshot,
                    }),
                    next_cursor: None,
                    more: false,
                    incomplete_tail: false,
                    bytes_read,
                })
            }
            PrepSourceKind::Append => {
                let start = match from {
                    Some(cursor) if cursor.identity != identity => {
                        return Err(error(PipelineErrorKind::SourceChanged));
                    }
                    Some(cursor) => usize::try_from(cursor.offset).unwrap_or(usize::MAX),
                    None => 0,
                };
                if start > source.bytes.len() {
                    return Err(error(PipelineErrorKind::SourceChanged));
                }
                let budget = limits.read.max_batch_bytes.max(1);
                let available = &source.bytes[start..];
                let mut records = Vec::new();
                let mut consumed = 0usize;
                while let Some(end) = available[consumed..].iter().position(|b| *b == b'\n') {
                    let line = &available[consumed..consumed + end];
                    if line.len() > limits.read.max_record_bytes {
                        return Err(error(PipelineErrorKind::RecordLimit));
                    }
                    if !line.is_empty() {
                        records.push(NativeRecord {
                            offset: (start + consumed) as u64,
                            bytes: line.to_vec(),
                        });
                    }
                    consumed += end + 1;
                    if consumed >= budget || records.len() >= limits.read.max_records {
                        break;
                    }
                }
                let rest = &available[consumed..];
                let more = rest.contains(&b'\n');
                Ok(PrepBatch {
                    input: PrepInput::Records(records),
                    next_cursor: Some(ReadCursor {
                        source: stat.path.clone(),
                        identity,
                        offset: (start + consumed) as u64,
                    }),
                    more,
                    incomplete_tail: !more && !rest.is_empty(),
                    bytes_read: consumed as u64,
                })
            }
        }
    }

    fn anchor(&self, stat: &PrepSourceStat, offset: u64) -> Result<String, PipelineError> {
        if self.kind == PrepSourceKind::Snapshot {
            return Err(error(PipelineErrorKind::Unsupported));
        }
        let inner = self.lock();
        let source = inner
            .sources
            .get(&stat.file)
            .ok_or_else(|| error(PipelineErrorKind::Read))?;
        let end = usize::try_from(offset).unwrap_or(usize::MAX);
        let prefix = source
            .bytes
            .get(..end)
            .ok_or_else(|| error(PipelineErrorKind::SourceChanged))?;
        Ok(format!("memory:{:016x}", fnv(prefix)))
    }

    fn record_at(
        &self,
        path: &Path,
        address: &NativeAddress,
        max_bytes: usize,
    ) -> Result<Vec<u8>, PipelineError> {
        let file = self.file_of(path)?;
        let inner = self.lock();
        let source = inner
            .sources
            .get(&file)
            .ok_or_else(|| error(PipelineErrorKind::Read))?;
        let bytes = match (self.kind, address.offset, address.key.as_deref()) {
            (PrepSourceKind::Append, Some(offset), _) => {
                let start = usize::try_from(offset).unwrap_or(usize::MAX);
                if start > 0 && source.bytes.get(start - 1) != Some(&b'\n') {
                    return Err(error(PipelineErrorKind::InvalidInput));
                }
                let rest = source
                    .bytes
                    .get(start..)
                    .ok_or_else(|| error(PipelineErrorKind::InvalidInput))?;
                let end = rest
                    .iter()
                    .position(|b| *b == b'\n')
                    .ok_or_else(|| error(PipelineErrorKind::InvalidInput))?;
                rest[..end].to_vec()
            }
            (PrepSourceKind::Snapshot, _, Some(key)) => source
                .snapshot
                .iter()
                .find(|r| r.key == key)
                .map(|r| r.bytes.clone())
                .ok_or_else(|| error(PipelineErrorKind::InvalidInput))?,
            _ => return Err(error(PipelineErrorKind::InvalidInput)),
        };
        if bytes.len() > max_bytes {
            return Err(error(PipelineErrorKind::RecordLimit));
        }
        Ok(bytes)
    }
}

// ---------------------------------------------------------------------------
// Fold
// ---------------------------------------------------------------------------

/// Deterministic fold for engine tests: every non-empty record (or snapshot
/// record) becomes one `first` call whose `msg_id` is the record text; a record
/// equal to `FAIL` makes the batch fail with `InvalidData`.
pub struct RecordingFold {
    harness: &'static str,
    policy: &'static str,
    kind: PrepSourceKind,
}

impl RecordingFold {
    pub const fn new(harness: &'static str, policy: &'static str, kind: PrepSourceKind) -> Self {
        Self {
            harness,
            policy,
            kind,
        }
    }
}

impl PrepFold for RecordingFold {
    fn harness(&self) -> &'static str {
        self.harness
    }
    fn policy(&self) -> &'static str {
        self.policy
    }
    fn kind(&self) -> PrepSourceKind {
        self.kind
    }
    fn pattern(&self) -> &'static str {
        "**/*"
    }
    fn describe(&self, file: &str) -> PrepSourceMeta {
        PrepSourceMeta {
            is_sub: file.contains("/sub/"),
            agent_id: None,
            project: file.split('/').next().map(str::to_owned),
        }
    }
    fn open(
        &self,
        _meta: &PrepSourceMeta,
        source: &str,
        generation: u32,
        saved: Option<&PrepCheckpoint>,
    ) -> Result<Box<dyn PrepFoldSession>, PipelineError> {
        let records = match saved {
            None => 0,
            Some(checkpoint)
                if checkpoint.format == PREP_CHECKPOINT_FORMAT
                    && checkpoint.policy == self.policy =>
            {
                checkpoint.fold["records"]
                    .as_u64()
                    .ok_or_else(|| error(PipelineErrorKind::InvalidData))?
            }
            Some(_) => return Err(error(PipelineErrorKind::InvalidData)),
        };
        Ok(Box::new(RecordingSession {
            policy: self.policy,
            source: source.to_owned(),
            generation,
            records,
        }))
    }
}

struct RecordingSession {
    policy: &'static str,
    source: String,
    generation: u32,
    records: u64,
}

impl PrepFoldSession for RecordingSession {
    fn fold(
        &mut self,
        input: &PrepInput,
        _options: PrepOptions,
    ) -> Result<PrepRows, PipelineError> {
        let items: Vec<(Option<u64>, Option<String>, &[u8])> = match input {
            PrepInput::Records(records) => records
                .iter()
                .map(|r| (Some(r.offset), None, r.bytes.as_slice()))
                .collect(),
            PrepInput::Snapshot(snapshot) => snapshot
                .records
                .iter()
                .map(|r| (None, Some(r.key.clone()), r.bytes.as_slice()))
                .collect(),
        };
        let mut rows = PrepRows::default();
        for (offset, key, bytes) in items {
            if bytes == b"FAIL" {
                return Err(error(PipelineErrorKind::InvalidData));
            }
            self.records += 1;
            rows.calls.push(PrepCallRow {
                source: self.source.clone(),
                generation: self.generation,
                native_offset: offset,
                native_key: key,
                sighting: CallSighting::First,
                msg_id: Some(String::from_utf8_lossy(bytes).into_owned()),
                request_id: None,
                ts: None,
                ts_ms: None,
                model: None,
                stop_reason: None,
                input: Some(1),
                cw_1h: None,
                cw_5m: None,
                cache_read: None,
                output: None,
                cache_write_basis: CacheWriteBasis::None,
                is_sidechain: false,
                gap_ms: None,
                turn_no: None,
                call_in_turn: None,
                records: 1,
            });
        }
        Ok(rows)
    }

    fn checkpoint(&self) -> PrepCheckpoint {
        PrepCheckpoint {
            format: PREP_CHECKPOINT_FORMAT,
            policy: self.policy.to_owned(),
            fold: json!({"records": self.records}),
        }
    }

    fn facts(&self) -> SessionFacts {
        SessionFacts {
            records: self.records,
            calls: self.records,
            ..SessionFacts::default()
        }
    }
}
