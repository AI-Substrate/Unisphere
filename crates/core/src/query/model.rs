use super::{
    AdapterId, Digest, EntityId, EntityKind, HarnessId, QueryFailure, QueryFailureCode,
    RecoveryAction, SourceId,
};
use serde::{
    Deserialize, Serialize,
    ser::{SerializeMap, SerializeStruct},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
    sync::Arc,
};

query_enum! { pub enum Dataset { Sources => "sources", Sessions => "sessions", Turns => "turns", Messages => "messages", Tools => "tools", Events => "events" } }
query_enum! { pub enum RepoScope { Exact => "exact", Tree => "tree", Worktrees => "worktrees" } }
query_enum! { pub enum SavedFormat { QueryJsonV1 => "query_json_v1", QueryJsonlV1 => "query_jsonl_v1" } }
query_enum! { pub enum UnresolvedPolicy { Reject => "reject", Separate => "separate" } }
query_enum! { pub enum SortDirection { Ascending => "ascending", Descending => "descending" } }
query_enum! { pub enum Predicate { Equal => "equal", In => "in", Exclude => "exclude", Contains => "contains", Regex => "regex", Glob => "glob", AtLeast => "at_least", Has => "has" } }
query_enum! { pub enum Metric { Count => "count", MeasuredCount => "measured_count", MissingDurationCount => "missing_duration_count", Succeeded => "succeeded", Failures => "failures", Cancelled => "cancelled", Incomplete => "incomplete", Unknown => "unknown", FailureRate => "failure_rate", MeanMs => "mean_ms", MinMs => "min_ms", MaxMs => "max_ms", P50Ms => "p50_ms", P95Ms => "p95_ms", InputTokens => "input_tokens", OutputTokens => "output_tokens", CacheReadTokens => "cache_read_tokens", CacheWriteTokens => "cache_write_tokens" } }
query_enum! { pub enum Completeness { Complete => "complete", Subset => "subset", Unknown => "unknown" } }
query_enum! { pub enum UniverseBasis { LiveView => "live_view", SavedSelection => "saved_selection", ProvidedRows => "provided_rows" } }
query_enum! { pub enum OutputFormat { Json => "json", Jsonl => "jsonl", Csv => "csv", Table => "table", Text => "text", Markdown => "markdown" } }
query_enum! { pub enum CsvSafety { Spreadsheet => "spreadsheet", Raw => "raw" } }

