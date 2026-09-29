#![forbid(unsafe_code)]
//! External SDK consumer: runs the same incremental prep in-process with its
//! own store (no Parquet), then folds one source with no store at all.
//! Usage: unisphere-prep-consumer <claude-root> <main-session-file>

use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::ExitCode,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use unisphere_sdk::{
    PipelineError, ReadLimits, SnapshotLimits,
    prep::{
        PrepApi, PrepBinding, PrepCommit, PrepCompactReport, PrepLoaded, PrepLoader, PrepOptions,
        PrepReadLimits, PrepRequest, PrepRows, PrepSourceSet, PrepState, PrepStore, Preparer,
        fold_source,
    },
};

/// The consumer's own durable store: an in-memory map it can inspect afterwards.
#[derive(Clone, Default)]
struct OwnStore(Arc<Mutex<(Option<PrepState>, u64)>>);

impl OwnStore {
    fn inner(&self) -> MutexGuard<'_, (Option<PrepState>, u64)> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl PrepStore for OwnStore {
    fn state(&self) -> Result<Option<PrepState>, PipelineError> {
        Ok(self.inner().0.clone())
    }
    fn load(&self) -> Result<PrepLoaded, PipelineError> {
        Ok(PrepLoaded { state: self.state()?, orphans_removed: 0 })
    }
    fn commit(&self, rows: &PrepRows, state: &PrepState) -> Result<PrepCommit, PipelineError> {
        let mut inner = self.inner();
        inner.0 = Some(state.clone());
        inner.1 += rows.len() as u64;
        Ok(PrepCommit::default())
    }
    fn compact(&self) -> Result<PrepCompactReport, PipelineError> {
        Ok(PrepCompactReport::default())
    }
}

fn limits() -> PrepReadLimits {
    PrepReadLimits {
        read: ReadLimits { max_records: usize::MAX, ..ReadLimits::default() },
        snapshot: SnapshotLimits::default(),
    }
}

fn run(root: PathBuf, main_file: PathBuf) -> Result<Value, PipelineError> {
    let binding = PrepBinding {
        fold: Arc::new(unisphere_adapter_claude::ClaudePrepFold),
        loader: Arc::new(unisphere_loader_jsonl::FileSessionLoader),
    };
    let store = OwnStore::default();
    let api = Preparer::new(vec![binding.clone()], store.clone());
    let request = PrepRequest {
        target: PathBuf::from("/consumer-owned"),
        roots: vec![PrepSourceSet { harness: "claude-code".into(), label: "demo".into(), root: root.clone() }],
        options: PrepOptions::default(),
        limits: limits(),
        threads: 2,
        modified_since_ns: None,
    };
    let mut runs = Vec::new();
    for _ in 0..2 {
        let report = api.prep(&request)?;
        runs.push(json!({
            "run": report.run,
            "bytes_read": report.bytes_read,
            "rows_written": report.rows_written,
            "by_status": report.sets.iter().map(|s| &s.by_status).collect::<Vec<_>>(),
        }));
    }
    let committed_rows = store.inner().1;
    // Single-source fold with no store and no target directory (Plan 029 shape).
    let stat = binding.loader.stat(&root, &main_file)?;
    let mut rows = 0u64;
    let folded = fold_source(
        binding.loader.as_ref(),
        binding.fold.as_ref(),
        &stat,
        "claude-code/demo/main",
        0,
        None,
        PrepOptions::default(),
        limits(),
        &mut |batch| rows += batch.len() as u64,
    )?;
    Ok(json!({
        "runs": runs,
        "committed_rows": committed_rows,
        "fold_source": {
            "rows": rows,
            "calls": folded.facts.calls,
            "turns": folded.facts.turns,
            "compactions_known": folded.facts.compactions.is_some(),
            "pending_tail_bytes": folded.pending_tail_bytes,
        }
    }))
}

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let (Some(root), Some(main_file)) = (args.next(), args.next()) else {
        eprintln!("usage: unisphere-prep-consumer <claude-root> <main-session-file>");
        return ExitCode::from(2);
    };
    match run(root, main_file) {
        Ok(value) => {
            println!("{value}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("prep consumer failed: {error}");
            ExitCode::FAILURE
        }
    }
}
