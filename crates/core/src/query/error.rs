use super::{AdapterId, Dataset, EntityId, FieldId, LimitKind, SourceId};
use serde::Serialize;
use std::{error::Error, fmt};

query_enum! {
    /// A cursor can disagree with the request without its sources changing.
    pub enum CursorMismatchReason {
        QueryOptionsChanged => "query_options_changed",
        SourceViewChanged => "source_view_changed"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryFailureCode {
    InvalidArgument,
    InvalidField,
    InvalidPattern,
    InvalidTime,
    MissingSource,
    UnreadableSource,
    UnsupportedSource,
    UnsupportedSchema,
    UnsupportedOperation,
    InvalidData,
    AmbiguousIdentity,
    AmbiguousBranch,
    StaleCursor(CursorMismatchReason),
    MissingField,
    InputSubset,
    ViewScopeMismatch,
    ContentConsentRequired,
    ResourceLimit,
    OutputFailure,
}
impl QueryFailureCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "UNI-QUERY-ARGUMENT",
            Self::InvalidField => "UNI-QUERY-FIELD",
            Self::InvalidPattern => "UNI-QUERY-PATTERN",
            Self::InvalidTime => "UNI-QUERY-TIME",
            Self::MissingSource => "UNI-QUERY-SOURCE-MISSING",
            Self::UnreadableSource => "UNI-QUERY-SOURCE-READ",
            Self::UnsupportedSource => "UNI-QUERY-SOURCE-UNSUPPORTED",
            Self::UnsupportedSchema => "UNI-QUERY-SCHEMA",
            Self::UnsupportedOperation => "UNI-QUERY-OPERATION",
            Self::InvalidData => "UNI-QUERY-DATA",
            Self::AmbiguousIdentity => "UNI-QUERY-IDENTITY",
            Self::AmbiguousBranch => "UNI-QUERY-BRANCH",
            Self::StaleCursor(_) => "UNI-QUERY-CURSOR",
            Self::MissingField => "UNI-QUERY-FIELD-MISSING",
            Self::InputSubset => "UNI-QUERY-INPUT-SUBSET",
            Self::ViewScopeMismatch => "UNI-QUERY-VIEW-SCOPE",
            Self::ContentConsentRequired => "UNI-QUERY-CONTENT-CONSENT",
            Self::ResourceLimit => "UNI-QUERY-LIMIT",
            Self::OutputFailure => "UNI-QUERY-OUTPUT",
        }
    }
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidArgument => "The query arguments are not valid for this operation.",
            Self::InvalidField => "This dataset does not declare the requested field.",
            Self::InvalidPattern => {
                "The requested pattern is invalid or exceeds its supported bounds."
            }
            Self::InvalidTime => "Use an RFC3339 instant or a YYYY-MM-DD UTC date.",
            Self::MissingSource => "The selected source is not available in this scope.",
            Self::UnreadableSource => "A selected source could not be read consistently.",
            Self::UnsupportedSource => "The selected source representation is not supported.",
            Self::UnsupportedSchema => "The input is not a supported versioned query document.",
            Self::UnsupportedOperation => "This dataset does not support the requested operation.",
            Self::InvalidData => "The supplied evidence violates the query data contract.",
            Self::AmbiguousIdentity => "The supplied identity resolves to conflicting candidates.",
            Self::AmbiguousBranch => "The selected branch or membership is ambiguous.",
            Self::StaleCursor(CursorMismatchReason::QueryOptionsChanged) => {
                "The continuation belongs to different query options."
            }
            Self::StaleCursor(CursorMismatchReason::SourceViewChanged) => {
                "The source view changed since this continuation was issued."
            }
            Self::MissingField => "The supplied view does not contain a required field.",
            Self::InputSubset => {
                "The supplied rows cannot establish the required complete context."
            }
            Self::ViewScopeMismatch => "The request widens the scope admitted by this view.",
            Self::ContentConsentRequired => {
                "The requested projection requires explicit content consent."
            }
            Self::ResourceLimit => "The operation exceeds a declared query resource bound.",
            Self::OutputFailure => {
                "Output did not complete; partial bytes must not be treated as a complete result."
            }
        }
    }
}

