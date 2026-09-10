//! Repository-note commands through an injected constructor and core application port.
use crate::CliContext;
use clap::{Args, ColorChoice, Parser, Subcommand};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use unisphere_core::{
    GitNoteSelection, GitNotesApi, GitNotesError, GitNotesLimits, GitNotesListing, GitNotesRequest,
    GitNotesScope, MAX_OUTPUT_BATCH_BYTES, MappingOptions, PipelineError, PipelineErrorKind,
};

#[derive(Parser)]
#[command(name = "unisphere sessions", color = ColorChoice::Never, about = "Read one explicitly selected local Git Notes ref; standard Git required, Git AI not required")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// List commit-attached notes and their pinned Git object identities.
    List {
        #[command(flatten)]
        source: SourceArguments,
    },
    /// Export the complete bounded selection as attribution OTLP JSONL, not a conversation archive.
    Export {
        #[command(flatten)]
        source: SourceArguments,
        /// New file outside the source worktree and Git directories. Default: stdout.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Include human-author strings, custom attributes and legacy message content/URLs.
        #[arg(long)]
        include_content: bool,
    },
}
#[derive(Args)]
struct SourceArguments {
    #[arg(long)]
    adapter: String,
    #[arg(long)]
    repo: PathBuf,
    #[arg(long, default_value = "refs/notes/ai")]
    notes_ref: String,
    /// Full lowercase commit object ID; repeat to select several. Default: all notes in the selected ref.
    #[arg(long)]
    commit: Vec<String>,
    /// Absolute standard Git executable; otherwise resolved by the app from PATH.
    #[arg(long)]
    git_executable: Option<PathBuf>,
    #[arg(long, default_value_t = 1000)]
    max_notes: usize,
    #[arg(long, default_value_t = 10_000)]
    max_records: usize,
    #[arg(long, default_value_t = 1_048_576)]
    max_note_bytes: usize,
    #[arg(long, default_value_t = 16_777_216)]
    max_total_bytes: usize,
    #[arg(long, default_value_t = 1_048_576)]
    max_listing_bytes: usize,
    #[arg(long, default_value_t = 5000)]
    command_timeout_ms: u64,
}

/// The app owns executable resolution and concrete composition; help/invalid input never construct it.
pub fn run_git_notes<C: GitNotesApi>(
    args: Vec<OsString>,
    context: &CliContext,
    make_collector: impl FnOnce(Option<PathBuf>) -> Result<C, GitNotesError>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let parsed = match Arguments::try_parse_from(
        std::iter::once(OsString::from("unisphere sessions")).chain(args.into_iter().skip(2)),
    ) {
        Ok(parsed) => parsed,
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => {
            return if stdout
                .write_all(error.to_string().as_bytes())
                .and_then(|()| stdout.flush())
                .is_ok()
            {
                0
            } else {
                1
            };
        }
        Err(_) => return failure(stderr, &GitNotesError::InvalidInput, 2),
    };
    let (source, output, include_content, list_only) = match parsed.command {
        Command::List { source } => (source, None, false, true),
        Command::Export {
            source,
            output,
            include_content,
        } => (source, output, include_content, false),
    };
    let limits = GitNotesLimits {
        max_notes: source.max_notes,
        max_records: source.max_records,
        max_note_bytes: source.max_note_bytes,
        max_total_bytes: source.max_total_bytes,
        max_listing_bytes: source.max_listing_bytes,
        command_timeout_ms: source.command_timeout_ms,
    };
    let scope = GitNotesScope {
        repository: absolute(source.repo, context),
        notes_ref: source.notes_ref,
        selection: if source.commit.is_empty() {
            GitNoteSelection::All
        } else {
            GitNoteSelection::Commits(source.commit)
        },
    };
    if source.adapter != "git-ai"
        || source
            .git_executable
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
    {
        return failure(stderr, &GitNotesError::InvalidInput, 2);
    }
    if let Err(error) = scope.validate(limits) {
        return failure(stderr, &error, 2);
    }
    let collector = match make_collector(source.git_executable) {
        Ok(collector) => collector,
        Err(error) => return failure(stderr, &error, 1),
    };
    let result = (|| -> Result<(), GitNotesError> {
        if list_only {
            let listing = collector.list_notes(&scope, limits)?;
            return json_line(
                stdout,
                &json!({"ok":true,"command":"sessions.list","v":1,"data":{"adapter":"git-ai","listing":listing}}),
            );
        }
        let request = GitNotesRequest {
            scope,
            limits,
            options: MappingOptions { include_content },
        };
        let collection = if let Some(output) = output {
            let listing = collector.list_notes(&request.scope, limits)?;
            let path = output_path(absolute(output, context), &listing)?;
            let mut destination = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
            collector.collect_notes(&request, &mut destination)?
        } else {
            collector.collect_notes(&request, stdout)?
        };
        json_line(
            stderr,
            &json!({"ok":true,"command":"sessions.export","v":1,"data":{
                "adapter":"git-ai","records":collection.records_written,"notes":collection.listing.notes.len(),
                "notes_ref":collection.listing.notes_ref,"notes_tip":collection.listing.notes_tip,
                "semantics":"replace_projection","finality":"unknown","persisted_resume":false
            }}),
        )
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            let exit = if error == GitNotesError::InvalidInput {
                2
            } else {
                1
            };
            failure(stderr, &error, exit)
        }
    }
}
fn absolute(path: PathBuf, context: &CliContext) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        context.cwd.join(path)
    }
}
fn output_path(path: PathBuf, listing: &GitNotesListing) -> Result<PathBuf, GitNotesError> {
    let leaf = path.file_name().ok_or(GitNotesError::InvalidInput)?;
    let parent = fs::canonicalize(path.parent().ok_or(GitNotesError::InvalidInput)?)
        .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
    let path = parent.join(leaf);
    let roots = [
        Some(listing.repository_id.as_path()),
        Some(listing.git_dir.as_path()),
        listing.worktree_root.as_deref(),
    ];
    if !path.is_absolute()
        || path.to_str().is_none()
        || roots
            .into_iter()
            .flatten()
            .any(|root: &Path| path.starts_with(root))
    {
        return Err(GitNotesError::InvalidInput);
    }
    Ok(path)
}
fn json_line(destination: &mut dyn Write, value: &Value) -> Result<(), GitNotesError> {
    let mut bytes = serde_json::to_vec(value)
        .map_err(|_| PipelineError::new(PipelineErrorKind::InvalidData, None))?;
    if bytes.len() >= MAX_OUTPUT_BATCH_BYTES {
        return Err(PipelineError::new(PipelineErrorKind::OutputLimit, None).into());
    }
    bytes.push(b'\n');
    destination
        .write_all(&bytes)
        .and_then(|()| destination.flush())
        .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
    Ok(())
}
fn failure(stderr: &mut dyn Write, error: &GitNotesError, exit: u8) -> u8 {
    let output_code = match error {
        GitNotesError::Output(error) => Some(error.code()),
        _ => None,
    };
    if json_line(
        stderr,
        &json!({"ok":false,"command":"sessions","v":1,"error":{
            "kind":error.kind(),"message":error.message(),"output_code":output_code
        }}),
    )
    .is_err()
    {
        1
    } else {
        exit
    }
}
