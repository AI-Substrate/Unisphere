use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use unisphere_core::query::{
    AdapterId, AssociationBasis, AssociationExtent, AssociationObservation, AvailabilityCode,
    BranchEvidence, BranchLink, Completeness, ContentAccess, ControlKind, Dataset, Digest,
    DurationBasis, EntityId, EntityKind, FieldId, HarnessId, LineageKind, MembershipPolicy,
    MessageRole, NativeQueryView, NativeSequence, Observation, ObservationFacet, ObservationPart,
    Outcome, PartitionId, QueryFailure, QueryInput, QueryLimits, QueryScope, RepoScope,
    RetainedCapability, SourceEvidence, SourceId, SourceReadFacts, SourceReadStatus, SourceRef,
    SourceSelection, SourceSelector, Timestamp, ViewDigestBasis, ViewInputBasis, ViewSourceBinding,
};

use super::{
    rows::{EventRow, MessageRow, SavedRow, SessionRow, SourceRow, ToolRow, TurnRow, UsageFact},
    saved,
};

const VIEW_SCHEMA_VERSION: u16 = 1;
const RECONSTRUCTION_VERSION: u16 = 2;

/// Immutable, bounded logical reconstruction over one retained source view.
///
/// The raw typed rows are intentionally not serializable. Call [`crate::query::execute_view`]
/// to obtain privacy-checked [`unisphere_core::query::ProjectedRow`] values.
pub struct QueryView {
    digest: Digest,
    admitted_scope: QueryScope,
    source_selection: SourceSelection,
    retained: RetainedCapability,
    input_basis: ViewInputBasis,
    coverage: unisphere_core::query::Coverage,
    sources: Vec<SourceRow>,
    sessions: Vec<SessionRow>,
    turns: Vec<TurnRow>,
    messages: Vec<MessageRow>,
    tools: Vec<ToolRow>,
    events: Vec<EventRow>,
    usage: Vec<UsageFact>,
    saved_rows: BTreeMap<Dataset, Vec<SavedRow>>,
    available_fields: BTreeSet<FieldId>,
    saved_universe_basis: Option<unisphere_core::query::UniverseBasis>,
    saved_bounded_by_input: bool,
    saved_rows_complete: Completeness,
    saved_partitions_complete: Completeness,
}

impl QueryView {
    /// Validate supplied evidence and construct a standalone offline-capable view.
    /// Query services use [`Self::from_input_for`] to bind the exact admitted scope.
    pub fn from_input(input: QueryInput, limits: &QueryLimits) -> Result<Self, QueryFailure> {
        let scope = match &input {
            QueryInput::Native(native) if !native.repository_roots.is_empty() => {
                QueryScope::Repository {
                    path: native.repository_roots[0].clone(),
                    scope: RepoScope::Tree,
                }
            }
            QueryInput::Native(native) if native.sources.len() == 1 => QueryScope::Source {
                selector: SourceSelector::Id(native.sources[0].id),
            },
            _ => QueryScope::Offline {
                input: unisphere_core::query::OfflineRef::Stdin,
            },
        };
        Self::from_input_for(
            input,
            scope,
            SourceSelection::default(),
            ContentAccess::default(),
            limits,
        )
    }

    /// Construct a view bound to an explicitly admitted scope and retained capability.
    pub fn from_input_for(
        input: QueryInput,
        admitted_scope: QueryScope,
        source_selection: SourceSelection,
        access: ContentAccess,
        limits: &QueryLimits,
    ) -> Result<Self, QueryFailure> {
        limits.validate()?;
        source_selection.validate()?;
        match input {
            QueryInput::Native(native) => {
                Self::from_native(native, admitted_scope, source_selection, access, limits)
            }
            saved_input @ QueryInput::Saved { .. } => {
                Self::from_saved(saved_input, admitted_scope, source_selection, limits)
            }
        }
    }

    pub const fn digest(&self) -> Digest {
        self.digest
    }

    pub fn admitted_scope(&self) -> &QueryScope {
        &self.admitted_scope
    }

    pub fn source_selection(&self) -> &SourceSelection {
        &self.source_selection
    }

    pub fn retained_capability(&self) -> &RetainedCapability {
        &self.retained
    }

    pub fn input_basis(&self) -> &ViewInputBasis {
        &self.input_basis
    }

    pub fn coverage(&self) -> &unisphere_core::query::Coverage {
        &self.coverage
    }

    pub fn sources(&self) -> &[SourceRow] {
        &self.sources
    }

    pub fn sessions(&self) -> &[SessionRow] {
        &self.sessions
    }

    pub fn turns(&self) -> &[TurnRow] {
        &self.turns
    }

    pub fn messages(&self) -> &[MessageRow] {
        &self.messages
    }

    pub fn tools(&self) -> &[ToolRow] {
        &self.tools
    }

    pub fn events(&self) -> &[EventRow] {
        &self.events
    }

    pub(crate) fn usage(&self) -> &[UsageFact] {
        &self.usage
    }

    pub(crate) fn saved_rows(&self, dataset: Dataset) -> Option<&[SavedRow]> {
        self.saved_rows.get(&dataset).map(Vec::as_slice)
    }
    pub(crate) fn available_fields(&self) -> &BTreeSet<FieldId> {
        &self.available_fields
    }

    pub(crate) fn saved_universe_basis(&self) -> Option<unisphere_core::query::UniverseBasis> {
        self.saved_universe_basis
    }

    pub(crate) const fn saved_bounded_by_input(&self) -> bool {
        self.saved_bounded_by_input
    }

    pub(crate) const fn saved_rows_complete(&self) -> Completeness {
        self.saved_rows_complete
    }

    pub(crate) const fn saved_partitions_complete(&self) -> Completeness {
        self.saved_partitions_complete
    }

    fn from_saved(
        input: QueryInput,
        admitted_scope: QueryScope,
        source_selection: SourceSelection,
        limits: &QueryLimits,
    ) -> Result<Self, QueryFailure> {
        let parsed = saved::parse(&input, limits)?;
        let sources = parsed
            .source_revisions
            .iter()
            .map(|(source_id, revision)| ViewSourceBinding {
                source_id: *source_id,
                revision: revision.clone(),
                representation: "saved-query-v1".to_owned(),
                query_policy_version: "saved-projection-v1".to_owned(),
            })
            .collect();
        let retained = RetainedCapability {
            access: parsed.retained_access,
            fields_by_source: parsed.fields_by_source,
        };
        let basis = ViewDigestBasis {
            view_schema_version: VIEW_SCHEMA_VERSION,
            reconstruction_version: RECONSTRUCTION_VERSION,
            admitted_scope: admitted_scope.clone(),
            admitted_repository_roots: Vec::new(),
            source_selection: source_selection.clone(),
            sources,
            selected_associations: Vec::new(),
            source_read_facts: SourceReadFacts::from_coverage(&parsed.coverage),
            retained: retained.clone(),
            input: parsed.input_basis.clone(),
        };
        let digest = basis.digest()?;
        Ok(Self {
            digest,
            admitted_scope,
            source_selection,
            retained,
            input_basis: parsed.input_basis,
            coverage: parsed.coverage,
            sources: Vec::new(),
            sessions: Vec::new(),
            turns: Vec::new(),
            messages: Vec::new(),
            tools: Vec::new(),
            events: Vec::new(),
            usage: Vec::new(),
            saved_rows: parsed.rows,
            available_fields: parsed.available_fields,
            saved_universe_basis: Some(parsed.universe_basis),
            saved_bounded_by_input: parsed.bounded_by_input,
            saved_rows_complete: parsed.rows_complete,
            saved_partitions_complete: parsed.partitions_complete,
        })
    }

