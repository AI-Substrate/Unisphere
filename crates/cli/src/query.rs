use std::io::{self, Write};

use serde_json::json;
use unisphere_core::query::{
    Dataset, OfflineRef, OperationKind, OutputFormat, QueryAction, QueryApi, QueryFailure,
    QueryFailureCode, QueryOutputOptions, QueryScope, QueryWriter, RecoveryAction, RenderedAction,
    SourceSelector, schema,
};

use crate::{
    CliContext,
    args::{CliParseFailure, DocsCommand, OutputMode, QueryCommand, SchemaCommand},
    docs,
    output::StagedOutput,
};

/// Execute one already-parsed query through injected semantic and serialization ports.
/// The frontend acquires no source provider and never reparses argv.
pub fn run_query(
    command: &QueryCommand,
    _context: &CliContext,
    query: &dyn QueryApi,
    writer: &dyn QueryWriter,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    if let Err(failure) = command.request.validate() {
        return emit_query_failure_inner(
            &command.request,
            &failure,
            command.diagnostic_mode,
            stderr,
        );
    }
    if command
        .output
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return emit_query_failure_inner(
            &command.request,
            &QueryFailure::new(
                QueryFailureCode::InvalidArgument,
                RecoveryAction::ReadQueryHelp,
            ),
            command.diagnostic_mode,
            stderr,
        );
    }
    let mut staged = match command.output.as_ref() {
        Some(path) => match StagedOutput::create(path) {
            Ok(staged) => Some(staged),
            Err(_) => {
                return emit_query_failure_inner(
                    &command.request,
                    &output_failure(),
                    command.diagnostic_mode,
                    stderr,
                );
            }
        },
        None => None,
    };
    let response = match query.execute(&command.request) {
        Ok(response) => response,
        Err(failure) => {
            return emit_query_failure_inner(
                &command.request,
                &failure,
                command.diagnostic_mode,
                stderr,
            );
        }
    };
    let next_action = render_action(&response.next_action, command);
    let options = QueryOutputOptions {
        format: command.format,
        csv_safety: command.csv_safety,
        max_output_bytes: command.request.limits.max_output_bytes,
        next_action: next_action.clone(),
    };
    if let Err(failure) = options.validate_for(&response, &command.request.limits) {
        return emit_query_failure_inner(
            &command.request,
            &failure,
            command.diagnostic_mode,
            stderr,
        );
    }

    let written = if let Some(staged) = staged.as_mut() {
        writer.write(&response, &options, staged.writer())
    } else {
        writer
            .write(&response, &options, stdout)
            .and_then(|()| stdout.flush().map_err(|_| output_failure()))
    };
    if let Err(failure) = written {
        return emit_query_failure_inner(
            &command.request,
            &failure,
            command.diagnostic_mode,
            stderr,
        );
    }
    if let Some(staged) = staged {
        if staged.publish().is_err() {
            return emit_query_failure_inner(
                &command.request,
                &output_failure(),
                command.diagnostic_mode,
                stderr,
            );
        }
    }

    if command.format == OutputFormat::Json {
        return 0;
    }
    if emit_query_summary(
        &command.request,
        response.matched,
        response.emitted,
        &next_action,
        command.diagnostic_mode,
        stderr,
    )
    .is_err()
    {
        1
    } else {
        0
    }
}

fn output_failure() -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::OutputFailure,
        RecoveryAction::ChooseNewOutput {
            discard_partial: true,
        },
    )
    .retryable_after_recovery(true)
}

