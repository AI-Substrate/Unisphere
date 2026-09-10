//! Composition over native file sources and pinned Git objects; semantics remain in the SDK.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use unisphere_loader_query::LocalQuerySource;
use unisphere_sdk::query::*;
use unisphere_sdk::{
    GitNoteLoader, GitNoteSelection, GitNotesError, GitNotesLimits, GitNotesScope,
};

pub struct GitSource<L> {
    pub loader: Option<L>,
    pub adapter: Arc<dyn QueryAdapter>,
    pub policy: &'static str,
    pub repository: PathBuf,
    pub output: Option<PathBuf>,
}

pub struct Sources<L> {
    pub local: LocalQuerySource,
    pub identities: Vec<(AdapterId, HarnessId)>,
    pub git: Option<GitSource<L>>,
}

impl<L: GitNoteLoader> QuerySource for Sources<L> {
    fn load(
        &self,
        scope: &QueryScope,
        selection: &SourceSelection,
        limits: &QueryLimits,
        access: ContentAccess,
    ) -> Result<QueryInput, QueryFailure> {
        if matches!(scope, QueryScope::Offline { .. }) {
            return self.local.load(scope, selection, limits, access);
        }
        limits.validate()?;
        selection.validate()?;
        let note_adapter = AdapterId::new("git-ai").map_err(|_| QueryFailure::invalid_data())?;
        let note_harness = HarnessId::new("git-ai").map_err(|_| QueryFailure::invalid_data())?;
        let explicit_adapter = match scope {
            QueryScope::Source {
                selector: SourceSelector::Path { adapter, .. },
            } => adapter.as_ref(),
            _ => None,
        };
        let git_admitted = self.git.is_some()
            && selection.admits(&note_adapter, &note_harness)
            && (!matches!(
                scope,
                QueryScope::Source {
                    selector: SourceSelector::Path { .. }
                }
            ) || explicit_adapter == Some(&note_adapter));
        let local_admitted = explicit_adapter.is_none_or(|id| id != &note_adapter)
            && self
                .identities
                .iter()
                .any(|(adapter, harness)| selection.admits(adapter, harness));
        if explicit_adapter == Some(&note_adapter) && !git_admitted {
            return Err(QueryFailure::new(
                QueryFailureCode::InvalidArgument,
                RecoveryAction::ChooseAdapter {
                    allowed: selection.include_adapters.iter().cloned().collect(),
                },
            ));
        }
        if !git_admitted {
            let mut input = self.local.load(scope, selection, limits, access)?;
            if self.git.is_some()
                && let QueryInput::Native(view) = &mut input
            {
                view.coverage.excluded_adapters.push(note_adapter);
                view.coverage.excluded_adapters.sort();
                view.coverage.excluded_adapters.dedup();
            }
            return Ok(input);
        }
        if !local_admitted && self.git.as_ref().is_some_and(|git| git.loader.is_none()) {
            return Err(QueryFailure::new(
                QueryFailureCode::UnreadableSource,
                RecoveryAction::FixSource {
                    reason: SourceProblem::GitUnavailable,
                    source: None,
                },
            ));
        }
        let git = self.git.as_ref().ok_or_else(QueryFailure::invalid_data)?;
        let (mut notes, bytes) = git.load(scope, limits, access.clone())?;
        if local_admitted {
            let remaining = QueryLimits {
                max_sources: limits
                    .max_sources
                    .checked_sub(notes.sources.len())
                    .ok_or_else(|| QueryFailure::limit(LimitKind::Sources))?,
                max_total_input_bytes: limits
                    .max_total_input_bytes
                    .checked_sub(bytes)
                    .ok_or_else(|| QueryFailure::limit(LimitKind::TotalInputBytes))?,
                max_source_bytes: limits
                    .max_source_bytes
                    .min(limits.max_total_input_bytes.saturating_sub(bytes)),
                max_observations_and_rows: limits
                    .max_observations_and_rows
                    .checked_sub(notes.observations.len())
                    .ok_or_else(|| QueryFailure::limit(LimitKind::ObservationsAndRows))?,
                ..*limits
            };
            let mut local_selection = selection.clone();
            local_selection.include_adapters.remove(&note_adapter);
            local_selection.include_harnesses.remove(&note_harness);
            local_selection
                .exclude_adapters
                .insert(note_adapter.clone());
            match self.local.load(scope, &local_selection, &remaining, access) {
                Ok(QueryInput::Native(local)) => combine(&mut notes, local)?,
                Err(error)
                    if matches!(
                        scope,
                        QueryScope::Source {
                            selector: SourceSelector::Id(_)
                        }
                    ) && error.kind() == QueryFailureCode::MissingSource
                        && !notes.sources.is_empty() => {}
                Err(error) => return Err(error),
                Ok(QueryInput::Saved { .. }) => return Err(QueryFailure::invalid_data()),
            }
        } else {
            notes
                .coverage
                .excluded_adapters
                .extend(self.identities.iter().map(|(adapter, _)| adapter.clone()));
        }
        notes
            .coverage
            .excluded_adapters
            .retain(|adapter| adapter != &note_adapter);
        notes.coverage.excluded_adapters.sort();
        notes.coverage.excluded_adapters.dedup();
        notes.sources.sort_by_key(|source| source.id);
        notes.validate(limits)?;
        if matches!(
            scope,
            QueryScope::Source {
                selector: SourceSelector::Id(_)
            }
        ) && notes.sources.is_empty()
        {
            return Err(QueryFailure::new(
                QueryFailureCode::MissingSource,
                RecoveryAction::FixSource {
                    reason: SourceProblem::Missing,
                    source: None,
                },
            ));
        }
        Ok(QueryInput::Native(notes))
    }
}