    fn from_native(
        mut native: NativeQueryView,
        admitted_scope: QueryScope,
        source_selection: SourceSelection,
        access: ContentAccess,
        limits: &QueryLimits,
    ) -> Result<Self, QueryFailure> {
        native.validate(limits)?;
        if native
            .sources
            .iter()
            .any(|source| !source_selection.admits(&source.adapter, &source.harness))
        {
            return Err(QueryFailure::invalid_data());
        }
        match &admitted_scope {
            QueryScope::Source {
                selector: SourceSelector::Id(expected),
            } if native.sources.iter().any(|source| source.id != *expected) => {
                return Err(QueryFailure::invalid_data());
            }
            QueryScope::Source {
                selector: SourceSelector::Path { path, adapter },
            } if native.sources.iter().any(|source| {
                !matches!(&source.locator, unisphere_core::query::SourceLocator::LocalPath(actual) if actual == path)
                    || adapter.as_ref().is_some_and(|expected| expected != &source.adapter)
            }) => {
                return Err(QueryFailure::invalid_data());
            }
            _ => {}
        }
        native.sources.sort_by_key(|source| source.id);
        let source_map = native
            .sources
            .iter()
            .map(|source| (source.id, source))
            .collect::<BTreeMap<_, _>>();
        let mut retained_bytes = native
            .sources
            .iter()
            .try_fold(0usize, |sum, source| {
                sum.checked_add(source.representation.len())?
                    .checked_add(source.revision.len())?
                    .checked_add(source.query_policy_version.len())
            })
            .ok_or_else(|| QueryFailure::limit(unisphere_core::query::LimitKind::RetainedBytes))?;
        for observation in &native.observations {
            retained_bytes = retained_bytes
                .checked_add(observation_weight(observation))
                .ok_or_else(|| {
                    QueryFailure::limit(unisphere_core::query::LimitKind::RetainedBytes)
                })?;
            if retained_bytes > limits.max_retained_bytes {
                return Err(QueryFailure::limit(
                    unisphere_core::query::LimitKind::RetainedBytes,
                ));
            }
        }

        let mut source_rows = Vec::with_capacity(native.sources.len());
        for source in &native.sources {
            let mut availability = native
                .coverage
                .issues
                .iter()
                .filter(|issue| issue.source == Some(source.id))
                .map(|issue| issue.code)
                .collect::<Vec<_>>();
            if let Some(code) = read_status_code(source.read_status) {
                availability.push(code);
            }
            source_rows.push(SourceRow {
                id: source.id.entity(),
                source_id: source.id,
                source_refs: native
                    .observations
                    .iter()
                    .filter(|observation| observation.source_ref.source_id == source.id)
                    .map(|observation| observation.source_ref.clone())
                    .collect(),
                adapter: source.adapter.clone(),
                harness: source.harness.clone(),
                representation: source.representation.clone(),
                read_status: source.read_status,
                associations: source.associations.clone(),
                revision: source.revision.clone(),
                locator: source.locator.clone(),
                availability,
            });
        }

        let mut observations = native.observations.iter().collect::<Vec<_>>();
        observations.sort_by(|left, right| {
            (
                left.source_ref.source_id,
                left.branch.partition(),
                &left.sequence,
                left.source_ref.subrecord.as_bytes(),
            )
                .cmp(&(
                    right.source_ref.source_id,
                    right.branch.partition(),
                    &right.sequence,
                    right.source_ref.subrecord.as_bytes(),
                ))
        });

        let mut session_builders = BTreeMap::<EntityId, SessionBuilder>::new();
        let mut messages = Vec::<MessageRow>::new();
        let mut tools = Vec::<ToolRow>::new();
        let mut events = Vec::<EventRow>::new();
        let mut usage = Vec::<UsageFact>::new();
        let mut turns = BTreeMap::<EntityId, TurnBuilder>::new();
        let mut active_turn = BTreeMap::<(EntityId, Option<EntityId>), EntityId>::new();
        let mut pending_calls = BTreeMap::<CallKey, Vec<usize>>::new();
        let mut selected_associations = Vec::new();
        let branch_analysis = analyze_branches(&native.observations, limits)?;
        let mut session_keys = BTreeMap::<EntityId, SessionIdentity>::new();

        for observation in observations {
            let source = source_map
                .get(&observation.source_ref.source_id)
                .copied()
                .ok_or_else(QueryFailure::invalid_data)?;
            let admitted = observation_admitted(
                observation,
                source,
                &admitted_scope,
                &native.repository_roots,
            );
            if !admitted
                && !matches!(
                    admitted_scope,
                    QueryScope::Source { .. } | QueryScope::Offline { .. }
                )
            {
                continue;
            }
            let session_identity = observation_session_identity(observation, source);
            let session_id = session_identity.as_ref().map(session_id);
            let branch_ids = observation_branch_ids(observation, &branch_analysis.memberships);
            if let (Some(id), Some(identity)) = (session_id, session_identity.clone()) {
                session_keys.entry(id).or_insert(identity.clone());
                let builder = session_builders.entry(id).or_insert_with(|| {
                    SessionBuilder::new(
                        id,
                        identity,
                        source.adapter.clone(),
                        source.harness.clone(),
                    )
                });
                if let Some(issues) = observation_branch_key(observation)
                    .and_then(|key| branch_analysis.issues.get(&key))
                {
                    builder.availability.extend(issues.iter().copied());
                }
                builder.observe(observation, source, &access, &branch_ids);
            }
            let mut diagnostics = observation
                .diagnostics
                .iter()
                .map(|issue| issue.code)
                .collect::<Vec<_>>();
            if let Some(issues) =
                observation_branch_key(observation).and_then(|key| branch_analysis.issues.get(&key))
            {
                diagnostics.extend(issues.iter().copied());
            }
            if branch_ids.is_empty() {
                diagnostics.push(match &observation.branch {
                    BranchEvidence::Unavailable { reason, .. } => *reason,
                    _ => AvailabilityCode::Ambiguous,
                });
            }

            for (facet_index, facet) in observation.facets.iter().enumerate() {
                let mut message_id = None;
                let mut call_id = None;
                let mut facet_turn = None;
                let mut event_parts = Vec::new();
                let event_kind = match facet {
                    ObservationFacet::SessionMetadata {
                        created_at,
                        associations,
                        ..
                    } => {
                        selected_associations.extend(
                            associations
                                .iter()
                                .filter(|association| {
                                    association_selected(
                                        association,
                                        &admitted_scope,
                                        &native.repository_roots,
                                    )
                                })
                                .cloned(),
                        );
                        if let (Some(id), Some(created_at)) = (session_id, created_at)
                            && let Some(builder) = session_builders.get_mut(&id)
                        {
                            builder.started_at = merge_timestamp(
                                builder.started_at.take(),
                                Some(created_at.clone()),
                                &mut builder.availability,
                            );
                        }
                        "session_metadata"
                    }
                    ObservationFacet::Message {
                        native_id,
                        role,
                        parts,
                        request_marker,
                        turn_id: native_turn_id,
                    } => {
                        event_parts = retain_payload(parts, FieldId::Parts, &access);
                        if let Some(session_id) = session_id {
                            let id = message_id_for(
                                session_id,
                                observation,
                                native_id.as_deref(),
                                facet_index,
                            );
                            let turn_id = resolve_turn(
                                session_id,
                                &branch_ids,
                                native_turn_id.as_deref(),
                                *request_marker,
                                id,
                                observation,
                                &mut active_turn,
                            );
                            let row = MessageRow {
                                id,
                                source_refs: vec![observation.source_ref.clone()],
                                native_id: native_id.clone(),
                                adapter: source.adapter.clone(),
                                harness: source.harness.clone(),
                                availability: diagnostics.clone(),
                                session_id,
                                branch_ids: branch_ids.clone(),
                                turn_id,
                                role: *role,
                                timestamp: observation.timestamp.clone(),
                                parts: retain_message_parts(parts, &access),
                                model: None,
                                sequence: observation.sequence.clone(),
                            };
                            if let Some(turn_id) = turn_id {
                                turns
                                    .entry(turn_id)
                                    .or_insert_with(|| {
                                        TurnBuilder::new(
                                            turn_id,
                                            session_id,
                                            native_turn_id.clone(),
                                            source.adapter.clone(),
                                            source.harness.clone(),
                                            observation.sequence.clone(),
                                            if native_turn_id.is_some() {
                                                "native_turn_id"
                                            } else {
                                                "initiating_request_v1"
                                            },
                                        )
                                    })
                                    .add_message(&row);
                            }
                            messages.push(row);
                            message_id = Some(id);
                            facet_turn = turn_id;
                        }
                        "message"
                    }
                    ObservationFacet::ToolCall {
                        native_call_id,
                        native_name,
                        family,
                        input,
                        turn_id: native_turn_id,
                    } => {
                        event_parts = retain_payload(input, FieldId::Parts, &access);
                        if let Some(session_id) = session_id {
                            let turn_id = native_turn_id
                                .as_deref()
                                .map(|value| explicit_turn_id(session_id, value))
                                .or_else(|| active_turn_for(session_id, &branch_ids, &active_turn));
                            let occurrence = pending_calls
                                .get(&CallKey::new(session_id, &branch_ids, native_call_id))
                                .map_or(0, Vec::len);
                            let id =
                                tool_id_for(session_id, &branch_ids, native_call_id, occurrence);
                            let row = ToolRow {
                                id,
                                source_refs: vec![observation.source_ref.clone()],
                                native_id: native_call_id.clone(),
                                adapter: source.adapter.clone(),
                                harness: source.harness.clone(),
                                availability: diagnostics.clone(),
                                session_id,
                                branch_ids: branch_ids.clone(),
                                turn_id,
                                tool_name: access
                                    .permits_payload(FieldId::ToolName)
                                    .then(|| native_name.clone()),
                                tool_family: family.clone(),
                                started_at: observation.timestamp.clone(),
                                ended_at: None,
                                duration_ms: None,
                                duration_basis: None,
                                status: Outcome::Incomplete,
                                status_reason: Some(AvailabilityCode::MissingCompletion),
                                exit_code: None,
                                command: None,
                                input: retain_payload(input, FieldId::Input, &access),
                                output: Vec::new(),
                                progress: Vec::new(),
                                model: None,
                                sequence: observation.sequence.clone(),
                            };
                            let index = tools.len();
                            tools.push(row);
                            pending_calls
                                .entry(CallKey::new(session_id, &branch_ids, native_call_id))
                                .or_default()
                                .push(index);
                            if let Some(turn_id) = turn_id {
                                turns
                                    .entry(turn_id)
                                    .or_insert_with(|| {
                                        TurnBuilder::new(
                                            turn_id,
                                            session_id,
                                            native_turn_id.clone(),
                                            source.adapter.clone(),
                                            source.harness.clone(),
                                            observation.sequence.clone(),
                                            "native_turn_id",
                                        )
                                    })
                                    .add_tool(&tools[index]);
                            }
                            call_id = Some(id);
                            facet_turn = turn_id;
                        }
                        "tool_call"
                    }
                    ObservationFacet::ToolResult {
                        native_call_id,
                        native_name,
                        output,
                        outcome,
                        exit_code,
                        reported_duration_ms,
                        turn_id: native_turn_id,
                    } => {
                        event_parts = retain_payload(output, FieldId::Parts, &access);
                        if let Some(session_id) = session_id {
                            let key = CallKey::new(session_id, &branch_ids, native_call_id);
                            let all_candidates = pending_calls
                                .get(&key)
                                .into_iter()
                                .flatten()
                                .copied()
                                .collect::<Vec<_>>();
                            let candidates = all_candidates
                                .iter()
                                .copied()
                                .filter(|index| tools[*index].ended_at.is_none())
                                .collect::<Vec<_>>();
                            if candidates.len() == 1 {
                                let index = candidates[0];
                                complete_tool(
                                    &mut tools[index],
                                    observation,
                                    &access
                                        .permits_payload(FieldId::ToolName)
                                        .then(|| native_name.clone())
                                        .flatten(),
                                    &retain_payload(output, FieldId::Output, &access),
                                    *outcome,
                                    *exit_code,
                                    *reported_duration_ms,
                                );
                                call_id = Some(tools[index].id);
                                facet_turn = tools[index].turn_id;
                            } else {
                                for index in &all_candidates {
                                    tools[*index].status = Outcome::Unknown;
                                    tools[*index].status_reason = Some(AvailabilityCode::Ambiguous);
                                    tools[*index].availability.push(AvailabilityCode::Ambiguous);
                                }
                                let id = tool_id_for(
                                    session_id,
                                    &branch_ids,
                                    native_call_id,
                                    all_candidates.len().saturating_add(1),
                                );
                                let turn_id = native_turn_id
                                    .as_deref()
                                    .map(|value| explicit_turn_id(session_id, value));
                                tools.push(ToolRow {
                                    id,
                                    source_refs: vec![observation.source_ref.clone()],
                                    native_id: native_call_id.clone(),
                                    adapter: source.adapter.clone(),
                                    harness: source.harness.clone(),
                                    availability: vec![AvailabilityCode::Ambiguous],
                                    session_id,
                                    branch_ids: branch_ids.clone(),
                                    turn_id,
                                    tool_name: access
                                        .permits_payload(FieldId::ToolName)
                                        .then(|| native_name.clone())
                                        .flatten(),
                                    tool_family: None,
                                    started_at: None,
                                    ended_at: observation.timestamp.clone(),
                                    duration_ms: reported_duration_ms.map(|value| value as f64),
                                    duration_basis: reported_duration_ms
                                        .map(|_| DurationBasis::SourceReported),
                                    status: *outcome,
                                    status_reason: Some(AvailabilityCode::Ambiguous),
                                    exit_code: *exit_code,
                                    command: None,
                                    input: Vec::new(),
                                    output: retain_payload(output, FieldId::Output, &access),
                                    progress: Vec::new(),
                                    model: None,
                                    sequence: observation.sequence.clone(),
                                });
                                call_id = Some(id);
                                facet_turn = turn_id;
                            }
                        }
                        "tool_result"
                    }
                    ObservationFacet::ToolProgress {
                        native_call_id,
                        parts,
                    } => {
                        event_parts = retain_payload(parts, FieldId::Parts, &access);
                        if let Some(session_id) = session_id {
                            let key = CallKey::new(session_id, &branch_ids, native_call_id);
                            let candidates = pending_calls
                                .get(&key)
                                .into_iter()
                                .flatten()
                                .copied()
                                .filter(|index| tools[*index].ended_at.is_none())
                                .collect::<Vec<_>>();
                            if candidates.len() == 1 {
                                tools[candidates[0]].progress.extend(retain_payload(
                                    parts,
                                    FieldId::Output,
                                    &access,
                                ));
                                call_id = Some(tools[candidates[0]].id);
                                facet_turn = tools[candidates[0]].turn_id;
                            }
                        }
                        "tool_progress"
                    }
                    ObservationFacet::Control { kind, .. } => match kind {
                        ControlKind::Branch => "branch",
                        ControlKind::Compaction => "compaction",
                        ControlKind::Summary => "summary",
                        ControlKind::ContextChange => "context_change",
                        ControlKind::Other => "control",
                    },
                    ObservationFacet::Usage {
                        owner,
                        scope,
                        counters,
                    } => {
                        usage.push(UsageFact {
                            session_id,
                            turn_id: owner
                                .as_deref()
                                .zip(session_id)
                                .map(|(owner, session)| explicit_turn_id(session, owner)),
                            owner: owner.clone(),
                            scope: *scope,
                            counters: counters.clone(),
                            sequence: observation.sequence.clone(),
                            source_id: observation.source_ref.source_id,
                        });
                        "usage"
                    }
                    ObservationFacet::Attribution { .. } => "attribution",
                };
                let event_id = event_id_for(observation, facet_index);
                events.push(EventRow {
                    id: event_id,
                    source_refs: vec![observation.source_ref.clone()],
                    native_id: observation.native_record_id.clone(),
                    adapter: source.adapter.clone(),
                    harness: source.harness.clone(),
                    availability: diagnostics.clone(),
                    session_id,
                    branch_ids: branch_ids.clone(),
                    turn_id: facet_turn,
                    call_id,
                    message_id,
                    kind: event_kind.to_owned(),
                    timestamp: observation.timestamp.clone(),
                    parts: event_parts,
                    sequence: observation.sequence.clone(),
                });
            }
        }
        selected_associations.extend(native.sources.iter().flat_map(|source| {
            source
                .associations
                .iter()
                .filter(|association| {
                    association_selected(association, &admitted_scope, &native.repository_roots)
                })
                .cloned()
        }));
        for tool in &tools {
            let Some(turn_id) = tool.turn_id else {
                continue;
            };
            turns
                .entry(turn_id)
                .or_insert_with(|| {
                    TurnBuilder::new(
                        turn_id,
                        tool.session_id,
                        None,
                        tool.adapter.clone(),
                        tool.harness.clone(),
                        tool.sequence.clone(),
                        "native_turn_id",
                    )
                })
                .finalize_tool(tool);
        }

        let mut turns = turns
            .into_values()
            .map(TurnBuilder::finish)
            .collect::<Vec<_>>();
        turns.sort_by(|left, right| {
            (left.session_id, &left.sequence, left.id).cmp(&(
                right.session_id,
                &right.sequence,
                right.id,
            ))
        });
        let mut ordinal_by_branch = BTreeMap::<(EntityId, Option<EntityId>), u64>::new();
        for turn in &mut turns {
            let mut ordinal = None;
            let mut inconsistent = false;
            if turn.branch_ids.is_empty() {
                let value = ordinal_by_branch
                    .entry((turn.session_id, None))
                    .or_default();
                *value = value
                    .checked_add(1)
                    .ok_or_else(QueryFailure::invalid_data)?;
                ordinal = Some(*value);
            } else {
                for branch_id in &turn.branch_ids {
                    let value = ordinal_by_branch
                        .entry((turn.session_id, Some(*branch_id)))
                        .or_default();
                    *value = value
                        .checked_add(1)
                        .ok_or_else(QueryFailure::invalid_data)?;
                    if ordinal.is_some_and(|ordinal| ordinal != *value) {
                        inconsistent = true;
                    }
                    ordinal = Some(ordinal.map_or(*value, |ordinal| ordinal.min(*value)));
                }
            }
            turn.ordinal = ordinal.ok_or_else(QueryFailure::invalid_data)?;
            if inconsistent && !turn.availability.contains(&AvailabilityCode::Ambiguous) {
                turn.availability.push(AvailabilityCode::Ambiguous);
            }
        }

        resolve_session_lineage(&mut session_builders, &session_keys);
        let mut sessions = session_builders
            .into_values()
            .map(|builder| builder.finish(&turns, &messages, &tools))
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.id);

