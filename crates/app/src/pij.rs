//! Optional identity resolution; native loading and reconstruction stay in the SDK path.
use std::{collections::BTreeSet, io::Write};

use unisphere_cli::{CliContext, PijQueryCommand, PijTarget};
use unisphere_loader_query::pij::{PijLookupError, PijLookupLimits, resolve_pij};
use unisphere_output_query::ProjectedQueryWriter;
use unisphere_sdk::query::{
    ContentAccess, Dataset, FieldId, FieldValue, Filter, Operation, Predicate, QueryApi,
    QueryFailure, QueryInput, QueryRequest, QueryResponse, QueryScope, QueryService,
    QuerySource, QueryView, SourceSelection, SourceSelector, execute_view,
};

struct RetainedView(QueryView);
impl QueryApi for RetainedView {
    fn execute(&self, request: &QueryRequest) -> Result<QueryResponse, QueryFailure> {
        execute_view(&self.0, request)
    }
}

pub fn run(
    pending: &PijQueryCommand,
    context: &CliContext,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let mode = pending.query.diagnostic_mode;
    let executable = super::find_executable("pij", context);
    let resolved = match resolve_pij(executable.as_deref(), &pending.id, PijLookupLimits::default()) {
        Ok(resolved) => resolved,
        Err(error) => return unisphere_cli::emit_pij_failure(error.code(), error.message(), recovery(error), mode, stderr),
    };
    let source = match super::query_source(&pending.query, context) {
        Ok(source) => source,
        Err(error) => return unisphere_cli::emit_query_failure(&pending.query, &error, stdout, stderr),
    };
    let selection = SourceSelection {
        include_harnesses: BTreeSet::from([resolved.harness.clone()]),
        ..SourceSelection::default()
    };
    // Discover metadata through registered native decoders, not filename guesses,
    // raw transcript scans or Pij private state. Cwd/machine remain optional hints.
    let discovered = match source.load(&pending.query.request.scope, &selection,
        &pending.query.request.limits, ContentAccess::default()) {
        Ok(QueryInput::Native(view)) => view,
        Ok(QueryInput::Saved { .. }) => return unavailable(mode, stderr),
        Err(error) => return unisphere_cli::emit_query_failure(&pending.query, &error, stdout, stderr),
    };
    let matching_sources = discovered.observations.iter()
        .filter(|observation| observation.session.as_ref()
            .is_some_and(|session| session.native_id == resolved.native_session_id))
        .map(|observation| observation.source_ref.source_id)
        .collect::<BTreeSet<_>>();
    if matching_sources.is_empty() {
        return unavailable(mode, stderr);
    }
    if matching_sources.len() != 1 {
        return unisphere_cli::emit_pij_failure("UNI-PIJ-AMBIGUOUS-SOURCE",
            "The resolved native session occurs in more than one local source.",
            "Use sources list with the native harness, then choose an explicit --source; no copy is selected automatically.", mode, stderr);
    }
    let source_id = *matching_sources.first().expect("one matched source");
    drop(discovered);
    let mut command = pending.query.clone();
    command.request.scope = QueryScope::Source { selector: SourceSelector::Id(source_id) };
    command.request.filters.push(Filter { field: FieldId::Harness, predicate: Predicate::In,
        values: vec![FieldValue::String(resolved.harness.to_string())], ignore_case: false });
    // Re-open only the verified source with the requested content capability, then
    // pin its immutable native view. A mapping or source change cannot retarget it.
    let service = QueryService::new(source);
    let range = command.request.turn_range.take();
    let view = match service.open_view(&command.request) {
        Ok(view) => view,
        Err(error) => return unisphere_cli::emit_query_failure(&command, &error, stdout, stderr),
    };
    command.request.turn_range = range;
    let sessions = view.sessions().iter().filter(|session|
        session.native_id == resolved.native_session_id && session.harness == resolved.harness
    ).map(|session| session.id).collect::<Vec<_>>();
    if sessions.is_empty() {
        return unavailable(mode, stderr);
    }
    if sessions.len() != 1 {
        return unisphere_cli::emit_pij_failure("UNI-PIJ-AMBIGUOUS-SESSION",
            "The resolved source contains ambiguous native session membership.",
            "Inspect the explicit source and select one canonical session ID before retrying.", mode, stderr);
    }
    let session_id = sessions[0];
    match command.request.dataset {
        Dataset::Sources if pending.target == PijTarget::SourceCheck =>
            command.request.operation = Operation::Check { source: source_id },
        Dataset::Sources => {},
        Dataset::Sessions if pending.target == PijTarget::SessionShow => command.request.operation = Operation::Show { entity: session_id },
        dataset => command.request.filters.push(Filter {
            field: if dataset == Dataset::Sessions { FieldId::Id } else { FieldId::SessionId },
            predicate: Predicate::In, values: vec![FieldValue::Id(session_id)], ignore_case: false,
        }),
    }
    if unisphere_cli::emit_pij_resolution(&resolved.pij_id, resolved.harness.as_str(), session_id,
        source_id, mode, stderr).is_err() {
        return 1;
    }
    unisphere_cli::run_query(&command, context, &RetainedView(view), &ProjectedQueryWriter, stdout, stderr)
}

fn unavailable(mode: unisphere_cli::OutputMode, stderr: &mut dyn Write) -> u8 {
    unisphere_cli::emit_pij_failure("UNI-PIJ-TRANSCRIPT-MISSING",
        "Pij resolved a native identity, but no matching readable local transcript was found.",
        "Use a local native --source or run on the source host. Retired seats are supported; no remote transcript is fetched.", mode, stderr)
}

fn recovery(error: PijLookupError) -> &'static str {
    match error {
        PijLookupError::MissingExecutable => "Use native --source/--repo selectors or install Pij separately; Unisphere does not install or start it.",
        PijLookupError::LookupUnavailable | PijLookupError::Timeout => "Check Pij daemon/authentication availability or use a native selector; no lifecycle repair is performed.",
        PijLookupError::UnknownSeat => "Check the Pij ID and selected daemon instance, or use a native selector; there is no roster or federation fallback.",
        PijLookupError::NativeSessionUnavailable => "This seat exists but has no recorded native session ID. Select a native source explicitly.",
        PijLookupError::UnsupportedHarness => "Use a supported native source; Pij lookup currently supports Claude, Copilot CLI, Codex, Oh My Pi and Pi.",
        PijLookupError::InvalidResponse | PijLookupError::OutputLimitExceeded => "Use a compatible Pij state response or a native selector; rejected response data is not displayed.",
        PijLookupError::InvalidInput | PijLookupError::InvalidLimits => "Supply one valid Pij seat ID or use a native selector.",
    }
}