query_enum! {
    pub enum SourceProblem {
        Missing => "missing", Permissions => "permissions", UnsupportedDialect => "unsupported_dialect",
        InvalidData => "invalid_data", ChangedDuringRead => "changed_during_read", GitUnavailable => "git_unavailable"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecoveryAction {
    ChooseField {
        dataset: Dataset,
        allowed: Vec<FieldId>,
    },
    ChooseAdapter {
        allowed: Vec<AdapterId>,
    },
    ChooseEntity {
        dataset: Dataset,
        candidates: Vec<EntityId>,
    },
    ChooseBranch {
        candidates: Vec<EntityId>,
    },
    FixSource {
        reason: SourceProblem,
        source: Option<SourceId>,
    },
    StartFreshQuery,
    SupplyFields {
        required: Vec<FieldId>,
    },
    UseMetadataOrConsent {
        fields: Vec<FieldId>,
    },
    NarrowQuery {
        limit: LimitKind,
    },
    ChooseNewOutput {
        discard_partial: bool,
    },
    UseCompleteInput,
    ReopenView,
    ConsultSchema {
        dataset: Dataset,
    },
}
impl RecoveryAction {
    pub const fn guidance(&self) -> &'static str {
        match self {
            Self::ChooseField { .. } => "Choose a declared field from the dataset schema.",
            Self::ChooseAdapter { .. } => {
                "Choose a supported source adapter from the supplied alternatives."
            }
            Self::ChooseEntity { .. } => {
                "Select one exact local entity identifier from the candidates."
            }
            Self::ChooseBranch { .. } => "Select one explicit branch from the candidates.",
            Self::FixSource { reason, .. } => match reason {
                SourceProblem::Missing => {
                    "Discover the source again or supply an existing explicit source."
                }
                SourceProblem::Permissions => {
                    "Check source access permissions or choose an accessible source."
                }
                SourceProblem::UnsupportedDialect => {
                    "Choose a registered representation supported by this release."
                }
                SourceProblem::InvalidData => {
                    "Regenerate valid input without changing source history in this query."
                }
                SourceProblem::ChangedDuringRead => {
                    "Open a fresh view after the source stops changing during the read."
                }
                SourceProblem::GitUnavailable => {
                    "Supply the absolute path to a trusted standard Git executable."
                }
            },
            Self::StartFreshQuery => {
                "Start without a continuation, or repeat the original query options exactly."
            }
            Self::SupplyFields { .. } => "Supply a versioned input retaining the required fields.",
            Self::UseMetadataOrConsent { .. } => {
                "Choose metadata-only columns or deliberately opt in to content output."
            }
            Self::NarrowQuery { .. } => {
                "Narrow the selection or configure an explicit bound within the supported ceiling."
            }
            Self::ChooseNewOutput { .. } => {
                "Choose a new writable destination and do not accept partial output as complete."
            }
            Self::UseCompleteInput => {
                "Supply a complete versioned JSON extraction with needed partition and field metadata."
            }
            Self::ReopenView => "Open a fresh source view before issuing a new query.",
            Self::ConsultSchema { .. } => {
                "Inspect the dataset schema and use its supported fields and operation."
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct QueryErrorLocation {
    pub source: Option<SourceId>,
    pub entity: Option<EntityId>,
    pub field: Option<FieldId>,
    pub offset: Option<u64>,
}

/// Contains no raw parser text, source path, native payload, or arbitrary detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryFailure {
    code: QueryFailureCode,
    location: Box<QueryErrorLocation>,
    recovery: RecoveryAction,
    retryable: bool,
}
impl QueryFailure {
    pub fn new(code: QueryFailureCode, recovery: RecoveryAction) -> Self {
        Self {
            code,
            location: Box::default(),
            recovery,
            retryable: false,
        }
    }
    pub fn stale_cursor(reason: CursorMismatchReason) -> Self {
        let recovery = match reason {
            CursorMismatchReason::QueryOptionsChanged => RecoveryAction::StartFreshQuery,
            CursorMismatchReason::SourceViewChanged => RecoveryAction::ReopenView,
        };
        Self::new(QueryFailureCode::StaleCursor(reason), recovery)
    }
    pub fn invalid_data() -> Self {
        Self::new(
            QueryFailureCode::InvalidData,
            RecoveryAction::UseCompleteInput,
        )
    }
    pub fn limit(kind: LimitKind) -> Self {
        Self::new(
            QueryFailureCode::ResourceLimit,
            RecoveryAction::NarrowQuery { limit: kind },
        )
    }
    pub fn at(mut self, location: QueryErrorLocation) -> Self {
        *self.location = location;
        self
    }
    pub fn retryable_after_recovery(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
    pub const fn kind(&self) -> QueryFailureCode {
        self.code
    }
    pub const fn code(&self) -> &'static str {
        self.code.as_str()
    }
    pub const fn message(&self) -> &'static str {
        self.code.message()
    }
    pub const fn location(&self) -> &QueryErrorLocation {
        &self.location
    }
    pub const fn recovery(&self) -> &RecoveryAction {
        &self.recovery
    }
    pub const fn retryable(&self) -> bool {
        self.retryable
    }
    pub const fn cursor_reason(&self) -> Option<CursorMismatchReason> {
        match self.code {
            QueryFailureCode::StaleCursor(reason) => Some(reason),
            _ => None,
        }
    }
}
impl fmt::Display for QueryFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {} {}",
            self.code(),
            self.message(),
            self.recovery.guidance()
        )
    }
}
impl Error for QueryFailure {}
impl Serialize for QueryFailure {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("QueryFailure", 6)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", self.message())?;
        state.serialize_field("location", &self.location)?;
        state.serialize_field("reason", &self.cursor_reason())?;
        state.serialize_field("recovery", &self.recovery)?;
        state.serialize_field("retryable", &self.retryable)?;
        state.end()
    }
}