        let total_rows = source_rows
            .len()
            .checked_add(sessions.len())
            .and_then(|value| value.checked_add(turns.len()))
            .and_then(|value| value.checked_add(messages.len()))
            .and_then(|value| value.checked_add(tools.len()))
            .and_then(|value| value.checked_add(events.len()))
            .ok_or_else(|| {
                QueryFailure::limit(unisphere_core::query::LimitKind::ObservationsAndRows)
            })?;
        if total_rows > limits.max_observations_and_rows {
            return Err(QueryFailure::limit(
                unisphere_core::query::LimitKind::ObservationsAndRows,
            ));
        }
        let branch_memberships = sessions
            .iter()
            .map(|row| row.branch_ids.len())
            .chain(turns.iter().map(|row| row.branch_ids.len()))
            .chain(messages.iter().map(|row| row.branch_ids.len()))
            .chain(tools.iter().map(|row| row.branch_ids.len()))
            .chain(events.iter().map(|row| row.branch_ids.len()))
            .try_fold(0usize, usize::checked_add)
            .ok_or_else(|| {
                QueryFailure::limit(unisphere_core::query::LimitKind::BranchMemberships)
            })?;
        if branch_memberships > limits.max_branch_memberships {
            return Err(QueryFailure::limit(
                unisphere_core::query::LimitKind::BranchMemberships,
            ));
        }