impl<L: GitNoteLoader> GitSource<L> {
    fn load(
        &self,
        scope: &QueryScope,
        limits: &QueryLimits,
        access: ContentAccess,
    ) -> Result<(NativeQueryView, usize), QueryFailure> {
        let repository = match scope {
            QueryScope::Repository { path, .. }
            | QueryScope::Source {
                selector: SourceSelector::Path { path, .. },
            } => path,
            _ => &self.repository,
        };
        let requested = GitNotesScope {
            repository: repository.clone(),
            notes_ref: "refs/notes/ai".into(),
            selection: GitNoteSelection::All,
        };
        // Use the accepted native loader defaults as tighter bounds; never bypass
        // its independent hard ceilings when adapting a larger query budget.
        let defaults = GitNotesLimits::default();
        let per_note = limits.max_source_bytes.min(defaults.max_note_bytes);
        let note_limits = GitNotesLimits {
            max_notes: limits.max_sources.min(defaults.max_notes),
            max_records: limits.max_observations_and_rows.min(defaults.max_records),
            max_note_bytes: per_note,
            max_total_bytes: limits.max_total_input_bytes.min(defaults.max_total_bytes),
            max_listing_bytes: limits.max_total_input_bytes.min(defaults.max_listing_bytes),
            ..GitNotesLimits::default()
        };
        let mut view = NativeQueryView {
            sources: Vec::new(),
            observations: Vec::new(),
            repository_roots: Vec::new(),
            coverage: Coverage {
                source_read_complete: true,
                ..Coverage::default()
            },
        };
        // Never authorize a file destination if the admitted Git source's roots
        // cannot be established, even on partial or ID-based query paths.
        if self.output.is_some() && self.loader.is_none() {
            return Err(invalid_output());
        }
        let loader = match self.loader.as_ref() {
            Some(loader) => loader,
            None if matches!(
                scope,
                QueryScope::Source {
                    selector: SourceSelector::Id(_)
                }
            ) =>
            {
                return Ok((view, 0));
            }
            None => {
                add_unavailable(
                    &mut view,
                    repository,
                    None,
                    self.policy,
                    &GitNotesError::GitUnavailable,
                )?;
                return Ok((view, 0));
            }
        };
        let listing = match loader.list_notes(&requested, note_limits) {
            Err(_) if self.output.is_some() => return Err(invalid_output()),
            Ok(listing) => listing,
            Err(_)
                if matches!(
                    scope,
                    QueryScope::Source {
                        selector: SourceSelector::Id(_)
                    }
                ) =>
            {
                return Ok((view, 0));
            }
            Err(error) => {
                add_unavailable(&mut view, repository, None, self.policy, &error)?;
                return Ok((view, 0));
            }
        };
        listing
            .validate(&requested, note_limits)
            .map_err(|_| QueryFailure::invalid_data())?;
        if let Some(output) = &self.output {
            let leaf = output.file_name().ok_or_else(invalid_output)?;
            let parent = std::fs::canonicalize(output.parent().ok_or_else(invalid_output)?)
                .map_err(|_| invalid_output())?;
            if !listing.allows_output_path(&parent.join(leaf)) {
                return Err(invalid_output());
            }
        }
        if let QueryScope::Repository { path, .. } = scope {
            view.repository_roots
                .push(std::fs::canonicalize(path).map_err(|_| QueryFailure::invalid_data())?);
        }
        let mut consumed = 0usize;
        for note in listing.notes {
            let common = note
                .repository_id
                .to_str()
                .ok_or_else(QueryFailure::invalid_data)?;
            let id = SourceId::derive([
                b"git-ai".as_slice(),
                b"git_notes",
                common.as_bytes(),
                note.notes_ref.as_bytes(),
                note.target_commit.as_bytes(),
            ]);
            if matches!(scope,QueryScope::Source{selector:SourceSelector::Id(requested)} if *requested!=id)
            {
                continue;
            }
            let source = SourceEvidence {
                id,
                adapter: AdapterId::new("git-ai").map_err(|_| QueryFailure::invalid_data())?,
                harness: HarnessId::new("git-ai").map_err(|_| QueryFailure::invalid_data())?,
                representation: "git_notes".into(),
                locator: SourceLocator::LocalPath(repository.clone()),
                revision: format!("{}:{}", note.notes_tip, note.note_blob),
                query_policy_version: self.policy.into(),
                read_status: SourceReadStatus::Readable,
                associations: vec![AssociationObservation {
                    basis: AssociationBasis::GitRepositoryIdentity,
                    path: Some(listing.repository.clone()),
                    partition: PartitionId::derive(id, b"repository"),
                    applies_to: AssociationExtent::Partition,
                }],
                available_fields: BTreeSet::new(),
            };
            let loaded = match loader.read_note(&note, note_limits) {
                Ok(loaded) => loaded,
                Err(error) => {
                    add_unavailable(&mut view, repository, Some(source), self.policy, &error)?;
                    continue;
                }
            };
            if loaded.source != note {
                return Err(QueryFailure::invalid_data());
            }
            consumed = consumed
                .checked_add(loaded.bytes.len())
                .ok_or_else(|| QueryFailure::limit(LimitKind::TotalInputBytes))?;
            if consumed > note_limits.max_total_bytes {
                return Err(QueryFailure::limit(LimitKind::TotalInputBytes));
            }
            let locator = NativeLocator::GitNote {
                repository_id: common.into(),
                notes_ref: note.notes_ref,
                notes_tip: note.notes_tip,
                target_commit: note.target_commit,
                note_blob: note.note_blob,
            };
            let inspected = match self.adapter.inspect(
                NativeQueryInput::ProvidedObject {
                    source: &source,
                    bytes: &loaded.bytes,
                    locator: &locator,
                },
                access.clone(),
                limits,
            ) {
                Ok(inspected) => inspected,
                Err(error) if error.kind() == QueryFailureCode::ResourceLimit => return Err(error),
                Err(error) => {
                    let native_error = if matches!(
                        error.kind(),
                        QueryFailureCode::UnsupportedSource | QueryFailureCode::UnsupportedSchema
                    ) {
                        GitNotesError::UnsupportedFormat
                    } else {
                        GitNotesError::InvalidData
                    };
                    add_unavailable(
                        &mut view,
                        repository,
                        Some(source),
                        self.policy,
                        &native_error,
                    )?;
                    continue;
                }
            };
            inspected.validate(limits)?;
            if inspected.source.id != id
                || inspected.source.revision != source.revision
                || inspected.source.adapter != source.adapter
                || inspected.source.harness != source.harness
                || inspected.source.locator != source.locator
            {
                return Err(QueryFailure::invalid_data());
            }
            if view
                .observations
                .len()
                .saturating_add(inspected.observations.len())
                > limits.max_observations_and_rows
            {
                return Err(QueryFailure::limit(LimitKind::ObservationsAndRows));
            }
            view.coverage.discovered_sources += 1;
            view.coverage.loaded_sources += 1;
            view.coverage.selected_sources += 1;
            *view
                .coverage
                .source_status
                .entry(SourceReadStatus::Readable)
                .or_default() += 1;
            view.coverage.issues.extend(inspected.issues);
            view.observations.extend(inspected.observations);
            view.sources.push(inspected.source);
        }
        Ok((view, consumed))
    }
}

