use std::{ffi::OsString, path::PathBuf};

use clap::{
    Args, ColorChoice, CommandFactory, FromArgMatches, Parser, Subcommand, error::ErrorKind,
};

#[derive(Parser)]
#[command(
    name = "unisphere",
    bin_name = "unisphere",
    about = "Inspect explicit configuration and export explicit session records",
    after_help = "Session commands: unisphere sessions list --root <leaf-project-directory>\n                  unisphere sessions export --input <file> [--include-content]\nSession exports are OTLP JSONL; no implicit source discovery or daemon.",
    disable_version_flag = true,
    disable_help_subcommand = true,
    color = ColorChoice::Never,
    term_width = 80
)]
struct Cli {
    /// Write one versioned JSON response to stdout
    #[arg(long, global = true, conflicts_with = "human")]
    json: bool,
    /// Write readable output, with failures on stderr
    #[arg(long, global = true, conflicts_with = "json")]
    human: bool,
    /// Print the application version
    #[arg(short = 'V', long, global = true)]
    version: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect configuration without opening source roots
    #[command(disable_help_subcommand = true)]
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Validate explicit settings and return the effective configuration
    Check(Check),
}

#[derive(Args)]
pub(crate) struct Check {
    /// Read this JSON file only; relative paths resolve against the supplied cwd
    #[arg(long, value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    /// Replace source roots; repeat this option to preserve order and duplicates
    #[arg(
        long = "source-root",
        value_name = "ROOT",
        conflicts_with = "clear_source_roots"
    )]
    pub(crate) source_roots: Option<Vec<String>>,
    /// Explicitly replace source roots with an empty list
    #[arg(long, conflicts_with = "source_roots")]
    pub(crate) clear_source_roots: bool,
}

pub(crate) enum Action {
    Check(Check),
    Help(String),
    Version,
    InvalidArguments,
}

pub(crate) fn parse(args: Vec<OsString>) -> Action {
    let matches = match Cli::command().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            return Action::Help(error.to_string());
        }
        // Clap errors can contain hostile argument values. Only generated help
        // is rendered; all other failures use the core-owned safe diagnostic.
        Err(_) => return Action::InvalidArguments,
    };
    let Ok(cli) = Cli::from_arg_matches(&matches) else {
        return Action::InvalidArguments;
    };
    if cli.version {
        return Action::Version;
    }
    match cli.command {
        Some(Command::Config {
            command: Some(ConfigCommand::Check(check)),
        }) => Action::Check(check),
        _ => Action::InvalidArguments,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Json,
    Human,
}

/// Select output even when clap cannot parse the invocation (including help).
/// Exact flags only: a value like `--source-root=--json` is not a mode switch,
/// and `--` ends option processing. Separate values cannot consume a hyphenated
/// mode flag in this grammar, so no second argument parser is needed here.
pub(crate) fn mode(args: &[OsString], stdout_is_terminal: bool) -> Mode {
    let mut human = false;
    for arg in args.iter().skip(1).take_while(|arg| *arg != "--") {
        if arg == "--json" {
            return Mode::Json;
        }
        if arg == "--human" {
            human = true;
        }
    }
    if human || stdout_is_terminal {
        Mode::Human
    } else {
        Mode::Json
    }
}