fn render_action(action: &QueryAction, command: &QueryCommand) -> RenderedAction {
    let request = &command.request;
    let (scope_argv, mut required_inputs) = rendered_scope(&request.scope, command.stdin_format);
    let mut argv = vec!["unisphere".to_owned()];
    let summary = match action {
        QueryAction::InspectEntity {
            dataset,
            entity,
            reason: _,
        } => {
            argv.extend([
                dataset.as_str().to_owned(),
                "show".to_owned(),
                entity.to_string(),
            ]);
            argv.extend(scope_argv);
            "Inspect the selected entity and its evidence coverage.".to_owned()
        }
        QueryAction::Continue { cursor, reason: _ } => {
            argv.extend([
                request.dataset.as_str().to_owned(),
                request.operation.kind().as_str().to_owned(),
            ]);
            argv.extend(scope_argv);
            argv.extend(["--cursor".to_owned(), cursor.clone()]);
            required_inputs.push("original_query_options".to_owned());
            "Repeat the original query options against the same source view with this cursor."
                .to_owned()
        }
        QueryAction::NarrowSelection {
            reason: _,
            needed_inputs,
        } => {
            required_inputs.extend(needed_inputs.iter().map(|field| field.as_str().to_owned()));
            argv.clear();
            "Narrow the selection with the named required inputs, then repeat the query.".to_owned()
        }
        QueryAction::InspectCoverage { reason: _ } => {
            argv.extend(["sources".to_owned(), "list".to_owned()]);
            argv.extend(scope_argv);
            "Inspect source coverage before changing the selection.".to_owned()
        }
        QueryAction::ReadSchema { dataset, reason: _ } => {
            argv.extend([
                "schema".to_owned(),
                "show".to_owned(),
                dataset.as_str().to_owned(),
                "--json".to_owned(),
            ]);
            required_inputs.clear();
            "Inspect supported fields, operations, formats, and availability.".to_owned()
        }
        QueryAction::ReadRecipe { topic, reason: _ } => {
            let topic = if docs::get(topic).is_some() {
                topic
            } else {
                "start"
            };
            argv.extend([
                "docs".to_owned(),
                "get".to_owned(),
                topic.to_owned(),
                "--human".to_owned(),
            ]);
            required_inputs.clear();
            "Read the bundled workflow before issuing the next query.".to_owned()
        }
    };
    RenderedAction {
        summary,
        argv,
        required_inputs,
    }
}

fn rendered_scope(
    scope: &QueryScope,
    stdin_format: unisphere_core::query::SavedFormat,
) -> (Vec<String>, Vec<String>) {
    match scope {
        QueryScope::Repository { scope, .. } => {
            let mut argv = Vec::new();
            if *scope != unisphere_core::query::RepoScope::Tree {
                argv.extend(["--repo-scope".to_owned(), scope.as_str().to_owned()]);
            }
            argv.push("--repo".to_owned());
            (argv, vec!["repository_path".to_owned()])
        }
        QueryScope::Source {
            selector: SourceSelector::Id(source),
        } => (vec!["--source".to_owned(), source.to_string()], Vec::new()),
        QueryScope::Source {
            selector: SourceSelector::Path { adapter, .. },
        } => {
            let mut argv = Vec::new();
            if let Some(adapter) = adapter {
                argv.extend(["--source-adapter".to_owned(), adapter.as_str().to_owned()]);
            }
            argv.push("--source".to_owned());
            (argv, vec!["source_path".to_owned()])
        }
        QueryScope::Offline {
            input: OfflineRef::Stdin,
        } => (
            vec![
                "--input".to_owned(),
                "-".to_owned(),
                "--stdin-format".to_owned(),
                match stdin_format {
                    unisphere_core::query::SavedFormat::QueryJsonV1 => "json".to_owned(),
                    unisphere_core::query::SavedFormat::QueryJsonlV1 => "jsonl".to_owned(),
                },
            ],
            Vec::new(),
        ),
        QueryScope::Offline {
            input: OfflineRef::File(_),
        } => (vec!["--input".to_owned()], vec!["input_path".to_owned()]),
    }
}

fn emit_query_summary(
    request: &unisphere_core::query::QueryRequest,
    matched: u64,
    emitted: u64,
    action: &RenderedAction,
    mode: OutputMode,
    stderr: &mut dyn Write,
) -> io::Result<()> {
    match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            serde_json::to_writer(
                &mut *stderr,
                &json!({
                    "ok": true,
                    "command": command_name(request.dataset, request.operation.kind()),
                    "v": 1,
                    "data": {"matched": matched, "emitted": emitted},
                    "next_action": action,
                }),
            )
            .map_err(io::Error::other)?;
            stderr.write_all(b"\n")?;
        }
        OutputMode::Human => {
            writeln!(stderr, "Matched {matched}; emitted {emitted}.")?;
            writeln!(stderr, "Next: {}", action.summary)?;
            if !action.argv.is_empty() {
                write!(stderr, "Command:")?;
                for argument in &action.argv {
                    write!(stderr, " ")?;
                    serde_json::to_writer(&mut *stderr, argument).map_err(io::Error::other)?;
                }
                writeln!(stderr)?;
            }
            if !action.required_inputs.is_empty() {
                writeln!(
                    stderr,
                    "Required inputs: {}",
                    action.required_inputs.join(", ")
                )?;
            }
        }
    }
    stderr.flush()
}

