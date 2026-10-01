use super::{
    Dataset, FieldId, Metric, OperationKind, OutputFormat, Predicate, QueryFailure,
    QueryFailureCode, RecoveryAction,
};

query_enum! { pub enum FieldType { Bool => "bool", U64 => "u64", I64 => "i64", FiniteF64 => "finite_f64", String => "string", Timestamp => "timestamp", EntityId => "entity_id", SourceRefs => "source_refs", IdList => "id_list", EnumList => "enum_list", Structured => "structured" } }
query_enum! { pub enum Sensitivity { Metadata => "metadata", Sensitive => "sensitive" } }
query_enum! { pub enum FieldUnit { Count => "count", Milliseconds => "milliseconds", Tokens => "tokens", Ratio => "ratio" } }
query_enum! { pub enum Availability { Required => "required", SourceQualified => "source_qualified", ProjectedOptional => "projected_optional" } }

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct FieldSchema {
    pub id: FieldId,
    pub field_type: FieldType,
    pub nullable: bool,
    pub unit: Option<FieldUnit>,
    pub sensitivity: Sensitivity,
    pub availability: Availability,
    pub allowed_predicates: &'static [Predicate],
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FormatCapability {
    pub format: OutputFormat,
    pub operations: &'static [OperationKind],
    pub lossy: bool,
    pub losses: &'static [FormatLoss],
}
query_enum! { pub enum FormatLoss { AbsenceNullEmptyCollapse => "absence_null_empty_collapse", StructuredAsJsonText => "structured_as_json_text", HumanRounding => "human_rounding", PresentationOnly => "presentation_only", TimestampBasis => "timestamp_basis" } }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DatasetSchema {
    pub schema_version: u16,
    pub dataset: Dataset,
    pub fields: &'static [FieldSchema],
    pub reserved_fields: &'static [FieldId],
    pub default_columns: &'static [FieldId],
    pub default_time_field: Option<FieldId>,
    pub default_order: &'static [FieldId],
    pub permitted_operations: &'static [OperationKind],
    pub grouping_fields: &'static [FieldId],
    pub metrics: &'static [Metric],
    pub capability_notes: &'static [&'static str],
    pub formats: &'static [FormatCapability],
}
impl DatasetSchema {
    pub fn field(&self, id: FieldId) -> Option<&FieldSchema> {
        self.fields.iter().find(|field| field.id == id)
    }
    pub fn validate_field(
        &self,
        id: FieldId,
        predicate: Predicate,
    ) -> Result<&FieldSchema, QueryFailure> {
        let field = self.field(id).ok_or_else(|| {
            QueryFailure::new(
                QueryFailureCode::InvalidField,
                RecoveryAction::ChooseField {
                    dataset: self.dataset,
                    allowed: self.fields.iter().map(|field| field.id).collect(),
                },
            )
        })?;
        if !field.allowed_predicates.contains(&predicate) {
            return Err(QueryFailure::new(
                QueryFailureCode::UnsupportedOperation,
                RecoveryAction::ConsultSchema {
                    dataset: self.dataset,
                },
            ));
        }
        Ok(field)
    }
    pub fn validate_projection(
        &self,
        fields: &[FieldId],
        include_content: bool,
    ) -> Result<(), QueryFailure> {
        let mut denied = Vec::new();
        for id in fields {
            let field = self.field(*id).ok_or_else(|| {
                QueryFailure::new(
                    QueryFailureCode::InvalidField,
                    RecoveryAction::ChooseField {
                        dataset: self.dataset,
                        allowed: self.fields.iter().map(|field| field.id).collect(),
                    },
                )
            })?;
            if field.sensitivity == Sensitivity::Sensitive && !include_content {
                denied.push(*id);
            }
        }
        if denied.is_empty() {
            Ok(())
        } else {
            Err(QueryFailure::new(
                QueryFailureCode::ContentConsentRequired,
                RecoveryAction::UseMetadataOrConsent { fields: denied },
            ))
        }
    }
    pub fn format(
        &self,
        operation: OperationKind,
        format: OutputFormat,
    ) -> Option<&FormatCapability> {
        self.formats.iter().find(|capability| {
            capability.format == format && capability.operations.contains(&operation)
        })
    }

