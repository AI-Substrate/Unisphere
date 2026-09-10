use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

use globset::{GlobBuilder, GlobMatcher};
use regex::{Regex, RegexBuilder};
use unisphere_core::query::{
    ActionReason, Completeness, ContentAccess, CursorBinding, Dataset, Digest, EntityId,
    EntityKind, FieldId, FieldValue, Filter, LimitKind, Metric, Operation, Outcome, Predicate, ProjectedRow,
    QueryAction, QueryDescription, QueryFailure, QueryFailureCode, QueryRequest, QueryResponse,
    QueryScope, RecoveryAction, ResultUniverse, SortDirection, SortKey, SourceProblem,
    SourceReadStatus, SourceSelection, SourceSelector, UniverseBasis,
};

use super::{
    QueryView, cursor,
    rows::{RowRef, UsageFact},
};

pub(crate) fn source_selection(request: &QueryRequest) -> Result<SourceSelection, QueryFailure> {
    request.validate()?;
    validate_operation_projection(request)?;
    validate_filter_values(request)?;
    if let Operation::Stats { group_by, .. } = &request.operation {
        let denied = group_by
            .iter()
            .copied()
            .filter(|field| {
                unisphere_core::query::schema(request.dataset)
                    .field(*field)
                    .is_some_and(|schema| {
                        schema.sensitivity == unisphere_core::query::Sensitivity::Sensitive
                    })
            })
            .collect::<Vec<_>>();
        if !denied.is_empty() && !request.include_content {
            return Err(QueryFailure::new(
                QueryFailureCode::ContentConsentRequired,
                RecoveryAction::UseMetadataOrConsent { fields: denied },
            ));
        }
    }
    let patterns = request
        .filters
        .iter()
        .filter(|filter| filter.predicate != Predicate::Has)
        .try_fold(0usize, |total, filter| {
            total.checked_add(filter.values.len())
        })
        .ok_or_else(|| QueryFailure::limit(LimitKind::Patterns))?;
    if patterns > request.limits.max_patterns {
        return Err(QueryFailure::limit(LimitKind::Patterns));
    }
    if matches!(request.scope, QueryScope::Offline { .. }) {
        return Ok(SourceSelection::default());
    }
    let mut selection = SourceSelection::default();
    for filter in &request.filters {
        let target = match filter.field {
            FieldId::Adapter => Some((true, &mut selection)),
            FieldId::Harness => Some((false, &mut selection)),
            _ => None,
        };
        let Some((adapter, selection)) = target else {
            continue;
        };
        if !matches!(
            filter.predicate,
            Predicate::Equal | Predicate::In | Predicate::Exclude
        ) {
            continue;
        }
        for value in &filter.values {
            let FieldValue::String(value) = value else {
                return Err(QueryFailure::new(
                    QueryFailureCode::InvalidArgument,
                    RecoveryAction::ConsultSchema {
                        dataset: request.dataset,
                    },
                ));
            };
            if adapter {
                let value = unisphere_core::query::AdapterId::from_str(value).map_err(|_| {
                    QueryFailure::new(
                        QueryFailureCode::InvalidArgument,
                        RecoveryAction::ConsultSchema {
                            dataset: request.dataset,
                        },
                    )
                })?;
                if filter.predicate == Predicate::Exclude {
                    selection.exclude_adapters.insert(value);
                } else {
                    selection.include_adapters.insert(value);
                }
            } else {
                let value = unisphere_core::query::HarnessId::from_str(value).map_err(|_| {
                    QueryFailure::new(
                        QueryFailureCode::InvalidArgument,
                        RecoveryAction::ConsultSchema {
                            dataset: request.dataset,
                        },
                    )
                })?;
                if filter.predicate == Predicate::Exclude {
                    selection.exclude_harnesses.insert(value);
                } else {
                    selection.include_harnesses.insert(value);
                }
            }
        }
    }
    selection.validate()?;
    if let QueryScope::Source {
        selector:
            SourceSelector::Path {
                adapter: Some(adapter),
                ..
            },
    } = &request.scope
        && (!selection.include_adapters.is_empty() && !selection.include_adapters.contains(adapter)
            || selection.exclude_adapters.contains(adapter))
    {
        return Err(QueryFailure::new(
            QueryFailureCode::InvalidArgument,
            RecoveryAction::ChooseAdapter {
                allowed: selection.include_adapters.iter().cloned().collect(),
            },
        ));
    }
    Ok(selection)
}

fn validate_operation_projection(request: &QueryRequest) -> Result<(), QueryFailure> {
    if matches!(&request.operation, Operation::Stats { .. }) {
        return Ok(());
    }
    let uses_stats_field = request
        .columns
        .iter()
        .flatten()
        .copied()
        .chain(request.sort.iter().map(|sort| sort.field))
        .any(is_stats_metric_field);
    if uses_stats_field {
        return Err(QueryFailure::new(
            QueryFailureCode::UnsupportedOperation,
            RecoveryAction::ConsultSchema {
                dataset: request.dataset,
            },
        ));
    }
    Ok(())
}

fn is_stats_metric_field(field: FieldId) -> bool {
    Metric::ALL
        .iter()
        .any(|metric| metric_field(*metric) == field)
}

