//! Incremental prep orchestration: discover → compare committed state → read only
//! new complete records → pure fold → durable rows, then cursor.
//!
//! Each source is independent: a failure keeps that source's previous committed
//! state and discards its rows for this run. Unchanged sources cost one `stat`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use unisphere_core::{
    PipelineError, PipelineErrorKind, ReadCursor, SessionRef,
    prep::{
        PREP_TABLE_SCHEMA_VERSION, PrepApi, PrepFold, PrepLoader, PrepReport, PrepRequest,
        PrepRows, PrepSourceMeta, PrepSourceOutcome, PrepSourceStat, PrepSourceState, PrepState,
        PrepStore, PrepTableCounts,
    },
};

/// Composes a loader, a pure fold and a store behind [`PrepApi`].
pub struct Preparer<L, F, S> {
    loader: L,
    fold: F,
    store: S,
}

impl<L, F, S> Preparer<L, F, S> {
    pub fn new(loader: L, fold: F, store: S) -> Self {
        Self {
            loader,
            fold,
            store,
        }
    }
}

impl<L: PrepLoader, F: PrepFold, S: PrepStore> PrepApi for Preparer<L, F, S> {
    fn prep(&self, request: &PrepRequest) -> Result<PrepReport, PipelineError> {
        run_prep(&self.loader, &self.fold, &self.store, request)
    }
}

struct SourceResult {
    outcome: PrepSourceOutcome,
    state: Option<PrepSourceState>,
    rows: PrepRows,
}

