//! Repository-note commands through typed root parsing and an injected application port.
use crate::{
    CliContext, NativeExportCommand, NativeGitNotesListCommand, ParsedCommand, output::StagedOutput,
};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use unisphere_core::{
    GitNoteSelection, GitNotesApi, GitNotesError, GitNotesLimits, GitNotesListing, GitNotesRequest,
    GitNotesScope, MAX_OUTPUT_BATCH_BYTES, MappingOptions, PipelineError, PipelineErrorKind,
};

/// Backwards-compatible Git Notes frontend using the crate's single root parser.
///
/// The app owns executable resolution and concrete composition; help and invalid
/// input never construct the Git Notes port.
pub fn run_git_notes<C: GitNotesApi>(
    args: Vec<OsString>,
    context: &CliContext,
    make_collector: impl FnOnce(Option<PathBuf>) -> Result<C, GitNotesError>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    match crate::parse(args, context) {
        Ok(ParsedCommand::Help(command)) => crate::run_help(&command, stdout, stderr),
        Ok(command) => run_native_git_notes(&command, make_collector, stdout, stderr),
        Err(_) => failure(stderr, &GitNotesError::InvalidInput, 2),
    }
}

/// Execute one already-parsed native Git Notes list or export command.
///
/// Only `NativeGitNotesList` and a `git-ai` `NativeExport` are accepted. The
/// constructor is called only after the typed DTO has passed all Git Notes
/// validation.
pub fn run_native_git_notes<C: GitNotesApi>(
    command: &ParsedCommand,
    make_collector: impl FnOnce(Option<PathBuf>) -> Result<C, GitNotesError>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let invocation = match Invocation::from_command(command) {
        Ok(invocation) => invocation,
        Err(error) => return failure(stderr, &error, 2),
    };
    let git_executable = invocation.git_executable().cloned();
    let collector = match make_collector(git_executable) {
        Ok(collector) => collector,
        Err(error) => return failure(stderr, &error, 1),
    };
    let result = match invocation {
        Invocation::List { scope, limits, .. } => list(&collector, &scope, limits, stdout),
        Invocation::Export {
            scope,
            limits,
            output,
            include_content,
            ..
        } => export(
            &collector,
            scope,
            limits,
            output,
            include_content,
            stdout,
            stderr,
        ),
    };
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

enum Invocation {
    List {
        scope: GitNotesScope,
        limits: GitNotesLimits,
        git_executable: Option<PathBuf>,
    },
    Export {
        scope: GitNotesScope,
        limits: GitNotesLimits,
        git_executable: Option<PathBuf>,
        output: Option<PathBuf>,
        include_content: bool,
    },
}

impl Invocation {
    fn from_command(command: &ParsedCommand) -> Result<Self, GitNotesError> {
        match command {
            ParsedCommand::NativeGitNotesList(command) => Self::list(command),
            ParsedCommand::NativeExport(command) if command.adapter == "git-ai" => {
                Self::export(command)
            }
            _ => Err(GitNotesError::InvalidInput),
        }
    }

    fn list(command: &NativeGitNotesListCommand) -> Result<Self, GitNotesError> {
        if command.adapter != "git-ai"
            || command
                .git_executable
                .as_ref()
                .is_some_and(|path| !absolute_utf8(path))
        {
            return Err(GitNotesError::InvalidInput);
        }
        let limits = GitNotesLimits {
            max_notes: command.max_notes,
            max_records: command.max_records,
            max_note_bytes: command.max_note_bytes,
            max_total_bytes: command.max_total_bytes,
            max_listing_bytes: command.max_listing_bytes,
            command_timeout_ms: command.command_timeout_ms,
        };
        let scope = scope(&command.repo, &command.notes_ref, &command.commits, limits)?;
        Ok(Self::List {
            scope,
            limits,
            git_executable: command.git_executable.clone(),
        })
    }

    fn export(command: &NativeExportCommand) -> Result<Self, GitNotesError> {
        if command.adapter != "git-ai"
            || command.input.is_some()
            || command.repo.is_none()
            || command.source_format.is_some()
            || command.table.is_some()
            || command.session_id.is_some()
            || command.max_record_bytes.is_some()
            || command.max_batch_bytes.is_some()
            || command.max_snapshot_bytes.is_some()
            || command.notes_ref.is_none()
            || command.max_records.is_none()
            || command.max_notes.is_none()
            || command.max_note_bytes.is_none()
            || command.max_total_bytes.is_none()
            || command.max_listing_bytes.is_none()
            || command.command_timeout_ms.is_none()
            || command
                .git_executable
                .as_ref()
                .is_some_and(|path| !absolute_utf8(path))
            || command
                .output
                .as_ref()
                .is_some_and(|path| !absolute_utf8(path))
        {
            return Err(GitNotesError::InvalidInput);
        }
        let limits = GitNotesLimits {
            max_notes: command.max_notes.ok_or(GitNotesError::InvalidInput)?,
            max_records: command.max_records.ok_or(GitNotesError::InvalidInput)?,
            max_note_bytes: command.max_note_bytes.ok_or(GitNotesError::InvalidInput)?,
            max_total_bytes: command.max_total_bytes.ok_or(GitNotesError::InvalidInput)?,
            max_listing_bytes: command
                .max_listing_bytes
                .ok_or(GitNotesError::InvalidInput)?,
            command_timeout_ms: command
                .command_timeout_ms
                .ok_or(GitNotesError::InvalidInput)?,
        };
        let scope = scope(
            command.repo.as_ref().ok_or(GitNotesError::InvalidInput)?,
            command
                .notes_ref
                .as_deref()
                .ok_or(GitNotesError::InvalidInput)?,
            &command.commits,
            limits,
        )?;
        Ok(Self::Export {
            scope,
            limits,
            git_executable: command.git_executable.clone(),
            output: command.output.clone(),
            include_content: command.include_content,
        })
    }

    fn git_executable(&self) -> Option<&PathBuf> {
        match self {
            Self::List { git_executable, .. } | Self::Export { git_executable, .. } => {
                git_executable.as_ref()
            }
        }
    }
}

fn scope(
    repository: &Path,
    notes_ref: &str,
    commits: &[String],
    limits: GitNotesLimits,
) -> Result<GitNotesScope, GitNotesError> {
    let scope = GitNotesScope {
        repository: repository.to_owned(),
        notes_ref: notes_ref.to_owned(),
        selection: if commits.is_empty() {
            GitNoteSelection::All
        } else {
            GitNoteSelection::Commits(commits.to_vec())
        },
    };
    scope.validate(limits)?;
    Ok(scope)
}

fn list(
    collector: &dyn GitNotesApi,
    scope: &GitNotesScope,
    limits: GitNotesLimits,
    stdout: &mut dyn Write,
) -> Result<(), GitNotesError> {
    let listing = collector.list_notes(scope, limits)?;
    let next_action = if listing.notes.is_empty() {
        action(
            "No commit-attached notes matched. Choose another explicit notes ref or commit selection.",
            &[
                "unisphere",
                "sessions",
                "list",
                "--adapter",
                "git-ai",
                "--repo",
            ],
            &["repository_path"],
        )
    } else {
        action(
            "Export the bounded Git Notes selection as attribution OTLP JSONL.",
            &[
                "unisphere",
                "sessions",
                "export",
                "--adapter",
                "git-ai",
                "--repo",
            ],
            &["repository_path"],
        )
    };
    json_line(
        stdout,
        &json!({
            "ok":true,
            "command":"sessions.list",
            "v":1,
            "data":{"adapter":"git-ai","listing":listing},
            "next_action":next_action
        }),
    )
}

fn export(
    collector: &dyn GitNotesApi,
    scope: GitNotesScope,
    limits: GitNotesLimits,
    output: Option<PathBuf>,
    include_content: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), GitNotesError> {
    let request = GitNotesRequest {
        scope,
        limits,
        options: MappingOptions { include_content },
    };
    let collection = if let Some(output) = output {
        let listing = collector.list_notes(&request.scope, limits)?;
        let path = output_path(output, &listing)?;
        let mut staged = StagedOutput::create(&path).map_err(output_error)?;
        let collection = collector.collect_notes(&request, staged.writer())?;
        staged.publish().map_err(output_error)?;
        collection
    } else {
        collector.collect_notes(&request, stdout)?
    };
    json_line(
        stderr,
        &json!({
            "ok":true,
            "command":"sessions.export",
            "v":1,
            "data":{
                "adapter":"git-ai",
                "records":collection.records_written,
                "notes":collection.listing.notes.len(),
                "notes_ref":collection.listing.notes_ref,
                "notes_tip":collection.listing.notes_tip,
                "semantics":"replace_projection",
                "finality":"unknown",
                "persisted_resume":false
            },
            "next_action":action(
                "Treat this output as a current attribution projection, then read the bundled output and schema guidance before downstream use.",
                &["unisphere", "docs", "get", "output-and-schema", "--human"],
                &[],
            )
        }),
    )
}

fn absolute_utf8(path: &Path) -> bool {
    path.is_absolute() && path.to_str().is_some()
}

fn output_path(path: PathBuf, listing: &GitNotesListing) -> Result<PathBuf, GitNotesError> {
    let leaf = path.file_name().ok_or(GitNotesError::InvalidInput)?;
    let parent = fs::canonicalize(path.parent().ok_or(GitNotesError::InvalidInput)?)
        .map_err(output_error)?;
    let path = parent.join(leaf);
    let roots = [
        Some(listing.repository_id.as_path()),
        Some(listing.git_dir.as_path()),
        listing.worktree_root.as_deref(),
    ];
    if !absolute_utf8(&path)
        || roots
            .into_iter()
            .flatten()
            .any(|root: &Path| path.starts_with(root))
    {
        return Err(GitNotesError::InvalidInput);
    }
    Ok(path)
}

fn output_error(_: std::io::Error) -> GitNotesError {
    PipelineError::new(PipelineErrorKind::Write, None).into()
}

fn action(summary: &str, argv: &[&str], required_inputs: &[&str]) -> Value {
    json!({
        "summary":summary,
        "argv":argv,
        "required_inputs":required_inputs,
    })
}

fn failure_action(error: &GitNotesError) -> Value {
    match error {
        GitNotesError::InvalidInput => action(
            "Use one typed native Git Notes command with absolute paths, a refs/notes/* ref, full lowercase object IDs, and bounded positive limits.",
            &["unisphere", "sessions", "list", "--help"],
            &[],
        ),
        GitNotesError::InvalidRef => action(
            "Choose an existing commit-valued refs/notes/* ref and retry the same bounded operation.",
            &[
                "unisphere",
                "sessions",
                "list",
                "--adapter",
                "git-ai",
                "--repo",
                "--notes-ref",
            ],
            &["repository_path", "notes_ref"],
        ),
        GitNotesError::GitUnavailable => action(
            "Supply an absolute standard Git executable path; Git AI is not required.",
            &[
                "unisphere",
                "sessions",
                "list",
                "--adapter",
                "git-ai",
                "--repo",
                "--git-executable",
            ],
            &["repository_path", "git_executable_path"],
        ),
        GitNotesError::UnsafeRepository => action(
            "Use a caller-owned checkout or have the operator establish Git trust separately.",
            &["unisphere", "docs", "get", "troubleshooting", "--human"],
            &[],
        ),
        GitNotesError::ObjectRead => action(
            "Retry with a complete readable local repository and an explicit notes ref or commit selection.",
            &[
                "unisphere",
                "sessions",
                "list",
                "--adapter",
                "git-ai",
                "--repo",
            ],
            &["repository_path"],
        ),
        GitNotesError::Timeout => action(
            "Narrow the commit selection or choose a bounded command timeout and retry.",
            &[
                "unisphere",
                "sessions",
                "list",
                "--adapter",
                "git-ai",
                "--repo",
                "--command-timeout-ms",
            ],
            &["repository_path", "timeout_ms"],
        ),
        GitNotesError::UnsupportedPlatform => action(
            "Run local Git-object collection on a supported Unix host; supplied-data mapping remains portable.",
            &["unisphere", "docs", "get", "troubleshooting", "--human"],
            &[],
        ),
        GitNotesError::UnsupportedRepository => action(
            "Use a complete local repository; Unisphere refuses partial-clone reads that could fetch implicitly.",
            &["unisphere", "docs", "get", "troubleshooting", "--human"],
            &[],
        ),
        GitNotesError::UnsupportedTarget => action(
            "Select only notes attached to commit objects.",
            &[
                "unisphere",
                "sessions",
                "list",
                "--adapter",
                "git-ai",
                "--repo",
                "--commit",
            ],
            &["repository_path", "commit_id"],
        ),
        GitNotesError::UnsupportedFormat | GitNotesError::InvalidData => action(
            "Inspect the supported Git Notes attribution schema and retry with valid local note data.",
            &["unisphere", "docs", "get", "output-and-schema", "--human"],
            &[],
        ),
        GitNotesError::NoteLimit => action(
            "Select smaller notes or increase both compatible note and total byte bounds.",
            &["unisphere", "sessions", "export", "--help"],
            &[],
        ),
        GitNotesError::ListingLimit => action(
            "Select specific commits or increase the compatible note and listing bounds.",
            &["unisphere", "sessions", "list", "--help"],
            &[],
        ),
        GitNotesError::BatchLimit => action(
            "Select fewer commits or increase the total native-byte bound.",
            &["unisphere", "sessions", "export", "--help"],
            &[],
        ),
        GitNotesError::RecordLimit => action(
            "Select fewer commits or increase the mapped-record bound.",
            &["unisphere", "sessions", "export", "--help"],
            &[],
        ),
        GitNotesError::Output(_) => action(
            "Discard incomplete destination bytes, choose a new writable outside-source path, and retry.",
            &["unisphere", "sessions", "export", "--help"],
            &[],
        ),
    }
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
        .map_err(output_error)?;
    Ok(())
}

fn failure(stderr: &mut dyn Write, error: &GitNotesError, exit: u8) -> u8 {
    let output_code = match error {
        GitNotesError::Output(error) => Some(error.code()),
        _ => None,
    };
    if json_line(
        stderr,
        &json!({
            "ok":false,
            "command":"sessions",
            "v":1,
            "error":{
                "kind":error.kind(),
                "message":error.message(),
                "output_code":output_code
            },
            "next_action":failure_action(error)
        }),
    )
    .is_err()
    {
        1
    } else {
        exit
    }
}
