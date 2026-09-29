//! Incremental prep orchestration: bind roots to folds and loaders, compare each
//! discovered source with its committed state, read only new complete input,
//! fold it purely, then commit rows before the state that references them.
//!
//! Each source is independent: a failure keeps that source's previous committed
//! state and contributes no rows. Unchanged sources cost one `stat`.

use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use globset::{Glob, GlobMatcher};
use unisphere_core::{PipelineError, PipelineErrorKind, ReadCursor};

pub use unisphere_core::prep::*;

/// One harness representation: a pure fold and the loader for its sources.
#[derive(Clone)]
pub struct PrepBinding {
    pub fold: Arc<dyn PrepFold>,
    pub loader: Arc<dyn PrepLoader>,
}

/// Composes bindings and a store behind [`PrepApi`].
pub struct Preparer<S> {
    bindings: Vec<PrepBinding>,
    store: S,
}

impl<S> Preparer<S> {
    pub fn new(bindings: Vec<PrepBinding>, store: S) -> Self {
        Self { bindings, store }
    }
}

impl<S: PrepStore> PrepApi for Preparer<S> {
    fn prep(&self, request: &PrepRequest) -> Result<PrepReport, PipelineError> {
        run_prep(&self.bindings, &self.store, request)
    }

    fn compact(&self, _request: &PrepCompactRequest) -> Result<PrepCompactReport, PipelineError> {
        self.store.compact()
    }

    fn record(&self, request: &PrepRecordRequest) -> Result<PrepRecord, PipelineError> {
        fetch_record(&self.bindings, &self.store, request)
    }
}

/// Where a resumed fold continues: the committed cursor and checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct PrepResume {
    pub cursor: ReadCursor,
    pub checkpoint: PrepCheckpoint,
}

/// Result of folding one source to its last complete record.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFold {
    pub checkpoint: PrepCheckpoint,
    pub facts: SessionFacts,
    /// Append: cursor after the last complete LF.
    pub cursor: Option<ReadCursor>,
    /// Append: anchor at `cursor`.
    pub anchor: Option<String>,
    /// Snapshot: revision of the folded snapshot.
    pub revision: Option<String>,
    pub bytes_read: u64,
    pub pending_tail_bytes: u64,
    pub rows: u64,
}

/// Fold one source from `resume` (or the start) to its last complete record,
/// handing every batch's rows to `sink`. No store and no target directory.
#[allow(clippy::too_many_arguments)]
pub fn fold_source(
    loader: &dyn PrepLoader,
    fold: &dyn PrepFold,
    stat: &PrepSourceStat,
    source: &str,
    generation: u32,
    resume: Option<&PrepResume>,
    options: PrepOptions,
    limits: PrepReadLimits,
    sink: &mut dyn FnMut(PrepRows),
) -> Result<SourceFold, PipelineError> {
    let meta = fold.describe(&stat.file);
    let mut session = fold.open(
        &meta,
        source,
        generation,
        resume.map(|resume| &resume.checkpoint),
    )?;
    let advanced = advance(
        loader,
        session.as_mut(),
        stat,
        resume.map(|resume| resume.cursor.clone()),
        options,
        limits,
        sink,
    )?;
    Ok(SourceFold {
        checkpoint: session.checkpoint(),
        facts: session.facts(),
        cursor: advanced.cursor,
        anchor: advanced.anchor,
        revision: advanced.revision,
        bytes_read: advanced.bytes_read,
        pending_tail_bytes: advanced.pending_tail_bytes,
        rows: advanced.rows,
    })
}

/// Where [`advance`] left a source: [`SourceFold`] without the fold's state.
#[derive(Debug, Default)]
pub(crate) struct Advanced {
    pub cursor: Option<ReadCursor>,
    pub anchor: Option<String>,
    pub revision: Option<String>,
    pub bytes_read: u64,
    pub pending_tail_bytes: u64,
    pub rows: u64,
}

