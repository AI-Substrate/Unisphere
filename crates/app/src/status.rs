//! Session-status composition root: the SDK status service over the prep
//! bindings and catalogue default roots, plus the Pij/tmux target resolver.

use std::{
    env,
    io::Write,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use unisphere_cli::SessionStatusCommand;
use unisphere_loader_query::status_target::{
    CommandRunner, PsProcessTable, StatusTargetResolver, SystemCommandRunner, SystemFs,
};
use unisphere_sdk::{
    prep::{PrepSourceSet, default_set},
    status::StatusService,
};

/// Default home-relative roots for every harness the prep bindings interpret.
fn default_roots(home: Option<&PathBuf>) -> Vec<PrepSourceSet> {
    let Some(home) = home.filter(|home| home.is_absolute()) else {
        return Vec::new();
    };
    let bound: Vec<&str> = crate::prep::bindings()
        .iter()
        .map(|binding| binding.fold.harness())
        .collect();
    crate::adapters::catalogue_locations()
        .into_iter()
        .filter(|(id, _)| bound.contains(id))
        .filter_map(|(id, locations)| {
            locations
                .iter()
                .find(|hint| {
                    // Hints name an OS ("macos") or a family ("unix").
                    hint.base == "home"
                        && hint
                            .platforms
                            .iter()
                            .any(|p| *p == env::consts::OS || *p == env::consts::FAMILY)
                })
                .map(|hint| home.join(hint.path))
                .filter(|root| root.is_dir())
                .map(|root| default_set(id, root))
        })
        .collect()
}

pub fn run(command: &SessionStatusCommand, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let home = env::var_os("HOME").map(PathBuf::from);
    let service = StatusService::new(crate::prep::bindings(), default_roots(home.as_ref()));
    let runner: Arc<dyn CommandRunner> = Arc::new(SystemCommandRunner::default());
    let resolver = StatusTargetResolver::new(
        runner.clone(),
        Arc::new(PsProcessTable::new(runner)),
        Arc::new(SystemFs),
        home.unwrap_or_default(),
    );
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        });
    unisphere_cli::run_status(command, &service, &resolver, now_ms, stdout, stderr)
}
