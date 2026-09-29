//! Incremental prep orchestration: bind roots to folds and loaders, compare each
//! discovered source with its committed state, read only new complete input,
//! fold it purely, then commit rows before the state that references them.
//!
//! Each source is independent: a failure keeps that source's previous committed
//! state and contributes no rows. Unchanged sources cost one `stat`.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
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
    let mut cursor = resume.map(|resume| resume.cursor.clone());
    let start = cursor.as_ref().map_or(0, |cursor| cursor.offset);
    let mut result = SourceFold {
        checkpoint: session.checkpoint(),
        facts: SessionFacts::default(),
        cursor: None,
        anchor: None,
        revision: None,
        bytes_read: 0,
        pending_tail_bytes: 0,
        rows: 0,
    };
    loop {
        let batch = loader.read(stat, cursor.as_ref(), limits)?;
        result.bytes_read += batch.bytes_read;
        if let PrepInput::Snapshot(snapshot) = &batch.input {
            result.revision = Some(snapshot.revision.clone());
        }
        let rows = session.fold(&batch.input, options)?;
        result.rows += rows.len() as u64;
        sink(rows);
        cursor = batch.next_cursor.or(cursor);
        if !batch.more {
            break;
        }
    }
    if let Some(cursor) = &cursor {
        result.pending_tail_bytes = stat.size.saturating_sub(cursor.offset);
        if cursor.offset > 0 {
            result.anchor = Some(loader.anchor(stat, cursor.offset)?);
        }
        debug_assert!(cursor.offset >= start);
    }
    result.cursor = cursor;
    result.checkpoint = session.checkpoint();
    result.facts = session.facts();
    Ok(result)
}

struct Work<'a> {
    set: &'a PrepSourceSet,
    binding: &'a PrepBinding,
    stat: PrepSourceStat,
    key: String,
}

struct SourceResult {
    outcome: PrepSourceOutcome,
    state: Option<PrepSourceState>,
    rows: PrepRows,
}

fn matcher(pattern: &str) -> Result<GlobMatcher, PipelineError> {
    Glob::new(pattern)
        .map(|glob| glob.compile_matcher())
        .map_err(|_| PipelineError::new(PipelineErrorKind::InvalidInput, None))
}