        let retained = native_retained(&native.sources, access);
        let available_fields = retained
            .fields_by_source
            .values()
            .flat_map(|fields| fields.iter().copied())
            .collect();
        let input_basis = ViewInputBasis::LiveNative;
        let digest_basis = ViewDigestBasis {
            view_schema_version: VIEW_SCHEMA_VERSION,
            reconstruction_version: RECONSTRUCTION_VERSION,
            admitted_scope: admitted_scope.clone(),
            admitted_repository_roots: native.repository_roots.clone(),
            source_selection: source_selection.clone(),
            sources: native
                .sources
                .iter()
                .map(|source| ViewSourceBinding {
                    source_id: source.id,
                    revision: source.revision.clone(),
                    representation: source.representation.clone(),
                    query_policy_version: source.query_policy_version.clone(),
                })
                .collect(),
            selected_associations,
            source_read_facts: SourceReadFacts::from_coverage(&native.coverage),
            retained: retained.clone(),
            input: input_basis.clone(),
        };
        let digest = digest_basis.digest()?;
        Ok(Self {
            digest,
            admitted_scope,
            source_selection,
            retained,
            input_basis,
            coverage: native.coverage,
            sources: source_rows,
            sessions,
            turns,
            messages,
            tools,
            events,
            usage,
            saved_rows: BTreeMap::new(),
            available_fields,
            saved_universe_basis: None,
            saved_bounded_by_input: false,
            saved_rows_complete: Completeness::Complete,
            saved_partitions_complete: Completeness::Complete,
        })
    }
}

#[derive(Clone)]
struct SessionIdentity {
    source_id: SourceId,
    namespace: String,
    native_id: String,
    participant_id: Option<String>,
    parent_native_id: Option<String>,
    fork_native_id: Option<String>,
    membership: MembershipPolicy,
}

struct SessionBuilder {
    id: EntityId,
    identity: SessionIdentity,
    adapter: AdapterId,
    harness: HarnessId,
    source_refs: Vec<SourceRef>,
    source_ids: BTreeSet<EntityId>,
    branch_ids: BTreeSet<EntityId>,
    availability: Vec<AvailabilityCode>,
    name: Option<String>,
    models: BTreeSet<String>,
    started_at: Option<Timestamp>,
    first_event_at: Option<Timestamp>,
    parent_ids: BTreeSet<EntityId>,
    lineage: Vec<BranchLink>,
}

impl SessionBuilder {
    fn new(
        id: EntityId,
        identity: SessionIdentity,
        adapter: AdapterId,
        harness: HarnessId,
    ) -> Self {
        Self {
            id,
            identity,
            adapter,
            harness,
            source_refs: Vec::new(),
            source_ids: BTreeSet::new(),
            branch_ids: BTreeSet::new(),
            availability: Vec::new(),
            name: None,
            models: BTreeSet::new(),
            started_at: None,
            first_event_at: None,
            parent_ids: BTreeSet::new(),
            lineage: Vec::new(),
        }
    }