    pub fn validate_metrics(&self, requested: &[Metric]) -> Result<(), QueryFailure> {
        if requested.is_empty()
            || requested
                .iter()
                .any(|metric| !self.metrics.contains(metric))
        {
            return Err(QueryFailure::new(
                QueryFailureCode::UnsupportedOperation,
                RecoveryAction::ConsultSchema {
                    dataset: self.dataset,
                },
            ));
        }
        Ok(())
    }
}

const EQ: &[Predicate] = &[Predicate::Equal, Predicate::In, Predicate::Exclude];
const TEXT: &[Predicate] = &[
    Predicate::Equal,
    Predicate::In,
    Predicate::Exclude,
    Predicate::Contains,
    Predicate::Regex,
    Predicate::Glob,
    Predicate::Has,
];
const ORDERED: &[Predicate] = &[
    Predicate::Equal,
    Predicate::In,
    Predicate::Exclude,
    Predicate::AtLeast,
    Predicate::Has,
];
const HAS: &[Predicate] = &[Predicate::Has];
const NO_PREDICATES: &[Predicate] = &[];

macro_rules! field {
    ($id:ident, $ty:ident, $nullable:expr, $sensitivity:ident, $availability:ident, $predicates:expr) => {
        FieldSchema {
            id: FieldId::$id,
            field_type: FieldType::$ty,
            nullable: $nullable,
            unit: None,
            sensitivity: Sensitivity::$sensitivity,
            availability: Availability::$availability,
            allowed_predicates: $predicates,
        }
    };
    ($id:ident, $ty:ident, $nullable:expr, $unit:ident, $sensitivity:ident, $availability:ident, $predicates:expr) => {
        FieldSchema {
            id: FieldId::$id,
            field_type: FieldType::$ty,
            nullable: $nullable,
            unit: Some(FieldUnit::$unit),
            sensitivity: Sensitivity::$sensitivity,
            availability: Availability::$availability,
            allowed_predicates: $predicates,
        }
    };
}

