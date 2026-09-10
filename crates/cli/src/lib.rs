//! Explicit-context command-line frontend for configuration, native export, and queries.
//!
//! The caller supplies argv, process context, and application ports. Parsing and
//! bundled docs/schema access acquire no configuration, filesystem, Git, process,
//! environment, clock, or network capability.
#![forbid(unsafe_code)]

use std::{ffi::OsString, io::Write};

use unisphere_core::{ConfigOverrides, ConfigSource, Failure, InspectionApi, InspectionRequest};

mod args;
mod catalog;
mod docs;
mod output;
mod query;
pub mod sessions;
pub mod snapshots;

pub use args::{
    CatalogCommand, CliParseFailure, ConfigCommand, DocsCommand, HelpCommand, NativeExportCommand,
    NativeGitNotesListCommand, NativeRootListCommand, OutputMode, ParsedCommand, QueryCommand,
    SchemaCommand, diagnostic_mode, parse,
};
pub use catalog::{run_adapters, run_catalog};
pub use query::{emit_parse_failure, emit_query_failure, run_docs, run_query, run_schema};
pub use sessions::{run_native_export, run_native_list, run_sessions, session_error};
pub use snapshots::{run_native_snapshot_export, run_snapshot_sessions};
pub use unisphere_core::{AdapterCapabilities, AdapterDescriptor, LocationHint};

use output::Response;

/// Process context captured by the executable, never discovered by the frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliContext {
    /// Absolute working directory used to resolve explicit relative paths.
    pub cwd: std::path::PathBuf,
    /// Whether stdout is a terminal; explicit output flags take precedence.
    pub stdout_is_terminal: bool,
    /// Application version returned by `--version`.
    pub version: String,
}

/// Execute one parsed configuration command through the injected inspector.
pub fn run_config(
    command: &ConfigCommand,
    context: &CliContext,
    inspector: &dyn InspectionApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let source = match command.config.as_ref() {
        None => ConfigSource::Defaults,
        Some(path) if path.is_absolute() => ConfigSource::File(path.clone()),
        Some(path) if context.cwd.is_absolute() => ConfigSource::File(context.cwd.join(path)),
        Some(_) => {
            let failure = Failure::invalid_arguments(None);
            return output::emit(Response::Failure(&failure), command.mode, stdout, stderr);
        }
    };
    let overrides = ConfigOverrides {
        source_roots: if command.clear_source_roots {
            Some(Vec::new())
        } else {
            command.source_roots.clone()
        },
    };
    match inspector.inspect(&InspectionRequest { source, overrides }) {
        Ok(report) => output::emit(Response::Report(&report), command.mode, stdout, stderr),
        Err(failure) => output::emit(Response::Failure(&failure), command.mode, stdout, stderr),
    }
}

/// Render already-parsed help without acquiring any application port.
pub fn run_help(command: &HelpCommand, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    output::emit(Response::Help(&command.text), command.mode, stdout, stderr)
}

/// Render the executable-supplied version without acquiring any application port.
pub fn run_version(
    context: &CliContext,
    mode: OutputMode,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    output::emit(Response::Version(&context.version), mode, stdout, stderr)
}

/// Backwards-compatible configuration frontend.
///
/// New composition roots should call [`parse`] once and dispatch [`ParsedCommand`]
/// through the relevant injected port. This function remains for existing SDK
/// consumers of configuration inspection.
pub fn run(
    args: impl IntoIterator<Item = OsString>,
    context: &CliContext,
    inspector: &dyn InspectionApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let args: Vec<_> = args.into_iter().collect();
    let fallback_mode = args::diagnostic_mode(&args, context.stdout_is_terminal);
    match args::parse(args, context) {
        Ok(ParsedCommand::Help(help)) => run_help(&help, stdout, stderr),
        Ok(ParsedCommand::Version { mode }) => run_version(context, mode, stdout, stderr),
        Ok(ParsedCommand::Config(command)) => {
            run_config(&command, context, inspector, stdout, stderr)
        }
        Ok(_) | Err(_) => {
            let failure = Failure::invalid_arguments(None);
            output::emit(Response::Failure(&failure), fallback_mode, stdout, stderr)
        }
    }
}