    fn observe(
        &mut self,
        observation: &Observation,
        source: &SourceEvidence,
        access: &ContentAccess,
        branch_ids: &[EntityId],
    ) {
        push_source_ref(&mut self.source_refs, &observation.source_ref);
        self.source_ids.insert(source.id.entity());
        self.branch_ids.extend(branch_ids.iter().copied());
        if branch_ids.is_empty() {
            self.availability.push(AvailabilityCode::Ambiguous);
        }
        self.first_event_at = earliest(self.first_event_at.take(), observation.timestamp.clone());
        self.availability
            .extend(observation.diagnostics.iter().map(|issue| issue.code));
        for facet in &observation.facets {
            if let ObservationFacet::SessionMetadata {
                name,
                models,
                created_at,
                lineage,
                ..
            } = facet
            {
                let retained_name = access
                    .permits_payload(FieldId::Name)
                    .then(|| name.clone())
                    .flatten();
                if self.name.is_some()
                    && retained_name.is_some()
                    && self.name.as_ref() != retained_name.as_ref()
                {
                    self.availability.push(AvailabilityCode::Conflict);
                } else if self.name.is_none() {
                    self.name = retained_name;
                }
                if access.permits_payload(FieldId::Models) {
                    self.models.extend(models.iter().cloned());
                }
                self.started_at = merge_timestamp(
                    self.started_at.take(),
                    created_at.clone(),
                    &mut self.availability,
                );
                self.lineage.extend(lineage.iter().cloned());
            }
        }
    }

    fn finish(self, turns: &[TurnRow], messages: &[MessageRow], tools: &[ToolRow]) -> SessionRow {
        let turn_count = turns.iter().filter(|row| row.session_id == self.id).count() as u64;
        let message_count = messages
            .iter()
            .filter(|row| row.session_id == self.id)
            .count() as u64;
        let tool_call_count = tools.iter().filter(|row| row.session_id == self.id).count() as u64;
        SessionRow {
            id: self.id,
            source_refs: self.source_refs,
            native_id: self.identity.native_id,
            namespace: self.identity.namespace,
            participant_id: self.identity.participant_id,
            adapter: self.adapter,
            harness: self.harness,
            availability: dedup_availability(self.availability),
            name: self.name,
            models: self.models.into_iter().collect(),
            started_at: self.started_at,
            first_event_at: self.first_event_at,
            source_ids: self.source_ids.into_iter().collect(),
            parent_ids: self.parent_ids.into_iter().collect(),
            branch_ids: self.branch_ids.into_iter().collect(),
            lineage: self.lineage,
            turn_count: Some(turn_count),
            message_count: Some(message_count),
            tool_call_count: Some(tool_call_count),
            transcript_available: message_count != 0,
        }
    }
}

struct TurnBuilder {
    id: EntityId,
    session_id: EntityId,
    native_id: Option<String>,
    adapter: AdapterId,
    harness: HarnessId,
    availability: Vec<AvailabilityCode>,
    source_refs: Vec<SourceRef>,
    branch_ids: BTreeSet<EntityId>,
    started_at: Option<Timestamp>,
    message_ids: Vec<EntityId>,
    call_ids: Vec<EntityId>,
    roles: BTreeSet<MessageRole>,
    tool_names: BTreeSet<String>,
    tool_families: BTreeSet<String>,
    has_errors: Option<bool>,
    sequence: NativeSequence,
    boundary_basis: String,
}

