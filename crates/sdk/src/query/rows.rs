use std::collections::{BTreeMap, BTreeSet};

use unisphere_core::query::{
    AdapterId, AssociationObservation, AvailabilityCode, BranchLink, ContentAccess, Dataset,
    DurationBasis, EntityId, FieldId, FieldValue, HarnessId, MessageRole, NativeLocator,
    NativeSequence, ObservationPart, Outcome, ProjectedLocatorKind, ProjectedRow,
    ProjectedSourceRef, QueryFailure, SourceId, SourceLocator, SourceReadStatus, SourceRef,
    Timestamp, UsageCounters, UsageScope,
};

#[derive(Clone, PartialEq)]
pub struct SourceRow {
    pub id: EntityId,
    pub source_id: SourceId,
    pub source_refs: Vec<SourceRef>,
    pub adapter: AdapterId,
    pub harness: HarnessId,
    pub representation: String,
    pub read_status: SourceReadStatus,
    pub associations: Vec<AssociationObservation>,
    pub revision: String,
    pub locator: SourceLocator,
    pub availability: Vec<AvailabilityCode>,
}

#[derive(Clone, PartialEq)]
pub struct SessionRow {
    pub id: EntityId,
    pub source_refs: Vec<SourceRef>,
    pub native_id: String,
    pub namespace: String,
    pub participant_id: Option<String>,
    pub adapter: AdapterId,
    pub harness: HarnessId,
    pub availability: Vec<AvailabilityCode>,
    pub name: Option<String>,
    pub models: Vec<String>,
    pub started_at: Option<Timestamp>,
    pub first_event_at: Option<Timestamp>,
    pub source_ids: Vec<EntityId>,
    pub parent_ids: Vec<EntityId>,
    pub branch_ids: Vec<EntityId>,
    pub lineage: Vec<BranchLink>,
    pub turn_count: Option<u64>,
    pub message_count: Option<u64>,
    pub tool_call_count: Option<u64>,
    pub transcript_available: bool,
}

#[derive(Clone, PartialEq)]
pub struct TurnRow {
    pub id: EntityId,
    pub source_refs: Vec<SourceRef>,
    pub native_id: Option<String>,
    pub adapter: AdapterId,
    pub harness: HarnessId,
    pub availability: Vec<AvailabilityCode>,
    pub session_id: EntityId,
    pub branch_ids: Vec<EntityId>,
    pub ordinal: u64,
    pub started_at: Option<Timestamp>,
    pub message_ids: Vec<EntityId>,
    pub call_ids: Vec<EntityId>,
    pub roles: Vec<MessageRole>,
    pub tool_names: Vec<String>,
    pub tool_families: Vec<String>,
    pub has_errors: Option<bool>,
    pub boundary_basis: String,
    pub sequence: NativeSequence,
}

#[derive(Clone, PartialEq)]
pub struct MessageRow {
    pub id: EntityId,
    pub source_refs: Vec<SourceRef>,
    pub native_id: Option<String>,
    pub adapter: AdapterId,
    pub harness: HarnessId,
    pub availability: Vec<AvailabilityCode>,
    pub session_id: EntityId,
    pub branch_ids: Vec<EntityId>,
    pub turn_id: Option<EntityId>,
    pub role: MessageRole,
    pub timestamp: Option<Timestamp>,
    pub parts: Vec<ObservationPart>,
    pub model: Option<String>,
    pub sequence: NativeSequence,
}

#[derive(Clone, PartialEq)]
pub struct ToolRow {
    pub id: EntityId,
    pub source_refs: Vec<SourceRef>,
    pub native_id: String,
    pub adapter: AdapterId,
    pub harness: HarnessId,
    pub availability: Vec<AvailabilityCode>,
    pub session_id: EntityId,
    pub branch_ids: Vec<EntityId>,
    pub turn_id: Option<EntityId>,
    pub tool_name: Option<String>,
    pub tool_family: Option<String>,
    pub started_at: Option<Timestamp>,
    pub ended_at: Option<Timestamp>,
    pub duration_ms: Option<f64>,
    pub duration_basis: Option<DurationBasis>,
    pub status: Outcome,
    pub status_reason: Option<AvailabilityCode>,
    pub exit_code: Option<i64>,
    pub command: Option<String>,
    pub input: Vec<ObservationPart>,
    pub output: Vec<ObservationPart>,
    pub progress: Vec<ObservationPart>,
    pub model: Option<String>,
    pub sequence: NativeSequence,
}