fn validate_filter_values(request: &QueryRequest) -> Result<(), QueryFailure> {
    let schema = unisphere_core::query::schema(request.dataset);
    for filter in &request.filters {
        if filter.predicate == Predicate::Has {
            continue;
        }
        let field = schema.field(filter.field).ok_or_else(|| {
            QueryFailure::new(
                QueryFailureCode::InvalidField,
                RecoveryAction::ConsultSchema {
                    dataset: request.dataset,
                },
            )
        })?;
        let valid = filter.values.iter().all(|value| match field.field_type {
            unisphere_core::query::FieldType::Bool => matches!(value, FieldValue::Bool(_)),
            unisphere_core::query::FieldType::U64 => matches!(value, FieldValue::Unsigned(_)),
            unisphere_core::query::FieldType::I64 => matches!(value, FieldValue::Integer(_)),
            unisphere_core::query::FieldType::FiniteF64 => matches!(value, FieldValue::Float(_)),
            unisphere_core::query::FieldType::String => matches!(value, FieldValue::String(_)),
            unisphere_core::query::FieldType::Timestamp => {
                matches!(value, FieldValue::Timestamp(_))
            }
            unisphere_core::query::FieldType::EntityId => matches!(value, FieldValue::Id(_)),
            unisphere_core::query::FieldType::IdList => {
                matches!(value, FieldValue::Id(_) | FieldValue::IdList(_))
            }
            unisphere_core::query::FieldType::EnumList => {
                matches!(value, FieldValue::String(_) | FieldValue::Strings(_))
            }
            unisphere_core::query::FieldType::SourceRefs
            | unisphere_core::query::FieldType::Structured => false,
        });
        if !valid {
            return Err(QueryFailure::new(
                QueryFailureCode::InvalidArgument,
                RecoveryAction::ConsultSchema {
                    dataset: request.dataset,
                },
            ));
        }
    }
    Ok(())
}

pub fn execute_view(
    view: &QueryView,
    request: &QueryRequest,
) -> Result<QueryResponse, QueryFailure> {
    request.validate()?;
    if (request.context.before != 0 || request.context.after != 0)
        && request.operation.kind() != unisphere_core::query::OperationKind::Extract
    {
        return Err(QueryFailure::new(
            QueryFailureCode::UnsupportedOperation,
            RecoveryAction::ConsultSchema {
                dataset: request.dataset,
            },
        ));
    }
    let requested_selection = source_selection(request)?;
    validate_view_request(view, request, &requested_selection)?;
    validate_source_completeness(view, request)?;
    let access = request.content_access()?;
    let query_digest = unisphere_core::query::query_binding_digest(request, &requested_selection)?;
    let binding = CursorBinding {
        query_digest,
        view_digest: view.digest(),
    };

    let mut rows = rows_for(view, request.dataset);
    apply_operation(view, request, &mut rows)?;
    apply_branch_and_range(request, &mut rows);
    let compiled = compile_filters(&request.filters)?;
    let mut scanned_bytes = 0usize;
    let mut filtered = Vec::with_capacity(rows.len());
    for row in rows {
        if row_matches(
            &row,
            &compiled,
            &mut scanned_bytes,
            request.limits.max_scanned_text_bytes,
        )? && time_matches(&row, request)
        {
            filtered.push(row);
        }
    }
    let mut rows = filtered;
    let matched = rows.len();

    if (request.context.before != 0 || request.context.after != 0)
        && view.saved_rows(request.dataset).is_some()
        && view.saved_partitions_complete() != Completeness::Complete
    {
        return Err(QueryFailure::new(
            QueryFailureCode::InputSubset,
            RecoveryAction::UseCompleteInput,
        ));
    }
    let mut context_ids = BTreeSet::new();
    if request.context.before != 0 || request.context.after != 0 {
        let expanded = expand_context(view, request, &rows)?;
        rows = expanded.0;
        context_ids = expanded.1;
    }

    if let Operation::Stats { group_by, metrics } = &request.operation {
        return execute_stats(
            view,
            request,
            rows,
            matched,
            group_by,
            metrics,
            query_digest,
            binding,
            &access,
        );
    }

    if request.context.before != 0 || request.context.after != 0 {
        sort_context_rows(&mut rows);
    } else {
        sort_rows(request, &mut rows);
    }
    let start = match &request.cursor {
        Some(cursor_value) => cursor::decode(cursor_value, binding, rows.len())?,
        None => 0,
    };
    let applied_limit = effective_limit(request);
    let end = applied_limit
        .and_then(|limit| start.checked_add(limit))
        .map_or(rows.len(), |end| end.min(rows.len()));
    let next_cursor = (end < rows.len())
        .then(|| cursor::encode(binding, end))
        .transpose()?;
    let columns = normalized_columns(request);
    let mut projected = Vec::with_capacity(end.saturating_sub(start));
    let mut projected_bytes = 0usize;
    for row in rows[start..end].iter().copied() {
        let is_context = (request.context.before != 0 || request.context.after != 0)
            .then(|| context_ids.contains(&row.id()));
        let row = row.project(&columns, &access, is_context)?;
        projected_bytes = projected_bytes
            .checked_add(
                serde_json::to_vec(&row)
                    .map_err(|_| QueryFailure::invalid_data())?
                    .len(),
            )
            .ok_or_else(|| QueryFailure::limit(LimitKind::RetainedBytes))?;
        if projected_bytes > request.limits.max_retained_bytes {
            return Err(QueryFailure::limit(LimitKind::RetainedBytes));
        }
        projected.push(row);
    }
    finish_response(
        view,
        request,
        projected,
        matched,
        next_cursor,
        columns,
        applied_limit,
        query_digest,
    )
}

