//! Catalog commands render supplied metadata without consulting source stores.
use std::ffi::OsString;
use std::io::Write;

use clap::{ColorChoice, Parser, Subcommand};
use unisphere_core::{AdapterDescriptor, Failure};

use crate::{
    CliContext, args,
    output::{self, Response},
};

#[derive(Parser)]
#[command(
    name = "unisphere adapters",
    about = "Describe registered adapters and symbolic location hints; no source discovery",
    disable_help_subcommand = true,
    color = ColorChoice::Never,
    term_width = 80
)]
struct Arguments {
    /// Write one versioned JSON response.
    #[arg(long, global = true, conflicts_with = "human")]
    json: bool,
    /// Write readable descriptions and symbolic location hints.
    #[arg(long, global = true, conflicts_with = "json")]
    human: bool,
    #[command(subcommand)]
    command: CatalogCommand,
}

#[derive(Subcommand)]
enum CatalogCommand {
    /// List registered production adapters, not detected installations.
    List,
}

/// Render an injected catalog. Args include the binary name and `adapters`.
/// No loader, inspector, environment lookup or source path is acquired here.
/// Returns 0 for success/help, 2 for invalid arguments, and 1 for output failure.
pub fn run_adapters(
    args: Vec<OsString>,
    context: &CliContext,
    adapters: &[&AdapterDescriptor],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let mode = args::mode(&args, context.stdout_is_terminal);
    let parsed = Arguments::try_parse_from(
        std::iter::once(OsString::from("unisphere adapters")).chain(args.into_iter().skip(2)),
    );
    match parsed {
        Ok(Arguments {
            command: CatalogCommand::List,
            ..
        }) => output::emit(Response::Catalog(adapters), mode, stdout, stderr),
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => {
            output::emit(Response::Help(&error.to_string()), mode, stdout, stderr)
        }
        Err(_) => output::emit(
            Response::CatalogFailure(&Failure::invalid_arguments(None)),
            mode,
            stdout,
            stderr,
        ),
    }
}
