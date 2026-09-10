//! Catalog commands render supplied metadata without consulting source stores.
use std::{ffi::OsString, io::Write};

use unisphere_core::{AdapterDescriptor, Failure};

use crate::{
    CatalogCommand, CliContext, ParsedCommand, args,
    output::{self, Response},
    run_help,
};

/// Render an already-parsed injected catalog without acquiring any provider.
pub fn run_catalog(
    command: &CatalogCommand,
    adapters: &[&AdapterDescriptor],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    output::emit(Response::Catalog(adapters), command.mode, stdout, stderr)
}

/// Backwards-compatible catalog frontend implemented through the sole root parser.
pub fn run_adapters(
    args: Vec<OsString>,
    context: &CliContext,
    adapters: &[&AdapterDescriptor],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let fallback_mode = args::diagnostic_mode(&args, context.stdout_is_terminal);
    match args::parse(args, context) {
        Ok(ParsedCommand::Catalog(command)) => run_catalog(&command, adapters, stdout, stderr),
        Ok(ParsedCommand::Help(help)) => run_help(&help, stdout, stderr),
        Ok(_) | Err(_) => output::emit(
            Response::CatalogFailure(&Failure::invalid_arguments(None)),
            fallback_mode,
            stdout,
            stderr,
        ),
    }
}