impl TurnBuilder {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: EntityId,
        session_id: EntityId,
        native_id: Option<String>,
        adapter: AdapterId,
        harness: HarnessId,
        sequence: NativeSequence,
        boundary_basis: &str,
    ) -> Self {
        Self {
            id,
            session_id,
            native_id,
            adapter,
            harness,
            availability: Vec::new(),
            source_refs: Vec::new(),
            branch_ids: BTreeSet::new(),
            started_at: None,
            message_ids: Vec::new(),
            call_ids: Vec::new(),
            roles: BTreeSet::new(),
            tool_names: BTreeSet::new(),
            tool_families: BTreeSet::new(),
            has_errors: None,
            sequence,
            boundary_basis: boundary_basis.to_owned(),
        }
    }

    fn add_message(&mut self, row: &MessageRow) {
        self.message_ids.push(row.id);
        self.roles.insert(row.role);
        self.branch_ids.extend(row.branch_ids.iter().copied());
        self.started_at = earliest(self.started_at.take(), row.timestamp.clone());
        for reference in &row.source_refs {
            push_source_ref(&mut self.source_refs, reference);
        }
    }

    fn add_tool(&mut self, row: &ToolRow) {
        self.call_ids.push(row.id);
        self.branch_ids.extend(row.branch_ids.iter().copied());
        if let Some(name) = &row.tool_name {
            self.tool_names.insert(name.clone());
        }
        if let Some(family) = &row.tool_family {
            self.tool_families.insert(family.clone());
        }
        self.started_at = earliest(self.started_at.take(), row.started_at.clone());
        self.has_errors = Some(self.has_errors.unwrap_or(false) || row.status == Outcome::Failed);
        for reference in &row.source_refs {
            push_source_ref(&mut self.source_refs, reference);
        }
    }
    fn finalize_tool(&mut self, row: &ToolRow) {
        if !self.call_ids.contains(&row.id) {
            self.add_tool(row);
            return;
        }
        self.has_errors = Some(self.has_errors.unwrap_or(false) || row.status == Outcome::Failed);
        self.availability.extend(row.availability.iter().copied());
        for reference in &row.source_refs {
            push_source_ref(&mut self.source_refs, reference);
        }
    }

    fn finish(self) -> TurnRow {
        TurnRow {
            id: self.id,
            source_refs: self.source_refs,
            native_id: self.native_id,
            adapter: self.adapter,
            harness: self.harness,
            availability: dedup_availability(self.availability),
            session_id: self.session_id,
            branch_ids: self.branch_ids.into_iter().collect(),
            ordinal: 0,
            started_at: self.started_at,
            message_ids: self.message_ids,
            call_ids: self.call_ids,
            roles: self.roles.into_iter().collect(),
            tool_names: self.tool_names.into_iter().collect(),
            tool_families: self.tool_families.into_iter().collect(),
            has_errors: self.has_errors,
            boundary_basis: self.boundary_basis,
            sequence: self.sequence,
        }
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CallKey {
    session: EntityId,
    branch_scope: Digest,
    native_id: String,
}

impl CallKey {
    fn new(session: EntityId, branch_ids: &[EntityId], native_id: &str) -> Self {
        Self {
            session,
            branch_scope: branch_scope_digest(branch_ids),
            native_id: native_id.to_owned(),
        }
    }
}

fn observation_session_identity(
    observation: &Observation,
    source: &SourceEvidence,
) -> Option<SessionIdentity> {
    if let Some(session) = &observation.session {
        return Some(SessionIdentity {
            source_id: source.id,
            namespace: session.namespace.clone(),
            native_id: session.native_id.clone(),
            participant_id: session.participant_id.clone(),
            parent_native_id: session.parent_native_id.clone(),
            fork_native_id: session.fork_native_id.clone(),
            membership: session.membership_basis,
        });
    }
    observation.facets.iter().find_map(|facet| {
        if let ObservationFacet::SessionMetadata { native_id, .. } = facet {
            Some(SessionIdentity {
                source_id: source.id,
                namespace: source.representation.clone(),
                native_id: native_id.clone(),
                participant_id: None,
                parent_native_id: None,
                fork_native_id: None,
                membership: MembershipPolicy::ValidatedHeader,
            })
        } else {
            None
        }
    })
}

fn session_id(identity: &SessionIdentity) -> EntityId {
    EntityId::derive(
        EntityKind::Session,
        [
            identity.source_id.digest().bytes().as_slice(),
            identity.namespace.as_bytes(),
            identity.native_id.as_bytes(),
            identity.participant_id.as_deref().unwrap_or("").as_bytes(),
            identity.membership.as_str().as_bytes(),
        ],
    )
}

fn message_id_for(
    session_id: EntityId,
    observation: &Observation,
    native_id: Option<&str>,
    facet_index: usize,
) -> EntityId {
    if let Some(native_id) = native_id {
        return EntityId::derive(
            EntityKind::Message,
            [session_id.digest().bytes().as_slice(), native_id.as_bytes()],
        );
    }
    let fallback = source_ref_identity(&observation.source_ref, facet_index);
    EntityId::derive(
        EntityKind::Message,
        [session_id.digest().bytes().as_slice(), fallback.as_bytes()],
    )
}

fn explicit_turn_id(session_id: EntityId, native_id: &str) -> EntityId {
    EntityId::derive(
        EntityKind::Turn,
        [session_id.digest().bytes().as_slice(), native_id.as_bytes()],
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_turn(
    session_id: EntityId,
    branch_ids: &[EntityId],
    native_turn_id: Option<&str>,
    marker: unisphere_core::query::RequestMarker,
    message_id: EntityId,
    observation: &Observation,
    active_turn: &mut BTreeMap<(EntityId, Option<EntityId>), EntityId>,
) -> Option<EntityId> {
    if let Some(native_id) = native_turn_id {
        let id = explicit_turn_id(session_id, native_id);
        set_active_turn(session_id, branch_ids, id, active_turn);
        return Some(id);
    }
    if marker == unisphere_core::query::RequestMarker::Initiating {
        let id = EntityId::derive(
            EntityKind::Turn,
            [
                session_id.digest().bytes().as_slice(),
                message_id.digest().bytes().as_slice(),
                observation.sequence.key.as_slice(),
            ],
        );
        set_active_turn(session_id, branch_ids, id, active_turn);
        return Some(id);
    }
    active_turn_for(session_id, branch_ids, active_turn)
}

fn set_active_turn(
    session_id: EntityId,
    branch_ids: &[EntityId],
    turn_id: EntityId,
    active_turn: &mut BTreeMap<(EntityId, Option<EntityId>), EntityId>,
) {
    if branch_ids.is_empty() {
        active_turn.insert((session_id, None), turn_id);
    } else {
        for branch_id in branch_ids {
            active_turn.insert((session_id, Some(*branch_id)), turn_id);
        }
    }
}

fn active_turn_for(
    session_id: EntityId,
    branch_ids: &[EntityId],
    active_turn: &BTreeMap<(EntityId, Option<EntityId>), EntityId>,
) -> Option<EntityId> {
    if branch_ids.is_empty() {
        return active_turn.get(&(session_id, None)).copied();
    }
    let mut resolved = None;
    for branch_id in branch_ids {
        let candidate = active_turn.get(&(session_id, Some(*branch_id))).copied()?;
        if resolved.is_some_and(|resolved| resolved != candidate) {
            return None;
        }
        resolved = Some(candidate);
    }
    resolved
}

fn branch_scope_digest(branch_ids: &[EntityId]) -> Digest {
    match branch_ids {
        [] => Digest::of_bytes(b"unavailable-branch"),
        [branch_id] => branch_id.digest(),
        branch_ids => {
            let digests = branch_ids
                .iter()
                .map(|branch_id| branch_id.digest())
                .collect::<Vec<_>>();
            Digest::framed(
                b"unisphere/query-branch-scope/v1",
                digests.iter().map(|digest| digest.bytes().as_slice()),
            )
        }
    }
}

fn tool_id_for(
    session_id: EntityId,
    branch_ids: &[EntityId],
    native_id: &str,
    occurrence: usize,
) -> EntityId {
    let occurrence = occurrence.to_le_bytes();
    let branch_scope = branch_scope_digest(branch_ids);
    EntityId::derive(
        EntityKind::Tool,
        [
            session_id.digest().bytes().as_slice(),
            branch_scope.bytes().as_slice(),
            native_id.as_bytes(),
            occurrence.as_slice(),
        ],
    )
}

fn event_id_for(observation: &Observation, facet_index: usize) -> EntityId {
    let fallback = source_ref_identity(&observation.source_ref, facet_index);
    EntityId::derive(
        EntityKind::Event,
        [
            observation.source_ref.source_id.digest().bytes().as_slice(),
            fallback.as_bytes(),
        ],
    )
}

fn source_ref_identity(reference: &SourceRef, facet_index: usize) -> String {
    let locator = match &reference.locator {
        unisphere_core::query::NativeLocator::Jsonl { offset } => format!("jsonl:{offset}"),
        unisphere_core::query::NativeLocator::Snapshot { key } => {
            format!("snapshot:{}:{key}", reference.revision)
        }
        unisphere_core::query::NativeLocator::GitNote {
            repository_id,
            notes_ref,
            notes_tip,
            target_commit,
            note_blob,
        } => {
            format!("git-note:{repository_id}:{notes_ref}:{notes_tip}:{target_commit}:{note_blob}")
        }
    };
    format!("{locator}:{}:{facet_index}", reference.subrecord)
}

fn complete_tool(
    row: &mut ToolRow,
    observation: &Observation,
    native_name: &Option<String>,
    output: &[ObservationPart],
    outcome: Outcome,
    exit_code: Option<i64>,
    reported_duration_ms: Option<u64>,
) {
    push_source_ref(&mut row.source_refs, &observation.source_ref);
    if row.tool_name.is_none() {
        row.tool_name.clone_from(native_name);
    }
    row.output.extend(output.iter().cloned());
    row.ended_at.clone_from(&observation.timestamp);
    row.status = outcome;
    row.exit_code = exit_code;
    row.status_reason = None;
    if let Some(duration) = reported_duration_ms {
        row.duration_ms = Some(duration as f64);
        row.duration_basis = Some(DurationBasis::SourceReported);
    } else if let (Some(start), Some(end)) = (&row.started_at, &row.ended_at) {
        let nanos = end.unix_nanos().checked_sub(start.unix_nanos());
        if let Some(nanos) = nanos.filter(|value| *value >= 0) {
            let duration = nanos as f64 / 1_000_000.0;
            if duration.is_finite() {
                row.duration_ms = Some(duration);
                row.duration_basis = Some(DurationBasis::PairedClock);
            } else {
                row.status_reason = Some(AvailabilityCode::InvalidClock);
                row.availability.push(AvailabilityCode::InvalidClock);
            }
        } else {
            row.status_reason = Some(AvailabilityCode::InvalidClock);
            row.availability.push(AvailabilityCode::InvalidClock);
        }
    } else {
        row.status_reason = Some(AvailabilityCode::MissingCompletion);
        row.availability.push(AvailabilityCode::MissingCompletion);
    }
}

fn resolve_session_lineage(
    sessions: &mut BTreeMap<EntityId, SessionBuilder>,
    identities: &BTreeMap<EntityId, SessionIdentity>,
) {
    let mut lookup = BTreeMap::new();
    for (id, identity) in identities {
        lookup.insert(
            (
                identity.source_id,
                identity.namespace.clone(),
                identity.native_id.clone(),
            ),
            *id,
        );
    }
    for (id, builder) in sessions.iter_mut() {
        for (target, kind) in [
            (&builder.identity.parent_native_id, LineageKind::Parent),
            (&builder.identity.fork_native_id, LineageKind::Fork),
        ] {
            let Some(target) = target else { continue };
            if let Some(parent) = lookup.get(&(
                builder.identity.source_id,
                builder.identity.namespace.clone(),
                target.clone(),
            )) {
                if parent == id {
                    builder.availability.push(AvailabilityCode::Conflict);
                } else {
                    builder.parent_ids.insert(*parent);
                    builder.lineage.push(BranchLink {
                        kind,
                        target: parent.to_string(),
                    });
                }
            } else {
                builder.availability.push(AvailabilityCode::NotCaptured);
            }
        }
    }
}

fn native_retained(sources: &[SourceEvidence], access: ContentAccess) -> RetainedCapability {
    let fields_by_source = sources
        .iter()
        .map(|source| {
            let mut fields = source.available_fields.clone();
            fields.insert(FieldId::Id);
            fields.insert(FieldId::SourceRefs);
            fields.retain(|field| !sensitive(*field) || access.permits_payload(*field));
            fields.extend(access.inspect_fields.iter().copied());
            (source.id, fields)
        })
        .collect();
    RetainedCapability {
        access,
        fields_by_source,
    }
}

fn sensitive(field: FieldId) -> bool {
    Dataset::ALL.iter().copied().any(|dataset| {
        unisphere_core::query::schema(dataset)
            .field(field)
            .is_some_and(|schema| {
                schema.sensitivity == unisphere_core::query::Sensitivity::Sensitive
            })
    })
}

fn observation_admitted(
    observation: &Observation,
    source: &SourceEvidence,
    scope: &QueryScope,
    repository_roots: &[PathBuf],
) -> bool {
    if !matches!(scope, QueryScope::Repository { .. }) {
        return true;
    }
    source
        .associations
        .iter()
        .chain(observation.facets.iter().flat_map(|facet| match facet {
            ObservationFacet::SessionMetadata { associations, .. } => associations.as_slice(),
            _ => &[],
        }))
        .any(|association| {
            association_selected(association, scope, repository_roots)
                && association_applies(association, &observation.sequence)
        })
}

fn association_selected(
    association: &AssociationObservation,
    scope: &QueryScope,
    repository_roots: &[PathBuf],
) -> bool {
    let QueryScope::Repository { path, scope } = scope else {
        return true;
    };
    association_matches(association, path, *scope)
        || (*scope == RepoScope::Worktrees
            && repository_roots
                .iter()
                .any(|root| association_matches(association, root, RepoScope::Tree)))
}

fn association_applies(association: &AssociationObservation, sequence: &NativeSequence) -> bool {
    match &association.applies_to {
        AssociationExtent::Partition => true,
        AssociationExtent::Record(record) => record == sequence,
        AssociationExtent::From { start, until } => {
            sequence >= start && until.as_ref().is_none_or(|until| sequence < until)
        }
    }
}

fn association_matches(
    association: &AssociationObservation,
    root: &Path,
    scope: RepoScope,
) -> bool {
    if association.basis == AssociationBasis::StoreHint {
        return false;
    }
    let Some(path) = association.path.as_deref() else {
        return false;
    };
    match scope {
        RepoScope::Exact => path == root,
        RepoScope::Tree | RepoScope::Worktrees => component_descendant(path, root),
    }
}

fn component_descendant(path: &Path, root: &Path) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_ok_and(|suffix| !suffix.as_os_str().is_empty())
}

type BranchNodeKey = (SourceId, PartitionId, String);

struct BranchAnalysis {
    issues: BTreeMap<BranchNodeKey, Vec<AvailabilityCode>>,
    memberships: BTreeMap<BranchNodeKey, Vec<EntityId>>,
}

#[derive(Clone, PartialEq, Eq)]
struct BranchNodeSignature {
    parent: Option<String>,
    declared_branch: Option<String>,
    links: Vec<BranchLink>,
}

struct BranchCandidate {
    path: Vec<BranchNodeKey>,
    declared_branch: Option<String>,
}

fn observation_branch_key(observation: &Observation) -> Option<BranchNodeKey> {
    match &observation.branch {
        BranchEvidence::Node {
            partition,
            native_id,
            ..
        } => Some((
            observation.source_ref.source_id,
            *partition,
            native_id.clone(),
        )),
        _ => None,
    }
}

fn observation_branch_ids(
    observation: &Observation,
    memberships: &BTreeMap<BranchNodeKey, Vec<EntityId>>,
) -> Vec<EntityId> {
    match &observation.branch {
        BranchEvidence::Linear { partition } => vec![declared_branch_id(*partition, "linear")],
        BranchEvidence::Node { .. } => observation_branch_key(observation)
            .and_then(|key| memberships.get(&key))
            .cloned()
            .unwrap_or_default(),
        BranchEvidence::Unavailable { .. } => Vec::new(),
    }
}

fn analyze_branches(
    observations: &[Observation],
    limits: &QueryLimits,
) -> Result<BranchAnalysis, QueryFailure> {
    let mut signatures = BTreeMap::<BranchNodeKey, BranchNodeSignature>::new();
    let mut issues = BTreeMap::<BranchNodeKey, Vec<AvailabilityCode>>::new();
    for observation in observations {
        let BranchEvidence::Node {
            partition,
            native_id,
            parent,
            declared_branch,
            links,
        } = &observation.branch
        else {
            continue;
        };
        let key = (
            observation.source_ref.source_id,
            *partition,
            native_id.clone(),
        );
        let signature = BranchNodeSignature {
            parent: parent.clone(),
            declared_branch: declared_branch.clone(),
            links: links.clone(),
        };
        if signatures
            .get(&key)
            .is_some_and(|previous| previous != &signature)
        {
            issues
                .entry(key.clone())
                .or_default()
                .push(AvailabilityCode::Conflict);
        } else {
            signatures.entry(key.clone()).or_insert(signature);
        }
        if parent.as_deref() == Some(native_id) {
            issues
                .entry(key)
                .or_default()
                .push(AvailabilityCode::Conflict);
        }
    }

    let mut analyzed = BTreeSet::new();
    for key in signatures.keys() {
        if analyzed.contains(key) {
            continue;
        }
        let mut path = BTreeSet::new();
        let mut current = key;
        loop {
            if analyzed.contains(current) {
                break;
            }
            if !path.insert(current) {
                for member in path.iter().copied() {
                    issues
                        .entry((*member).clone())
                        .or_default()
                        .push(AvailabilityCode::Conflict);
                }
                break;
            }
            let Some(parent) = signatures
                .get(current)
                .and_then(|signature| signature.parent.as_ref())
            else {
                break;
            };
            let parent_key = (current.0, current.1, parent.clone());
            let Some((parent_key, _)) = signatures.get_key_value(&parent_key) else {
                issues
                    .entry(current.clone())
                    .or_default()
                    .push(AvailabilityCode::NotCaptured);
                break;
            };
            current = parent_key;
        }
        analyzed.extend(path);
    }

    let mut parents = BTreeSet::new();
    for (key, signature) in &signatures {
        if let Some(parent) = &signature.parent {
            parents.insert((key.0, key.1, parent.clone()));
        }
    }
    let mut candidates = Vec::new();
    let mut candidate_memberships = 0usize;
    for leaf in signatures.keys().filter(|key| !parents.contains(*key)) {
        let mut current = leaf.clone();
        let mut seen = BTreeSet::new();
        let mut path = Vec::new();
        let mut valid = true;
        loop {
            if !seen.insert(current.clone())
                || issues.get(&current).is_some_and(|values| {
                    values.contains(&AvailabilityCode::Conflict)
                        || values.contains(&AvailabilityCode::NotCaptured)
                })
            {
                valid = false;
                break;
            }
            path.push(current.clone());
            let Some(parent) = signatures
                .get(&current)
                .and_then(|value| value.parent.as_ref())
            else {
                break;
            };
            current = (current.0, current.1, parent.clone());
        }
        if valid {
            candidate_memberships =
                candidate_memberships
                    .checked_add(path.len())
                    .ok_or_else(|| {
                        QueryFailure::limit(unisphere_core::query::LimitKind::BranchMemberships)
                    })?;
            if candidate_memberships > limits.max_branch_memberships {
                return Err(QueryFailure::limit(
                    unisphere_core::query::LimitKind::BranchMemberships,
                ));
            }
            let declared_branch = path.iter().find_map(|key| {
                signatures
                    .get(key)
                    .and_then(|signature| signature.declared_branch.clone())
            });
            candidates.push(BranchCandidate {
                path,
                declared_branch,
            });
        }
    }

    let mut declared_counts = BTreeMap::<(SourceId, PartitionId, String), usize>::new();
    for candidate in &candidates {
        if let Some(declared) = &candidate.declared_branch {
            let leaf = &candidate.path[0];
            *declared_counts
                .entry((leaf.0, leaf.1, declared.clone()))
                .or_default() += 1;
        }
    }

    let mut memberships = BTreeMap::<BranchNodeKey, BTreeSet<EntityId>>::new();
    let mut membership_count = 0usize;
    for candidate in candidates {
        let leaf = &candidate.path[0];
        let branch = candidate
            .declared_branch
            .as_ref()
            .filter(|declared| {
                declared_counts
                    .get(&(leaf.0, leaf.1, (*declared).clone()))
                    .copied()
                    == Some(1)
            })
            .map_or_else(
                || native_tree_branch_id(leaf.1, &leaf.2),
                |declared| declared_branch_id(leaf.1, declared),
            );
        for member in candidate.path {
            if memberships.entry(member).or_default().insert(branch) {
                membership_count = membership_count.checked_add(1).ok_or_else(|| {
                    QueryFailure::limit(unisphere_core::query::LimitKind::BranchMemberships)
                })?;
                if membership_count > limits.max_branch_memberships {
                    return Err(QueryFailure::limit(
                        unisphere_core::query::LimitKind::BranchMemberships,
                    ));
                }
            }
        }
    }

    for values in issues.values_mut() {
        values.sort();
        values.dedup();
    }
    Ok(BranchAnalysis {
        issues,
        memberships: memberships
            .into_iter()
            .map(|(key, values)| (key, values.into_iter().collect()))
            .collect(),
    })
}

fn declared_branch_id(partition: PartitionId, declared: &str) -> EntityId {
    EntityId::derive(
        EntityKind::Branch,
        [
            partition.entity().digest().bytes().as_slice(),
            declared.as_bytes(),
        ],
    )
}

fn native_tree_branch_id(partition: PartitionId, leaf_native_id: &str) -> EntityId {
    EntityId::derive(
        EntityKind::Branch,
        [
            partition.entity().digest().bytes().as_slice(),
            b"native-tree",
            leaf_native_id.as_bytes(),
        ],
    )
}
fn merge_timestamp(
    current: Option<Timestamp>,
    candidate: Option<Timestamp>,
    availability: &mut Vec<AvailabilityCode>,
) -> Option<Timestamp> {
    match (current, candidate) {
        (Some(current), Some(candidate)) if current != candidate => {
            availability.push(AvailabilityCode::Conflict);
            Some(current.min(candidate))
        }
        (None, candidate) => candidate,
        (current, None) => current,
        (current, Some(_)) => current,
    }
}

fn earliest(current: Option<Timestamp>, candidate: Option<Timestamp>) -> Option<Timestamp> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.min(candidate)),
        (None, candidate) => candidate,
        (current, None) => current,
    }
}

