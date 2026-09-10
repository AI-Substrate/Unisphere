//! Configuration, session and catalog commands over injected core data and ports.
//!
//! This is a frontend library, not an executable or an SDK composition root.
//! The caller supplies argv (including argv[0]), working directory, terminal
//! status, version and application ports. Session export opens only an explicit
//! new output file; source loading stays behind the injected collection port.
#![forbid(unsafe_code)]

use std::{ffi::OsString, io::Write, path::PathBuf};

use unisphere_core::{ConfigOverrides, ConfigSource, Failure, InspectionApi, InspectionRequest};

mod args;
mod catalog;
pub mod git_notes;
mod output;
pub mod sessions;
pub mod snapshots;
pub use catalog::run_adapters;
pub use git_notes::run_git_notes;
pub use sessions::{requested_session_adapter, run_sessions, session_error};
pub use snapshots::run_snapshot_sessions;
pub use unisphere_core::{AdapterCapabilities, AdapterDescriptor, LocationHint};

use args::Action;
use output::Response;

/// Process context captured by the executable, never discovered by the frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliContext {
    /// Absolute working directory used to resolve an explicit relative config path.
    pub cwd: PathBuf,
    /// Whether stdout is a terminal; explicit output flags take precedence.
    pub stdout_is_terminal: bool,
    /// Application version returned by `--version` (not the frontend's version).
    pub version: String,
}

/// Run one invocation without exiting the process or acquiring ambient context.
///
/// Arguments include argv[0]. Returns 0 for success/help/version, 1 for
/// configuration, input-I/O or output-I/O failures, and 2 for invalid arguments.
/// Help/version and invalid invocations never call the inspector. The caller
/// owns the writers; completed output is flushed but they are not closed.
pub fn run(
    args: impl IntoIterator<Item = OsString>,
    context: &CliContext,
    inspector: &dyn InspectionApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let args: Vec<_> = args.into_iter().collect();
    let mode = args::mode(&args, context.stdout_is_terminal);
    match args::parse(args) {
        Action::Help(text) => output::emit(Response::Help(&text), mode, stdout, stderr),
        Action::Version => output::emit(Response::Version(&context.version), mode, stdout, stderr),
        Action::InvalidArguments => output::emit(
            Response::Failure(&Failure::invalid_arguments(None)),
            mode,
            stdout,
            stderr,
        ),
        Action::Check(check) => {
            let source = match check.config {
                None => ConfigSource::Defaults,
                Some(path) if path.is_absolute() => ConfigSource::File(path),
                Some(path) if context.cwd.is_absolute() => {
                    ConfigSource::File(context.cwd.join(path))
                }
                Some(_) => {
                    return output::emit(
                        Response::Failure(&Failure::invalid_arguments(None)),
                        mode,
                        stdout,
                        stderr,
                    );
                }
            };
            let overrides = ConfigOverrides {
                source_roots: if check.clear_source_roots {
                    Some(Vec::new())
                } else {
                    check.source_roots
                },
            };
            match inspector.inspect(&InspectionRequest { source, overrides }) {
                Ok(report) => output::emit(Response::Report(&report), mode, stdout, stderr),
                Err(failure) => output::emit(Response::Failure(&failure), mode, stdout, stderr),
            }
        }
    }
}