fn add_unavailable(
    view: &mut NativeQueryView,
    repository: &Path,
    source: Option<SourceEvidence>,
    policy: &str,
    error: &GitNotesError,
) -> Result<(), QueryFailure> {
    if matches!(
        error,
        GitNotesError::NoteLimit
            | GitNotesError::ListingLimit
            | GitNotesError::BatchLimit
            | GitNotesError::RecordLimit
    ) {
        return Err(QueryFailure::limit(LimitKind::TotalInputBytes));
    }
    let status = if matches!(
        error,
        GitNotesError::UnsupportedFormat
            | GitNotesError::UnsupportedPlatform
            | GitNotesError::UnsupportedRepository
            | GitNotesError::GitUnavailable
    ) {
        SourceReadStatus::Unsupported
    } else {
        SourceReadStatus::Unreadable
    };
    let mut source = source.unwrap_or(SourceEvidence {
        id: SourceId::derive([
            b"git-ai".as_slice(),
            b"git_notes",
            repository.as_os_str().as_encoded_bytes(),
            b"refs/notes/ai",
        ]),
        adapter: AdapterId::new("git-ai").map_err(|_| QueryFailure::invalid_data())?,
        harness: HarnessId::new("git-ai").map_err(|_| QueryFailure::invalid_data())?,
        representation: "git_notes".into(),
        locator: SourceLocator::LocalPath(repository.into()),
        revision: format!("unavailable:{}", error.kind()),
        query_policy_version: policy.into(),
        read_status: status,
        associations: Vec::new(),
        available_fields: BTreeSet::new(),
    });
    source.read_status = status;
    view.coverage.source_read_complete = false;
    view.coverage.discovered_sources += 1;
    *view.coverage.source_status.entry(status).or_default() += 1;
    view.coverage.issues.push(AvailabilityIssue {
        code: if status == SourceReadStatus::Unsupported {
            AvailabilityCode::NotSupported
        } else {
            AvailabilityCode::Partial
        },
        source: Some(source.id),
        field: None,
        entity: None,
        offset: None,
    });
    view.sources.push(source);
    Ok(())
}

fn combine(notes: &mut NativeQueryView, local: NativeQueryView) -> Result<(), QueryFailure> {
    notes.sources.extend(local.sources);
    notes.observations.extend(local.observations);
    notes.repository_roots.extend(local.repository_roots);
    notes.repository_roots.sort();
    notes.repository_roots.dedup();
    let coverage = &mut notes.coverage;
    coverage.discovered_sources += local.coverage.discovered_sources;
    coverage.loaded_sources += local.coverage.loaded_sources;
    coverage.selected_sources += local.coverage.selected_sources;
    coverage.source_read_complete &= local.coverage.source_read_complete;
    for (status, count) in local.coverage.source_status {
        *coverage.source_status.entry(status).or_default() += count;
    }
    for (status, count) in local.coverage.association_status {
        *coverage.association_status.entry(status).or_default() += count;
    }
    coverage
        .excluded_adapters
        .extend(local.coverage.excluded_adapters);
    coverage.issues.extend(local.coverage.issues);
    coverage.validate()
}

fn invalid_output() -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::InvalidArgument,
        RecoveryAction::ChooseNewOutput {
            discard_partial: false,
        },
    )
}
