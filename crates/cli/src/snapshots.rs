//! Explicit snapshot export through an injected core application port.
use crate::{CliContext, session_error};
use clap::{ColorChoice, Parser, Subcommand, ValueEnum};
use serde_json::json;
use std::{ffi::OsString, fs::OpenOptions, io::Write, path::PathBuf};
use unisphere_core::{
    MappingOptions, PipelineError, PipelineErrorKind, SnapshotCollectionApi, SnapshotFormat,
    SnapshotLimits, SnapshotRef, SnapshotRequest,
};

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    JsonDocument,
    JsonJournal,
    SqliteKeyValue,
}
#[derive(Parser)]
#[command(name = "unisphere sessions", color = ColorChoice::Never,
    about = "Export one explicit native snapshot revision; no persisted resume or implicit discovery")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Write a full revision projection and closing replacement manifest as OTLP JSONL.
    Export {
        #[arg(long)]
        adapter: String,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        include_content: bool,
        /// Explicit storage representation; defaults to the selected registered adapter.
        #[arg(long)]
        source_format: Option<Format>,
        /// SQLite key/value table; valid only for sqlite-key-value sources.
        #[arg(long)]
        table: Option<String>,
        /// Native logical session selection, interpreted only by the pure mapper.
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long, default_value_t = 100_000)]
        max_records: usize,
        #[arg(long, default_value_t = 33_554_432)]
        max_record_bytes: usize,
        #[arg(long, default_value_t = 67_108_864)]
        max_snapshot_bytes: usize,
    },
}

/// Use the selected snapshot collector; no append cursor or resume file is invented.
pub fn run_snapshot_sessions(
    args: Vec<OsString>,
    context: &CliContext,
    collector: &dyn SnapshotCollectionApi,
    default_format: SnapshotFormat,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
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
    let Command::Export {
        adapter,
        input,
        output,
        include_content,
        source_format,
        table,
        session_id,
        max_records,
        max_record_bytes,
        max_snapshot_bytes,
    } = parsed.command;
    let format = match (source_format, default_format, table) {
        (
            None | Some(Format::SqliteKeyValue),
            SnapshotFormat::SqliteKeyValue { table },
            override_table,
        ) => SnapshotFormat::SqliteKeyValue {
            table: override_table.unwrap_or(table),
        },
        (Some(Format::SqliteKeyValue), _, Some(table)) => SnapshotFormat::SqliteKeyValue { table },
        (Some(Format::JsonDocument), _, None) => SnapshotFormat::JsonDocument,
        (Some(Format::JsonJournal), _, None) => SnapshotFormat::JsonJournal,
        (None, format, None) => format,
        _ => {
            return session_error(
                stderr,
                &PipelineError::new(PipelineErrorKind::InvalidInput, None),
                2,
            );
        }
    };
    let request = SnapshotRequest {
        source: SnapshotRef {
            path: if input.is_absolute() {
                input
            } else {
                context.cwd.join(input)
            },
            format,
            session_id,
        },
        limits: SnapshotLimits {
            max_records,
            max_record_bytes,
            max_snapshot_bytes,
        },
        options: MappingOptions { include_content },
    };
    if let Err(error) = request
        .source
        .validate()
        .and_then(|()| request.limits.validate())
    {
        return session_error(stderr, &error, 2);
    }
    let result = if let Some(output) = output {
        let path = if output.is_absolute() {
            output
        } else {
            context.cwd.join(output)
        };
        if !path.is_absolute() || path.to_str().is_none() {
            return session_error(
                stderr,
                &PipelineError::new(PipelineErrorKind::InvalidInput, None),
                2,
            );
        }
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut destination) => collector.collect_snapshot(&request, &mut destination),
            Err(_) => Err(PipelineError::new(PipelineErrorKind::Write, None)),
        }
    } else {
        collector.collect_snapshot(&request, stdout)
    };
    match result {
        Ok(collection) => {
            let summary = json!({"ok":true,"command":"sessions.export","v":1,"data":{
                "adapter":adapter,"records":collection.records_written,
                "diagnostics":collection.diagnostics.len(),"revision":collection.checkpoint.revision,
                "semantics":"replace_projection","finality":"unknown","persisted_resume":false
            }});
            if serde_json::to_writer(&mut *stderr, &summary).is_err()
                || stderr
                    .write_all(b"\n")
                    .and_then(|()| stderr.flush())
                    .is_err()
            {
                1
            } else {
                0
            }
        }
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
