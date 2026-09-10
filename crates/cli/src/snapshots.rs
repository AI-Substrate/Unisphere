//! Explicit snapshot export through an injected core application port.
use crate::{
    CliContext, NativeExportCommand, ParsedCommand, args, output::StagedOutput, run_help,
    session_error,
};
use serde_json::json;
use std::{ffi::OsString, io::Write};
use unisphere_core::{
    MappingOptions, PipelineError, PipelineErrorKind, SnapshotCollectionApi, SnapshotFormat,
    SnapshotLimits, SnapshotRef, SnapshotRequest,
};

/// Execute one parsed native replacement-snapshot export.
pub fn run_native_snapshot_export(
    command: &NativeExportCommand,
    collector: &dyn SnapshotCollectionApi,
    default_format: SnapshotFormat,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let result = (|| {
        if command.adapter == "git-ai"
            || command.repo.is_some()
            || command
                .input
                .as_ref()
                .is_none_or(|path| !path.is_absolute())
            || command
                .output
                .as_ref()
                .is_some_and(|path| !path.is_absolute())
            || command.max_batch_bytes.is_some()
            || command.notes_ref.is_some()
            || !command.commits.is_empty()
            || command.git_executable.is_some()
            || command.max_notes.is_some()
            || command.max_note_bytes.is_some()
            || command.max_total_bytes.is_some()
            || command.max_listing_bytes.is_some()
            || command.command_timeout_ms.is_some()
        {
            return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None));
        }
        let format = match (
            command.source_format.as_deref(),
            default_format,
            command.table.clone(),
        ) {
            (
                None | Some("sqlite-key-value"),
                SnapshotFormat::SqliteKeyValue { table },
                override_table,
            ) => SnapshotFormat::SqliteKeyValue {
                table: override_table.unwrap_or(table),
            },
            (Some("sqlite-key-value"), _, Some(table)) => SnapshotFormat::SqliteKeyValue { table },
            (Some("json-document"), _, None) => SnapshotFormat::JsonDocument,
            (Some("json-journal"), _, None) => SnapshotFormat::JsonJournal,
            (None, format, None) => format,
            _ => return Err(PipelineError::new(PipelineErrorKind::InvalidInput, None)),
        };
        let request = SnapshotRequest {
            source: SnapshotRef {
                path: command
                    .input
                    .clone()
                    .expect("validated native snapshot input"),
                format,
                session_id: command.session_id.clone(),
            },
            limits: SnapshotLimits {
                max_records: command.max_records.unwrap_or(100_000),
                max_record_bytes: command.max_record_bytes.unwrap_or(33_554_432),
                max_snapshot_bytes: command.max_snapshot_bytes.unwrap_or(67_108_864),
            },
            options: MappingOptions {
                include_content: command.include_content,
            },
        };
        request.source.validate()?;
        request.limits.validate()?;
        let collection = if let Some(path) = &command.output {
            let mut destination = StagedOutput::create(path)
                .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
            let collection = collector.collect_snapshot(&request, destination.writer())?;
            destination
                .publish()
                .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
            collection
        } else {
            collector.collect_snapshot(&request, stdout)?
        };
        serde_json::to_writer(
            &mut *stderr,
            &json!({"ok":true,"command":"sessions.export","v":1,"data":{
                "adapter":command.adapter,"records":collection.records_written,
                "diagnostics":collection.diagnostics.len(),"revision":collection.checkpoint.revision,
                "semantics":"replace_projection","finality":"unknown","persisted_resume":false,
                "output":command.output
            },"next_action":{"summary":"Import the completed replacement projection as OTLP JSONL while retaining its native revision and unknown-finality qualification.",
                "argv":["unisphere","adapters","list","--json"],"required_inputs":[]}}),
        )
        .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
        stderr
            .write_all(b"\n")
            .and_then(|()| stderr.flush())
            .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))
    })();
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

/// Backwards-compatible snapshot frontend implemented through the sole root parser.
pub fn run_snapshot_sessions(
    args: Vec<OsString>,
    context: &CliContext,
    collector: &dyn SnapshotCollectionApi,
    default_format: SnapshotFormat,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    match args::parse(args, context) {
        Ok(ParsedCommand::NativeExport(command)) => {
            run_native_snapshot_export(&command, collector, default_format, stdout, stderr)
        }
        Ok(ParsedCommand::Help(help)) => run_help(&help, stdout, stderr),
        Ok(_) | Err(_) => session_error(
            stderr,
            &PipelineError::new(PipelineErrorKind::InvalidInput, None),
            2,
        ),
    }
}