fn validate_view_request(
    view: &QueryView,
    request: &QueryRequest,
    requested_selection: &SourceSelection,
) -> Result<(), QueryFailure> {
    if !scope_within(&request.scope, view.admitted_scope()) {
        return Err(QueryFailure::new(
            QueryFailureCode::ViewScopeMismatch,
            RecoveryAction::ReopenView,
        ));
    }
    if !selection_within(requested_selection, view.source_selection()) {
        return Err(QueryFailure::new(
            QueryFailureCode::ViewScopeMismatch,
            RecoveryAction::ReopenView,
        ));
    }
    if view.saved_rows(request.dataset).is_some() {
        let required = required_fields(request);
        let available = view.available_fields();
        let missing = required.difference(available).copied().collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(QueryFailure::new(
                QueryFailureCode::MissingField,
                RecoveryAction::SupplyFields { required: missing },
            ));
        }
    }
    Ok(())
}

fn selection_within(requested: &SourceSelection, admitted: &SourceSelection) -> bool {
    group_within(
        &requested.include_adapters,
        &requested.exclude_adapters,
        &admitted.include_adapters,
        &admitted.exclude_adapters,
    ) && group_within(
        &requested.include_harnesses,
        &requested.exclude_harnesses,
        &admitted.include_harnesses,
        &admitted.exclude_harnesses,
    )
}

fn group_within<T: Ord>(
    requested_include: &BTreeSet<T>,
    requested_exclude: &BTreeSet<T>,
    admitted_include: &BTreeSet<T>,
    admitted_exclude: &BTreeSet<T>,
) -> bool {
    if requested_include.is_empty() {
        admitted_include.is_empty() && admitted_exclude.is_subset(requested_exclude)
    } else {
        requested_include.iter().all(|value| {
            !requested_exclude.contains(value)
                && !admitted_exclude.contains(value)
                && (admitted_include.is_empty() || admitted_include.contains(value))
        })
    }
}

fn required_fields(request: &QueryRequest) -> BTreeSet<FieldId> {
    let mut required = normalized_columns(request)
        .into_iter()
        .collect::<BTreeSet<_>>();
    required.remove(&FieldId::IsContext);
    required.extend(request.filters.iter().map(|filter| filter.field));
    required.extend(request.sort.iter().map(|sort| sort.field));
    if let Some(field) = request
        .time
        .field
        .or(unisphere_core::query::schema(request.dataset).default_time_field)
        && (request.time.since.is_some() || request.time.until.is_some())
    {
        required.insert(field);
    }
    if let Operation::Stats { group_by, metrics } = &request.operation {
        required.extend(group_by.iter().copied());
        for metric in metrics {
            match metric {
                Metric::MeasuredCount
                | Metric::MissingDurationCount
                | Metric::MeanMs
                | Metric::MinMs
                | Metric::MaxMs
                | Metric::P50Ms
                | Metric::P95Ms => {
                    required.insert(FieldId::DurationMs);
                }
                Metric::Succeeded
                | Metric::Failures
                | Metric::Cancelled
                | Metric::Incomplete
                | Metric::Unknown
                | Metric::FailureRate => {
                    required.insert(FieldId::Status);
                }
                Metric::InputTokens
                | Metric::OutputTokens
                | Metric::CacheReadTokens
                | Metric::CacheWriteTokens => {
                    required.insert(metric_field(*metric));
                }
                Metric::Count => {}
            }
        }
    }
    required
}

fn validate_source_completeness(
    view: &QueryView,
    request: &QueryRequest,
) -> Result<(), QueryFailure> {
    if request.dataset == Dataset::Sources || request.allow_partial {
        return Ok(());
    }
    if view.coverage().source_read_complete {
        return Ok(());
    }
    let has = |status| {
        view.coverage()
            .source_status
            .get(&status)
            .is_some_and(|count| *count != 0)
    };
    let (code, recovery) = if has(SourceReadStatus::Absent) {
        (
            QueryFailureCode::MissingSource,
            RecoveryAction::FixSource {
                reason: SourceProblem::Missing,
                source: None,
            },
        )
    } else if has(SourceReadStatus::Unreadable) || has(SourceReadStatus::Partial) {
        (
            QueryFailureCode::UnreadableSource,
            RecoveryAction::FixSource {
                reason: SourceProblem::Permissions,
                source: None,
            },
        )
    } else if has(SourceReadStatus::Unsupported) {
        (
            QueryFailureCode::UnsupportedSource,
            RecoveryAction::FixSource {
                reason: SourceProblem::UnsupportedDialect,
                source: None,
            },
        )
    } else {
        return Ok(());
    };
    Err(QueryFailure::new(code, recovery))
}

fn scope_within(requested: &QueryScope, admitted: &QueryScope) -> bool {
    if requested == admitted {
        return true;
    }
    match (requested, admitted) {
        (
            QueryScope::Repository {
                path: requested_path,
                scope: requested_scope,
            },
            QueryScope::Repository {
                path: admitted_path,
                scope: admitted_scope,
            },
        ) if requested_path == admitted_path => matches!(
            (requested_scope, admitted_scope),
            (
                unisphere_core::query::RepoScope::Exact,
                unisphere_core::query::RepoScope::Tree
                    | unisphere_core::query::RepoScope::Worktrees
            ) | (
                unisphere_core::query::RepoScope::Tree,
                unisphere_core::query::RepoScope::Worktrees
            )
        ),
        _ => false,
    }
}