fn push_source_ref(target: &mut Vec<SourceRef>, value: &SourceRef) {
    if !target.contains(value) {
        target.push(value.clone());
    }
}

fn dedup_availability(mut values: Vec<AvailabilityCode>) -> Vec<AvailabilityCode> {
    values.sort();
    values.dedup();
    values
}

fn read_status_code(status: SourceReadStatus) -> Option<AvailabilityCode> {
    match status {
        SourceReadStatus::Readable => None,
        SourceReadStatus::Absent => Some(AvailabilityCode::Absent),
        SourceReadStatus::Unreadable | SourceReadStatus::Partial => Some(AvailabilityCode::Partial),
        SourceReadStatus::Unsupported => Some(AvailabilityCode::NotSupported),
    }
}

fn retain_message_parts(parts: &[ObservationPart], access: &ContentAccess) -> Vec<ObservationPart> {
    if access.permits_payload(FieldId::Parts) {
        return parts.to_vec();
    }
    if access.permits_payload(FieldId::Text) {
        let retained = parts
            .iter()
            .filter(|part| matches!(part, ObservationPart::Text(_)))
            .cloned()
            .collect::<Vec<_>>();
        if !retained.is_empty() {
            return retained;
        }
    }
    (!parts.is_empty())
        .then_some(ObservationPart::Unavailable(
            AvailabilityCode::SensitiveOmitted,
        ))
        .into_iter()
        .collect()
}

