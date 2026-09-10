use std::io::{self, Write};

use serde_json::json;
use unisphere_core::query::{
    Completeness, CsvSafety, Dataset, EntityId, FieldId, FieldValue, Filter, OfflineRef,
    OperationKind, OutputFormat, Predicate, QueryAction, QueryApi, QueryFailure, QueryFailureCode,
    QueryOutputOptions, QueryResponse, QueryScope, QueryWriter, RecoveryAction, RenderedAction,
    SourceId, SourceSelector, schema,
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
    // Refuse an existing destination before source I/O, including dangling links.
    // Actual staging waits until providers have checked source-specific safety.
    if command
        .output
        .as_ref()
        .is_some_and(|path| StagedOutput::preflight(path).is_err())
    {
        return emit_query_failure_inner(
            &command.request,
            &output_destination_failure(),
            command.diagnostic_mode,
            stderr,
        );
    }
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

    let mut staged = match command.output.as_ref() {
        Some(path) => match StagedOutput::create(path) {
            Ok(staged) => Some(staged),
            Err(_) => {
                return emit_query_failure_inner(
                    &command.request,
                    &output_destination_failure(),
                    command.diagnostic_mode,
                    stderr,
                );
            }
        },
        None => None,
    };
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
    if let Some(staged) = staged
        && staged.publish().is_err()
    {
        return emit_query_failure_inner(
            &command.request,
            &output_failure(),
            command.diagnostic_mode,
            stderr,
        );
    }

    if command.format == OutputFormat::Json {
        return 0;
    }
    if emit_query_summary(&command.request, &response, &next_action, command, stderr).is_err() {
        1
    } else {
        0
    }
}

fn output_destination_failure() -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::OutputFailure,
        RecoveryAction::ChooseNewOutput {
            discard_partial: false,
        },
    )
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
            if let Some(session) = exact_session_binding(request) {
                argv.extend(["--session".to_owned(), session.to_string()]);
            }
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

fn exact_session_binding(request: &unisphere_core::query::QueryRequest) -> Option<EntityId> {
    let field = if request.dataset == Dataset::Sessions {
        FieldId::Id
    } else {
        FieldId::SessionId
    };
    request.filters.iter().find_map(|filter| match filter {
        Filter {
            field: candidate,
            predicate: Predicate::In | Predicate::Equal,
            values,
            ..
        } if *candidate == field => match values.as_slice() {
            [FieldValue::Id(session)] => Some(*session),
            _ => None,
        },
        _ => None,
    })
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
    response: &QueryResponse,
    action: &RenderedAction,
    command: &QueryCommand,
    stderr: &mut dyn Write,
) -> io::Result<()> {
    let format = schema(request.dataset)
        .format(request.operation.kind(), command.format)
        .expect("query format was validated before serialization");
    match command.diagnostic_mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            serde_json::to_writer(
                &mut *stderr,
                &json!({
                    "ok": true,
                    "command": command_name(request.dataset, request.operation.kind()),
                    "v": 1,
                    "data": {
                        "matched": response.matched,
                        "emitted": response.emitted,
                        "coverage": &response.coverage,
                        "universe": &response.universe,
                        "output": {
                            "format": command.format,
                            "lossy": format.lossy,
                            "losses": format.losses,
                            "csv_safety": (command.format == OutputFormat::Csv).then_some(command.csv_safety),
                            "formula_interpretation_risk": command.format == OutputFormat::Csv
                                && command.csv_safety == CsvSafety::Raw,
                            "preserve_absent_null_empty_with":
                                (command.format == OutputFormat::Csv).then_some(["json", "jsonl"]),
                        },
                    },
                    "next_action": action,
                }),
            )
            .map_err(io::Error::other)?;
            stderr.write_all(b"\n")?;
        }
        OutputMode::Human => {
            writeln!(
                stderr,
                "Matched {}; emitted {}.",
                response.matched, response.emitted
            )?;
            writeln!(
                stderr,
                "Coverage: loaded {}/{} discovered sources; selected {}; source read complete: {}.",
                response.coverage.loaded_sources,
                response.coverage.discovered_sources,
                response.coverage.selected_sources,
                response.coverage.source_read_complete,
            )?;
            writeln!(
                stderr,
                "Universe: {}; rows {}; partitions {}; bounded by input: {}.",
                response.universe.basis.as_str(),
                response.universe.rows_complete_for_selection.as_str(),
                response.universe.partitions_complete.as_str(),
                response.universe.bounded_by_input,
            )?;
            if !response.coverage.source_read_complete
                || !response.coverage.issues.is_empty()
                || response.universe.rows_complete_for_selection != Completeness::Complete
                || response.universe.partitions_complete != Completeness::Complete
            {
                writeln!(
                    stderr,
                    "Warning: evidence coverage is incomplete, subset, or unknown; counts are not a complete universe."
                )?;
            }
            if command.format == OutputFormat::Csv {
                writeln!(
                    stderr,
                    "CSV loss: absent fields, null values, and empty values share an empty cell; choose JSON or JSONL when the distinction matters."
                )?;
                if command.csv_safety == CsvSafety::Raw {
                    writeln!(
                        stderr,
                        "CSV safety: raw cells can be interpreted as formulas by spreadsheet software."
                    )?;
                }
            }
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
    if result.is_err() {
        1
    } else {
        query_failure_exit(failure.kind())
    }
}