fn rows_for(view: &QueryView, dataset: Dataset) -> Vec<RowRef<'_>> {
    if let Some(rows) = view.saved_rows(dataset) {
        return rows.iter().map(RowRef::Saved).collect();
    }
    match dataset {
        Dataset::Sources => view.sources().iter().map(RowRef::Source).collect(),
        Dataset::Sessions => view.sessions().iter().map(RowRef::Session).collect(),
        Dataset::Turns => view.turns().iter().map(RowRef::Turn).collect(),
        Dataset::Messages => view.messages().iter().map(RowRef::Message).collect(),
        Dataset::Tools => view.tools().iter().map(RowRef::Tool).collect(),
        Dataset::Events => view.events().iter().map(RowRef::Event).collect(),
    }
}

fn apply_operation<'a>(
    view: &'a QueryView,
    request: &QueryRequest,
    rows: &mut Vec<RowRef<'a>>,
) -> Result<(), QueryFailure> {
    match &request.operation {
        Operation::Show { entity } => rows.retain(|row| row.id() == *entity),
        Operation::Check { source } => rows.retain(|row| row.id() == source.entity()),
        Operation::Tree { session } => {
            let sessions = view.sessions();
            let mut selected = BTreeSet::from([*session]);
            loop {
                let before = selected.len();
                for row in sessions {
                    if row
                        .parent_ids
                        .iter()
                        .any(|parent| selected.contains(parent))
                    {
                        selected.insert(row.id);
                    }
                }
                if selected.len() == before {
                    break;
                }
                if selected.len() > request.limits.max_branch_memberships {
                    return Err(QueryFailure::limit(LimitKind::BranchMemberships));
                }
            }
            rows.retain(|row| selected.contains(&row.id()));
        }
        Operation::List | Operation::Extract | Operation::Stats { .. } => {}
    }
    Ok(())
}

fn apply_branch_and_range(request: &QueryRequest, rows: &mut Vec<RowRef<'_>>) {
    if let Some(branch) = request.branch {
        rows.retain(|row| row.branch_ids().contains(&branch));
    }
    if let Some(range) = request.turn_range {
        rows.retain(|row| match row.field(FieldId::Ordinal) {
            Some(FieldValue::Unsigned(value)) => {
                value >= range.start as u64 && value <= range.end as u64
            }
            _ => false,
        });
    }
}

struct FilterGroup {
    field: FieldId,
    predicate: Predicate,
    values: Vec<FieldValue>,
    regex: Vec<Regex>,
    glob: Vec<GlobMatcher>,
    ignore_case: bool,
}

fn compile_filters(filters: &[Filter]) -> Result<Vec<FilterGroup>, QueryFailure> {
    let mut grouped = BTreeMap::<(FieldId, Predicate, bool), Vec<FieldValue>>::new();
    for filter in filters {
        grouped
            .entry((filter.field, filter.predicate, filter.ignore_case))
            .or_default()
            .extend(filter.values.iter().cloned());
    }
    grouped
        .into_iter()
        .map(|((field, predicate, ignore_case), values)| {
            let mut regex = Vec::new();
            let mut glob = Vec::new();
            if predicate == Predicate::Regex {
                for value in &values {
                    let FieldValue::String(pattern) = value else {
                        return Err(invalid_pattern());
                    };
                    regex.push(
                        RegexBuilder::new(pattern)
                            .case_insensitive(ignore_case)
                            .size_limit(1024 * 1024)
                            .build()
                            .map_err(|_| invalid_pattern())?,
                    );
                }
            }
            if predicate == Predicate::Glob {
                for value in &values {
                    let FieldValue::String(pattern) = value else {
                        return Err(invalid_pattern());
                    };
                    glob.push(
                        GlobBuilder::new(pattern)
                            .case_insensitive(ignore_case)
                            .build()
                            .map_err(|_| invalid_pattern())?
                            .compile_matcher(),
                    );
                }
            }
            Ok(FilterGroup {
                field,
                predicate,
                values,
                regex,
                glob,
                ignore_case,
            })
        })
        .collect()
}

fn invalid_pattern() -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::InvalidPattern,
        RecoveryAction::ReadQueryHelp,
    )
}