fn retain_payload(
    parts: &[ObservationPart],
    field: FieldId,
    access: &ContentAccess,
) -> Vec<ObservationPart> {
    if access.permits_payload(field) {
        parts.to_vec()
    } else {
        (!parts.is_empty())
            .then_some(ObservationPart::Unavailable(
                AvailabilityCode::SensitiveOmitted,
            ))
            .into_iter()
            .collect()
    }
}
fn observation_weight(observation: &Observation) -> usize {
    let mut bytes = observation.source_ref.revision.len()
        + observation.source_ref.subrecord.len()
        + observation.native_record_id.as_deref().map_or(0, str::len)
        + observation
            .parent_ids
            .iter()
            .map(String::len)
            .sum::<usize>()
        + observation.sequence.key.len();
    for facet in &observation.facets {
        bytes = bytes.saturating_add(match facet {
            ObservationFacet::SessionMetadata {
                native_id,
                name,
                models,
                ..
            } => {
                native_id.len()
                    + name.as_deref().map_or(0, str::len)
                    + models.iter().map(String::len).sum::<usize>()
            }
            ObservationFacet::Message {
                native_id, parts, ..
            } => native_id.as_deref().map_or(0, str::len) + parts_weight(parts),
            ObservationFacet::ToolCall {
                native_call_id,
                native_name,
                family,
                input,
                ..
            } => {
                native_call_id.len()
                    + native_name.len()
                    + family.as_deref().map_or(0, str::len)
                    + parts_weight(input)
            }
            ObservationFacet::ToolResult {
                native_call_id,
                native_name,
                output,
                ..
            } => {
                native_call_id.len()
                    + native_name.as_deref().map_or(0, str::len)
                    + parts_weight(output)
            }
            ObservationFacet::ToolProgress {
                native_call_id,
                parts,
            } => native_call_id.len() + parts_weight(parts),
            ObservationFacet::Control { .. } => 0,
            ObservationFacet::Usage { owner, .. } => owner.as_deref().map_or(0, str::len),
            ObservationFacet::Attribution {
                native_key,
                declared_agent,
                target_commit,
                ranges,
                ..
            } => {
                native_key.len()
                    + declared_agent.as_deref().map_or(0, str::len)
                    + target_commit.as_deref().map_or(0, str::len)
                    + ranges.iter().map(String::len).sum::<usize>()
            }
        });
    }
    bytes
}

fn parts_weight(parts: &[ObservationPart]) -> usize {
    parts
        .iter()
        .map(|part| match part {
            ObservationPart::Text(value) | ObservationPart::Reasoning(value) => value.len(),
            ObservationPart::Structured(value) => value.to_string().len(),
            ObservationPart::Unavailable(_) => 0,
        })
        .sum()
}