/// Run one idempotent prep pass. Nothing is written when nothing changed.
pub fn run_prep(
    bindings: &[PrepBinding],
    store: &dyn PrepStore,
    request: &PrepRequest,
) -> Result<PrepReport, PipelineError> {
    request.limits.read.validate()?;
    request.limits.snapshot.validate()?;
    let loaded = store.load()?;
    let previous = loaded.state;
    let schema_ok = previous.as_ref().is_some_and(|state| {
        state.table_schema_version == PREP_TABLE_SCHEMA_VERSION
            && state.checkpoint_format == PREP_CHECKPOINT_FORMAT
    });
    let mut report = PrepReport {
        target: request.target.clone(),
        table_schema_version: PREP_TABLE_SCHEMA_VERSION,
        ..PrepReport::default()
    };
    let mut sets: BTreeMap<String, PrepSetState> = previous
        .as_ref()
        .filter(|_| schema_ok)
        .map(|state| state.sets.clone())
        .unwrap_or_default();
    let mut work: Vec<Work<'_>> = Vec::new();
    // Discovered but excluded by `modified_since`: reported, not read, state kept.
    let mut skipped: Vec<String> = Vec::new();
    let mut discovered_keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for set in &request.roots {
        let set_bindings: Vec<&PrepBinding> = bindings
            .iter()
            .filter(|binding| binding.fold.harness() == set.harness)
            .collect();
        let mut set_report = PrepSetReport {
            harness: set.harness.clone(),
            label: set.label.clone(),
            root: set.root.clone(),
            supported: !set_bindings.is_empty(),
            ..PrepSetReport::default()
        };
        if set_bindings.is_empty() {
            *set_report
                .by_status
                .entry(PrepSourceStatus::Unsupported.label().into())
                .or_default() += 1;
            report.sets.push(set_report);
            continue;
        }
        let policies: Vec<&str> = set_bindings.iter().map(|b| b.fold.policy()).collect();
        set_report.policy = Some(policies.join("+"));
        let keys = discovered_keys.entry(set.key()).or_default();
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
                keys.push(key.clone());
                if request
                    .modified_since_ns
                    .is_some_and(|since| stat.mtime_ns < since)
                {
                    skipped.push(key);
                    continue;
                }
                work.push(Work {
                    set,
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
        report.sets.push(set_report);
    }

    let prior = previous.as_ref().filter(|_| schema_ok);
    let root_moved = |set: &PrepSourceSet| {
        previous
            .as_ref()
            .and_then(|state| state.sets.get(&set.key()))
            .is_some_and(|old| old.root != set.root)
    };
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<SourceResult>>> =
        Mutex::new((0..work.len()).map(|_| None).collect());
    thread::scope(|scope| {
        for _ in 0..request.threads.max(1) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = work.get(index) else { break };
                    let prev = previous
                        .as_ref()
                        .and_then(|state| state.sources.get(&item.key));
                    let incompatible = if prev.is_none() {
                        None
                    } else if !schema_ok {
                        Some(PrepReplaceReason::Schema)
                    } else if root_moved(item.set) {
                        Some(PrepReplaceReason::RootMoved)
                    } else if prev
                        .is_some_and(|p| p.checkpoint.policy != item.binding.fold.policy())
                    {
                        Some(PrepReplaceReason::Policy)
                    } else {
                        None
                    };
                    let result = prep_source(item, prev, incompatible, request);
                    if let Ok(mut slots) = results.lock() {
                        slots[index] = Some(result);
                    }
                }
            });
        }
    });
    let results = results
        .into_inner()
        .map_err(|_| PipelineError::new(PipelineErrorKind::InvalidData, None))?;

    let mut sources = prior.map(|state| state.sources.clone()).unwrap_or_default();
    let mut rows = PrepRows::default();
    let mut changed = previous.is_some() && !schema_ok;
    for (item, result) in work.iter().zip(results) {
        let Some(result) = result else {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        };
        let set_report = report
            .sets
            .iter_mut()
            .find(|r| r.harness == item.set.harness && r.label == item.set.label);
        if let Some(set_report) = set_report {
            *set_report
                .by_status
                .entry(result.outcome.status.label().into())
                .or_default() += 1;
        }
        report.bytes_read += result.outcome.bytes_read;
        report.pending_tail_bytes += result.outcome.pending_tail_bytes;
        match result.state {
            Some(state) => {
                if result.outcome.status != PrepSourceStatus::Unchanged {
                    changed = true;
                }
                sources.insert(item.key.clone(), state);
            }
            None => {
                sources.remove(&item.key);
            }
        }
        if !matches!(
            result.outcome.status,
            PrepSourceStatus::Unchanged | PrepSourceStatus::Skipped
        ) {
            report.sources.push(result.outcome);
        }
        rows.extend(result.rows);
    }
    for set_report in &mut report.sets {
        let set_key = format!("{}/{}", set_report.harness, set_report.label);
        let prefix = format!("{set_key}/");
        let scoped_out = skipped
            .iter()
            .filter(|key| key.starts_with(&prefix))
            .count() as u64;
        if scoped_out > 0 {
            set_report
                .by_status
                .insert(PrepSourceStatus::Skipped.label().into(), scoped_out);
        }
        let seen = discovered_keys.get(&set_key);
        let missing = sources
            .iter()
            .filter(|(key, state)| {
                state.set == set_key && !seen.is_some_and(|keys| keys.contains(key))
            })
            .count() as u64;
        if missing > 0 {
            set_report
                .by_status
                .insert(PrepSourceStatus::Missing.label().into(), missing);
        }
    }
    report.rows_written = rows.counts();
    let runs = previous.as_ref().map_or(0, |state| state.runs);
    report.run = runs;
    if changed || !rows.is_empty() || previous.is_none() {
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

fn prep_source(
    item: &Work<'_>,
    prev: Option<&PrepSourceState>,
    incompatible: Option<PrepReplaceReason>,
    request: &PrepRequest,
) -> SourceResult {
    let stat = &item.stat;
    let loader = item.binding.loader.as_ref();
    let compatible = prev.filter(|_| incompatible.is_none());
    if let Some(prev) = compatible
        && prev.identity == stat.identity
        && prev.size == stat.size
        && prev.mtime_ns == stat.mtime_ns
    {
        let mut unchanged = outcome(&item.key, PrepSourceStatus::Unchanged, prev.generation);
        unchanged.committed_offset = prev.offset;
        unchanged.pending_tail_bytes = stat.size.saturating_sub(prev.offset);
        let mut state = prev.clone();
        state.status = PrepSourceStatus::Unchanged;
        return SourceResult {
            outcome: unchanged,
            state: Some(state),
            rows: PrepRows::default(),
        };
    }
    let mut anchor_bytes = 0;
    let mut reason = incompatible;
    let resume = match (compatible, stat.kind) {
        (Some(prev), PrepSourceKind::Append) => {
            if prev.identity != stat.identity {
                reason = Some(PrepReplaceReason::Rotated);
                None
            } else if stat.size < prev.offset {
                reason = Some(PrepReplaceReason::Truncated);
                None
            } else {
                anchor_bytes = prev.offset.min(8192);
                let anchor = if prev.offset == 0 {
                    None
                } else {
                    loader.anchor(stat, prev.offset).ok()
                };
                if anchor == prev.anchor {
                    Some(prev)
                } else {
                    reason = Some(PrepReplaceReason::Rewritten);
                    None
                }
            }
        }
        _ => None,
    };
    let attempt = match resume {
        Some(prev) => fold_into(item, prev.generation, Some(prev), request),
        None => fold_into(item, prev.map_or(0, |p| p.generation + 1), None, request),
    };
    let generation_prev = prev.map_or(0, |p| p.generation);
    match attempt {
        Ok((fold, rows)) => {
            // A snapshot whose revision did not change is unchanged; its rows are discarded.
            if stat.kind == PrepSourceKind::Snapshot
                && let Some(prev) = compatible
                && prev.revision.is_some()
                && prev.revision == fold.revision
            {
                let mut unchanged =
                    outcome(&item.key, PrepSourceStatus::Unchanged, prev.generation);
                unchanged.bytes_read = fold.bytes_read;
                let mut state = prev.clone();
                state.size = stat.size;
                state.mtime_ns = stat.mtime_ns;
                state.status = PrepSourceStatus::Unchanged;
                return SourceResult {
                    outcome: unchanged,
                    state: Some(state),
                    rows: PrepRows::default(),
                };
            }
            let status = match (resume, prev, reason) {
                (Some(_), _, _) => PrepSourceStatus::Appended,
                (None, None, _) => PrepSourceStatus::New,
                (None, Some(_), Some(reason)) => PrepSourceStatus::Replaced { reason },
                (None, Some(_), None) => PrepSourceStatus::Replaced {
                    reason: PrepReplaceReason::Revision,
                },
            };
            let generation = resume.map_or(prev.map_or(0, |p| p.generation + 1), |p| p.generation);
            let mut result = outcome(&item.key, status, generation);
            result.bytes_read = fold.bytes_read + anchor_bytes;
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
                rows,
            }
        }
        Err(error) => {
            let mut failed = outcome(&item.key, PrepSourceStatus::Unreadable, generation_prev);
            failed.error = Some(error.to_string());
            SourceResult {
                outcome: failed,
                state: compatible.cloned(),
                rows: PrepRows::default(),
            }
        }
    }
}

fn fold_into(
    item: &Work<'_>,
    generation: u32,
    prev: Option<&PrepSourceState>,
    request: &PrepRequest,
) -> Result<(SourceFold, PrepRows), PipelineError> {
    let resume = prev.map(|prev| PrepResume {
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
    let invalid = || PipelineError::new(PipelineErrorKind::InvalidInput, None);
    if !request.include_content {
        return Err(invalid());
    }
    let state = store.state()?.ok_or_else(invalid)?;
    let source = state.sources.get(&request.source).ok_or_else(invalid)?;
    let set = state.sets.get(&source.set).ok_or_else(invalid)?;
    let binding = bindings
        .iter()
        .find(|binding| {
            binding.fold.harness() == set.harness
                && binding.loader.kind() == source.kind
                && matcher(binding.fold.pattern()).is_ok_and(|m| m.is_match(&source.file))
        })
        .ok_or_else(|| PipelineError::new(PipelineErrorKind::Unsupported, None))?;
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