/// Feed one source from `cursor` (or the start) to its last complete record
/// into an already open `session`, handing every batch's rows to `sink`. On
/// error the session may have folded part of the input and must be discarded.
pub(crate) fn advance(
    loader: &dyn PrepLoader,
    session: &mut dyn PrepFoldSession,
    stat: &PrepSourceStat,
    mut cursor: Option<ReadCursor>,
    options: PrepOptions,
    limits: PrepReadLimits,
    sink: &mut dyn FnMut(PrepRows),
) -> Result<Advanced, PipelineError> {
    let start = cursor.as_ref().map_or(0, |cursor| cursor.offset);
    let mut result = Advanced::default();
    loop {
        let batch = loader.read(stat, cursor.as_ref(), limits)?;
        result.bytes_read += batch.bytes_read;
        if let PrepInput::Snapshot(snapshot) = &batch.input {
            result.revision = Some(snapshot.revision.clone());
        }
        let rows = session.fold(&batch.input, options)?;
        result.rows += rows.len() as u64;
        sink(rows);
        let before = cursor.as_ref().map(|cursor| cursor.offset);
        cursor = batch.next_cursor.or(cursor);
        if !batch.more {
            break;
        }
        // A loader that asks for more without advancing would never finish.
        if cursor.as_ref().map(|cursor| cursor.offset) <= before {
            return Err(PipelineError::new(PipelineErrorKind::BatchLimit, before));
        }
    }
    if let Some(cursor) = &cursor {
        if cursor.offset < start {
            return Err(PipelineError::new(
                PipelineErrorKind::SourceChanged,
                Some(start),
            ));
        }
        result.pending_tail_bytes = stat.size.saturating_sub(cursor.offset);
        if cursor.offset > 0 {
            result.anchor = Some(loader.anchor(stat, cursor.offset)?);
        }
    }
    result.cursor = cursor;
    Ok(result)
}

struct Work<'a> {
    set: &'a PrepSourceSet,
    /// Index of the owning set in `PrepReport::sets`.
    set_index: usize,
    binding: &'a PrepBinding,
    stat: PrepSourceStat,
    key: String,
}

struct SourceResult {
    outcome: PrepSourceOutcome,
    /// State to commit for this key; `None` drops it.
    state: Option<PrepSourceState>,
    /// The committed state must change (rows, cursor, revision or stat drift).
    dirty: bool,
    rows: PrepRows,
}

fn invalid_input() -> PipelineError {
    PipelineError::new(PipelineErrorKind::InvalidInput, None)
}

fn matcher(pattern: &str) -> Result<GlobMatcher, PipelineError> {
    Glob::new(pattern)
        .map(|glob| glob.compile_matcher())
        .map_err(|_| invalid_input())
}

fn count(report: &mut PrepSetReport, status: PrepSourceStatus) {
    *report.by_status.entry(status.label().into()).or_default() += 1;
}

