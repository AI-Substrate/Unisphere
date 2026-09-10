use super::{
    AdapterId, ContentAccess, EntityId, FieldId, HarnessId, PartitionId, QueryFailure, QueryInput,
    QueryLimits, QueryScope, SourceId, SourceSelection, Timestamp,
};
use crate::NativeSnapshot;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

query_enum! { pub enum SourceViewKind { Conversation => "conversation", MainSpine => "main_spine", LegacyChat => "legacy_chat", LegacyTimeline => "legacy_timeline", SourceOnly => "source_only" } }
query_enum! { pub enum MembershipPolicy { ExplicitRecordIds => "explicit_record_ids", ValidatedHeader => "validated_header", NativeContainment => "native_containment", Unavailable => "unavailable" } }
query_enum! { pub enum SourceReadStatus { Readable => "readable", Absent => "absent", Unreadable => "unreadable", Unsupported => "unsupported", Partial => "partial" } }
query_enum! { pub enum AssociationStatus { Matched => "matched", OutsideScope => "outside_scope", Unassociated => "unassociated", Conflict => "conflict" } }
query_enum! { pub enum AvailabilityCode { Absent => "absent", NotSupported => "not_supported", NotCaptured => "not_captured", Partial => "partial", Ambiguous => "ambiguous", Conflict => "conflict", InvalidClock => "invalid_clock", MissingCompletion => "missing_completion", SensitiveOmitted => "sensitive_omitted", ProjectionMissing => "projection_missing", InputSubset => "input_subset", Unassociated => "unassociated", Stale => "stale" } }
query_enum! { pub enum LineageKind { Parent => "parent", Fork => "fork", Subagent => "subagent", CompactionFrom => "compaction_from", FirstKept => "first_kept", NativeLink => "native_link" } }
query_enum! { pub enum AssociationBasis { NativeCwd => "native_cwd", NativeGitRoot => "native_git_root", VerifiedWorkspaceMetadata => "verified_workspace_metadata", GitRepositoryIdentity => "git_repository_identity", StoreHint => "store_hint" } }
query_enum! { pub enum RequestMarker { Initiating => "initiating", ToolResponse => "tool_response", Injected => "injected", Summary => "summary", Unknown => "unknown" } }
query_enum! { pub enum MessageRole { User => "user", Assistant => "assistant", System => "system", Tool => "tool", Developer => "developer", Unknown => "unknown" } }
query_enum! { pub enum Outcome { Succeeded => "succeeded", Failed => "failed", Cancelled => "cancelled", Incomplete => "incomplete", Unknown => "unknown" } }
query_enum! { pub enum DurationBasis { SourceReported => "source_reported", PairedClock => "paired_clock" } }
query_enum! { pub enum ControlKind { Branch => "branch", Compaction => "compaction", Summary => "summary", ContextChange => "context_change", Other => "other" } }
query_enum! { pub enum UsageScope { Invocation => "invocation", Turn => "turn", Session => "session", CumulativeSnapshot => "cumulative_snapshot" } }
query_enum! { pub enum IdentityKind { Human => "human", Agent => "agent", CommitAuthor => "commit_author", Committer => "committer" } }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailabilityIssue {
    pub code: AvailabilityCode,
    pub field: Option<FieldId>,
    pub source: Option<SourceId>,
    pub entity: Option<EntityId>,
    pub offset: Option<u64>,
}

/// Immutable source-read and admission facts. It deliberately contains no response universe.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    pub discovered_sources: u64,
    pub loaded_sources: u64,
    pub selected_sources: u64,
    pub source_status: BTreeMap<SourceReadStatus, u64>,
    pub association_status: BTreeMap<AssociationStatus, u64>,
    pub excluded_adapters: Vec<AdapterId>,
    pub source_read_complete: bool,
    pub issues: Vec<AvailabilityIssue>,
}
impl Coverage {
    pub fn validate(&self) -> Result<(), QueryFailure> {
        if self.loaded_sources > self.discovered_sources
            || self.selected_sources > self.loaded_sources
        {
            return Err(QueryFailure::invalid_data());
        }
        let statuses = self
            .source_status
            .values()
            .try_fold(0_u64, |sum, value| sum.checked_add(*value))
            .ok_or_else(QueryFailure::invalid_data)?;
        if statuses > self.discovered_sources {
            return Err(QueryFailure::invalid_data());
        }
        Ok(())
    }
}

