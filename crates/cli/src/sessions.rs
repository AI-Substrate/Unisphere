//! Session commands use only the core application port; source adapters are selected by the app.
use crate::CliContext;
use clap::{Parser, Subcommand};
use serde_json::json;
use std::{ffi::OsString, fs::OpenOptions, io::Write, path::PathBuf};
use unisphere_core::{
    CollectionApi, MappingOptions, PipelineError, PipelineErrorKind, ReadCursor, ReadLimits,
    SessionRef, SourceScope,
};

#[derive(Parser)]
#[command(
    name = "unisphere sessions",
    about = "List an explicit leaf project directory or export source-derived OTLP JSONL; no implicit HOME scan"
)]
struct Arguments {
    #[command(subcommand)]
    command: SessionCommand,
}
#[derive(Subcommand)]
enum SessionCommand {
    /// List immediate .jsonl files only (not recursive); choose the Claude leaf project directory.
    List {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value_t = 4096)]
        max_sessions: usize,
    },
    /// Export one explicit file; content is omitted unless --include-content is given.
    Export {
        #[arg(long, default_value = "claude-code")]
        adapter: String,
        #[arg(long)]
        input: PathBuf,
        /// New destination file; existing files are never overwritten. Default: OTLP JSONL on stdout.
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        include_content: bool,
        #[arg(long, default_value_t = 128)]
        max_records: usize,
        #[arg(long, default_value_t = 1_048_576)]
        max_record_bytes: usize,
        #[arg(long, default_value_t = 4_194_304)]
        max_batch_bytes: usize,
    },
}

/// Inspect only the adapter selection for the composition root. Parsing and all
/// argument validation still occur in `run_sessions` before any collection I/O.
/// App registries can add another adapter name without changing the frontend.
pub fn requested_session_adapter(args: &[OsString]) -> &str {
    let mut selected = "claude-code";
    let mut iter = args.iter().skip(2);
    while let Some(arg) = iter.next() {
        if arg == "--adapter" {
            if let Some(value) = iter.next().and_then(|value| value.to_str()) {
                selected = value;
            }
        } else if let Some(value) = arg
            .to_str()
            .and_then(|value| value.strip_prefix("--adapter="))
        {
            selected = value;
        }
    }
    selected
}

fn absolute(path: PathBuf, context: &CliContext) -> Result<PathBuf, PipelineError> {
    let path = if path.is_absolute() {
        path
    } else {
        context.cwd.join(path)
    };
    SessionRef { path: path.clone() }.validate()?;
    Ok(path)
}

fn json_line(output: &mut dyn Write, value: &serde_json::Value) -> Result<(), PipelineError> {
    serde_json::to_writer(&mut *output, value)
        .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))
}

/// Emit safe diagnostics on stderr, never into a telemetry output stream.
pub fn session_error(stderr: &mut dyn Write, error: &PipelineError, exit: u8) -> u8 {
    let result = json_line(
        stderr,
        &json!({"ok":false,"command":"sessions","v":1,
        "error":{"kind":error.kind(),"code":error.code(),"message":error.message(),
        "fix":error.fix(),"offset":error.offset()}}),
    );
    if result.is_err() { 1 } else { exit }
}

/// Args include binary name and the `sessions` token. The supplied collector must
/// correspond to the name selected by `requested_session_adapter` in the app.
pub fn run_sessions(
    args: impl IntoIterator<Item = OsString>,
    context: &CliContext,
    collector: &dyn CollectionApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let args: Vec<_> = args.into_iter().collect();
    let parsed = Arguments::try_parse_from(
        std::iter::once(OsString::from("unisphere sessions")).chain(args.into_iter().skip(2)),
    );
    let parsed = match parsed {
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
        Err(_) => {
            return session_error(
                stderr,
                &PipelineError::new(PipelineErrorKind::InvalidInput, None),
                2,
            );
        }
    };
    let result = match parsed.command {
        SessionCommand::List { root, max_sessions } => (|| {
            let root = absolute(root, context)?;
            let scope = SourceScope { root, max_sessions };
            scope.validate()?;
            let sessions = collector.list_sessions(&scope)?;
            if sessions.is_empty() {
                json_line(
                    stderr,
                    &json!({"ok":true,"command":"sessions.list","v":1,
                    "note":"No immediate .jsonl files found; listing is not recursive, so select a leaf project directory."}),
                )?;
            }
            json_line(
                stdout,
                &json!({"ok":true,"command":"sessions.list","v":1,
                "data":{"root":scope.root,"recursive":false,"sessions":sessions}}),
            )
        })(),
        SessionCommand::Export {
            adapter,
            input,
            output,
            include_content,
            max_records,
            max_record_bytes,
            max_batch_bytes,
        } => (|| {
            let limits = ReadLimits {
                max_records,
                max_record_bytes,
                max_batch_bytes,
            };
            limits.validate()?;
            let session = SessionRef {
                path: absolute(input, context)?,
            };
            let output = output.map(|path| absolute(path, context)).transpose()?;
            let mut file = if let Some(path) = output.as_ref() {
                Some(
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)
                        .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?,
                )
            } else {
                None
            };
            let mut cursor: Option<ReadCursor> = None;
            let mut count = 0usize;
            let mut batches = 0usize;
            let mut diagnostics = 0usize;
            let incomplete_tail;
            loop {
                let batch = if let Some(file) = file.as_mut() {
                    collector.collect_batch(
                        &session,
                        cursor.as_ref(),
                        limits,
                        MappingOptions { include_content },
                        file,
                    )?
                } else {
                    collector.collect_batch(
                        &session,
                        cursor.as_ref(),
                        limits,
                        MappingOptions { include_content },
                        stdout,
                    )?
                };
                let prior = cursor.as_ref().map_or(0, |cursor| cursor.offset);
                if batch.next_cursor.source != session.path
                    || batch.next_cursor.offset < prior
                    || (batch.more && batch.next_cursor.offset == prior)
                {
                    return Err(PipelineError::new(
                        PipelineErrorKind::InvalidData,
                        Some(prior),
                    ));
                }
                count = count
                    .checked_add(batch.mapped.records.len())
                    .ok_or_else(|| PipelineError::new(PipelineErrorKind::BatchLimit, None))?;
                diagnostics = diagnostics
                    .checked_add(batch.mapped.diagnostics.len())
                    .ok_or_else(|| PipelineError::new(PipelineErrorKind::BatchLimit, None))?;
                batches += 1;
                cursor = Some(batch.next_cursor);
                if !batch.more {
                    incomplete_tail = batch.incomplete_tail;
                    break;
                }
            }
            json_line(
                stderr,
                &json!({"ok":true,"command":"sessions.export","v":1,
                "data":{"adapter":adapter,"records":count,"batches":batches,
                    "diagnostics":diagnostics,"incomplete_tail":incomplete_tail,
                    "offset":cursor.map(|cursor| cursor.offset),"output":output}}),
            )
        })(),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            let exit = if error.kind() == PipelineErrorKind::InvalidInput {
                2
            } else {
                1
            };
            session_error(stderr, &error, exit)
        }
    }
}