/// Run one idempotent prep pass. Nothing is written when nothing changed.
pub fn run_prep(
    bindings: &[PrepBinding],
    store: &dyn PrepStore,
    request: &PrepRequest,
) -> Result<PrepReport, PipelineError> {
    request.limits.read.validate()?;
    request.limits.snapshot.validate()?;
    let mut set_keys = BTreeSet::new();
    if !request.roots.iter().all(|set| set_keys.insert(set.key())) {
        return Err(invalid_input());
    }
    let loaded = store.load()?;
    let previous = loaded.state;
    let schema_ok = previous.as_ref().is_some_and(|state| {
        state.table_schema_version == PREP_TABLE_SCHEMA_VERSION
            && state.checkpoint_format == PREP_CHECKPOINT_FORMAT
    });
    // An incompatible state contributes generations only; its sources and parts are dropped.
    let prior = previous.as_ref().filter(|_| schema_ok);
    let mut report = PrepReport {
        target: request.target.clone(),
        table_schema_version: PREP_TABLE_SCHEMA_VERSION,
        ..PrepReport::default()
    };
    let mut sets = prior.map(|state| state.sets.clone()).unwrap_or_default();
    let mut work: Vec<Work<'_>> = Vec::new();
    // Per requested set: every discovered key, including those scoped out.
    let mut discovered: Vec<BTreeSet<String>> = Vec::new();
    for set in &request.roots {
        let set_bindings: Vec<&PrepBinding> = bindings
            .iter()
            .filter(|binding| binding.fold.harness() == set.harness)
            .collect();
        let set_index = report.sets.len();
        let mut set_report = PrepSetReport {
            harness: set.harness.clone(),
            label: set.label.clone(),
            root: set.root.clone(),
            supported: !set_bindings.is_empty(),
            ..PrepSetReport::default()
        };
        let mut keys = BTreeSet::new();
        if set_bindings.is_empty() {
            count(&mut set_report, PrepSourceStatus::Unsupported);
        } else {
            let policies: Vec<&str> = set_bindings.iter().map(|b| b.fold.policy()).collect();
            set_report.policy = Some(policies.join("+"));
            for binding in set_bindings {
                let accept = matcher(binding.fold.pattern())?;
                let discovery = binding
                    .loader
                    .discover(&set.root, &|file| accept.is_match(file))?;
                set_report.skipped.symlinks += discovery.skipped.symlinks;
                set_report.skipped.hidden += discovery.skipped.hidden;
                set_report.skipped.unreadable_entries += discovery.skipped.unreadable_entries;
                for stat in discovery.sources {
                    let key = set.source_key(&stat.file);
                    // The first binding that accepts a file owns it.
                    if !keys.insert(key.clone()) {
                        continue;
                    }
                    if request
                        .modified_since_ns
                        .is_some_and(|since| stat.mtime_ns < since)
                    {
                        count(&mut set_report, PrepSourceStatus::Skipped);
                        continue;
                    }
                    work.push(Work {
                        set,
                        set_index,
                        binding,
                        stat,
                        key,
                    });
                }
            }
            set_report.discovered = keys.len() as u64;
            sets.insert(
                set.key(),
                PrepSetState {
                    harness: set.harness.clone(),
                    label: set.label.clone(),
                    root: set.root.clone(),
                    policy: set_report.policy.clone().unwrap_or_default(),
                },
            );
        }
        report.sets.push(set_report);
        discovered.push(keys);
    }

    let threads = request.threads.clamp(1, work.len().max(1));
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<SourceResult>>> =
        Mutex::new((0..work.len()).map(|_| None).collect());
    thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = work.get(index) else { break };
                    let prev = previous
                        .as_ref()
                        .and_then(|state| state.sources.get(&item.key));
                    let incompatible = prev.and_then(|prev| incompatibility(prev, schema_ok, item));
                    let result = prep_source(item, prev, incompatible, schema_ok, request);
                    results.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(result);
                }
            });
        }
    });
    let results = results.into_inner().unwrap_or_else(PoisonError::into_inner);

    let mut sources = prior.map(|state| state.sources.clone()).unwrap_or_default();
    let mut rows = PrepRows::default();
    let mut dirty = !schema_ok;
    for (item, result) in work.iter().zip(results) {
        let result =
            result.ok_or_else(|| PipelineError::new(PipelineErrorKind::InvalidData, None))?;
        count(&mut report.sets[item.set_index], result.outcome.status);
        report.bytes_read += result.outcome.bytes_read;
        report.pending_tail_bytes += result.outcome.pending_tail_bytes;
        dirty |= result.dirty;
        match result.state {
            Some(state) => sources.insert(item.key.clone(), state),
            None => sources.remove(&item.key),
        };
        if result.outcome.status != PrepSourceStatus::Unchanged {
            report.sources.push(result.outcome);
        }
        rows.extend(result.rows);
    }
    // Committed sources of a requested set that discovery no longer returns keep
    // their state and rows.
    for ((set, set_report), keys) in request.roots.iter().zip(&mut report.sets).zip(&discovered) {
        if !set_report.supported {
            continue;
        }
        let set_key = set.key();
        for (key, state) in &sources {
            if state.set != set_key || keys.contains(key) {
                continue;
            }
            count(set_report, PrepSourceStatus::Missing);
            let mut missing = outcome(key, PrepSourceStatus::Missing, state.generation);
            missing.committed_offset = state.offset;
            report.sources.push(missing);
        }
    }
    report.rows_written = rows.counts();
    let runs = previous.as_ref().map_or(0, |state| state.runs);
    report.run = runs;
    if dirty {
        let state = PrepState {
            table_schema_version: PREP_TABLE_SCHEMA_VERSION,
            checkpoint_format: PREP_CHECKPOINT_FORMAT,
            runs: runs + 1,
            parts: prior.map(|state| state.parts.clone()).unwrap_or_default(),
            sets,
            sources,
        };
        report.run = state.runs;
        report.commit = store.commit(&rows, &state)?;
    }
    report.commit.orphans_removed = loaded.orphans_removed;
    Ok(report)
}