/// Render a typed query initialization or execution failure on the diagnostic
/// channel without writing into stdout data streams.
pub fn emit_query_failure(
    command: &QueryCommand,
    failure: &QueryFailure,
    _stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    emit_query_failure_inner(&command.request, failure, command.diagnostic_mode, stderr)
}

fn emit_query_failure_inner(
    request: &unisphere_core::query::QueryRequest,
    failure: &QueryFailure,
    mode: OutputMode,
    stderr: &mut dyn Write,
) -> u8 {
    let result = match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => (|| {
            serde_json::to_writer(
                &mut *stderr,
                &json!({
                    "ok": false,
                    "command": command_name(request.dataset, request.operation.kind()),
                    "v": 1,
                    "error": failure,
                    "next_action": failure.recovery().guidance(),
                }),
            )
            .map_err(io::Error::other)?;
            stderr.write_all(b"\n")?;
            stderr.flush()
        })(),
        OutputMode::Human => writeln!(
            stderr,
            "{}: {}\nRetryable: {}\nNext: {}",
            failure.code(),
            failure.message(),
            failure.retryable(),
            failure.recovery().guidance()
        )
        .and_then(|()| stderr.flush()),
    };
    if result.is_err() { 1 } else { 1 }
}

fn command_name(dataset: Dataset, operation: OperationKind) -> String {
    format!("{}.{}", dataset.as_str(), operation.as_str())
}

/// Render a bundled documentation command without acquiring providers or configuration.
pub fn run_docs(command: &DocsCommand, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    match command {
        DocsCommand::List { mode } => finish_static(emit_docs_list(*mode, stdout), stderr),
        DocsCommand::Get { topic, mode } => match docs::get(topic) {
            Some(topic) => finish_static(emit_doc(topic, *mode, stdout), stderr),
            None => {
                let alternatives = docs::topics()
                    .iter()
                    .map(|topic| topic.id)
                    .collect::<Vec<_>>();
                let result = emit_static_error(
                    "docs.get",
                    "UNI-DOC-TOPIC",
                    "The requested bundled documentation topic is not available.",
                    "Run `unisphere docs list --json` and choose one of the returned topic IDs.",
                    &alternatives,
                    *mode,
                    stderr,
                );
                if result.is_ok() { 2 } else { 1 }
            }
        },
    }
}

fn emit_docs_list(mode: OutputMode, stdout: &mut dyn Write) -> io::Result<()> {
    match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            let topics = docs::topics()
                .iter()
                .map(|topic| json!({"id": topic.id, "title": topic.title, "summary": topic.summary, "related": topic.related}))
                .collect::<Vec<_>>();
            serde_json::to_writer(
                &mut *stdout,
                &json!({
                    "ok": true,
                    "command": "docs.list",
                    "v": 1,
                    "data": {"topics": topics},
                    "next_action": {"summary":"Read the start workflow.","argv":["unisphere","docs","get","start","--human"],"required_inputs":[]}
                }),
            )
            .map_err(io::Error::other)?;
            stdout.write_all(b"\n")?;
        }
        OutputMode::Human => {
            writeln!(stdout, "Bundled documentation topics:")?;
            for topic in docs::topics() {
                writeln!(stdout, "  {} — {}", topic.id, topic.summary)?;
            }
            writeln!(stdout, "Next: run `unisphere docs get start --human`.")?;
        }
    }
    stdout.flush()
}

fn emit_doc(topic: &docs::DocTopic, mode: OutputMode, stdout: &mut dyn Write) -> io::Result<()> {
    match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            serde_json::to_writer(
                &mut *stdout,
                &json!({
                    "ok": true,
                    "command": "docs.get",
                    "v": 1,
                    "data": {"id":topic.id,"title":topic.title,"summary":topic.summary,"text":topic.text,"related":topic.related},
                    "next_action": {"summary":"Follow the documented workflow or inspect a related topic.","argv":["unisphere","docs","list","--json"],"required_inputs":[]}
                }),
            )
            .map_err(io::Error::other)?;
            stdout.write_all(b"\n")?;
        }
        OutputMode::Human => {
            stdout.write_all(topic.text.as_bytes())?;
            if !topic.text.ends_with('\n') {
                stdout.write_all(b"\n")?;
            }
            writeln!(
                stdout,
                "Next: follow the workflow above or run `unisphere docs list --human`."
            )?;
        }
    }
    stdout.flush()
}

