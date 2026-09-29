//! Prep composition root: the fold/loader bindings, catalogue default roots and
//! the Parquet target store. Nothing else constructs concrete prep adapters.

use std::{
    env,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use unisphere_cli::{PrepCommand, PrepCompactCommand, PrepRecordCommand};
use unisphere_output_prep::ParquetPrepStore;
use unisphere_sdk::{
    PipelineError, PipelineErrorKind,
    prep::{PrepBinding, PrepSourceSet, Preparer, default_set},
};

/// Every harness representation prep interprets. The fold's harness id is the
/// adapter catalogue descriptor id; catalogued harnesses without a binding are
/// reported as unsupported sets.
fn bindings() -> Vec<PrepBinding> {
    vec![PrepBinding {
        fold: Arc::new(unisphere_adapter_claude::ClaudePrepFold),
        loader: Arc::new(unisphere_loader_jsonl::FileSessionLoader),
    }]
}

/// Catalogue default roots on this platform: one set per selected descriptor
/// whose home-relative root exists.
fn default_roots(command: &PrepCommand, home: Option<&Path>) -> Vec<PrepSourceSet> {
    let Some(home) = home.filter(|home| home.is_absolute()) else {
        return Vec::new();
    };
    crate::adapters::catalogue_locations()
        .into_iter()
        .filter(|(id, _)| command.wants_default_root(id))
        .filter_map(|(id, locations)| {
            locations
                .iter()
                .find(|hint| hint.base == "home" && hint.platforms.contains(&env::consts::OS))
                .map(|hint| home.join(hint.path))
                .filter(|root| root.is_dir())
                .map(|root| default_set(id, root))
        })
        .collect()
}

fn invalid() -> PipelineError {
    PipelineError::new(PipelineErrorKind::InvalidInput, None)
}

pub fn run_prep(command: &PrepCommand, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let home = env::var_os("HOME").map(PathBuf::from);
    let mut roots = default_roots(command, home.as_deref());
    roots.extend(command.explicit_sets());
    if roots.is_empty() {
        // No catalogue root exists here and none was given: nothing to prep.
        return unisphere_cli::session_error(stderr, &invalid(), 2);
    }
    match ParquetPrepStore::open(command.target.clone()) {
        Ok(store) => unisphere_cli::run_prep(
            command,
            roots,
            &Preparer::new(bindings(), store),
            stdout,
            stderr,
        ),
        Err(error) => unisphere_cli::session_error(stderr, &error, 1),
    }
}

pub fn run_compact(
    command: &PrepCompactCommand,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    match ParquetPrepStore::open(command.target.clone()) {
        Ok(store) => unisphere_cli::run_prep_compact(
            command,
            &Preparer::new(bindings(), store),
            stdout,
            stderr,
        ),
        Err(error) => unisphere_cli::session_error(stderr, &error, 1),
    }
}

pub fn run_record(
    command: &PrepRecordCommand,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    // Reading one record never creates the target or takes the writer lock.
    match ParquetPrepStore::open_read_only(command.target.clone()) {
        Ok(store) => unisphere_cli::run_prep_record(
            command,
            &Preparer::new(bindings(), store),
            stdout,
            stderr,
        ),
        Err(error) => unisphere_cli::session_error(stderr, &error, 1),
    }
}
