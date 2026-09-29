#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    env,
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use unisphere_cli::{CliContext, ParsedCommand, QueryCommand};
use unisphere_loader_query::{LocalQueryContext, LocalQuerySource, ProvidedInput};
use unisphere_output_query::ProjectedQueryWriter;
use unisphere_sdk::query::{
    LimitKind, OfflineRef, QueryFailure, QueryFailureCode, QueryScope, QueryService,
    RecoveryAction, RepoScope,
};
use unisphere_sdk::{Inspector, StdConfigReader};

mod adapters;
mod git_query;
mod pij;
mod prep;

fn main() -> ExitCode {
    let cwd = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(_) => {
            let _ = writeln!(
                io::stderr().lock(),
                "unisphere: cannot determine the working directory\nNext: rerun from an existing readable directory."
            );
            return ExitCode::FAILURE;
        }
    };
    let stdout = io::stdout();
    let stderr = io::stderr();
    let context = CliContext {
        cwd,
        stdout_is_terminal: stdout.is_terminal(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    let args = env::args_os().collect::<Vec<_>>();
    let mode = unisphere_cli::diagnostic_mode(&args, context.stdout_is_terminal);
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();
    let command = match unisphere_cli::parse(args, &context) {
        Ok(command) => command,
        Err(failure) => {
            return ExitCode::from(unisphere_cli::emit_parse_failure(
                &failure,
                mode,
                &mut stdout,
                &mut stderr,
            ));
        }
    };
    let exit = match &command {
        ParsedCommand::Help(command) => unisphere_cli::run_help(command, &mut stdout, &mut stderr),
        ParsedCommand::Version { mode } => {
            unisphere_cli::run_version(&context, *mode, &mut stdout, &mut stderr)
        }
        ParsedCommand::Docs(command) => unisphere_cli::run_docs(command, &mut stdout, &mut stderr),
        ParsedCommand::Schema(command) => {
            unisphere_cli::run_schema(command, &mut stdout, &mut stderr)
        }
        ParsedCommand::Config(command) => unisphere_cli::run_config(
            command,
            &context,
            &Inspector::new(StdConfigReader),
            &mut stdout,
            &mut stderr,
        ),
        ParsedCommand::Query(command) => match query_source(command, &context) {
            Ok(source) => unisphere_cli::run_query(
                command,
                &context,
                &QueryService::new(source),
                &ProjectedQueryWriter,
                &mut stdout,
                &mut stderr,
            ),
            Err(failure) => {
                unisphere_cli::emit_query_failure(command, &failure, &mut stdout, &mut stderr)
            }
        },
        ParsedCommand::PijQuery(command) => pij::run(command, &context, &mut stdout, &mut stderr),
        ParsedCommand::Prep(command) => prep::run_prep(command, &mut stdout, &mut stderr),
        ParsedCommand::PrepCompact(command) => prep::run_compact(command, &mut stdout, &mut stderr),
        ParsedCommand::PrepRecord(command) => prep::run_record(command, &mut stdout, &mut stderr),
        ParsedCommand::Catalog(_)
        | ParsedCommand::NativeRootList(_)
        | ParsedCommand::NativeGitNotesList(_)
        | ParsedCommand::NativeExport(_) => {
            adapters::run(&command, &context, &mut stdout, &mut stderr)
        }
    };
    ExitCode::from(exit)
}

fn query_source(
    command: &QueryCommand,
    context: &CliContext,
) -> Result<git_query::Sources<unisphere_loader_git::GitObjectLoader>, QueryFailure> {
    let mut local = LocalQueryContext::new(env::consts::OS, BTreeMap::new());
    if let QueryScope::Offline { input } = &command.request.scope {
        if matches!(input, OfflineRef::Stdin) {
            let cap = command.request.limits.max_source_bytes;
            let mut bytes = Vec::new();
            io::stdin()
                .lock()
                .take(u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1))
                .read_to_end(&mut bytes)
                .map_err(|_| {
                    QueryFailure::new(
                        QueryFailureCode::UnreadableSource,
                        RecoveryAction::UseCompleteInput,
                    )
                })?;
            if bytes.len() > cap {
                return Err(QueryFailure::limit(LimitKind::SourceBytes));
            }
            local.stdin = Some(ProvidedInput {
                bytes: bytes.into(),
                format: command.stdin_format,
            });
        }
        // Saved inputs never acquire source roots, adapter instances, or Git.
        return Ok(git_query::Sources {
            local: LocalQuerySource::new(Vec::new(), local),
            identities: Vec::new(),
            git: None,
        });
    }
    for (base, variable) in [("home", "HOME"), ("appdata", "APPDATA")] {
        if let Some(path) = env::var_os(variable).map(PathBuf::from) {
            local.bases.insert(base.into(), path);
        }
    }
    if matches!(
        command.request.scope,
        QueryScope::Repository {
            scope: RepoScope::Worktrees,
            ..
        }
    ) {
        local.git_executable = find_executable("git", context);
    }
    let registrations = adapters::query_registrations()?;
    let identities = registrations
        .iter()
        .map(|entry| {
            (
                unisphere_sdk::query::AdapterId::new(entry.descriptor.id)
                    .expect("registered adapter"),
                entry.harness.clone(),
            )
        })
        .collect();
    let git = adapters::git_query_source(
        context.cwd.clone(),
        find_executable("git", context),
        command.output.clone(),
    );
    Ok(git_query::Sources {
        local: LocalQuerySource::new(registrations, local),
        identities,
        git,
    })
}

fn find_executable(name: &str, context: &CliContext) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = env::var_os("PATH")?;
        if path.is_empty() {
            return None;
        }
        env::split_paths(&path).find_map(|directory| {
            let directory = if directory.is_absolute() {
                directory
            } else {
                context.cwd.join(directory)
            };
            let executable = std::fs::canonicalize(directory.join(name)).ok()?;
            let metadata = executable.metadata().ok()?;
            (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(executable)
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (name, context);
        None
    }
}