/// Render a version-matched schema without acquiring providers or configuration.
pub fn run_schema(command: &SchemaCommand, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let dataset_schema = schema(command.dataset);
    let result = match command.mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => (|| {
            serde_json::to_writer(
                &mut *stdout,
                &json!({
                    "ok":true,
                    "command":"schema.show",
                    "v":1,
                    "data":dataset_schema,
                    "next_action":{"summary":"Issue a query using declared fields; choose JSON or JSONL when absence, null, and empty strings must remain distinct.","argv":["unisphere",command.dataset.as_str(),"list"],"required_inputs":["scope"]}
                }),
            )
            .map_err(io::Error::other)?;
            stdout.write_all(b"\n")?;
            stdout.flush()
        })(),
        OutputMode::Human => (|| {
            writeln!(
                stdout,
                "{} schema v{}",
                command.dataset, dataset_schema.schema_version
            )?;
            for field in dataset_schema.fields {
                writeln!(
                    stdout,
                    "  {}: {} nullable={} sensitivity={} availability={}",
                    field.id,
                    field.field_type,
                    field.nullable,
                    field.sensitivity,
                    field.availability
                )?;
            }
            writeln!(stdout, "Formats:")?;
            for format in dataset_schema.formats {
                writeln!(
                    stdout,
                    "  {} lossy={} losses={}",
                    format.format,
                    format.lossy,
                    format
                        .losses
                        .iter()
                        .map(|loss| loss.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                )?;
            }
            writeln!(
                stdout,
                "Next: query {} with one explicit scope; use JSON or JSONL when absence, null, and empty strings must remain distinct.",
                command.dataset
            )?;
            stdout.flush()
        })(),
    };
    finish_static(result, stderr)
}

/// Emit a safe root-parser failure. No rejected argument values are rendered.
pub fn emit_parse_failure(
    failure: &CliParseFailure,
    mode: OutputMode,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let destination: &mut dyn Write = match mode {
        OutputMode::Json => stdout,
        OutputMode::JsonDiagnostic | OutputMode::Human => stderr,
    };
    let result = match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => (|| {
            serde_json::to_writer(
                &mut *destination,
                &json!({
                    "ok":false,
                    "command":"parse",
                    "v":1,
                    "error":{"code":failure.code(),"message":failure.message(),
                        "alternatives":failure.alternatives(),"retryable":failure.retryable()},
                    "next_action":failure.next_action()
                }),
            )
            .map_err(io::Error::other)?;
            destination.write_all(b"\n")?;
            destination.flush()
        })(),
        OutputMode::Human => (|| {
            writeln!(
                destination,
                "{}: {}\nRetryable: {}",
                failure.code(),
                failure.message(),
                failure.retryable(),
            )?;
            if !failure.alternatives().is_empty() {
                writeln!(
                    destination,
                    "Valid alternatives: {}",
                    failure.alternatives().join(", ")
                )?;
            }
            writeln!(destination, "Next: {}", failure.next_action())?;
            destination.flush()
        })(),
    };
    if result.is_ok() { 2 } else { 1 }
}

fn emit_static_error(
    command: &str,
    code: &str,
    message: &str,
    action: &str,
    alternatives: &[&str],
    mode: OutputMode,
    stderr: &mut dyn Write,
) -> io::Result<()> {
    match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            serde_json::to_writer(
                &mut *stderr,
                &json!({"ok":false,"command":command,"v":1,
                    "error":{"code":code,"message":message,"alternatives":alternatives,"retryable":false},
                    "next_action":action}),
            )
            .map_err(io::Error::other)?;
            stderr.write_all(b"\n")?;
        }
        OutputMode::Human => {
            writeln!(stderr, "{code}: {message}\nRetryable: false")?;
            writeln!(stderr, "Valid alternatives: {}", alternatives.join(", "))?;
            writeln!(stderr, "Next: {action}")?;
        }
    }
    stderr.flush()
}

fn finish_static(result: io::Result<()>, stderr: &mut dyn Write) -> u8 {
    if result.is_ok() {
        0
    } else {
        let _ = stderr
            .write_all(b"unisphere: output incomplete; choose a healthy destination and retry.\n");
        let _ = stderr.flush();
        1
    }
}