/// Why a committed source can no longer be resumed, whatever its bytes say.
fn incompatibility(
    prev: &PrepSourceState,
    schema_ok: bool,
    item: &Work<'_>,
) -> Option<PrepReplaceReason> {
    if !schema_ok || prev.checkpoint.format != PREP_CHECKPOINT_FORMAT {
        Some(PrepReplaceReason::Schema)
    } else if prev.path != item.stat.path {
        Some(PrepReplaceReason::RootMoved)
    } else if prev.kind != item.stat.kind || prev.checkpoint.policy != item.binding.fold.policy() {
        Some(PrepReplaceReason::Policy)
    } else {
        None
    }
}

fn outcome(key: &str, status: PrepSourceStatus, generation: u32) -> PrepSourceOutcome {
    PrepSourceOutcome {
        source: key.to_owned(),
        status,
        generation,
        bytes_read: 0,
        rows: 0,
        committed_offset: 0,
        pending_tail_bytes: 0,
        error: None,
    }
}

/// Nothing new to commit: the previous state with the observed stat.
fn unchanged(item: &Work<'_>, prev: &PrepSourceState, bytes_read: u64) -> SourceResult {
    let stat = &item.stat;
    let mut result = outcome(&item.key, PrepSourceStatus::Unchanged, prev.generation);
    result.bytes_read = bytes_read;
    result.committed_offset = prev.offset;
    if stat.kind == PrepSourceKind::Append {
        result.pending_tail_bytes = stat.size.saturating_sub(prev.offset);
    }
    let state = PrepSourceState {
        identity: stat.identity.clone(),
        size: stat.size,
        mtime_ns: stat.mtime_ns,
        ..prev.clone()
    };
    SourceResult {
        outcome: result,
        dirty: state != *prev,
        state: Some(state),
        rows: PrepRows::default(),
    }
}

fn prep_source(
    item: &Work<'_>,
    prev: Option<&PrepSourceState>,
    incompatible: Option<PrepReplaceReason>,
    schema_ok: bool,
    request: &PrepRequest,
) -> SourceResult {
    let stat = &item.stat;
    let compatible = prev.filter(|_| incompatible.is_none());
    // One stat decides the common case.
    if let Some(prev) = compatible
        && prev.identity == stat.identity
        && prev.size == stat.size
        && prev.mtime_ns == stat.mtime_ns
    {
        return unchanged(item, prev, 0);
    }
    // A failed source contributes no rows and keeps its committed state, which
    // is re-judged next run. Only a dropped (incompatible) state is not kept.
    let failed = |error: PipelineError| {
        let mut result = outcome(
            &item.key,
            PrepSourceStatus::Unreadable,
            prev.map_or(0, |p| p.generation),
        );
        result.committed_offset = prev.map_or(0, |p| p.offset);
        result.error = Some(error.to_string());
        SourceResult {
            outcome: result,
            state: prev.filter(|_| schema_ok).cloned(),
            dirty: false,
            rows: PrepRows::default(),
        }
    };
    let mut reason = incompatible;
    let mut resume = None;
    if let Some(prev) = compatible
        && stat.kind == PrepSourceKind::Append
    {
        if prev.identity != stat.identity {
            reason = Some(PrepReplaceReason::Rotated);
        } else if stat.size < prev.offset {
            reason = Some(PrepReplaceReason::Truncated);
        } else if prev.offset == 0 {
            resume = Some(prev);
        } else {
            match item.binding.loader.anchor(stat, prev.offset) {
                Ok(anchor) if prev.anchor.as_deref() == Some(anchor.as_str()) => {
                    resume = Some(prev);
                }
                Ok(_) => reason = Some(PrepReplaceReason::Rewritten),
                Err(error) => return failed(error),
            }
        }
    }
    let generation = match resume {
        Some(prev) => prev.generation,
        None => prev.map_or(0, |p| p.generation + 1),
    };
    let (fold, rows) = match fold_into(item, generation, resume, request) {
        Ok(folded) => folded,
        Err(error) => return failed(error),
    };
    if let Some(prev) = compatible {
        let nothing_new = match stat.kind {
            // Equal revision: the rows of the re-read are discarded.
            PrepSourceKind::Snapshot => prev.revision.is_some() && prev.revision == fold.revision,
            PrepSourceKind::Append => {
                resume.is_some()
                    && rows.is_empty()
                    && fold.cursor.as_ref().map(|c| c.offset) == Some(prev.offset)
            }
        };
        if nothing_new {
            return unchanged(item, prev, fold.bytes_read);
        }
    }
    let status = match (resume, prev) {
        (Some(_), _) => PrepSourceStatus::Appended,
        (None, None) => PrepSourceStatus::New,
        (None, Some(_)) => PrepSourceStatus::Replaced {
            reason: reason.unwrap_or(PrepReplaceReason::Revision),
        },
    };
    let mut result = outcome(&item.key, status, generation);
    result.bytes_read = fold.bytes_read;
    result.rows = fold.rows;
    result.committed_offset = fold.cursor.as_ref().map_or(0, |c| c.offset);
    result.pending_tail_bytes = fold.pending_tail_bytes;
    let identity = fold
        .cursor
        .as_ref()
        .map_or_else(|| stat.identity.clone(), |c| c.identity.clone());
    let state = PrepSourceState {
        set: item.set.key(),
        path: stat.path.clone(),
        file: stat.file.clone(),
        kind: stat.kind,
        identity,
        size: stat.size,
        mtime_ns: stat.mtime_ns,
        offset: result.committed_offset,
        anchor: fold.anchor,
        revision: fold.revision,
        generation,
        meta: item.binding.fold.describe(&stat.file),
        checkpoint: fold.checkpoint,
        facts: fold.facts,
        status,
    };
    SourceResult {
        outcome: result,
        state: Some(state),
        dirty: true,
        rows,
    }
}