/// Run one idempotent prep pass. Nothing is written when nothing changed.
pub fn run_prep(
    loader: &dyn PrepLoader,
    fold: &dyn PrepFold,
    store: &dyn PrepStore,
    request: &PrepRequest,
) -> Result<PrepReport, PipelineError> {
    request.limits.validate()?;
    let (previous, orphans_removed) = store.load()?;
    let compatible = previous.as_ref().is_some_and(|state| {
        state.table_schema_version == PREP_TABLE_SCHEMA_VERSION
            && state.policy == fold.policy()
            && state.harness == fold.harness()
            && state.root == request.root
    });
    let mut stats = loader.discover(&request.root)?;
    if let Some(since) = request.modified_since_ns {
        stats.retain(|stat| stat.mtime_ns >= since);
    }
    let prior = previous.as_ref().map(|state| &state.sources);
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<SourceResult>>> =
        Mutex::new((0..stats.len()).map(|_| None).collect());
    thread::scope(|scope| {
        for _ in 0..request.threads.max(1) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(stat) = stats.get(index) else { break };
                    let prev = prior.and_then(|sources| sources.get(&stat.file));
                    let result = prep_source(loader, fold, request, stat, prev, compatible);
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
    let mut sources: BTreeMap<String, PrepSourceState> = BTreeMap::new();
    if compatible && let Some(state) = previous.as_ref() {
        // Sources that vanished from disk keep their committed rows and state.
        sources.clone_from(&state.sources);
    }
    let mut report = PrepReport {
        target: request.target.clone(),
        root: request.root.clone(),
        harness: fold.harness().into(),
        policy: fold.policy().into(),
        table_schema_version: PREP_TABLE_SCHEMA_VERSION,
        sources_discovered: stats.len() as u64,
        ..PrepReport::default()
    };
    let mut rows = PrepRows::default();
    let mut changed = !compatible && previous.is_some();
    for (stat, result) in stats.iter().zip(results) {
        let Some(result) = result else {
            return Err(PipelineError::new(PipelineErrorKind::InvalidData, None));
        };
        *report
            .sources_by_status
            .entry(result.outcome.status.clone())
            .or_default() += 1;
        report.bytes_read += result.outcome.bytes_read;
        report.pending_tail_bytes += result.outcome.pending_tail_bytes;
        if let Some(state) = result.state {
            if result.outcome.status != "unchanged" {
                changed = true;
            }
            sources.insert(stat.file.clone(), state);
        } else if !compatible {
            // A failed source under a new policy has no valid committed state.
            sources.remove(&stat.file);
        }
        if result.outcome.status != "unchanged" {
            report.sources.push(result.outcome);
        }
        rows.extend(result.rows);
    }
    let discovered: std::collections::BTreeSet<&str> =
        stats.iter().map(|stat| stat.file.as_str()).collect();
    let missing = sources
        .keys()
        .filter(|file| !discovered.contains(file.as_str()))
        .count() as u64;
    if missing > 0 {
        report.sources_by_status.insert("missing".into(), missing);
    }
    report.rows_written = PrepTableCounts {
        calls: rows.calls.len() as u64,
        turns: rows.turns.len() as u64,
        triggers: rows.triggers.len() as u64,
        events: rows.events.len() as u64,
    };
    let runs = previous.as_ref().map_or(0, |state| state.runs);
    report.run = runs;
    if changed || !rows.is_empty() || previous.is_none() {
        let state = PrepState {
            table_schema_version: PREP_TABLE_SCHEMA_VERSION,
            harness: fold.harness().into(),
            policy: fold.policy().into(),
            root: request.root.clone(),
            runs: runs + 1,
            parts: previous.map(|state| state.parts).unwrap_or_default(),
            sources,
        };
        report.run = state.runs;
        report.commit = store.commit(&rows, &state)?;
    }
    report.commit.orphans_removed = orphans_removed;
    Ok(report)
}

fn outcome(stat: &PrepSourceStat, status: &str, generation: u32) -> PrepSourceOutcome {
    PrepSourceOutcome {
        file: stat.file.clone(),
        status: status.into(),
        generation,
        bytes_read: 0,
        rows: 0,
        committed_offset: 0,
        pending_tail_bytes: 0,
        error: None,
    }
}

fn prep_source(
    loader: &dyn PrepLoader,
    fold: &dyn PrepFold,
    request: &PrepRequest,
    stat: &PrepSourceStat,
    prev: Option<&PrepSourceState>,
    compatible: bool,
) -> SourceResult {
    let prev_ok = prev.filter(|_| compatible);
    if let Some(prev) = prev_ok
        && prev.identity == stat.identity
        && prev.size == stat.size
        && prev.mtime_ns == stat.mtime_ns
    {
        let mut unchanged = outcome(stat, "unchanged", prev.generation);
        unchanged.committed_offset = prev.offset;
        unchanged.pending_tail_bytes = stat.size.saturating_sub(prev.offset);
        return SourceResult {
            outcome: unchanged,
            state: Some(prev.clone()),
            rows: PrepRows::default(),
        };
    }
    let mut anchor_bytes = 0;
    let resumable = prev_ok.filter(|prev| {
        prev.identity == stat.identity && stat.size >= prev.offset && {
            anchor_bytes += prev.offset.min(8192);
            loader.anchor(&stat.path, prev.offset).ok() == prev.anchor
        }
    });
    let attempt = match resumable {
        Some(prev) => read_source(
            loader,
            fold,
            request,
            stat,
            prev.generation,
            Some(prev),
            "appended",
        ),
        None => Err(PipelineError::new(PipelineErrorKind::SourceChanged, None)),
    };
    let attempt = match attempt {
        Err(error) if error.kind() == PipelineErrorKind::SourceChanged => {
            let status = match prev {
                None => "new",
                Some(_) if !compatible => "policy",
                Some(_) => "replaced",
            };
            let generation = prev.map_or(0, |prev| prev.generation + 1);
            read_source(loader, fold, request, stat, generation, None, status)
        }
        other => other,
    };
    match attempt {
        Ok(mut result) => {
            result.outcome.bytes_read += anchor_bytes;
            result
        }
        Err(error) => {
            let mut failed = outcome(stat, "failed", prev.map_or(0, |p| p.generation));
            failed.error = Some(error.to_string());
            SourceResult {
                outcome: failed,
                state: prev_ok.cloned(),
                rows: PrepRows::default(),
            }
        }
    }
}

/// Read from the committed cursor (or the start) to the last complete LF.
fn read_source(
    loader: &dyn PrepLoader,
    fold: &dyn PrepFold,
    request: &PrepRequest,
    stat: &PrepSourceStat,
    generation: u32,
    prev: Option<&PrepSourceState>,
    status: &str,
) -> Result<SourceResult, PipelineError> {
    let meta = PrepSourceMeta {
        file: stat.file.clone(),
        is_sub: fold.is_sub(&stat.file),
    };
    let mut session = fold.open(&meta, generation, prev.map(|p| &p.fold))?;
    let source = SessionRef {
        path: stat.path.clone(),
    };
    let mut cursor = prev.map(|prev| ReadCursor {
        source: stat.path.clone(),
        identity: prev.identity.clone(),
        offset: prev.offset,
    });
    let start = cursor.as_ref().map_or(0, |c| c.offset);
    let mut rows = PrepRows::default();
    loop {
        let batch = loader.read_batch(&source, cursor.as_ref(), request.limits)?;
        rows.extend(session.fold(&batch.records, request.options)?);
        let more = batch.more;
        cursor = Some(batch.next_cursor);
        if !more {
            break;
        }
    }
    let cursor = cursor.ok_or_else(|| PipelineError::new(PipelineErrorKind::InvalidData, None))?;
    let anchor = if cursor.offset == 0 {
        None
    } else {
        Some(loader.anchor(&stat.path, cursor.offset)?)
    };
    let mut result = outcome(stat, status, generation);
    result.bytes_read = cursor.offset - start + cursor.offset.min(8192);
    result.rows = rows.len() as u64;
    result.committed_offset = cursor.offset;
    result.pending_tail_bytes = stat.size.saturating_sub(cursor.offset);
    let state = PrepSourceState {
        path: stat.path.clone(),
        identity: cursor.identity.clone(),
        size: stat.size,
        mtime_ns: stat.mtime_ns,
        offset: cursor.offset,
        anchor,
        generation,
        is_sub: meta.is_sub,
        fold: session.save(),
        session: session.session(),
        last_status: status.into(),
    };
    Ok(SourceResult {
        outcome: result,
        state: Some(state),
        rows,
    })
}

/// Default location hint for Claude Code transcripts under an explicit home.
pub fn claude_projects_root(home: &Path) -> PathBuf {
    home.join(".claude").join("projects")
}
