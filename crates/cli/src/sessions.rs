//! Native JSONL session operations through injected collection ports.
use crate::{
    CliContext, NativeExportCommand, NativeRootListCommand, ParsedCommand, args,
    output::StagedOutput, run_help,
};
use serde_json::json;
use std::{ffi::OsString, io::Write};
use unisphere_core::{
    CollectionApi, MappingOptions, PipelineError, PipelineErrorKind, ReadCursor, ReadLimits,
    SessionRef, SourceScope,
};

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
            "fix":error.fix(),"offset":error.offset(),"retryable":false},
        "next_action":{"summary":error.fix(),"argv":["unisphere","sessions","--help"],
            "required_inputs":[]}}),
    );
    if result.is_err() { 1 } else { exit }
}

/// Execute one parsed native nonrecursive JSONL-root listing.
pub fn run_native_list(
    command: &NativeRootListCommand,
    collector: &dyn CollectionApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let result = (|| {
        let scope = SourceScope {
            root: command.root.clone(),
            max_sessions: command.max_sessions,
        };
        scope.validate()?;
        let sessions = collector.list_sessions(&scope)?;
        let empty = sessions.is_empty();
        let next_action = if empty {
            json!({"summary":"Choose the explicit leaf project directory or read the source-discovery workflow.",
                "argv":["unisphere","docs","get","find-sessions","--human"],"required_inputs":[]})
        } else {
            json!({"summary":"Inspect one returned native session file through the explicit OTLP exporter.",
                "argv":["unisphere","sessions","export","--input"],"required_inputs":["session_path"]})
        };
        json_line(
            stdout,
            &json!({"ok":true,"command":"sessions.list","v":1,
            "data":{"root":scope.root,"recursive":false,"sessions":sessions},
            "next_action":next_action}),
        )?;
        if empty {
            json_line(
                stderr,
                &json!({"ok":true,"command":"sessions.list","v":1,
                "note":"No immediate .jsonl files found; listing is not recursive, so select a leaf project directory.",
                "next_action":{"summary":"Choose the explicit leaf project directory or read the source-discovery workflow.",
                    "argv":["unisphere","docs","get","find-sessions","--human"],"required_inputs":[]}}),
            )?;
        }
        Ok(())
    })();
    finish(result, stderr)
}

/// Execute one parsed native append-JSONL export. Snapshot and Git-AI commands
/// have separate typed entrypoints and are rejected here before collector I/O.
pub fn run_native_export(
    command: &NativeExportCommand,
    collector: &dyn CollectionApi,
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
            || command.source_format.is_some()
            || command.table.is_some()
            || command.session_id.is_some()
            || command.max_snapshot_bytes.is_some()
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
        let defaults = ReadLimits::default();
        let limits = ReadLimits {
            max_records: command.max_records.unwrap_or(defaults.max_records),
            max_record_bytes: command.max_record_bytes.unwrap_or(defaults.max_record_bytes),
            max_batch_bytes: command.max_batch_bytes.unwrap_or(defaults.max_batch_bytes),
        };
        limits.validate()?;
        let session = SessionRef {
            path: command.input.clone().expect("validated native JSONL input"),
        };
        session.validate()?;
        let mut staged = command
            .output
            .as_ref()
            .map(|path| StagedOutput::create(path))
            .transpose()
            .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
        let mut cursor: Option<ReadCursor> = None;
        let mut count = 0usize;
        let mut batches = 0usize;
        let mut diagnostics = 0usize;
        let incomplete_tail;
        loop {
            let batch = if let Some(staged) = staged.as_mut() {
                collector.collect_batch(
                    &session,
                    cursor.as_ref(),
                    limits,
                    MappingOptions {
                        include_content: command.include_content,
                    },
                    staged.writer(),
                )?
            } else {
                collector.collect_batch(
                    &session,
                    cursor.as_ref(),
                    limits,
                    MappingOptions {
                        include_content: command.include_content,
                    },
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
            batches = batches
                .checked_add(1)
                .ok_or_else(|| PipelineError::new(PipelineErrorKind::BatchLimit, None))?;
            cursor = Some(batch.next_cursor);
            if !batch.more {
                incomplete_tail = batch.incomplete_tail;
                break;
            }
        }
        if let Some(staged) = staged {
            staged
                .publish()
                .map_err(|_| PipelineError::new(PipelineErrorKind::Write, None))?;
        }
        json_line(
            stderr,
            &json!({"ok":true,"command":"sessions.export","v":1,
            "data":{"adapter":command.adapter,"records":count,"batches":batches,
                "diagnostics":diagnostics,"incomplete_tail":incomplete_tail,
                "offset":cursor.map(|cursor| cursor.offset),"output":command.output},
            "next_action":{"summary":"Import the completed OTLP JSONL stream, or inspect adapter capabilities before another export.",
                "argv":["unisphere","adapters","list","--json"],"required_inputs":[]}}),
        )
    })();
    finish(result, stderr)
}

fn finish(result: Result<(), PipelineError>, stderr: &mut dyn Write) -> u8 {
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

/// Backwards-compatible native JSONL frontend implemented through the sole root parser.
pub fn run_sessions(
    args: impl IntoIterator<Item = OsString>,
    context: &CliContext,
    collector: &dyn CollectionApi,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let args = args.into_iter().collect::<Vec<_>>();
    match args::parse(args, context) {
        Ok(ParsedCommand::NativeRootList(command)) => {
            run_native_list(&command, collector, stdout, stderr)
        }
        Ok(ParsedCommand::NativeExport(command)) => {
            run_native_export(&command, collector, stdout, stderr)
        }
        Ok(ParsedCommand::Help(help)) => run_help(&help, stdout, stderr),
        Ok(_) | Err(_) => session_error(
            stderr,
            &PipelineError::new(PipelineErrorKind::InvalidInput, None),
            2,
        ),
    }
}