const SOURCE_FIELDS: &[FieldSchema] = &[
    field!(Id, EntityId, false, Metadata, Required, EQ),
    field!(SourceRefs, SourceRefs, false, Metadata, Required, HAS),
    field!(NativeId, String, true, Sensitive, SourceQualified, TEXT),
    field!(Harness, String, false, Metadata, Required, EQ),
    field!(Adapter, String, false, Metadata, Required, EQ),
    field!(Availability, EnumList, false, Metadata, Required, HAS),
    field!(Format, String, false, Metadata, Required, EQ),
    field!(ReadStatus, String, false, Metadata, Required, EQ),
    field!(Association, Structured, false, Sensitive, Required, HAS),
    field!(Revision, String, false, Metadata, Required, EQ),
    field!(ProjectPath, String, true, Sensitive, SourceQualified, TEXT),
    field!(SourcePath, String, true, Sensitive, SourceQualified, TEXT),
];
const SESSION_FIELDS: &[FieldSchema] = &[
    field!(Id, EntityId, false, Metadata, Required, EQ),
    field!(SourceRefs, SourceRefs, false, Metadata, Required, HAS),
    field!(NativeId, String, true, Sensitive, SourceQualified, TEXT),
    field!(Harness, String, false, Metadata, Required, EQ),
    field!(Adapter, String, false, Metadata, Required, EQ),
    field!(Availability, EnumList, false, Metadata, Required, HAS),
    field!(Name, String, true, Sensitive, SourceQualified, TEXT),
    field!(Models, EnumList, true, Sensitive, SourceQualified, TEXT),
    field!(
        StartedAt,
        Timestamp,
        true,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(
        FirstEventAt,
        Timestamp,
        true,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(SourceIds, IdList, false, Metadata, Required, HAS),
    field!(ParentIds, IdList, false, Metadata, SourceQualified, HAS),
    field!(BranchIds, IdList, false, Metadata, SourceQualified, HAS),
    field!(
        TurnCount,
        U64,
        true,
        Count,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(
        MessageCount,
        U64,
        true,
        Count,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(
        ToolCallCount,
        U64,
        true,
        Count,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(TranscriptAvailable, Bool, false, Metadata, Required, EQ),
    field!(
        Count,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        InputTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
    field!(
        OutputTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
    field!(
        CacheReadTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
    field!(
        CacheWriteTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
];
const TURN_FIELDS: &[FieldSchema] = &[
    field!(Id, EntityId, false, Metadata, Required, EQ),
    field!(SourceRefs, SourceRefs, false, Metadata, Required, HAS),
    field!(NativeId, String, true, Sensitive, SourceQualified, TEXT),
    field!(Harness, String, false, Metadata, Required, EQ),
    field!(Adapter, String, false, Metadata, Required, EQ),
    field!(Availability, EnumList, false, Metadata, Required, HAS),
    field!(
        IsContext,
        Bool,
        false,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(SessionId, EntityId, false, Metadata, Required, EQ),
    field!(BranchIds, IdList, false, Metadata, SourceQualified, HAS),
    field!(Ordinal, U64, false, Count, Metadata, Required, ORDERED),
    field!(
        StartedAt,
        Timestamp,
        true,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(MessageIds, IdList, false, Metadata, SourceQualified, HAS),
    field!(CallIds, IdList, false, Metadata, SourceQualified, HAS),
    field!(Roles, EnumList, false, Metadata, SourceQualified, HAS),
    field!(ToolNames, EnumList, false, Sensitive, SourceQualified, TEXT),
    field!(ToolFamilies, EnumList, false, Metadata, SourceQualified, EQ),
    field!(HasErrors, Bool, true, Metadata, SourceQualified, EQ),
    field!(
        ToolCallCount,
        U64,
        true,
        Count,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(BoundaryBasis, String, false, Metadata, SourceQualified, EQ),
    field!(
        Count,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        InputTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
    field!(
        OutputTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
    field!(
        CacheReadTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
    field!(
        CacheWriteTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
];
const MESSAGE_FIELDS: &[FieldSchema] = &[
    field!(Id, EntityId, false, Metadata, Required, EQ),
    field!(SourceRefs, SourceRefs, false, Metadata, Required, HAS),
    field!(NativeId, String, true, Sensitive, SourceQualified, TEXT),
    field!(Harness, String, false, Metadata, Required, EQ),
    field!(Adapter, String, false, Metadata, Required, EQ),
    field!(Availability, EnumList, false, Metadata, Required, HAS),
    field!(
        IsContext,
        Bool,
        false,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(SessionId, EntityId, false, Metadata, Required, EQ),
    field!(BranchIds, IdList, false, Metadata, SourceQualified, HAS),
    field!(TurnId, EntityId, true, Metadata, SourceQualified, EQ),
    field!(Role, String, false, Metadata, Required, EQ),
    field!(
        Timestamp,
        Timestamp,
        true,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(Text, String, true, Sensitive, ProjectedOptional, TEXT),
    field!(Parts, Structured, true, Sensitive, ProjectedOptional, HAS),
    field!(Model, String, true, Sensitive, SourceQualified, TEXT),
];
const TOOL_FIELDS: &[FieldSchema] = &[
    field!(Id, EntityId, false, Metadata, Required, EQ),
    field!(SourceRefs, SourceRefs, false, Metadata, Required, HAS),
    field!(NativeId, String, true, Sensitive, SourceQualified, TEXT),
    field!(Harness, String, false, Metadata, Required, EQ),
    field!(Adapter, String, false, Metadata, Required, EQ),
    field!(Availability, EnumList, false, Metadata, Required, HAS),
    field!(SessionId, EntityId, false, Metadata, Required, EQ),
    field!(BranchIds, IdList, false, Metadata, SourceQualified, HAS),
    field!(TurnId, EntityId, true, Metadata, SourceQualified, EQ),
    field!(ToolName, String, true, Sensitive, SourceQualified, TEXT),
    field!(ToolFamily, String, true, Metadata, SourceQualified, EQ),
    field!(
        StartedAt,
        Timestamp,
        true,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(EndedAt, Timestamp, true, Metadata, SourceQualified, ORDERED),
    field!(
        DurationMs,
        FiniteF64,
        true,
        Milliseconds,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(DurationBasis, String, true, Metadata, SourceQualified, EQ),
    field!(Status, String, false, Metadata, Required, EQ),
    field!(StatusReason, String, true, Metadata, SourceQualified, EQ),
    field!(ExitCode, I64, true, Metadata, SourceQualified, ORDERED),
    field!(Command, String, true, Sensitive, ProjectedOptional, TEXT),
    field!(Input, Structured, true, Sensitive, ProjectedOptional, HAS),
    field!(Output, Structured, true, Sensitive, ProjectedOptional, HAS),
    field!(Model, String, true, Sensitive, SourceQualified, TEXT),
    field!(
        Count,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        MeasuredCount,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        MissingDurationCount,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        Succeeded,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        Failures,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        Cancelled,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        Incomplete,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        Unknown,
        U64,
        false,
        Count,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        FailureRate,
        FiniteF64,
        true,
        Ratio,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        MeanMs,
        FiniteF64,
        true,
        Milliseconds,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        MinMs,
        FiniteF64,
        true,
        Milliseconds,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        MaxMs,
        FiniteF64,
        true,
        Milliseconds,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        P50Ms,
        FiniteF64,
        true,
        Milliseconds,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        P95Ms,
        FiniteF64,
        true,
        Milliseconds,
        Metadata,
        ProjectedOptional,
        NO_PREDICATES
    ),
    field!(
        InputTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
    field!(
        OutputTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
    field!(
        CacheReadTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
    field!(
        CacheWriteTokens,
        U64,
        true,
        Tokens,
        Metadata,
        SourceQualified,
        NO_PREDICATES
    ),
];
const EVENT_FIELDS: &[FieldSchema] = &[
    field!(Id, EntityId, false, Metadata, Required, EQ),
    field!(SourceRefs, SourceRefs, false, Metadata, Required, HAS),
    field!(NativeId, String, true, Sensitive, SourceQualified, TEXT),
    field!(Harness, String, false, Metadata, Required, EQ),
    field!(Adapter, String, false, Metadata, Required, EQ),
    field!(Availability, EnumList, false, Metadata, Required, HAS),
    field!(SessionId, EntityId, true, Metadata, SourceQualified, EQ),
    field!(BranchIds, IdList, false, Metadata, SourceQualified, HAS),
    field!(TurnId, EntityId, true, Metadata, SourceQualified, EQ),
    field!(CallId, EntityId, true, Metadata, SourceQualified, EQ),
    field!(MessageId, EntityId, true, Metadata, SourceQualified, EQ),
    field!(Kind, String, false, Metadata, Required, EQ),
    field!(
        Timestamp,
        Timestamp,
        true,
        Metadata,
        SourceQualified,
        ORDERED
    ),
    field!(Parts, Structured, true, Sensitive, ProjectedOptional, HAS),
];

const RESERVED: &[FieldId] = &[FieldId::Id, FieldId::SourceRefs];
const LIST_SHOW_EXTRACT: &[OperationKind] = &[
    OperationKind::List,
    OperationKind::Show,
    OperationKind::Extract,
];
const SESSION_OPS: &[OperationKind] = &[
    OperationKind::List,
    OperationKind::Show,
    OperationKind::Tree,
    OperationKind::Stats,
    OperationKind::Extract,
];
const ROW_OPS: &[OperationKind] = &[
    OperationKind::List,
    OperationKind::Show,
    OperationKind::Stats,
    OperationKind::Extract,
];
const SOURCE_OPS: &[OperationKind] = &[
    OperationKind::List,
    OperationKind::Show,
    OperationKind::Check,
];
const NO_METRICS: &[Metric] = &[];
const COUNT_USAGE_METRICS: &[Metric] = &[
    Metric::Count,
    Metric::InputTokens,
    Metric::OutputTokens,
    Metric::CacheReadTokens,
    Metric::CacheWriteTokens,
];
const TOOL_METRICS: &[Metric] = Metric::ALL;
const NOTES: &[&str] =
    &["Field availability remains source-qualified; absence is distinct from null."];
const LOSSES_CSV: &[FormatLoss] = &[
    FormatLoss::AbsenceNullEmptyCollapse,
    FormatLoss::StructuredAsJsonText,
];
const LOSSES_HUMAN: &[FormatLoss] = &[FormatLoss::HumanRounding, FormatLoss::PresentationOnly];
const LOSSES_TIMESTAMP_JSON: &[FormatLoss] = &[FormatLoss::TimestampBasis];
const LOSSES_TIMESTAMP_CSV: &[FormatLoss] = &[
    FormatLoss::AbsenceNullEmptyCollapse,
    FormatLoss::StructuredAsJsonText,
    FormatLoss::TimestampBasis,
];
const LOSSES_TIMESTAMP_HUMAN: &[FormatLoss] = &[
    FormatLoss::HumanRounding,
    FormatLoss::PresentationOnly,
    FormatLoss::TimestampBasis,
];
const LOSSES_TIMESTAMP_PRESENTATION: &[FormatLoss] =
    &[FormatLoss::PresentationOnly, FormatLoss::TimestampBasis];
const EXTRACT: &[OperationKind] = &[OperationKind::Extract];

const SOURCE_FORMATS: &[FormatCapability] = &[
    FormatCapability {
        format: OutputFormat::Json,
        operations: SOURCE_OPS,
        lossy: false,
        losses: &[],
    },
    FormatCapability {
        format: OutputFormat::Jsonl,
        operations: SOURCE_OPS,
        lossy: false,
        losses: &[],
    },
    FormatCapability {
        format: OutputFormat::Csv,
        operations: SOURCE_OPS,
        lossy: true,
        losses: LOSSES_CSV,
    },
    FormatCapability {
        format: OutputFormat::Table,
        operations: SOURCE_OPS,
        lossy: true,
        losses: LOSSES_HUMAN,
    },
];
const SESSION_FORMATS: &[FormatCapability] = &[
    FormatCapability {
        format: OutputFormat::Json,
        operations: SESSION_OPS,
        lossy: true,
        losses: LOSSES_TIMESTAMP_JSON,
    },
    FormatCapability {
        format: OutputFormat::Jsonl,
        operations: SESSION_OPS,
        lossy: true,
        losses: LOSSES_TIMESTAMP_JSON,
    },
    FormatCapability {
        format: OutputFormat::Csv,
        operations: SESSION_OPS,
        lossy: true,
        losses: LOSSES_TIMESTAMP_CSV,
    },
    FormatCapability {
        format: OutputFormat::Table,
        operations: SESSION_OPS,
        lossy: true,
        losses: LOSSES_TIMESTAMP_HUMAN,
    },
    FormatCapability {
        format: OutputFormat::Text,
        operations: EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_PRESENTATION,
    },
    FormatCapability {
        format: OutputFormat::Markdown,
        operations: EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_PRESENTATION,
    },
];
const ROW_FORMATS: &[FormatCapability] = &[
    FormatCapability {
        format: OutputFormat::Json,
        operations: ROW_OPS,
        lossy: true,
        losses: LOSSES_TIMESTAMP_JSON,
    },
    FormatCapability {
        format: OutputFormat::Jsonl,
        operations: ROW_OPS,
        lossy: true,
        losses: LOSSES_TIMESTAMP_JSON,
    },
    FormatCapability {
        format: OutputFormat::Csv,
        operations: ROW_OPS,
        lossy: true,
        losses: LOSSES_TIMESTAMP_CSV,
    },
    FormatCapability {
        format: OutputFormat::Table,
        operations: ROW_OPS,
        lossy: true,
        losses: LOSSES_TIMESTAMP_HUMAN,
    },
    FormatCapability {
        format: OutputFormat::Text,
        operations: EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_PRESENTATION,
    },
    FormatCapability {
        format: OutputFormat::Markdown,
        operations: EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_PRESENTATION,
    },
];
const MESSAGE_FORMATS: &[FormatCapability] = &[
    FormatCapability {
        format: OutputFormat::Json,
        operations: LIST_SHOW_EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_JSON,
    },
    FormatCapability {
        format: OutputFormat::Jsonl,
        operations: LIST_SHOW_EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_JSON,
    },
    FormatCapability {
        format: OutputFormat::Csv,
        operations: LIST_SHOW_EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_CSV,
    },
    FormatCapability {
        format: OutputFormat::Table,
        operations: LIST_SHOW_EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_HUMAN,
    },
    FormatCapability {
        format: OutputFormat::Text,
        operations: EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_PRESENTATION,
    },
    FormatCapability {
        format: OutputFormat::Markdown,
        operations: EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_PRESENTATION,
    },
];
const EVENT_FORMATS: &[FormatCapability] = &[
    FormatCapability {
        format: OutputFormat::Json,
        operations: LIST_SHOW_EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_JSON,
    },
    FormatCapability {
        format: OutputFormat::Jsonl,
        operations: LIST_SHOW_EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_JSON,
    },
    FormatCapability {
        format: OutputFormat::Csv,
        operations: LIST_SHOW_EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_CSV,
    },
    FormatCapability {
        format: OutputFormat::Table,
        operations: LIST_SHOW_EXTRACT,
        lossy: true,
        losses: LOSSES_TIMESTAMP_HUMAN,
    },
];

const SOURCE_SCHEMA: DatasetSchema = DatasetSchema {
    schema_version: 1,
    dataset: Dataset::Sources,
    fields: SOURCE_FIELDS,
    reserved_fields: RESERVED,
    default_columns: &[
        FieldId::Id,
        FieldId::Adapter,
        FieldId::Harness,
        FieldId::ReadStatus,
    ],
    default_time_field: None,
    default_order: &[FieldId::Id],
    permitted_operations: SOURCE_OPS,
    grouping_fields: &[],
    metrics: NO_METRICS,
    capability_notes: NOTES,
    formats: SOURCE_FORMATS,
};
const SESSION_SCHEMA: DatasetSchema = DatasetSchema {
    schema_version: 1,
    dataset: Dataset::Sessions,
    fields: SESSION_FIELDS,
    reserved_fields: RESERVED,
    default_columns: &[
        FieldId::Id,
        FieldId::StartedAt,
        FieldId::Harness,
        FieldId::Adapter,
    ],
    default_time_field: Some(FieldId::StartedAt),
    default_order: &[FieldId::StartedAt, FieldId::Id],
    permitted_operations: SESSION_OPS,
    grouping_fields: &[FieldId::Harness, FieldId::Adapter, FieldId::Models],
    metrics: COUNT_USAGE_METRICS,
    capability_notes: NOTES,
    formats: SESSION_FORMATS,
};
const TURN_SCHEMA: DatasetSchema = DatasetSchema {
    schema_version: 1,
    dataset: Dataset::Turns,
    fields: TURN_FIELDS,
    reserved_fields: RESERVED,
    default_columns: &[
        FieldId::Id,
        FieldId::SessionId,
        FieldId::Ordinal,
        FieldId::StartedAt,
    ],
    default_time_field: Some(FieldId::StartedAt),
    default_order: &[FieldId::StartedAt, FieldId::Id],
    permitted_operations: ROW_OPS,
    grouping_fields: &[FieldId::SessionId, FieldId::ToolFamilies],
    metrics: COUNT_USAGE_METRICS,
    capability_notes: NOTES,
    formats: ROW_FORMATS,
};
const MESSAGE_SCHEMA: DatasetSchema = DatasetSchema {
    schema_version: 1,
    dataset: Dataset::Messages,
    fields: MESSAGE_FIELDS,
    reserved_fields: RESERVED,
    default_columns: &[
        FieldId::Id,
        FieldId::SessionId,
        FieldId::Role,
        FieldId::Timestamp,
    ],
    default_time_field: Some(FieldId::Timestamp),
    default_order: &[FieldId::Timestamp, FieldId::Id],
    permitted_operations: LIST_SHOW_EXTRACT,
    grouping_fields: &[],
    metrics: NO_METRICS,
    capability_notes: NOTES,
    formats: MESSAGE_FORMATS,
};
const TOOL_SCHEMA: DatasetSchema = DatasetSchema {
    schema_version: 1,
    dataset: Dataset::Tools,
    fields: TOOL_FIELDS,
    reserved_fields: RESERVED,
    default_columns: &[
        FieldId::Id,
        FieldId::SessionId,
        FieldId::ToolFamily,
        FieldId::Status,
        FieldId::StartedAt,
    ],
    default_time_field: Some(FieldId::StartedAt),
    default_order: &[FieldId::StartedAt, FieldId::Id],
    permitted_operations: ROW_OPS,
    grouping_fields: &[FieldId::ToolFamily, FieldId::Status, FieldId::DurationBasis],
    metrics: TOOL_METRICS,
    capability_notes: NOTES,
    formats: ROW_FORMATS,
};
const EVENT_SCHEMA: DatasetSchema = DatasetSchema {
    schema_version: 1,
    dataset: Dataset::Events,
    fields: EVENT_FIELDS,
    reserved_fields: RESERVED,
    default_columns: &[FieldId::Id, FieldId::Kind, FieldId::Timestamp],
    default_time_field: Some(FieldId::Timestamp),
    default_order: &[FieldId::Timestamp, FieldId::Id],
    permitted_operations: LIST_SHOW_EXTRACT,
    grouping_fields: &[],
    metrics: NO_METRICS,
    capability_notes: NOTES,
    formats: EVENT_FORMATS,
};

pub const fn schema(dataset: Dataset) -> &'static DatasetSchema {
    match dataset {
        Dataset::Sources => &SOURCE_SCHEMA,
        Dataset::Sessions => &SESSION_SCHEMA,
        Dataset::Turns => &TURN_SCHEMA,
        Dataset::Messages => &MESSAGE_SCHEMA,
        Dataset::Tools => &TOOL_SCHEMA,
        Dataset::Events => &EVENT_SCHEMA,
    }
}