fn fold_into(
    item: &Work<'_>,
    generation: u32,
    resume: Option<&PrepSourceState>,
    request: &PrepRequest,
) -> Result<(SourceFold, PrepRows), PipelineError> {
    let resume = resume.map(|prev| PrepResume {
        cursor: ReadCursor {
            source: item.stat.path.clone(),
            identity: prev.identity.clone(),
            offset: prev.offset,
        },
        checkpoint: prev.checkpoint.clone(),
    });
    let mut rows = PrepRows::default();
    let fold = fold_source(
        item.binding.loader.as_ref(),
        item.binding.fold.as_ref(),
        &item.stat,
        &item.key,
        generation,
        resume.as_ref(),
        request.options,
        request.limits,
        &mut |batch| rows.extend(batch),
    )?;
    Ok((fold, rows))
}

fn fetch_record(
    bindings: &[PrepBinding],
    store: &dyn PrepStore,
    request: &PrepRecordRequest,
) -> Result<PrepRecord, PipelineError> {
    // Content is opt-in: refuse before touching the store or any native source.
    if !request.include_content {
        return Err(invalid_input());
    }
    let state = store.state()?.ok_or_else(invalid_input)?;
    let source = state
        .sources
        .get(&request.source)
        .ok_or_else(invalid_input)?;
    let set = state.sets.get(&source.set).ok_or_else(invalid_input)?;
    let binding = bindings
        .iter()
        .find(|binding| {
            binding.fold.harness() == set.harness
                && binding.loader.kind() == source.kind
                && matcher(binding.fold.pattern()).is_ok_and(|m| m.is_match(&source.file))
        })
        .ok_or_else(|| PipelineError::new(PipelineErrorKind::Unsupported, None))?;
    if source.kind == PrepSourceKind::Append {
        // Only committed records of the committed generation are addressable.
        let offset = request.address.offset.ok_or_else(invalid_input)?;
        if offset >= source.offset {
            return Err(invalid_input());
        }
        let changed = || PipelineError::new(PipelineErrorKind::SourceChanged, Some(offset));
        let stat = binding.loader.stat(&set.root, &source.path)?;
        if stat.identity != source.identity
            || stat.size < source.offset
            || source.anchor.as_deref()
                != Some(binding.loader.anchor(&stat, source.offset)?.as_str())
        {
            return Err(changed());
        }
    }
    let bytes = binding
        .loader
        .record_at(&source.path, &request.address, request.max_bytes)?;
    Ok(PrepRecord {
        source: request.source.clone(),
        path: source.path.clone(),
        address: request.address.clone(),
        bytes,
    })
}

/// Default location hint for Claude Code transcripts under an explicit home.
pub fn claude_projects_root(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".claude").join("projects")
}

/// A catalogue-default source set for `harness` rooted at `root`.
pub fn default_set(harness: &str, root: std::path::PathBuf) -> PrepSourceSet {
    PrepSourceSet {
        harness: harness.to_owned(),
        label: DEFAULT_ROOT_LABEL.to_owned(),
        root,
    }
}