query_enum! {
    /// Closed registry shared by schemas, filters, projections, availability, and grouping.
    pub enum FieldId {
        Id => "id", SourceRefs => "source_refs", NativeId => "native_id", Harness => "harness", Adapter => "adapter",
        Availability => "availability", Format => "format", ReadStatus => "read_status", Association => "association",
        Revision => "revision", ProjectPath => "project_path", SourcePath => "source_path", Name => "name", Models => "models",
        StartedAt => "started_at", FirstEventAt => "first_event_at", SourceIds => "source_ids", ParentIds => "parent_ids",
        BranchIds => "branch_ids", TurnCount => "turn_count", MessageCount => "message_count", ToolCallCount => "tool_call_count",
        TranscriptAvailable => "transcript_available", SessionId => "session_id", Ordinal => "ordinal", MessageIds => "message_ids",
        CallIds => "call_ids", Roles => "roles", ToolNames => "tool_names", ToolFamilies => "tool_families", HasErrors => "has_errors",
        BoundaryBasis => "boundary_basis", TurnId => "turn_id", Role => "role", Timestamp => "timestamp", Text => "text",
        Parts => "parts", Model => "model", ToolName => "tool_name", ToolFamily => "tool_family", EndedAt => "ended_at",
        DurationMs => "duration_ms", DurationBasis => "duration_basis", Status => "status", StatusReason => "status_reason",
        ExitCode => "exit_code", Command => "command", Input => "input", Output => "output", CallId => "call_id",
        MessageId => "message_id", Kind => "kind", Count => "count", MeasuredCount => "measured_count",
        MissingDurationCount => "missing_duration_count", Succeeded => "succeeded", Failures => "failures", Cancelled => "cancelled",
        Incomplete => "incomplete", Unknown => "unknown", FailureRate => "failure_rate", MeanMs => "mean_ms", MinMs => "min_ms",
        MaxMs => "max_ms", P50Ms => "p50_ms", P95Ms => "p95_ms", InputTokens => "input_tokens",
        OutputTokens => "output_tokens", CacheReadTokens => "cache_read_tokens", CacheWriteTokens => "cache_write_tokens"
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueryScope {
    Repository { path: PathBuf, scope: RepoScope },
    Source { selector: SourceSelector },
    Offline { input: OfflineRef },
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SourceSelector {
    Path {
        path: PathBuf,
        adapter: Option<AdapterId>,
    },
    Id(SourceId),
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OfflineRef {
    File(PathBuf),
    Stdin,
}

fn invalid_request(dataset: Dataset) -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::InvalidArgument,
        RecoveryAction::ConsultSchema { dataset },
    )
}

fn validate_scope(scope: &QueryScope, dataset: Dataset) -> Result<(), QueryFailure> {
    let path = match scope {
        QueryScope::Repository { path, .. }
        | QueryScope::Source {
            selector: SourceSelector::Path { path, .. },
        }
        | QueryScope::Offline {
            input: OfflineRef::File(path),
        } => Some(path),
        QueryScope::Source {
            selector: SourceSelector::Id(_),
        }
        | QueryScope::Offline {
            input: OfflineRef::Stdin,
        } => None,
    };
    if path.is_some_and(|path| !path.is_absolute() || path.to_str().is_none()) {
        return Err(invalid_request(dataset));
    }
    Ok(())
}

const fn dataset_entity_kind(dataset: Dataset) -> EntityKind {
    match dataset {
        Dataset::Sources => EntityKind::Source,
        Dataset::Sessions => EntityKind::Session,
        Dataset::Turns => EntityKind::Turn,
        Dataset::Messages => EntityKind::Message,
        Dataset::Tools => EntityKind::Tool,
        Dataset::Events => EntityKind::Event,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    List,
    Show {
        entity: EntityId,
    },
    Tree {
        session: EntityId,
    },
    Check {
        source: SourceId,
    },
    Stats {
        group_by: Vec<FieldId>,
        metrics: Vec<Metric>,
    },
    Extract,
}
impl Operation {
    pub const fn kind(&self) -> OperationKind {
        match self {
            Self::List => OperationKind::List,
            Self::Show { .. } => OperationKind::Show,
            Self::Tree { .. } => OperationKind::Tree,
            Self::Check { .. } => OperationKind::Check,
            Self::Stats { .. } => OperationKind::Stats,
            Self::Extract => OperationKind::Extract,
        }
    }
}
query_enum! { pub enum OperationKind { List => "list", Show => "show", Tree => "tree", Check => "check", Stats => "stats", Extract => "extract" } }

/// Exact instant plus its native evidence basis. `unix_nanos` is UTC since Unix epoch;
/// equality and ordering compare only that instant, never the evidence basis.
#[derive(Debug, Clone)]
pub struct Timestamp {
    unix_nanos: i128,
    basis: TimestampBasis,
}
query_enum! { pub enum TimestampBasis { Native => "native", SourceReported => "source_reported", DerivedFromSupportedNative => "derived_from_supported_native", SuppliedUnknown => "supplied_unknown" } }
impl Timestamp {
    pub fn new(unix_nanos: i128, basis: TimestampBasis) -> Result<Self, QueryFailure> {
        time::OffsetDateTime::from_unix_timestamp_nanos(unix_nanos).map_err(|_| invalid_time())?;
        Ok(Self { unix_nanos, basis })
    }
    pub fn parse(value: &str, basis: TimestampBasis) -> Result<Self, QueryFailure> {
        let instant = if value.is_ascii()
            && value.len() == 10
            && value.as_bytes().get(4) == Some(&b'-')
            && value.as_bytes().get(7) == Some(&b'-')
        {
            let year = value[0..4].parse().map_err(|_| invalid_time())?;
            let month = value[5..7]
                .parse::<u8>()
                .ok()
                .and_then(|month| time::Month::try_from(month).ok())
                .ok_or_else(invalid_time)?;
            let day = value[8..10].parse().map_err(|_| invalid_time())?;
            time::Date::from_calendar_date(year, month, day)
                .map_err(|_| invalid_time())?
                .with_time(time::Time::MIDNIGHT)
                .assume_utc()
        } else {
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
                .map_err(|_| invalid_time())?
                .to_offset(time::UtcOffset::UTC)
        };
        Self::new(instant.unix_timestamp_nanos(), basis)
    }
    pub const fn unix_nanos(&self) -> i128 {
        self.unix_nanos
    }
    pub const fn basis(&self) -> TimestampBasis {
        self.basis
    }
    pub fn to_wire(&self) -> Result<String, QueryFailure> {
        time::OffsetDateTime::from_unix_timestamp_nanos(self.unix_nanos)
            .map_err(|_| invalid_time())?
            .to_offset(time::UtcOffset::UTC)
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| invalid_time())
    }
}
impl PartialEq for Timestamp {
    fn eq(&self, other: &Self) -> bool {
        self.unix_nanos == other.unix_nanos
    }
}
impl Eq for Timestamp {}
impl PartialOrd for Timestamp {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Timestamp {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.unix_nanos.cmp(&other.unix_nanos)
    }
}
fn invalid_time() -> QueryFailure {
    QueryFailure::new(QueryFailureCode::InvalidTime, RecoveryAction::ReadQueryHelp)
}
impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_wire().map_err(serde::ser::Error::custom)?)
    }
}
impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value, TimestampBasis::SuppliedUnknown).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    pub field: Option<FieldId>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    pub include_undated: bool,
}
impl TimeWindow {
    fn has_invalid_bounds(&self) -> bool {
        self.since
            .as_ref()
            .zip(self.until.as_ref())
            .is_some_and(|(since, until)| since.unix_nanos() >= until.unix_nanos())
    }