const fn query_failure_exit(code: QueryFailureCode) -> u8 {
    match code {
        QueryFailureCode::MissingSource
        | QueryFailureCode::UnreadableSource
        | QueryFailureCode::UnsupportedSource
        | QueryFailureCode::StaleCursor(_)
        | QueryFailureCode::ResourceLimit
        | QueryFailureCode::OutputFailure => 1,
        QueryFailureCode::InvalidArgument
        | QueryFailureCode::InvalidField
        | QueryFailureCode::InvalidPattern
        | QueryFailureCode::InvalidTime
        | QueryFailureCode::UnsupportedSchema
        | QueryFailureCode::UnsupportedOperation
        | QueryFailureCode::InvalidData
        | QueryFailureCode::AmbiguousIdentity
        | QueryFailureCode::AmbiguousBranch
        | QueryFailureCode::MissingField
        | QueryFailureCode::InputSubset
        | QueryFailureCode::ViewScopeMismatch
        | QueryFailureCode::ContentConsentRequired => 2,
    }
}

/// Emit a safe application-supplied failure for optional Pij resolution.
///
/// `code`, `message`, and `next_action` must be static, payload-free strings.
pub fn emit_pij_failure(
    code: &str,
    message: &str,
    next_action: &str,
    mode: OutputMode,
    stderr: &mut dyn Write,
) -> u8 {
    let result = match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => (|| {
            serde_json::to_writer(
                &mut *stderr,
                &json!({
                    "ok": false,
                    "command": "pij.resolve",
                    "v": 1,
                    "error": {
                        "code": code,
                        "message": message,
                        "alternatives": [],
                        "retryable": false,
                    },
                    "next_action": next_action,
                }),
            )
            .map_err(io::Error::other)?;
            stderr.write_all(b"\n")?;
            stderr.flush()
        })(),
        OutputMode::Human => writeln!(
            stderr,
            "{code}: {message}\nRetryable: false\nNext: {next_action}"
        )
        .and_then(|()| stderr.flush()),
    };
    let _ = result;
    1
}

/// Emit payload-free provenance after one Pij seat resolves to local query IDs.
pub fn emit_pij_resolution(
    pij_id: &str,
    harness: &str,
    session_id: EntityId,
    source_id: SourceId,
    mode: OutputMode,
    stderr: &mut dyn Write,
) -> io::Result<()> {
    match mode {
        OutputMode::Json | OutputMode::JsonDiagnostic => {
            serde_json::to_writer(
                &mut *stderr,
                &json!({
                    "ok": true,
                    "command": "pij.resolve",
                    "v": 1,
                    "data": {
                        "pij_id": pij_id,
                        "harness": harness,
                        "session_id": session_id,
                        "source_id": source_id,
                    },
                }),
            )
            .map_err(io::Error::other)?;
            stderr.write_all(b"\n")?;
        }
        OutputMode::Human => writeln!(
            stderr,
            "Resolved Pij seat \"{}\" to harness \"{}\"; pinned session {} and source {} for this query.",
            safe_human_identifier(pij_id),
            safe_human_identifier(harness),
            session_id,
            source_id,
        )?,
    }
    stderr.flush()
}

fn safe_human_identifier(value: &str) -> String {
    let mut safe = String::with_capacity(value.len());
    for character in value.chars() {
        let code = u32::from(character);
        if character.is_control()
            || matches!(code, 0x061c | 0x200e | 0x200f | 0x202a..=0x202e | 0x2066..=0x2069)
        {
            safe.push_str(&format!("\\u{{{code:x}}}"));
        } else {
            safe.push(character);
        }
    }
    safe
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