/// Versioned source-local order. Labels are interpreted only by the owning adapter policy.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NativeSequence {
    pub version: u16,
    pub key: Vec<u8>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct BranchLink {
    pub kind: LineageKind,
    pub target: String,
}
#[derive(Clone, PartialEq, Eq)]
pub enum BranchEvidence {
    Linear {
        partition: PartitionId,
    },
    Node {
        partition: PartitionId,
        native_id: String,
        parent: Option<String>,
        declared_branch: Option<String>,
        links: Vec<BranchLink>,
    },
    Unavailable {
        partition: PartitionId,
        reason: AvailabilityCode,
    },
}
impl BranchEvidence {
    pub const fn partition(&self) -> PartitionId {
        match self {
            Self::Linear { partition }
            | Self::Node { partition, .. }
            | Self::Unavailable { partition, .. } => *partition,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum AssociationExtent {
    Partition,
    Record(NativeSequence),
    From {
        start: NativeSequence,
        until: Option<NativeSequence>,
    },
}
#[derive(Clone, PartialEq, Eq)]
pub struct AssociationObservation {
    pub basis: AssociationBasis,
    pub path: Option<PathBuf>,
    pub partition: PartitionId,
    pub applies_to: AssociationExtent,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SessionEvidenceKey {
    pub namespace: String,
    pub native_id: String,
    pub participant_id: Option<String>,
    pub parent_native_id: Option<String>,
    pub fork_native_id: Option<String>,
    pub membership_basis: MembershipPolicy,
}

#[derive(Clone, PartialEq, Eq)]
pub enum NativeLocator {
    Jsonl {
        offset: u64,
    },
    Snapshot {
        key: String,
    },
    GitNote {
        repository_id: String,
        notes_ref: String,
        notes_tip: String,
        target_commit: String,
        note_blob: String,
    },
}
impl NativeLocator {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Jsonl { .. } => "jsonl",
            Self::Snapshot { .. } => "snapshot",
            Self::GitNote { .. } => "git_note",
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub source_id: SourceId,
    pub revision: String,
    pub locator: NativeLocator,
    pub subrecord: String,
}

#[derive(Clone, PartialEq)]
pub enum ObservationPart {
    Text(String),
    Reasoning(String),
    Structured(serde_json::Value),
    Unavailable(AvailabilityCode),
}
#[derive(Clone, PartialEq, Eq)]
pub struct UsageCounters {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}
#[derive(Clone, PartialEq)]
pub enum ObservationFacet {
    SessionMetadata {
        native_id: String,
        name: Option<String>,
        models: Vec<String>,
        created_at: Option<Timestamp>,
        associations: Vec<AssociationObservation>,
        lineage: Vec<BranchLink>,
    },
    Message {
        native_id: Option<String>,
        role: MessageRole,
        parts: Vec<ObservationPart>,
        request_marker: RequestMarker,
        turn_id: Option<String>,
    },
    ToolCall {
        native_call_id: String,
        native_name: String,
        family: Option<String>,
        input: Vec<ObservationPart>,
        turn_id: Option<String>,
    },
    ToolResult {
        native_call_id: String,
        native_name: Option<String>,
        output: Vec<ObservationPart>,
        outcome: Outcome,
        exit_code: Option<i64>,
        reported_duration_ms: Option<u64>,
        turn_id: Option<String>,
    },
    ToolProgress {
        native_call_id: String,
        parts: Vec<ObservationPart>,
    },
    Control {
        kind: ControlKind,
        links: Vec<BranchLink>,
    },
    Usage {
        owner: Option<String>,
        scope: UsageScope,
        counters: UsageCounters,
    },
    Attribution {
        identity_kind: IdentityKind,
        native_key: String,
        declared_agent: Option<String>,
        target_commit: Option<String>,
        ranges: Vec<String>,
    },
}

/// Raw supplied evidence. Intentionally neither `Serialize` nor `Debug`.
#[derive(Clone, PartialEq)]
pub struct Observation {
    pub source_ref: SourceRef,
    pub native_record_id: Option<String>,
    pub session: Option<SessionEvidenceKey>,
    pub branch: BranchEvidence,
    pub parent_ids: Vec<String>,
    pub sequence: NativeSequence,
    pub timestamp: Option<Timestamp>,
    pub facets: Vec<ObservationFacet>,
    pub diagnostics: Vec<AvailabilityIssue>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SourcePartition {
    pub id: PartitionId,
    pub native_session_id: Option<String>,
    pub participant_id: Option<String>,
    pub view: SourceViewKind,
    pub membership: MembershipPolicy,
    pub associations: Vec<AssociationObservation>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum SourceLocator {
    LocalPath(PathBuf),
    Provided(String),
}
#[derive(Clone, PartialEq, Eq)]
pub struct SourceEvidence {
    pub id: SourceId,
    pub adapter: AdapterId,
    pub harness: HarnessId,
    pub representation: String,
    pub locator: SourceLocator,
    pub revision: String,
    pub query_policy_version: String,
    pub read_status: SourceReadStatus,
    pub associations: Vec<AssociationObservation>,
    pub available_fields: BTreeSet<FieldId>,
}
impl SourceEvidence {
    pub fn validate(&self) -> Result<(), QueryFailure> {
        if self.representation.is_empty()
            || self.revision.is_empty()
            || self.query_policy_version.is_empty()
        {
            return Err(QueryFailure::invalid_data());
        }
        if matches!(&self.locator, SourceLocator::LocalPath(path) if !path.is_absolute()) {
            return Err(QueryFailure::invalid_data());
        }
        Ok(())
    }
}

/// Exact C21 adapter output. Raw facts are not serializable projections.
#[derive(Clone, PartialEq)]
pub struct InspectedSource {
    pub source: SourceEvidence,
    pub partitions: Vec<SourcePartition>,
    pub observations: Vec<Observation>,
    pub issues: Vec<AvailabilityIssue>,
}
impl InspectedSource {
    pub fn validate(&self, limits: &QueryLimits) -> Result<(), QueryFailure> {
        self.source.validate()?;
        if self.observations.len() > limits.max_observations_and_rows {
            return Err(QueryFailure::limit(super::LimitKind::ObservationsAndRows));
        }
        if self.partitions.iter().any(|partition| {
            partition
                .associations
                .iter()
                .any(|association| association.partition != partition.id)
        }) || self.observations.iter().any(|observation| {
            observation.source_ref.source_id != self.source.id
                || observation.source_ref.revision != self.source.revision
        }) {
            return Err(QueryFailure::invalid_data());
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq)]
pub struct NativeQueryView {
    pub sources: Vec<SourceEvidence>,
    pub observations: Vec<Observation>,
    pub repository_roots: Vec<PathBuf>,
    pub coverage: Coverage,
}
impl NativeQueryView {
    pub fn validate(&self, limits: &QueryLimits) -> Result<(), QueryFailure> {
        limits.validate()?;
        self.coverage.validate()?;
        if self.sources.len() > limits.max_sources {
            return Err(QueryFailure::limit(super::LimitKind::Sources));
        }
        if self.observations.len() > limits.max_observations_and_rows {
            return Err(QueryFailure::limit(super::LimitKind::ObservationsAndRows));
        }
        if self.repository_roots.iter().any(|root| !root.is_absolute()) {
            return Err(QueryFailure::invalid_data());
        }
        let mut revisions = BTreeMap::new();
        for source in &self.sources {
            source.validate()?;
            if revisions
                .insert(source.id, source.revision.as_str())
                .is_some()
            {
                return Err(QueryFailure::invalid_data());
            }
        }
        if self.observations.iter().any(|observation| {
            revisions.get(&observation.source_ref.source_id).copied()
                != Some(observation.source_ref.revision.as_str())
        }) {
            return Err(QueryFailure::invalid_data());
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq)]
pub struct NativeRecord {
    pub locator: NativeLocator,
    pub bytes: Vec<u8>,
}
/// Borrowed pure adapter input; it carries no filesystem handle.
pub enum NativeQueryInput<'a> {
    Records {
        source: &'a SourceEvidence,
        records: &'a [NativeRecord],
    },
    Snapshot {
        source: &'a SourceEvidence,
        snapshot: &'a NativeSnapshot,
    },
    ProvidedObject {
        source: &'a SourceEvidence,
        bytes: &'a [u8],
        locator: &'a NativeLocator,
    },
}

pub trait QuerySource: Send + Sync {
    fn load(
        &self,
        scope: &QueryScope,
        selection: &SourceSelection,
        limits: &QueryLimits,
        access: ContentAccess,
    ) -> Result<QueryInput, QueryFailure>;
}
pub trait QueryAdapter: Send + Sync {
    fn inspect(
        &self,
        input: NativeQueryInput<'_>,
        access: ContentAccess,
        limits: &QueryLimits,
    ) -> Result<InspectedSource, QueryFailure>;
}