#[derive(Clone, PartialEq)]
pub struct EventRow {
    pub id: EntityId,
    pub source_refs: Vec<SourceRef>,
    pub native_id: Option<String>,
    pub adapter: AdapterId,
    pub harness: HarnessId,
    pub availability: Vec<AvailabilityCode>,
    pub session_id: Option<EntityId>,
    pub branch_ids: Vec<EntityId>,
    pub turn_id: Option<EntityId>,
    pub call_id: Option<EntityId>,
    pub message_id: Option<EntityId>,
    pub kind: String,
    pub timestamp: Option<Timestamp>,
    pub parts: Vec<ObservationPart>,
    pub sequence: NativeSequence,
}

#[derive(Clone, PartialEq)]
pub(crate) struct UsageFact {
    pub session_id: Option<EntityId>,
    pub turn_id: Option<EntityId>,
    pub owner: Option<String>,
    pub scope: UsageScope,
    pub counters: UsageCounters,
    pub sequence: NativeSequence,
    pub source_id: SourceId,
}

#[derive(Clone, PartialEq)]
pub(crate) struct SavedRow {
    pub row: ProjectedRow,
}

#[derive(Clone, Copy)]
pub(crate) enum RowRef<'a> {
    Source(&'a SourceRow),
    Session(&'a SessionRow),
    Turn(&'a TurnRow),
    Message(&'a MessageRow),
    Tool(&'a ToolRow),
    Event(&'a EventRow),
    Saved(&'a SavedRow),
}

impl RowRef<'_> {
    pub(crate) fn dataset(self) -> Dataset {
        match self {
            Self::Source(_) => Dataset::Sources,
            Self::Session(_) => Dataset::Sessions,
            Self::Turn(_) => Dataset::Turns,
            Self::Message(_) => Dataset::Messages,
            Self::Tool(_) => Dataset::Tools,
            Self::Event(_) => Dataset::Events,
            Self::Saved(row) => row.row.dataset(),
        }
    }

    pub(crate) fn id(self) -> EntityId {
        match self {
            Self::Source(row) => row.id,
            Self::Session(row) => row.id,
            Self::Turn(row) => row.id,
            Self::Message(row) => row.id,
            Self::Tool(row) => row.id,
            Self::Event(row) => row.id,
            Self::Saved(row) => row.row.id(),
        }
    }

    pub(crate) fn source_refs(self) -> Result<Vec<ProjectedSourceRef>, QueryFailure> {
        if let Self::Saved(row) = self {
            return Ok(row.row.source_refs().to_vec());
        }
        let refs = match self {
            Self::Source(row) => &row.source_refs,
            Self::Session(row) => &row.source_refs,
            Self::Turn(row) => &row.source_refs,
            Self::Message(row) => &row.source_refs,
            Self::Tool(row) => &row.source_refs,
            Self::Event(row) => &row.source_refs,
            Self::Saved(_) => unreachable!(),
        };
        project_source_refs(refs)
    }

    pub(crate) fn field(self, field: FieldId) -> Option<FieldValue> {
        match self {
            Self::Source(row) => source_field(row, field),
            Self::Session(row) => session_field(row, field),
            Self::Turn(row) => turn_field(row, field),
            Self::Message(row) => message_field(row, field),
            Self::Tool(row) => tool_field(row, field),
            Self::Event(row) => event_field(row, field),
            Self::Saved(row) => row.row.field(field).cloned(),
        }
    }

    pub(crate) fn branch_ids(self) -> &[EntityId] {
        match self {
            Self::Session(row) => &row.branch_ids,
            Self::Turn(row) => &row.branch_ids,
            Self::Message(row) => &row.branch_ids,
            Self::Tool(row) => &row.branch_ids,
            Self::Event(row) => &row.branch_ids,
            Self::Source(_) | Self::Saved(_) => &[],
        }
    }

    pub(crate) fn session_id(self) -> Option<EntityId> {
        match self {
            Self::Session(row) => Some(row.id),
            Self::Turn(row) => Some(row.session_id),
            Self::Message(row) => Some(row.session_id),
            Self::Tool(row) => Some(row.session_id),
            Self::Event(row) => row.session_id,
            Self::Source(_) => None,
            Self::Saved(row) => row.row.field(FieldId::SessionId).and_then(|value| match value {
                FieldValue::Id(id) => Some(*id),
                _ => None,
            }),
        }
    }

    pub(crate) fn native_order(self) -> Option<&NativeSequence> {
        match self {
            Self::Turn(row) => Some(&row.sequence),
            Self::Message(row) => Some(&row.sequence),
            Self::Tool(row) => Some(&row.sequence),
            Self::Event(row) => Some(&row.sequence),
            _ => None,
        }
    }

    pub(crate) fn project(
        self,
        columns: &[FieldId],
        access: &ContentAccess,
        is_context: Option<bool>,
    ) -> Result<ProjectedRow, QueryFailure> {
        let mut fields = columns
            .iter()
            .filter_map(|field| self.field(*field).map(|value| (*field, value)))
            .collect::<BTreeMap<_, _>>();
        if matches!(self.dataset(), Dataset::Turns | Dataset::Messages) {
            if let Some(is_context) = is_context {
                fields.insert(FieldId::IsContext, FieldValue::Bool(is_context));
            }
        }
        ProjectedRow::new(self.dataset(), self.id(), self.source_refs()?, fields, access)
    }
}

fn project_source_refs(refs: &[SourceRef]) -> Result<Vec<ProjectedSourceRef>, QueryFailure> {
    let mut seen = BTreeSet::new();
    let mut projected = Vec::new();
    for reference in refs {
        let (kind, offset) = match &reference.locator {
            NativeLocator::Jsonl { offset } => (ProjectedLocatorKind::Jsonl, Some(*offset)),
            NativeLocator::Snapshot { .. } => (ProjectedLocatorKind::Snapshot, None),
            NativeLocator::GitNote { .. } => (ProjectedLocatorKind::GitNote, None),
        };
        let key = (reference.source_id, reference.revision.clone(), kind, offset);
        if seen.insert(key.clone()) {
            projected.push(ProjectedSourceRef::new(key.0, key.1, key.2, key.3)?);
        }
    }
    Ok(projected)
}

fn availability(values: &[AvailabilityCode]) -> FieldValue {
    let mut strings = values.iter().map(ToString::to_string).collect::<Vec<_>>();
    strings.sort();
    strings.dedup();
    FieldValue::Strings(strings)
}

fn associations(values: &[AssociationObservation]) -> FieldValue {
    FieldValue::Structured(serde_json::Value::Array(
        values
            .iter()
            .map(|association| {
                serde_json::json!({
                    "basis": association.basis.as_str(),
                    "path": association.path.as_ref().map(|path| path.to_string_lossy()),
                    "partition": association.partition.to_string(),
                })
            })
            .collect(),
    ))
}

fn parts(values: &[ObservationPart]) -> FieldValue {
    FieldValue::Structured(serde_json::Value::Array(
        values
            .iter()
            .map(|part| match part {
                ObservationPart::Text(value) => serde_json::json!({"kind":"text","value":value}),
                ObservationPart::Reasoning(value) => {
                    serde_json::json!({"kind":"reasoning","value":value})
                }
                ObservationPart::Structured(value) => {
                    serde_json::json!({"kind":"structured","value":value})
                }
                ObservationPart::Unavailable(reason) => {
                    serde_json::json!({"kind":"unavailable","reason":reason.as_str()})
                }
            })
            .collect(),
    ))
}

fn source_field(row: &SourceRow, field: FieldId) -> Option<FieldValue> {
    match field {
        FieldId::Harness => Some(FieldValue::String(row.harness.to_string())),
        FieldId::Adapter => Some(FieldValue::String(row.adapter.to_string())),
        FieldId::Availability => Some(availability(&row.availability)),
        FieldId::Format => Some(FieldValue::String(row.representation.clone())),
        FieldId::ReadStatus => Some(FieldValue::String(row.read_status.to_string())),
        FieldId::Association => Some(associations(&row.associations)),
        FieldId::Revision => Some(FieldValue::String(row.revision.clone())),
        FieldId::ProjectPath => row
            .associations
            .iter()
            .find_map(|association| association.path.as_ref())
            .map(|path| FieldValue::String(path.to_string_lossy().into_owned())),
        FieldId::SourcePath => match &row.locator {
            SourceLocator::LocalPath(path) => {
                Some(FieldValue::String(path.to_string_lossy().into_owned()))
            }
            SourceLocator::Provided(value) => Some(FieldValue::String(value.clone())),
        },
        _ => None,
    }
}

fn session_field(row: &SessionRow, field: FieldId) -> Option<FieldValue> {
    match field {
        FieldId::NativeId => Some(FieldValue::String(row.native_id.clone())),
        FieldId::Harness => Some(FieldValue::String(row.harness.to_string())),
        FieldId::Adapter => Some(FieldValue::String(row.adapter.to_string())),
        FieldId::Availability => Some(availability(&row.availability)),
        FieldId::Name => row.name.clone().map(FieldValue::String),
        FieldId::Models => Some(FieldValue::Strings(row.models.clone())),
        FieldId::StartedAt => row.started_at.clone().map(FieldValue::Timestamp),
        FieldId::FirstEventAt => row.first_event_at.clone().map(FieldValue::Timestamp),
        FieldId::SourceIds => Some(FieldValue::IdList(row.source_ids.clone())),
        FieldId::ParentIds => Some(FieldValue::IdList(row.parent_ids.clone())),
        FieldId::BranchIds => Some(FieldValue::IdList(row.branch_ids.clone())),
        FieldId::TurnCount => row.turn_count.map(FieldValue::Unsigned),
        FieldId::MessageCount => row.message_count.map(FieldValue::Unsigned),
        FieldId::ToolCallCount => row.tool_call_count.map(FieldValue::Unsigned),
        FieldId::TranscriptAvailable => Some(FieldValue::Bool(row.transcript_available)),
        _ => None,
    }
}

fn turn_field(row: &TurnRow, field: FieldId) -> Option<FieldValue> {
    match field {
        FieldId::NativeId => row.native_id.clone().map(FieldValue::String),
        FieldId::Harness => Some(FieldValue::String(row.harness.to_string())),
        FieldId::Adapter => Some(FieldValue::String(row.adapter.to_string())),
        FieldId::Availability => Some(availability(&row.availability)),
        FieldId::SessionId => Some(FieldValue::Id(row.session_id)),
        FieldId::BranchIds => Some(FieldValue::IdList(row.branch_ids.clone())),
        FieldId::Ordinal => Some(FieldValue::Unsigned(row.ordinal)),
        FieldId::StartedAt => row.started_at.clone().map(FieldValue::Timestamp),
        FieldId::MessageIds => Some(FieldValue::IdList(row.message_ids.clone())),
        FieldId::CallIds => Some(FieldValue::IdList(row.call_ids.clone())),
        FieldId::Roles => Some(FieldValue::Strings(
            row.roles.iter().map(ToString::to_string).collect(),
        )),
        FieldId::ToolNames => Some(FieldValue::Strings(row.tool_names.clone())),
        FieldId::ToolFamilies => Some(FieldValue::Strings(row.tool_families.clone())),
        FieldId::HasErrors => row.has_errors.map(FieldValue::Bool),
        FieldId::ToolCallCount => Some(FieldValue::Unsigned(row.call_ids.len() as u64)),
        FieldId::BoundaryBasis => Some(FieldValue::String(row.boundary_basis.clone())),
        _ => None,
    }
}

fn message_field(row: &MessageRow, field: FieldId) -> Option<FieldValue> {
    match field {
        FieldId::NativeId => row.native_id.clone().map(FieldValue::String),
        FieldId::Harness => Some(FieldValue::String(row.harness.to_string())),
        FieldId::Adapter => Some(FieldValue::String(row.adapter.to_string())),
        FieldId::Availability => Some(availability(&row.availability)),
        FieldId::SessionId => Some(FieldValue::Id(row.session_id)),
        FieldId::BranchIds => Some(FieldValue::IdList(row.branch_ids.clone())),
        FieldId::TurnId => Some(row.turn_id.map_or(FieldValue::Null, FieldValue::Id)),
        FieldId::Role => Some(FieldValue::String(row.role.to_string())),
        FieldId::Timestamp => row.timestamp.clone().map(FieldValue::Timestamp),
        FieldId::Text => {
            let text = row
                .parts
                .iter()
                .filter_map(|part| match part {
                    ObservationPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(FieldValue::String(text))
        }
        FieldId::Parts => Some(parts(&row.parts)),
        FieldId::Model => row.model.clone().map(FieldValue::String),
        _ => None,
    }
}

fn tool_field(row: &ToolRow, field: FieldId) -> Option<FieldValue> {
    match field {
        FieldId::NativeId => Some(FieldValue::String(row.native_id.clone())),
        FieldId::Harness => Some(FieldValue::String(row.harness.to_string())),
        FieldId::Adapter => Some(FieldValue::String(row.adapter.to_string())),
        FieldId::Availability => Some(availability(&row.availability)),
        FieldId::SessionId => Some(FieldValue::Id(row.session_id)),
        FieldId::BranchIds => Some(FieldValue::IdList(row.branch_ids.clone())),
        FieldId::TurnId => Some(row.turn_id.map_or(FieldValue::Null, FieldValue::Id)),
        FieldId::ToolName => row.tool_name.clone().map(FieldValue::String),
        FieldId::ToolFamily => row.tool_family.clone().map(FieldValue::String),
        FieldId::StartedAt => row.started_at.clone().map(FieldValue::Timestamp),
        FieldId::EndedAt => row.ended_at.clone().map(FieldValue::Timestamp),
        FieldId::DurationMs => row.duration_ms.map(FieldValue::Float),
        FieldId::DurationBasis => row
            .duration_basis
            .map(|basis| FieldValue::String(basis.to_string())),
        FieldId::Status => Some(FieldValue::String(row.status.to_string())),
        FieldId::StatusReason => row
            .status_reason
            .map(|reason| FieldValue::String(reason.to_string())),
        FieldId::ExitCode => row.exit_code.map(FieldValue::Integer),
        FieldId::Command => row.command.clone().map(FieldValue::String),
        FieldId::Input => Some(parts(&row.input)),
        FieldId::Output => Some(parts(&row.output)),
        FieldId::Model => row.model.clone().map(FieldValue::String),
        _ => None,
    }
}

fn event_field(row: &EventRow, field: FieldId) -> Option<FieldValue> {
    match field {
        FieldId::NativeId => row.native_id.clone().map(FieldValue::String),
        FieldId::Harness => Some(FieldValue::String(row.harness.to_string())),
        FieldId::Adapter => Some(FieldValue::String(row.adapter.to_string())),
        FieldId::Availability => Some(availability(&row.availability)),
        FieldId::SessionId => Some(row.session_id.map_or(FieldValue::Null, FieldValue::Id)),
        FieldId::BranchIds => Some(FieldValue::IdList(row.branch_ids.clone())),
        FieldId::TurnId => Some(row.turn_id.map_or(FieldValue::Null, FieldValue::Id)),
        FieldId::CallId => Some(row.call_id.map_or(FieldValue::Null, FieldValue::Id)),
        FieldId::MessageId => Some(row.message_id.map_or(FieldValue::Null, FieldValue::Id)),
        FieldId::Kind => Some(FieldValue::String(row.kind.clone())),
        FieldId::Timestamp => row.timestamp.clone().map(FieldValue::Timestamp),
        FieldId::Parts => Some(parts(&row.parts)),
        _ => None,
    }
}