fn row_matches(
    row: &RowRef<'_>,
    filters: &[FilterGroup],
    scanned: &mut usize,
    maximum: usize,
) -> Result<bool, QueryFailure> {
    for filter in filters
        .iter()
        .filter(|filter| filter.predicate != Predicate::Exclude)
    {
        if !filter_matches(*row, filter, scanned, maximum)? {
            return Ok(false);
        }
    }
    for filter in filters
        .iter()
        .filter(|filter| filter.predicate == Predicate::Exclude)
    {
        if filter_matches(*row, filter, scanned, maximum)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn filter_matches(
    row: RowRef<'_>,
    filter: &FilterGroup,
    scanned: &mut usize,
    maximum: usize,
) -> Result<bool, QueryFailure> {
    if filter.predicate == Predicate::Has
        && unisphere_core::query::schema(row.dataset())
            .reserved_fields
            .contains(&filter.field)
    {
        return Ok(true);
    }
    let Some(actual) = row.field(filter.field) else {
        return Ok(false);
    };
    if filter.predicate == Predicate::Has {
        return Ok(true);
    }
    if matches!(actual, FieldValue::Null) {
        return Ok(false);
    }
    for text in strings(&actual) {
        *scanned = scanned
            .checked_add(text.len())
            .ok_or_else(|| QueryFailure::limit(LimitKind::ScannedTextBytes))?;
        if *scanned > maximum {
            return Err(QueryFailure::limit(LimitKind::ScannedTextBytes));
        }
    }
    Ok(match filter.predicate {
        Predicate::Equal | Predicate::In | Predicate::Exclude => filter
            .values
            .iter()
            .any(|expected| values_equal(&actual, expected, filter.ignore_case)),
        Predicate::Contains => strings(&actual).into_iter().any(|actual| {
            filter.values.iter().any(|expected| match expected {
                FieldValue::String(expected) if filter.ignore_case => {
                    actual.to_lowercase().contains(&expected.to_lowercase())
                }
                FieldValue::String(expected) => actual.contains(expected),
                _ => false,
            })
        }),
        Predicate::Regex => strings(&actual)
            .into_iter()
            .any(|actual| filter.regex.iter().any(|regex| regex.is_match(actual))),
        Predicate::Glob => strings(&actual)
            .into_iter()
            .any(|actual| filter.glob.iter().any(|glob| glob.is_match(actual))),
        Predicate::AtLeast => filter.values.iter().any(|expected| {
            value_cmp(&actual, expected).is_some_and(|order| order != Ordering::Less)
        }),
        Predicate::Has => true,
    })
}

fn strings(value: &FieldValue) -> Vec<&str> {
    match value {
        FieldValue::String(value) => vec![value],
        FieldValue::Strings(values) => values.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    }
}

fn values_equal(actual: &FieldValue, expected: &FieldValue, ignore_case: bool) -> bool {
    match (actual, expected) {
        (FieldValue::String(left), FieldValue::String(right)) if ignore_case => {
            left.to_lowercase() == right.to_lowercase()
        }
        (FieldValue::Strings(left), FieldValue::String(right)) if ignore_case => {
            let right = right.to_lowercase();
            left.iter().any(|value| value.to_lowercase() == right)
        }
        (FieldValue::Strings(left), FieldValue::String(right)) => left.contains(right),
        (FieldValue::IdList(left), FieldValue::Id(right)) => left.contains(right),
        (FieldValue::IdList(left), FieldValue::IdList(right)) => {
            right.iter().any(|value| left.contains(value))
        }
        _ => actual == expected,
    }
}

fn value_cmp(left: &FieldValue, right: &FieldValue) -> Option<Ordering> {
    match (left, right) {
        (FieldValue::Unsigned(left), FieldValue::Unsigned(right)) => Some(left.cmp(right)),
        (FieldValue::Integer(left), FieldValue::Integer(right)) => Some(left.cmp(right)),
        (FieldValue::Float(left), FieldValue::Float(right)) => left.partial_cmp(right),
        (FieldValue::Timestamp(left), FieldValue::Timestamp(right)) => Some(left.cmp(right)),
        (FieldValue::String(left), FieldValue::String(right)) => Some(left.cmp(right)),
        (FieldValue::Id(left), FieldValue::Id(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn time_matches(row: &RowRef<'_>, request: &QueryRequest) -> bool {
    if request.time.since.is_none() && request.time.until.is_none() {
        return true;
    }
    let field = request
        .time
        .field
        .or(unisphere_core::query::schema(request.dataset).default_time_field);
    let Some(FieldValue::Timestamp(value)) = field.and_then(|field| row.field(field)) else {
        return request.time.include_undated;
    };
    request
        .time
        .since
        .as_ref()
        .is_none_or(|since| value >= *since)
        && request
            .time
            .until
            .as_ref()
            .is_none_or(|until| value < *until)
}

fn expand_context<'a>(
    view: &'a QueryView,
    request: &QueryRequest,
    matched: &[RowRef<'a>],
) -> Result<(Vec<RowRef<'a>>, BTreeSet<EntityId>), QueryFailure> {
    let all = rows_for(view, request.dataset);
    let matched_ids = matched.iter().map(|row| row.id()).collect::<BTreeSet<_>>();
    let mut selected = matched_ids.clone();
    for matched_row in matched {
        let Some(session) = matched_row.session_id() else {
            continue;
        };
        let branches = if matched_row.branch_ids().is_empty() {
            vec![None]
        } else {
            matched_row.branch_ids().iter().copied().map(Some).collect()
        };
        for branch in branches {
            let mut partition = all
                .iter()
                .copied()
                .filter(|row| {
                    row.session_id() == Some(session)
                        && branch.is_none_or(|branch| row.branch_ids().contains(&branch))
                })
                .collect::<Vec<_>>();
            if partition.iter().any(|row| row.native_order().is_some()) {
                partition.sort_by(|left, right| {
                    match (left.native_order(), right.native_order()) {
                        (Some(left), Some(right)) => left.cmp(right),
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => Ordering::Equal,
                    }
                    .then_with(|| left.id().cmp(&right.id()))
                });
            }
            let Some(position) = partition
                .iter()
                .position(|row| row.id() == matched_row.id())
            else {
                continue;
            };
            let start = position.saturating_sub(request.context.before);
            let end = position
                .saturating_add(request.context.after)
                .saturating_add(1)
                .min(partition.len());
            selected.extend(partition[start..end].iter().map(|row| row.id()));
            if selected.len().saturating_sub(matched_ids.len())
                > request.limits.max_context_neighbours
            {
                return Err(QueryFailure::limit(LimitKind::ContextNeighbours));
            }
        }
    }
    let context = selected.difference(&matched_ids).copied().collect();
    Ok((
        all.into_iter()
            .filter(|row| selected.contains(&row.id()))
            .collect(),
        context,
    ))
}

fn sort_context_rows(rows: &mut [RowRef<'_>]) {
    rows.sort_by(|left, right| {
        left.session_id()
            .cmp(&right.session_id())
            .then_with(|| left.branch_ids().first().cmp(&right.branch_ids().first()))
            .then_with(|| match (left.native_order(), right.native_order()) {
                (Some(left), Some(right)) => left.cmp(right),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            })
            .then_with(|| left.id().cmp(&right.id()))
    });
}

fn sort_rows(request: &QueryRequest, rows: &mut [RowRef<'_>]) {
    let sort = effective_sort(request);
    rows.sort_by(|left, right| {
        for key in &sort {
            let order =
                compare_optional(left.field(key.field), right.field(key.field), key.direction);
            if order != Ordering::Equal {
                return order;
            }
        }
        left.id().cmp(&right.id())
    });
}

fn effective_sort(request: &QueryRequest) -> Vec<SortKey> {
    if !request.sort.is_empty() {
        return request.sort.clone();
    }
    let schema = unisphere_core::query::schema(request.dataset);
    schema
        .default_order
        .iter()
        .copied()
        .map(|field| SortKey {
            field,
            direction: if request.dataset == Dataset::Sessions && field == FieldId::StartedAt {
                SortDirection::Descending
            } else {
                SortDirection::Ascending
            },
        })
        .collect()
}

fn compare_optional(
    left: Option<FieldValue>,
    right: Option<FieldValue>,
    direction: SortDirection,
) -> Ordering {
    match (left, right) {
        (None | Some(FieldValue::Null), None | Some(FieldValue::Null)) => Ordering::Equal,
        (None | Some(FieldValue::Null), _) => Ordering::Greater,
        (_, None | Some(FieldValue::Null)) => Ordering::Less,
        (Some(left), Some(right)) => {
            let order = value_cmp(&left, &right)
                .unwrap_or_else(|| canonical_value(&left).cmp(&canonical_value(&right)));
            if direction == SortDirection::Descending {
                order.reverse()
            } else {
                order
            }
        }
    }
}

fn canonical_value(value: &FieldValue) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

fn normalized_columns(request: &QueryRequest) -> Vec<FieldId> {
    let requested = request
        .columns
        .as_deref()
        .unwrap_or(unisphere_core::query::schema(request.dataset).default_columns);
    let mut columns = vec![FieldId::Id, FieldId::SourceRefs];
    for field in requested {
        if !columns.contains(field) {
            columns.push(*field);
        }
    }
    if (request.context.before != 0 || request.context.after != 0)
        && !columns.contains(&FieldId::IsContext)
    {
        columns.push(FieldId::IsContext);
    }
    columns
}

fn effective_limit(request: &QueryRequest) -> Option<usize> {
    match request.limit {
        Some(0) => None,
        Some(limit) => Some(limit),
        None if request.operation.kind() == unisphere_core::query::OperationKind::List => Some(50),
        None => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_stats(
    view: &QueryView,
    request: &QueryRequest,
    rows: Vec<RowRef<'_>>,
    matched: usize,
    group_by: &[FieldId],
    metrics: &[Metric],
    query_digest: Digest,
    binding: CursorBinding,
    access: &ContentAccess,
) -> Result<QueryResponse, QueryFailure> {
    if request.unresolved == unisphere_core::query::UnresolvedPolicy::Reject {
        let candidates = rows
            .iter()
            .filter(|row| matches!(
                row.field(FieldId::Availability),
                Some(FieldValue::Strings(values)) if values.iter().any(|value| value == "conflict" || value == "ambiguous")
            ))
            .map(|row| row.id())
            .collect::<Vec<_>>();
        if !candidates.is_empty() {
            return Err(QueryFailure::new(
                QueryFailureCode::AmbiguousIdentity,
                RecoveryAction::ChooseEntity {
                    dataset: request.dataset,
                    candidates,
                },
            ));
        }
    }
    let mut grouped_bytes = 0usize;
    let mut groups = BTreeMap::<Vec<Vec<u8>>, Vec<RowRef<'_>>>::new();
    for row in rows {
        let key = group_by
            .iter()
            // A NUL-prefixed absence marker cannot collide with encoded JSON.
            .map(|field| {
                row.field(*field)
                    .map_or_else(|| b"\0absent".to_vec(), |value| canonical_value(&value))
            })
            .collect::<Vec<_>>();
        grouped_bytes = key
            .iter()
            .try_fold(grouped_bytes, |total, value| total.checked_add(value.len()))
            .ok_or_else(|| QueryFailure::limit(LimitKind::RetainedBytes))?;
        if grouped_bytes > request.limits.max_retained_bytes {
            return Err(QueryFailure::limit(LimitKind::RetainedBytes));
        }
        groups.entry(key).or_default().push(row);
    }
    if groups.is_empty() && group_by.is_empty() {
        groups.insert(Vec::new(), Vec::new());
    }
    let mut projected = Vec::with_capacity(groups.len());
    for (key, rows) in groups {
        let mut fields = BTreeMap::new();
        if let Some(first) = rows.first().copied() {
            for field in group_by {
                if let Some(value) = first.field(*field) {
                    fields.insert(*field, value);
                }
            }
        }
        for metric in metrics {
            fields.insert(
                metric_field(*metric),
                metric_value(*metric, &rows, view.usage())?,
            );
        }
        let id = EntityId::derive(
            EntityKind::Group,
            std::iter::once(request.dataset.as_str().as_bytes())
                .chain(key.iter().map(Vec::as_slice)),
        );
        let mut source_refs = Vec::new();
        for row in &rows {
            for reference in row.source_refs()? {
                if !source_refs.contains(&reference) {
                    source_refs.push(reference);
                }
            }
        }
        projected.push(ProjectedRow::new(
            request.dataset,
            id,
            source_refs,
            fields,
            access,
        )?);
    }
    let start = match &request.cursor {
        Some(value) => cursor::decode(value, binding, projected.len())?,
        None => 0,
    };
    let applied_limit = effective_limit(request);
    let end = applied_limit
        .and_then(|limit| start.checked_add(limit))
        .map_or(projected.len(), |end| end.min(projected.len()));
    let next_cursor = (end < projected.len())
        .then(|| cursor::encode(binding, end))
        .transpose()?;
    let rows = projected.drain(start..end).collect::<Vec<_>>();
    let mut columns = vec![FieldId::Id, FieldId::SourceRefs];
    columns.extend(group_by.iter().copied());
    columns.extend(metrics.iter().copied().map(metric_field));
    finish_response(
        view,
        request,
        rows,
        matched,
        next_cursor,
        columns,
        applied_limit,
        query_digest,
    )
}

fn metric_field(metric: Metric) -> FieldId {
    match metric {
        Metric::Count => FieldId::Count,
        Metric::MeasuredCount => FieldId::MeasuredCount,
        Metric::MissingDurationCount => FieldId::MissingDurationCount,
        Metric::Succeeded => FieldId::Succeeded,
        Metric::Failures => FieldId::Failures,
        Metric::Cancelled => FieldId::Cancelled,
        Metric::Incomplete => FieldId::Incomplete,
        Metric::Unknown => FieldId::Unknown,
        Metric::FailureRate => FieldId::FailureRate,
        Metric::MeanMs => FieldId::MeanMs,
        Metric::MinMs => FieldId::MinMs,
        Metric::MaxMs => FieldId::MaxMs,
        Metric::P50Ms => FieldId::P50Ms,
        Metric::P95Ms => FieldId::P95Ms,
        Metric::InputTokens => FieldId::InputTokens,
        Metric::OutputTokens => FieldId::OutputTokens,
        Metric::CacheReadTokens => FieldId::CacheReadTokens,
        Metric::CacheWriteTokens => FieldId::CacheWriteTokens,
    }
}

fn metric_value(
    metric: Metric,
    rows: &[RowRef<'_>],
    usage: &[UsageFact],
) -> Result<FieldValue, QueryFailure> {
    let measured_count = || measured_durations(rows).count();
    let outcome_count = |outcome: Outcome| {
        rows.iter().filter(|row| matches!(row.field(FieldId::Status), Some(FieldValue::String(value)) if value == outcome.as_str())).count() as u64
    };
    Ok(match metric {
        Metric::Count => FieldValue::Unsigned(rows.len() as u64),
        Metric::MeasuredCount => FieldValue::Unsigned(measured_count() as u64),
        Metric::MissingDurationCount => {
            FieldValue::Unsigned(rows.len().saturating_sub(measured_count()) as u64)
        }
        Metric::Succeeded => FieldValue::Unsigned(outcome_count(Outcome::Succeeded)),
        Metric::Failures => FieldValue::Unsigned(outcome_count(Outcome::Failed)),
        Metric::Cancelled => FieldValue::Unsigned(outcome_count(Outcome::Cancelled)),
        Metric::Incomplete => FieldValue::Unsigned(outcome_count(Outcome::Incomplete)),
        Metric::Unknown => FieldValue::Unsigned(outcome_count(Outcome::Unknown)),
        Metric::FailureRate => {
            let succeeded = outcome_count(Outcome::Succeeded);
            let failed = outcome_count(Outcome::Failed);
            let cancelled = outcome_count(Outcome::Cancelled);
            let denominator = succeeded + failed + cancelled;
            if denominator == 0 {
                FieldValue::Null
            } else {
                FieldValue::Float(failed as f64 / denominator as f64)
            }
        }
        Metric::MeanMs => {
            let (count, sum) = measured_durations(rows)
                .fold((0_u64, 0.0_f64), |(count, sum), value| {
                    (count + 1, sum + value)
                });
            if count == 0 {
                FieldValue::Null
            } else {
                FieldValue::Float(sum / count as f64)
            }
        }
        Metric::MinMs => measured_durations(rows)
            .reduce(f64::min)
            .map_or(FieldValue::Null, FieldValue::Float),
        Metric::MaxMs => measured_durations(rows)
            .reduce(f64::max)
            .map_or(FieldValue::Null, FieldValue::Float),
        Metric::P50Ms => percentile(measured_durations(rows).collect(), 50),
        Metric::P95Ms => percentile(measured_durations(rows).collect(), 95),
        Metric::InputTokens
        | Metric::OutputTokens
        | Metric::CacheReadTokens
        | Metric::CacheWriteTokens => {
            usage_metric(metric, rows, usage)?.map_or(FieldValue::Null, FieldValue::Unsigned)
        }
    })
}

fn measured_durations<'rows, 'data>(
    rows: &'rows [RowRef<'data>],
) -> impl Iterator<Item = f64> + 'rows {
    rows.iter()
        .filter_map(|row| match row.field(FieldId::DurationMs) {
            Some(FieldValue::Float(value)) if value.is_finite() && value >= 0.0 => Some(value),
            _ => None,
        })
}

fn percentile(mut values: Vec<f64>, percentile: usize) -> FieldValue {
    if values.is_empty() {
        return FieldValue::Null;
    }
    values.sort_by(f64::total_cmp);
    let rank = (percentile * values.len()).div_ceil(100).max(1);
    FieldValue::Float(values[rank - 1])
}

fn usage_metric(
    metric: Metric,
    rows: &[RowRef<'_>],
    usage: &[UsageFact],
) -> Result<Option<u64>, QueryFailure> {
    let dataset = rows.first().map(|row| row.dataset());
    let sessions = rows
        .iter()
        .filter(|row| row.dataset() == Dataset::Sessions)
        .map(|row| row.id())
        .collect::<BTreeSet<_>>();
    let turns = rows
        .iter()
        .filter(|row| row.dataset() == Dataset::Turns)
        .map(|row| row.id())
        .collect::<BTreeSet<_>>();
    let native_tools = rows
        .iter()
        .filter(|row| row.dataset() == Dataset::Tools)
        .filter_map(|row| {
            let session = row.session_id()?;
            match row.field(FieldId::NativeId) {
                Some(FieldValue::String(value)) => Some((session, value)),
                _ => None,
            }
        })
        .collect::<BTreeSet<_>>();
    let mut cumulative = BTreeMap::<
        (
            unisphere_core::query::SourceId,
            Option<EntityId>,
            Option<String>,
            unisphere_core::query::UsageScope,
        ),
        &UsageFact,
    >::new();
    let mut values = Vec::new();
    for fact in usage {
        let admitted = match dataset {
            Some(Dataset::Sessions) => fact.session_id.is_some_and(|id| sessions.contains(&id)),
            Some(Dataset::Turns) => fact.turn_id.is_some_and(|id| turns.contains(&id)),
            Some(Dataset::Tools) => fact
                .session_id
                .zip(fact.owner.as_ref())
                .is_some_and(|(session, owner)| native_tools.contains(&(session, owner.clone()))),
            _ => false,
        };
        if !admitted {
            continue;
        }
        if fact.scope == unisphere_core::query::UsageScope::CumulativeSnapshot {
            let key = (
                fact.source_id,
                fact.session_id,
                fact.owner.clone(),
                fact.scope,
            );
            if cumulative
                .get(&key)
                .is_none_or(|current| current.sequence < fact.sequence)
            {
                cumulative.insert(key, fact);
            }
        } else {
            values.push(fact);
        }
    }
    values.extend(cumulative.into_values());
    let mut sum = 0_u64;
    for fact in values {
        let value = match metric {
            Metric::InputTokens => fact.counters.input_tokens,
            Metric::OutputTokens => fact.counters.output_tokens,
            Metric::CacheReadTokens => fact.counters.cache_read_tokens,
            Metric::CacheWriteTokens => fact.counters.cache_write_tokens,
            _ => None,
        };
        let Some(value) = value else {
            return Ok(None);
        };
        sum = sum
            .checked_add(value)
            .ok_or_else(QueryFailure::invalid_data)?;
    }
    Ok(Some(sum))
}

#[allow(clippy::too_many_arguments)]
fn finish_response(
    view: &QueryView,
    request: &QueryRequest,
    rows: Vec<ProjectedRow>,
    matched: usize,
    next_cursor: Option<String>,
    columns: Vec<FieldId>,
    applied_limit: Option<usize>,
    query_digest: Digest,
) -> Result<QueryResponse, QueryFailure> {
    let columns_digest = Digest::framed(
        b"unisphere/query-columns/v1",
        columns.iter().map(|field| field.as_str().as_bytes()),
    );
    let emitted = rows.len();
    let basis = view
        .saved_universe_basis()
        .unwrap_or(UniverseBasis::LiveView);
    let rows_complete = if next_cursor.is_some() || emitted < matched {
        Completeness::Subset
    } else if basis == UniverseBasis::LiveView {
        Completeness::Complete
    } else {
        view.saved_rows_complete()
    };
    let universe = ResultUniverse {
        source_view_digest: Some(view.digest()),
        selection_digest: query_digest,
        columns_digest,
        columns,
        applied_limit,
        rows_complete_for_selection: rows_complete,
        partitions_complete: if basis == UniverseBasis::LiveView {
            Completeness::Complete
        } else {
            view.saved_partitions_complete()
        },
        basis,
        bounded_by_input: view.saved_bounded_by_input(),
    };
    universe.validate()?;
    let next_action = if let Some(cursor) = &next_cursor {
        QueryAction::Continue {
            cursor: cursor.clone(),
            reason: ActionReason::MoreRows,
        }
    } else if emitted == 0 {
        QueryAction::NarrowSelection {
            reason: ActionReason::EmptySelection,
            needed_inputs: Vec::new(),
        }
    } else if !view.coverage().source_read_complete || view.saved_bounded_by_input() {
        QueryAction::InspectCoverage {
            reason: ActionReason::PartialEvidence,
        }
    } else if request.operation.kind() == unisphere_core::query::OperationKind::List {
        QueryAction::InspectEntity {
            dataset: request.dataset,
            entity: rows[0].id(),
            reason: ActionReason::NextDetail,
        }
    } else {
        QueryAction::ReadRecipe {
            topic: "query-results".to_owned(),
            reason: ActionReason::NextDetail,
        }
    };
    let scope_bytes =
        serde_json::to_vec(&request.scope).map_err(|_| QueryFailure::invalid_data())?;
    Ok(QueryResponse {
        schema_version: 1,
        dataset: request.dataset,
        query: QueryDescription {
            dataset: request.dataset,
            operation: request.operation.kind(),
            scope_digest: Digest::of_bytes(&scope_bytes),
        },
        rows,
        coverage: view.coverage().clone(),
        universe,
        matched: matched as u64,
        emitted: emitted as u64,
        next_cursor,
        next_action,
    })
}