    pub fn validate(&self) -> Result<(), QueryFailure> {
        if self.has_invalid_bounds() {
            return Err(invalid_time());
        }
        Ok(())
    }

    fn validate_for(&self, dataset: Dataset) -> Result<(), QueryFailure> {
        if self.has_invalid_bounds() {
            return Err(QueryFailure::new(
                QueryFailureCode::InvalidTime,
                RecoveryAction::ConsultSchema { dataset },
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InclusiveRange {
    pub start: usize,
    pub end: usize,
}
impl InclusiveRange {
    pub fn new(start: usize, end: usize) -> Result<Self, QueryFailure> {
        let range = Self { start, end };
        range.validate()?;
        Ok(range)
    }
    pub fn validate(self) -> Result<(), QueryFailure> {
        if self.start == 0 || self.end < self.start {
            return Err(QueryFailure::new(
                QueryFailureCode::InvalidArgument,
                RecoveryAction::ReadQueryHelp,
            ));
        }
        Ok(())
    }
    fn validate_for(self, dataset: Dataset) -> Result<(), QueryFailure> {
        if self.start == 0 || self.end < self.start {
            return Err(invalid_request(dataset));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextWindow {
    pub before: usize,
    pub after: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortKey {
    pub field: FieldId,
    pub direction: SortDirection,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FieldValue {
    Null,
    Bool(bool),
    Unsigned(u64),
    Integer(i64),
    Float(f64),
    String(String),
    Timestamp(Timestamp),
    Id(EntityId),
    IdList(Vec<EntityId>),
    Strings(Vec<String>),
    Structured(serde_json::Value),
}
impl FieldValue {
    pub fn validate(&self) -> Result<(), QueryFailure> {
        if matches!(self, Self::Float(value) if !value.is_finite()) {
            return Err(QueryFailure::invalid_data());
        }
        Ok(())
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub field: FieldId,
    pub predicate: Predicate,
    pub values: Vec<FieldValue>,
    pub ignore_case: bool,
}
impl Filter {
    pub fn validate(&self, limits: &QueryLimits) -> Result<(), QueryFailure> {
        self.validate_with(limits, RecoveryAction::ReadQueryHelp)
    }
    fn validate_for(&self, limits: &QueryLimits, dataset: Dataset) -> Result<(), QueryFailure> {
        self.validate_with(limits, RecoveryAction::ConsultSchema { dataset })
    }
    fn validate_with(
        &self,
        limits: &QueryLimits,
        invalid_argument_recovery: RecoveryAction,
    ) -> Result<(), QueryFailure> {
        if self.predicate == Predicate::Has {
            if !self.values.is_empty() {
                return Err(QueryFailure::new(
                    QueryFailureCode::InvalidArgument,
                    invalid_argument_recovery,
                ));
            }
            return Ok(());
        }
        if self.values.is_empty() || self.values.len() > limits.max_patterns {
            return Err(QueryFailure::limit(LimitKind::Patterns));
        }
        for value in &self.values {
            value.validate()?;
            if matches!(value, FieldValue::String(text) if text.len() > limits.max_pattern_bytes) {
                return Err(QueryFailure::limit(LimitKind::PatternBytes));
            }
        }
        Ok(())
    }
}

/// Registration-level selection evaluated before providers enumerate or read stores.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSelection {
    pub include_adapters: BTreeSet<AdapterId>,
    pub exclude_adapters: BTreeSet<AdapterId>,
    pub include_harnesses: BTreeSet<HarnessId>,
    pub exclude_harnesses: BTreeSet<HarnessId>,
}
impl SourceSelection {
    pub fn admits(&self, adapter: &AdapterId, harness: &HarnessId) -> bool {
        (self.include_adapters.is_empty() || self.include_adapters.contains(adapter))
            && !self.exclude_adapters.contains(adapter)
            && (self.include_harnesses.is_empty() || self.include_harnesses.contains(harness))
            && !self.exclude_harnesses.contains(harness)
    }
    pub fn validate(&self) -> Result<(), QueryFailure> {
        if !self.include_adapters.is_disjoint(&self.exclude_adapters)
            || !self.include_harnesses.is_disjoint(&self.exclude_harnesses)
        {
            return Err(QueryFailure::new(
                QueryFailureCode::InvalidArgument,
                RecoveryAction::ReadQueryHelp,
            ));
        }
        Ok(())
    }
}

query_enum! {
    pub enum LimitKind {
        Sources => "sources", TotalInputBytes => "total_input_bytes", SourceBytes => "source_bytes",
        ObservationsAndRows => "observations_and_rows", RetainedBytes => "retained_bytes", OutputBytes => "output_bytes",
        PatternBytes => "pattern_bytes", Patterns => "patterns", ScannedTextBytes => "scanned_text_bytes",
        ContextNeighbours => "context_neighbours", BranchMemberships => "branch_memberships", CursorBytes => "cursor_bytes"
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryLimits {
    pub max_sources: usize,
    pub max_total_input_bytes: usize,
    pub max_source_bytes: usize,
    pub max_observations_and_rows: usize,
    pub max_retained_bytes: usize,
    pub max_output_bytes: usize,
    pub max_pattern_bytes: usize,
    pub max_patterns: usize,
    pub max_scanned_text_bytes: usize,
    pub max_context_neighbours: usize,
    pub max_branch_memberships: usize,
    pub max_cursor_bytes: usize,
}
impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            max_sources: 4_096,
            max_total_input_bytes: 256 * 1024 * 1024,
            max_source_bytes: 64 * 1024 * 1024,
            max_observations_and_rows: 200_000,
            max_retained_bytes: 256 * 1024 * 1024,
            max_output_bytes: 128 * 1024 * 1024,
            max_pattern_bytes: 4_096,
            max_patterns: 64,
            max_scanned_text_bytes: 256 * 1024 * 1024,
            max_context_neighbours: 1_000,
            max_branch_memberships: 1_000_000,
            max_cursor_bytes: 2_048,
        }
    }
}
impl QueryLimits {
    pub const HARD: Self = Self {
        max_sources: 16_384,
        max_total_input_bytes: 1024 * 1024 * 1024,
        max_source_bytes: 256 * 1024 * 1024,
        max_observations_and_rows: 1_000_000,
        max_retained_bytes: 1024 * 1024 * 1024,
        max_output_bytes: 512 * 1024 * 1024,
        max_pattern_bytes: 4_096,
        max_patterns: 64,
        max_scanned_text_bytes: 1024 * 1024 * 1024,
        max_context_neighbours: 1_000,
        max_branch_memberships: 4_000_000,
        max_cursor_bytes: 2_048,
    };
    pub fn validate(self) -> Result<(), QueryFailure> {
        let pairs = [
            (self.max_sources, Self::HARD.max_sources, LimitKind::Sources),
            (
                self.max_total_input_bytes,
                Self::HARD.max_total_input_bytes,
                LimitKind::TotalInputBytes,
            ),
            (
                self.max_source_bytes,
                Self::HARD.max_source_bytes,
                LimitKind::SourceBytes,
            ),
            (
                self.max_observations_and_rows,
                Self::HARD.max_observations_and_rows,
                LimitKind::ObservationsAndRows,
            ),
            (
                self.max_retained_bytes,
                Self::HARD.max_retained_bytes,
                LimitKind::RetainedBytes,
            ),
            (
                self.max_output_bytes,
                Self::HARD.max_output_bytes,
                LimitKind::OutputBytes,
            ),
            (
                self.max_pattern_bytes,
                Self::HARD.max_pattern_bytes,
                LimitKind::PatternBytes,
            ),
            (
                self.max_patterns,
                Self::HARD.max_patterns,
                LimitKind::Patterns,
            ),
            (
                self.max_scanned_text_bytes,
                Self::HARD.max_scanned_text_bytes,
                LimitKind::ScannedTextBytes,
            ),
            (
                self.max_context_neighbours,
                Self::HARD.max_context_neighbours,
                LimitKind::ContextNeighbours,
            ),
            (
                self.max_branch_memberships,
                Self::HARD.max_branch_memberships,
                LimitKind::BranchMemberships,
            ),
            (
                self.max_cursor_bytes,
                Self::HARD.max_cursor_bytes,
                LimitKind::CursorBytes,
            ),
        ];
        for (value, ceiling, kind) in pairs {
            if value == 0 || value > ceiling {
                return Err(QueryFailure::limit(kind));
            }
        }
        if self.max_source_bytes > self.max_total_input_bytes {
            return Err(QueryFailure::limit(LimitKind::SourceBytes));
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryRequest {
    pub dataset: Dataset,
    pub operation: Operation,
    pub scope: QueryScope,
    pub filters: Vec<Filter>,
    pub time: TimeWindow,
    pub branch: Option<EntityId>,
    pub turn_range: Option<InclusiveRange>,
    pub sort: Vec<SortKey>,
    pub columns: Option<Vec<FieldId>>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub context: ContextWindow,
    pub include_content: bool,
    pub allow_partial: bool,
    pub unresolved: UnresolvedPolicy,
    pub limits: QueryLimits,
}
fn has_exact_session_selection(filters: &[Filter]) -> bool {
    let mut selected = None;
    let mut found = false;
    for filter in filters.iter().filter(|filter| {
        filter.field == FieldId::SessionId
            && matches!(filter.predicate, Predicate::Equal | Predicate::In)
    }) {
        for value in &filter.values {
            let FieldValue::Id(id) = value else {
                return false;
            };
            if id.kind() != EntityKind::Session || selected.is_some_and(|selected| selected != *id)
            {
                return false;
            }
            selected = Some(*id);
            found = true;
        }
    }
    found
}

impl QueryRequest {
    /// Validate all effect-independent bounds before any source or writer is called.
    pub fn validate(&self) -> Result<(), QueryFailure> {
        self.limits.validate()?;
        self.time.validate_for(self.dataset)?;
        let schema = super::schema(self.dataset);
        if !schema.permitted_operations.contains(&self.operation.kind()) {
            return Err(QueryFailure::new(
                QueryFailureCode::UnsupportedOperation,
                RecoveryAction::ConsultSchema {
                    dataset: self.dataset,
                },
            ));
        }
        validate_scope(&self.scope, self.dataset)?;
        match &self.operation {
            Operation::Show { entity } if entity.kind() != dataset_entity_kind(self.dataset) => {
                return Err(invalid_request(self.dataset));
            }
            Operation::Tree { session } if session.kind() != EntityKind::Session => {
                return Err(invalid_request(self.dataset));
            }
            _ => {}
        }
        if self
            .branch
            .is_some_and(|branch| branch.kind() != EntityKind::Branch)
        {
            return Err(invalid_request(self.dataset));
        }
        let time_field = self.time.field.or(schema.default_time_field);
        if (self.time.field.is_some() || self.time.since.is_some() || self.time.until.is_some())
            && time_field
                .and_then(|field| schema.field(field))
                .is_none_or(|field| field.field_type != super::FieldType::Timestamp)
        {
            return Err(QueryFailure::new(
                QueryFailureCode::UnsupportedOperation,
                RecoveryAction::ConsultSchema {
                    dataset: self.dataset,
                },
            ));
        }
        if self
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.len() > self.limits.max_cursor_bytes)
        {
            return Err(QueryFailure::limit(LimitKind::CursorBytes));
        }
        if self
            .context
            .before
            .checked_add(self.context.after)
            .is_none_or(|n| n > self.limits.max_context_neighbours)
        {
            return Err(QueryFailure::limit(LimitKind::ContextNeighbours));
        }
        if (self.context.before != 0 || self.context.after != 0)
            && !matches!(self.dataset, Dataset::Turns | Dataset::Messages)
        {
            return Err(QueryFailure::new(
                QueryFailureCode::UnsupportedOperation,
                RecoveryAction::ConsultSchema {
                    dataset: self.dataset,
                },
            ));
        }
        for filter in &self.filters {
            filter.validate_for(&self.limits, self.dataset)?;
            schema.validate_field(filter.field, filter.predicate)?;
        }
        for sort in &self.sort {
            schema.field(sort.field).ok_or_else(|| {
                QueryFailure::new(
                    QueryFailureCode::InvalidField,
                    RecoveryAction::ConsultSchema {
                        dataset: self.dataset,
                    },
                )
            })?;
        }
        if let Some(columns) = &self.columns {
            let mut unique = BTreeSet::new();
            for field in columns {
                if !unique.insert(*field) || schema.field(*field).is_none() {
                    return Err(QueryFailure::new(
                        QueryFailureCode::InvalidField,
                        RecoveryAction::ChooseField {
                            dataset: self.dataset,
                            allowed: schema.fields.iter().map(|field| field.id).collect(),
                        },
                    ));
                }
            }
            schema.validate_projection(columns, self.include_content)?;
        }
        if let Some(range) = self.turn_range {
            range.validate_for(self.dataset)?;
            if self.dataset != Dataset::Turns || !has_exact_session_selection(&self.filters) {
                return Err(invalid_request(self.dataset));
            }
        }
        if let Operation::Stats { group_by, metrics } = &self.operation {
            schema.validate_metrics(metrics)?;
            if group_by
                .iter()
                .any(|field| !schema.grouping_fields.contains(field))
            {
                return Err(QueryFailure::new(
                    QueryFailureCode::UnsupportedOperation,
                    RecoveryAction::ConsultSchema {
                        dataset: self.dataset,
                    },
                ));
            }
        }
        Ok(())
    }

    /// Search predicates authorize inspection of only their named sensitive fields.
    pub fn content_access(&self) -> Result<ContentAccess, QueryFailure> {
        self.validate()?;
        let schema = super::schema(self.dataset);
        let inspect_fields = self
            .filters
            .iter()
            .filter_map(|filter| {
                schema
                    .field(filter.field)
                    .filter(|field| field.sensitivity == super::Sensitivity::Sensitive)
                    .map(|_| filter.field)
            })
            .collect();
        Ok(ContentAccess {
            inspect_fields,
            emit_content: self.include_content,
        })
    }
}

/// Only explicitly retained content may be inspected; emission requires separate consent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentAccess {
    pub inspect_fields: BTreeSet<FieldId>,
    pub emit_content: bool,
}
impl ContentAccess {
    pub fn permits_payload(&self, field: FieldId) -> bool {
        self.emit_content || self.inspect_fields.contains(&field)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultUniverse {
    pub source_view_digest: Option<Digest>,
    pub selection_digest: Digest,
    pub columns_digest: Digest,
    pub columns: Vec<FieldId>,
    pub applied_limit: Option<usize>,
    pub rows_complete_for_selection: Completeness,
    pub partitions_complete: Completeness,
    pub basis: UniverseBasis,
    pub bounded_by_input: bool,
}
impl ResultUniverse {
    pub fn validate(&self) -> Result<(), QueryFailure> {
        let mut unique = BTreeSet::new();
        if self.columns.iter().any(|field| !unique.insert(*field)) {
            return Err(QueryFailure::invalid_data());
        }
        if self.rows_complete_for_selection == Completeness::Complete
            && self.basis == UniverseBasis::ProvidedRows
            && self.bounded_by_input
        {
            return Err(QueryFailure::invalid_data());
        }
        Ok(())
    }
}

query_enum! { pub enum ProjectedLocatorKind { Jsonl => "jsonl", Snapshot => "snapshot", GitNote => "git_note" } }

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ProjectedSourceRef {
    source_id: SourceId,
    revision: String,
    locator_kind: ProjectedLocatorKind,
    offset: Option<u64>,
}
impl ProjectedSourceRef {
    pub fn new(
        source_id: SourceId,
        revision: String,
        locator_kind: ProjectedLocatorKind,
        offset: Option<u64>,
    ) -> Result<Self, QueryFailure> {
        if revision.is_empty() || (locator_kind != ProjectedLocatorKind::Jsonl && offset.is_some())
        {
            return Err(QueryFailure::invalid_data());
        }
        Ok(Self {
            source_id,
            revision,
            locator_kind,
            offset,
        })
    }
    pub const fn source_id(&self) -> SourceId {
        self.source_id
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub const fn locator_kind(&self) -> ProjectedLocatorKind {
        self.locator_kind
    }
    pub const fn offset(&self) -> Option<u64> {
        self.offset
    }
}

/// Deserialization target only. It is not accepted by writers until validation succeeds.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct UntrustedProjectedSourceRef {
    pub source_id: SourceId,
    pub revision: String,
    pub locator_kind: ProjectedLocatorKind,
    pub offset: Option<u64>,
}
#[derive(Clone, PartialEq, Deserialize)]
pub struct UntrustedProjectedRow {
    pub schema_version: u16,
    pub dataset: Dataset,
    pub id: EntityId,
    pub source_refs: Vec<UntrustedProjectedSourceRef>,
    pub fields: BTreeMap<FieldId, serde_json::Value>,
}

/// Validated output state. Private fields prevent bypassing schema and consent checks.
#[derive(Clone, PartialEq)]
pub struct ProjectedRow {
    schema_version: u16,
    dataset: Dataset,
    id: EntityId,
    source_refs: Vec<ProjectedSourceRef>,
    fields: BTreeMap<FieldId, FieldValue>,
}
impl ProjectedRow {
    pub fn new(
        dataset: Dataset,
        id: EntityId,
        source_refs: Vec<ProjectedSourceRef>,
        fields: BTreeMap<FieldId, FieldValue>,
        access: &ContentAccess,
    ) -> Result<Self, QueryFailure> {
        let schema = super::schema(dataset);
        let expected = dataset_entity_kind(dataset);
        if id.kind() != expected && id.kind() != super::EntityKind::Group {
            return Err(QueryFailure::invalid_data());
        }
        for (field, value) in &fields {
            let spec = schema.field(*field).ok_or_else(|| {
                QueryFailure::new(
                    QueryFailureCode::InvalidField,
                    RecoveryAction::ConsultSchema { dataset },
                )
            })?;
            if schema.reserved_fields.contains(field) {
                return Err(QueryFailure::invalid_data());
            }
            if spec.sensitivity == super::Sensitivity::Sensitive && !access.emit_content {
                return Err(QueryFailure::new(
                    QueryFailureCode::ContentConsentRequired,
                    RecoveryAction::UseMetadataOrConsent {
                        fields: vec![*field],
                    },
                ));
            }
            value.validate()?;
            if !field_value_matches(value, spec.field_type, spec.nullable) {
                return Err(QueryFailure::invalid_data());
            }
        }
        Ok(Self {
            schema_version: 1,
            dataset,
            id,
            source_refs,
            fields,
        })
    }
    pub fn from_untrusted(
        row: UntrustedProjectedRow,
        access: &ContentAccess,
    ) -> Result<Self, QueryFailure> {
        if row.schema_version != 1 {
            return Err(QueryFailure::new(
                QueryFailureCode::UnsupportedSchema,
                RecoveryAction::ConsultSchema {
                    dataset: row.dataset,
                },
            ));
        }
        let schema = super::schema(row.dataset);
        let source_refs = row
            .source_refs
            .into_iter()
            .map(|reference| {
                ProjectedSourceRef::new(
                    reference.source_id,
                    reference.revision,
                    reference.locator_kind,
                    reference.offset,
                )
            })
            .collect::<Result<_, _>>()?;
        let fields = row
            .fields
            .into_iter()
            .map(|(field, value)| {
                let spec = schema.field(field).ok_or_else(|| {
                    QueryFailure::new(
                        QueryFailureCode::InvalidField,
                        RecoveryAction::ConsultSchema {
                            dataset: row.dataset,
                        },
                    )
                })?;
                projected_value_from_json(value, spec.field_type, spec.nullable)
                    .map(|value| (field, value))
            })
            .collect::<Result<_, _>>()?;
        Self::new(row.dataset, row.id, source_refs, fields, access)
    }
    pub const fn schema_version(&self) -> u16 {
        self.schema_version
    }
    pub const fn dataset(&self) -> Dataset {
        self.dataset
    }
    pub const fn id(&self) -> EntityId {
        self.id
    }
    pub fn source_refs(&self) -> &[ProjectedSourceRef] {
        &self.source_refs
    }
    pub fn field(&self, field: FieldId) -> Option<&FieldValue> {
        self.fields.get(&field)
    }
    pub fn fields(&self) -> &BTreeMap<FieldId, FieldValue> {
        &self.fields
    }
}

struct ProjectedFields<'a>(&'a BTreeMap<FieldId, FieldValue>);
impl Serialize for ProjectedFields<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (field, value) in self.0 {
            map.serialize_entry(field, &ProjectedValue(value))?;
        }
        map.end()
    }
}

struct ProjectedValue<'a>(&'a FieldValue);
impl Serialize for ProjectedValue<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            FieldValue::Null => serializer.serialize_none(),
            FieldValue::Bool(value) => serializer.serialize_bool(*value),
            FieldValue::Unsigned(value) => serializer.serialize_u64(*value),
            FieldValue::Integer(value) => serializer.serialize_i64(*value),
            FieldValue::Float(value) => serializer.serialize_f64(*value),
            FieldValue::String(value) => serializer.serialize_str(value),
            FieldValue::Timestamp(value) => value.serialize(serializer),
            FieldValue::Id(value) => value.serialize(serializer),
            FieldValue::IdList(value) => value.serialize(serializer),
            FieldValue::Strings(value) => value.serialize(serializer),
            FieldValue::Structured(value) => value.serialize(serializer),
        }
    }
}

impl Serialize for ProjectedRow {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut row = serializer.serialize_struct("ProjectedRow", 5)?;
        row.serialize_field("schema_version", &self.schema_version)?;
        row.serialize_field("dataset", &self.dataset)?;
        row.serialize_field("id", &self.id)?;
        row.serialize_field("source_refs", &self.source_refs)?;
        row.serialize_field("fields", &ProjectedFields(&self.fields))?;
        row.end()
    }
}

fn field_value_matches(value: &FieldValue, expected: super::FieldType, nullable: bool) -> bool {
    match value {
        FieldValue::Null => nullable,
        FieldValue::Bool(_) => expected == super::FieldType::Bool,
        FieldValue::Unsigned(_) => expected == super::FieldType::U64,
        FieldValue::Integer(_) => expected == super::FieldType::I64,
        FieldValue::Float(_) => expected == super::FieldType::FiniteF64,
        FieldValue::String(_) => matches!(
            expected,
            super::FieldType::String | super::FieldType::EnumList
        ),
        FieldValue::Timestamp(_) => expected == super::FieldType::Timestamp,
        FieldValue::Id(_) => expected == super::FieldType::EntityId,
        FieldValue::IdList(_) => expected == super::FieldType::IdList,
        FieldValue::Strings(_) => expected == super::FieldType::EnumList,
        FieldValue::Structured(value) => {
            !value.is_null()
                && matches!(
                    expected,
                    super::FieldType::Structured | super::FieldType::SourceRefs
                )
        }
    }
}

fn projected_value_from_json(
    value: serde_json::Value,
    expected: super::FieldType,
    nullable: bool,
) -> Result<FieldValue, QueryFailure> {
    use serde_json::Value;
    let invalid = QueryFailure::invalid_data;
    match (value, expected) {
        (Value::Null, _) if nullable => Ok(FieldValue::Null),
        (Value::Bool(value), super::FieldType::Bool) => Ok(FieldValue::Bool(value)),
        (Value::Number(value), super::FieldType::U64) => {
            value.as_u64().map(FieldValue::Unsigned).ok_or_else(invalid)
        }
        (Value::Number(value), super::FieldType::I64) => {
            value.as_i64().map(FieldValue::Integer).ok_or_else(invalid)
        }
        (Value::Number(value), super::FieldType::FiniteF64) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(FieldValue::Float)
            .ok_or_else(invalid),
        (Value::String(value), super::FieldType::String | super::FieldType::EnumList) => {
            Ok(FieldValue::String(value))
        }
        (Value::String(value), super::FieldType::Timestamp) => {
            Timestamp::parse(&value, TimestampBasis::SuppliedUnknown).map(FieldValue::Timestamp)
        }
        (Value::String(value), super::FieldType::EntityId) => {
            value.parse().map(FieldValue::Id).map_err(|_| invalid())
        }
        (Value::Array(values), super::FieldType::IdList) => values
            .into_iter()
            .map(|value| match value {
                Value::String(value) => value.parse().map_err(|_| invalid()),
                _ => Err(invalid()),
            })
            .collect::<Result<_, _>>()
            .map(FieldValue::IdList),
        (Value::Array(values), super::FieldType::EnumList) => values
            .into_iter()
            .map(|value| match value {
                Value::String(value) => Ok(value),
                _ => Err(invalid()),
            })
            .collect::<Result<_, _>>()
            .map(FieldValue::Strings),
        (value, super::FieldType::Structured | super::FieldType::SourceRefs)
            if !value.is_null() =>
        {
            Ok(FieldValue::Structured(value))
        }
        _ => Err(invalid()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueryAction {
    InspectEntity {
        dataset: Dataset,
        entity: EntityId,
        reason: ActionReason,
    },
    Continue {
        cursor: String,
        reason: ActionReason,
    },
    NarrowSelection {
        reason: ActionReason,
        needed_inputs: Vec<FieldId>,
    },
    InspectCoverage {
        reason: ActionReason,
    },
    ReadSchema {
        dataset: Dataset,
        reason: ActionReason,
    },
    ReadRecipe {
        topic: String,
        reason: ActionReason,
    },
}
query_enum! { pub enum ActionReason { MoreRows => "more_rows", EmptySelection => "empty_selection", PartialEvidence => "partial_evidence", MissingInput => "missing_input", UnsupportedCapability => "unsupported_capability", NextDetail => "next_detail" } }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryDescription {
    pub dataset: Dataset,
    pub operation: OperationKind,
    pub scope_digest: Digest,
}
#[derive(Clone, PartialEq, Serialize)]
pub struct QueryResponse {
    pub schema_version: u16,
    pub dataset: Dataset,
    pub query: QueryDescription,
    pub rows: Vec<ProjectedRow>,
    pub coverage: super::Coverage,
    pub universe: ResultUniverse,
    pub matched: u64,
    pub emitted: u64,
    pub next_cursor: Option<String>,
    pub next_action: QueryAction,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedAction {
    pub summary: String,
    pub argv: Vec<String>,
    pub required_inputs: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryOutputOptions {
    pub format: OutputFormat,
    pub csv_safety: CsvSafety,
    pub max_output_bytes: usize,
    pub next_action: RenderedAction,
}
impl QueryOutputOptions {
    pub fn validate(&self, limits: &QueryLimits) -> Result<(), QueryFailure> {
        if self.max_output_bytes == 0 || self.max_output_bytes > limits.max_output_bytes {
            return Err(QueryFailure::limit(LimitKind::OutputBytes));
        }
        if self.next_action.summary.trim().is_empty() {
            return Err(QueryFailure::invalid_data());
        }
        Ok(())
    }
    pub fn validate_for(
        &self,
        response: &QueryResponse,
        limits: &QueryLimits,
    ) -> Result<(), QueryFailure> {
        self.validate(limits)?;
        if super::schema(response.dataset)
            .format(response.query.operation, self.format)
            .is_none()
        {
            return Err(QueryFailure::new(
                QueryFailureCode::UnsupportedOperation,
                RecoveryAction::ConsultSchema {
                    dataset: response.dataset,
                },
            ));
        }
        Ok(())
    }
}

/// Saved input bytes are owned and bounded by the source implementation.
#[derive(Clone, PartialEq)]
pub enum QueryInput {
    Native(super::NativeQueryView),
    Saved {
        bytes: Arc<[u8]>,
        format: SavedFormat,
    },
}

pub trait QueryApi: Send + Sync {
    fn execute(&self, request: &QueryRequest) -> Result<QueryResponse, QueryFailure>;
}
pub trait QueryWriter: Send + Sync {
    fn write(
        &self,
        response: &QueryResponse,
        options: &QueryOutputOptions,
        destination: &mut dyn Write,
    ) -> Result<(), QueryFailure>;
}
