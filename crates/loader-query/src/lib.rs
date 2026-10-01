//! Explicit, bounded local source discovery and native query input composition.
//!
//! Registrations, symbolic roots, stdin bytes and Git capability are injected by
//! the composition root. Native loading never discovers ambient roots, scans an
//! implicit current directory, fetches remote transcripts, or enriches saved input.
//! The optional [`pij`] identity lookup delegates configured transport/authentication
//! to the explicitly supplied Pij CLI; it is not a transcript loader.
//! [`status_target`] resolves session-status Pij/pane queries to explicit targets
//! through injected command, process and filesystem ports.
#![forbid(unsafe_code)]

pub mod pij;
pub mod status_target;

use globset::{GlobBuilder, GlobMatcher};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};
use unisphere_core::{
    AdapterDescriptor, PipelineError, PipelineErrorKind, ReadLimits, SessionRef, SnapshotFormat,
    SnapshotLimits, SnapshotLoader, SnapshotRef,
    query::{
        AdapterId, AssociationStatus, AvailabilityCode, AvailabilityIssue, ContentAccess, Coverage,
        HarnessId, InspectedSource, LimitKind, NativeQueryInput, NativeQueryView, OfflineRef,
        QueryAdapter, QueryFailure, QueryFailureCode, QueryInput, QueryLimits, QueryScope,
        QuerySource, RecoveryAction, RepoScope, SavedFormat, SourceEvidence, SourceId,
        SourceLocator, SourceProblem, SourceReadStatus, SourceSelection, SourceSelector,
    },
};
use unisphere_loader_jsonl::FileSessionLoader;
use unisphere_loader_snapshot::FileSnapshotLoader;

const WALK_ENTRIES_PER_SOURCE: usize = 64;
const UNAVAILABLE_REVISION_PREFIX: &str = "unavailable:v1:";

/// Storage framing selected by one application registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeRepresentation {
    Jsonl,
    JsonDocument,
    JsonJournal,
    SqliteKeyValue { table: String },
}

impl NativeRepresentation {
    pub fn storage_format(&self) -> &'static str {
        match self {
            Self::Jsonl => "jsonl",
            Self::JsonDocument => "json_document",
            Self::JsonJournal => "json_journal",
            Self::SqliteKeyValue { .. } => "sqlite_key_value",
        }
    }

    pub fn name(&self) -> Cow<'_, str> {
        match self {
            Self::SqliteKeyValue { table } => Cow::Owned(format!("sqlite_key_value:{table}")),
            _ => Cow::Borrowed(self.storage_format()),
        }
    }

    fn snapshot_format(&self) -> Option<SnapshotFormat> {
        match self {
            Self::Jsonl => None,
            Self::JsonDocument => Some(SnapshotFormat::JsonDocument),
            Self::JsonJournal => Some(SnapshotFormat::JsonJournal),
            Self::SqliteKeyValue { table } => Some(SnapshotFormat::SqliteKeyValue {
                table: table.clone(),
            }),
        }
    }
}

/// One registered native representation and its pure supplied-data adapter.
#[derive(Clone)]
pub struct QueryRegistration {
    pub descriptor: AdapterDescriptor,
    pub harness: HarnessId,
    pub representation: NativeRepresentation,
    pub query_policy_version: String,
    pub adapter: Arc<dyn QueryAdapter>,
}

impl QueryRegistration {
    pub fn new(
        descriptor: AdapterDescriptor,
        harness: HarnessId,
        representation: NativeRepresentation,
        adapter: Arc<dyn QueryAdapter>,
    ) -> Self {
        Self {
            descriptor,
            harness,
            representation,
            query_policy_version: "1".into(),
            adapter,
        }
    }

    pub fn with_query_policy_version(mut self, version: impl Into<String>) -> Self {
        self.query_policy_version = version.into();
        self
    }

    fn adapter_id(&self) -> Result<AdapterId, QueryFailure> {
        AdapterId::new(self.descriptor.id).map_err(|_| QueryFailure::invalid_data())
    }

    fn validate(&self) -> Result<AdapterId, QueryFailure> {
        let adapter = self.adapter_id()?;
        if self.query_policy_version.is_empty()
            || matches!(&self.representation, NativeRepresentation::SqliteKeyValue { table } if table.is_empty() || !table.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        {
            return Err(QueryFailure::invalid_data());
        }
        Ok(adapter)
    }
}

/// Caller-owned saved input. The format is explicit because a single JSONL row
/// and a JSON document are not safely distinguishable by sniffing.
#[derive(Clone, PartialEq, Eq)]
pub struct ProvidedInput {
    pub bytes: Arc<[u8]>,
    pub format: SavedFormat,
}

/// Every ambient-looking capability used by [`LocalQuerySource`] is supplied here.
#[derive(Clone)]
pub struct LocalQueryContext {
    pub platform: String,
    pub bases: BTreeMap<String, PathBuf>,
    pub git_executable: Option<PathBuf>,
    pub stdin: Option<ProvidedInput>,
}

impl LocalQueryContext {
    pub fn new(platform: impl Into<String>, bases: BTreeMap<String, PathBuf>) -> Self {
        Self {
            platform: platform.into(),
            bases,
            git_executable: None,
            stdin: None,
        }
    }
}

type RegisteredAdapter = (usize, AdapterId);

/// Imperative local provider; all association, reconciliation and row selection
/// remains in the SDK query service.
pub struct LocalQuerySource {
    registrations: Vec<QueryRegistration>,
    context: LocalQueryContext,
    jsonl: FileSessionLoader,
    snapshot: FileSnapshotLoader,
}

impl LocalQuerySource {
    pub fn new(registrations: Vec<QueryRegistration>, context: LocalQueryContext) -> Self {
        Self {
            registrations,
            context,
            jsonl: FileSessionLoader::new(),
            snapshot: FileSnapshotLoader::new(),
        }
    }

    fn admitted_registrations(
        &self,
        selection: &SourceSelection,
    ) -> Result<(Vec<RegisteredAdapter>, Vec<AdapterId>), QueryFailure> {
        selection.validate()?;
        let mut admitted = Vec::new();
        let mut excluded = BTreeSet::new();
        let mut supported = BTreeSet::new();
        for (index, registration) in self.registrations.iter().enumerate() {
            let adapter = registration.adapter_id()?;
            supported.insert(adapter.clone());
            if selection.admits(&adapter, &registration.harness) {
                registration.validate()?;
                admitted.push((index, adapter));
            } else {
                excluded.insert(adapter);
            }
        }
        if admitted.is_empty()
            && (!selection.include_adapters.is_empty() || !selection.include_harnesses.is_empty())
        {
            return Err(QueryFailure::new(
                QueryFailureCode::UnsupportedSource,
                RecoveryAction::ChooseAdapter {
                    allowed: supported.into_iter().collect(),
                },
            ));
        }
        Ok((admitted, excluded.into_iter().collect()))
    }

    fn load_offline(
        &self,
        input: &OfflineRef,
        limits: &QueryLimits,
    ) -> Result<QueryInput, QueryFailure> {
        let provided = match input {
            OfflineRef::Stdin => self.context.stdin.clone().ok_or_else(missing_source)?,
            OfflineRef::File(path) => ProvidedInput {
                bytes: read_saved_file(path, limits.max_source_bytes)?.into(),
                format: if path
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
                {
                    SavedFormat::QueryJsonlV1
                } else {
                    SavedFormat::QueryJsonV1
                },
            },
        };
        if provided.bytes.len() > limits.max_source_bytes
            || provided.bytes.len() > limits.max_total_input_bytes
        {
            return Err(QueryFailure::limit(LimitKind::SourceBytes));
        }
        Ok(QueryInput::Saved {
            bytes: provided.bytes,
            format: provided.format,
        })
    }

    fn repository_roots(
        &self,
        path: &Path,
        scope: RepoScope,
        limits: &QueryLimits,
    ) -> Result<Vec<PathBuf>, QueryFailure> {
        let root = canonical_directory(path)?;
        if scope != RepoScope::Worktrees {
            return Ok(vec![root]);
        }
        let git = self
            .context
            .git_executable
            .as_ref()
            .ok_or_else(git_unavailable)?;
        if !git.is_absolute()
            || !fs::symlink_metadata(git)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        {
            return Err(git_unavailable());
        }
        let mut child = Command::new(git)
            .arg("-C")
            .arg(&root)
            .args(["worktree", "list", "--porcelain", "-z"])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| git_unavailable())?;
        let mut bytes = Vec::new();
        let cap = limits.max_source_bytes;
        child
            .stdout
            .take()
            .ok_or_else(git_unavailable)?
            .take(u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| git_unavailable())?;
        if bytes.len() > cap {
            let _ = child.kill();
            let _ = child.wait();
            return Err(QueryFailure::limit(LimitKind::SourceBytes));
        }
        let status = child.wait().map_err(|_| git_unavailable())?;
        if !status.success() {
            return Err(git_unavailable());
        }
        let mut roots = vec![root];
        for field in bytes.split(|byte| *byte == 0) {
            let Some(path) = field.strip_prefix(b"worktree ") else {
                continue;
            };
            let path = std::str::from_utf8(path).map_err(|_| QueryFailure::invalid_data())?;
            roots.push(canonical_directory(Path::new(path))?);
        }
        roots.sort_unstable();
        roots.dedup();
        if roots.len() > limits.max_sources {
            return Err(QueryFailure::limit(LimitKind::Sources));
        }
        Ok(roots)
    }

    fn discover_repository(
        &self,
        admitted: &[(usize, AdapterId)],
        limits: &QueryLimits,
    ) -> Result<Vec<Candidate>, QueryFailure> {
        let walk_limit = limits
            .max_sources
            .checked_mul(WALK_ENTRIES_PER_SOURCE)
            .ok_or_else(|| QueryFailure::limit(LimitKind::Sources))?;
        let mut candidates = Vec::new();
        for (registration_index, adapter) in admitted {
            let registration = &self.registrations[*registration_index];
            let matching_hints: Vec<_> = registration
                .descriptor
                .locations
                .iter()
                .filter(|hint| hint.storage_format == registration.representation.storage_format())
                .collect();
            if matching_hints.is_empty() {
                push_candidate(
                    &mut candidates,
                    Candidate::unavailable(
                        *registration_index,
                        adapter.clone(),
                        SourceLocator::Provided("registered-representation".into()),
                        SourceReadStatus::Unsupported,
                    ),
                    limits,
                )?;
                continue;
            }
            for hint in matching_hints {
                let locator_label = format!("{}:{}:{}", hint.base, hint.path, hint.session_glob);
                if !platform_matches(hint.platforms, &self.context.platform) {
                    push_candidate(
                        &mut candidates,
                        Candidate::unavailable(
                            *registration_index,
                            adapter.clone(),
                            SourceLocator::Provided(locator_label),
                            SourceReadStatus::Unsupported,
                        ),
                        limits,
                    )?;
                    continue;
                }
                let Some(base) = self.context.bases.get(hint.base) else {
                    push_candidate(
                        &mut candidates,
                        Candidate::unavailable(
                            *registration_index,
                            adapter.clone(),
                            SourceLocator::Provided(locator_label),
                            SourceReadStatus::Absent,
                        ),
                        limits,
                    )?;
                    continue;
                };
                let expected_root = base.join(hint.path);
                let base = match fs::canonicalize(base) {
                    Ok(base) => base,
                    Err(failure) if failure.kind() == io::ErrorKind::NotFound => {
                        push_candidate(
                            &mut candidates,
                            Candidate::unavailable(
                                *registration_index,
                                adapter.clone(),
                                SourceLocator::LocalPath(expected_root),
                                SourceReadStatus::Absent,
                            ),
                            limits,
                        )?;
                        continue;
                    }
                    Err(_) => {
                        push_candidate(
                            &mut candidates,
                            Candidate::unavailable(
                                *registration_index,
                                adapter.clone(),
                                SourceLocator::LocalPath(expected_root),
                                SourceReadStatus::Unreadable,
                            ),
                            limits,
                        )?;
                        continue;
                    }
                };
                let root = match fs::canonicalize(&expected_root) {
                    Ok(root) if root.starts_with(&base) => root,
                    Ok(_) => {
                        push_candidate(
                            &mut candidates,
                            Candidate::unavailable(
                                *registration_index,
                                adapter.clone(),
                                SourceLocator::LocalPath(expected_root),
                                SourceReadStatus::Unsupported,
                            ),
                            limits,
                        )?;
                        continue;
                    }
                    Err(failure) if failure.kind() == io::ErrorKind::NotFound => {
                        push_candidate(
                            &mut candidates,
                            Candidate::unavailable(
                                *registration_index,
                                adapter.clone(),
                                SourceLocator::LocalPath(expected_root),
                                SourceReadStatus::Absent,
                            ),
                            limits,
                        )?;
                        continue;
                    }
                    Err(_) => {
                        push_candidate(
                            &mut candidates,
                            Candidate::unavailable(
                                *registration_index,
                                adapter.clone(),
                                SourceLocator::LocalPath(expected_root),
                                SourceReadStatus::Unreadable,
                            ),
                            limits,
                        )?;
                        continue;
                    }
                };
                let matcher = match compile_glob(hint.session_glob) {
                    Ok(matcher) => matcher,
                    Err(_) => {
                        push_candidate(
                            &mut candidates,
                            Candidate::unavailable(
                                *registration_index,
                                adapter.clone(),
                                SourceLocator::LocalPath(root),
                                SourceReadStatus::Unsupported,
                            ),
                            limits,
                        )?;
                        continue;
                    }
                };
                match walk_matching(&root, &matcher, walk_limit) {
                    Ok(found) if found.is_empty() => push_candidate(
                        &mut candidates,
                        Candidate::unavailable(
                            *registration_index,
                            adapter.clone(),
                            SourceLocator::LocalPath(root),
                            SourceReadStatus::Absent,
                        ),
                        limits,
                    )?,
                    Ok(found) => {
                        for (path, status) in found {
                            push_candidate(
                                &mut candidates,
                                Candidate {
                                    registration: *registration_index,
                                    adapter: adapter.clone(),
                                    locator: SourceLocator::LocalPath(path.clone()),
                                    path: Some(path),
                                    status,
                                },
                                limits,
                            )?;
                        }
                    }
                    Err(WalkFailure::Limit) => {
                        return Err(QueryFailure::limit(LimitKind::Sources));
                    }
                    Err(WalkFailure::Read) => push_candidate(
                        &mut candidates,
                        Candidate::unavailable(
                            *registration_index,
                            adapter.clone(),
                            SourceLocator::LocalPath(root),
                            SourceReadStatus::Unreadable,
                        ),
                        limits,
                    )?,
                }
            }
        }
        candidates.sort_by(|left, right| {
            left.adapter
                .cmp(&right.adapter)
                .then_with(|| locator_bytes(&left.locator).cmp(locator_bytes(&right.locator)))
                .then_with(|| left.registration.cmp(&right.registration))
        });
        candidates.dedup_by(|left, right| {
            left.registration == right.registration && left.locator == right.locator
        });
        Ok(candidates)
    }

    fn discover_explicit_path(
        &self,
        path: &Path,
        requested_adapter: Option<&AdapterId>,
        admitted: &[(usize, AdapterId)],
    ) -> Result<Vec<Candidate>, QueryFailure> {
        let mut matching = Vec::new();
        let mut fallback = Vec::new();
        for (index, adapter) in admitted {
            if requested_adapter.is_some_and(|requested| requested != adapter) {
                continue;
            }
            let registration = &self.registrations[*index];
            if self.registration_matches_path(registration, path) {
                matching.push((*index, adapter.clone()));
            } else if requested_adapter.is_some() {
                fallback.push((*index, adapter.clone()));
            }
        }
        // An explicitly named adapter can expose several native framings. Outside
        // its discovery roots, use only a literal extension declared by its hints.
        if matching.is_empty() && fallback.len() > 1 {
            fallback.retain(|(index, _)| {
                let registration = &self.registrations[*index];
                registration.descriptor.locations.iter().any(|hint| {
                    hint.storage_format == registration.representation.storage_format()
                        && Path::new(hint.session_glob)
                            .extension()
                            .is_some_and(|extension| path.extension() == Some(extension))
                })
            });
        }
        if matching.is_empty() && fallback.len() == 1 {
            matching = fallback;
        }
        if matching.len() != 1 {
            let allowed = if matching.is_empty() {
                admitted
                    .iter()
                    .map(|(_, adapter)| adapter.clone())
                    .collect()
            } else {
                matching
                    .iter()
                    .map(|(_, adapter)| adapter.clone())
                    .collect()
            };
            return Err(unsupported_source(allowed));
        }
        let (physical_path, status) = normalize_explicit_path(path);
        Ok(matching
            .into_iter()
            .map(|(registration, adapter)| Candidate {
                registration,
                adapter,
                locator: SourceLocator::LocalPath(path.to_path_buf()),
                path: Some(physical_path.clone()),
                status,
            })
            .collect())
    }

    fn registration_matches_path(&self, registration: &QueryRegistration, path: &Path) -> bool {
        registration.descriptor.locations.iter().any(|hint| {
            if hint.storage_format != registration.representation.storage_format()
                || !platform_matches(hint.platforms, &self.context.platform)
            {
                return false;
            }
            let Some(base) = self.context.bases.get(hint.base) else {
                return false;
            };
            let root = base.join(hint.path);
            let Ok(relative) = path.strip_prefix(root) else {
                return false;
            };
            compile_glob(hint.session_glob)
                .map(|matcher| matcher.is_match(relative))
                .unwrap_or(false)
        })
    }

    fn load_candidates(
        &self,
        candidates: Vec<Candidate>,
        repository_roots: Vec<PathBuf>,
        limits: &QueryLimits,
        access: ContentAccess,
        excluded_adapters: Vec<AdapterId>,
    ) -> Result<QueryInput, QueryFailure> {
        let mut sources = Vec::with_capacity(candidates.len());
        let mut observations = Vec::new();
        let mut coverage = Coverage {
            excluded_adapters,
            source_read_complete: true,
            ..Coverage::default()
        };
        let mut total_input_bytes = 0usize;
        for candidate in candidates {
            coverage.discovered_sources = coverage.discovered_sources.saturating_add(1);
            if candidate.status != SourceReadStatus::Readable {
                let source = seed_identity(&candidate, &self.registrations[candidate.registration]);
                record_unavailable(&mut coverage, &source);
                sources.push(source);
                continue;
            }
            let registration = &self.registrations[candidate.registration];
            let seed = seed_identity(&candidate, registration);
            let path = candidate
                .path
                .as_ref()
                .ok_or_else(QueryFailure::invalid_data)?;
            let (inspected, expected) = match &registration.representation {
                NativeRepresentation::Jsonl => {
                    let native = self.jsonl.read_query_snapshot(
                        &SessionRef { path: path.clone() },
                        ReadLimits {
                            max_records: limits.max_observations_and_rows,
                            max_record_bytes: limits.max_source_bytes,
                            max_batch_bytes: limits.max_source_bytes,
                        },
                    );
                    match native {
                        Ok(native) => {
                            total_input_bytes = add_input_bytes(
                                total_input_bytes,
                                native.input_bytes,
                                limits.max_total_input_bytes,
                            )?;
                            let source = SourceEvidence {
                                revision: native.revision.clone(),
                                ..seed
                            };
                            let inspected = registration.adapter.inspect(
                                NativeQueryInput::Records {
                                    source: &source,
                                    records: &native.records,
                                },
                                access.clone(),
                                limits,
                            );
                            (inspected, source)
                        }
                        Err(failure) => {
                            let source = pipeline_failure_evidence(seed, &failure);
                            record_unavailable(&mut coverage, &source);
                            coverage.issues.push(pipeline_issue(&source, &failure));
                            sources.push(source);
                            continue;
                        }
                    }
                }
                representation => {
                    let format = representation
                        .snapshot_format()
                        .ok_or_else(QueryFailure::invalid_data)?;
                    let snapshot = self.snapshot.read_snapshot(
                        &SnapshotRef {
                            path: path.clone(),
                            format,
                            session_id: None,
                        },
                        SnapshotLimits {
                            max_records: limits.max_observations_and_rows,
                            max_record_bytes: limits.max_source_bytes,
                            max_snapshot_bytes: limits.max_source_bytes,
                        },
                    );
                    match snapshot {
                        Ok(snapshot) => {
                            let bytes = snapshot
                                .records
                                .iter()
                                .try_fold(0usize, |total, record| {
                                    total
                                        .checked_add(record.key.len())
                                        .and_then(|value| value.checked_add(record.bytes.len()))
                                })
                                .ok_or_else(|| QueryFailure::limit(LimitKind::SourceBytes))?;
                            total_input_bytes = add_input_bytes(
                                total_input_bytes,
                                bytes,
                                limits.max_total_input_bytes,
                            )?;
                            let source = SourceEvidence {
                                revision: snapshot.revision.clone(),
                                ..seed
                            };
                            let inspected = registration.adapter.inspect(
                                NativeQueryInput::Snapshot {
                                    source: &source,
                                    snapshot: &snapshot,
                                },
                                access.clone(),
                                limits,
                            );
                            (inspected, source)
                        }
                        Err(failure) => {
                            let source = pipeline_failure_evidence(seed, &failure);
                            record_unavailable(&mut coverage, &source);
                            coverage.issues.push(pipeline_issue(&source, &failure));
                            sources.push(source);
                            continue;
                        }
                    }
                }
            };
            match inspected {
                Ok(inspected) => {
                    validate_inspected(&expected, &inspected, limits)?;
                    if observations
                        .len()
                        .saturating_add(inspected.observations.len())
                        > limits.max_observations_and_rows
                    {
                        return Err(QueryFailure::limit(LimitKind::ObservationsAndRows));
                    }
                    coverage.loaded_sources = coverage.loaded_sources.saturating_add(1);
                    coverage.selected_sources = coverage.selected_sources.saturating_add(1);
                    increment(&mut coverage.source_status, SourceReadStatus::Readable);
                    if inspected.source.associations.is_empty() {
                        increment(
                            &mut coverage.association_status,
                            AssociationStatus::Unassociated,
                        );
                        coverage.issues.push(AvailabilityIssue {
                            code: AvailabilityCode::Unassociated,
                            field: None,
                            source: Some(inspected.source.id),
                            entity: None,
                            offset: None,
                        });
                    }
                    coverage.issues.extend(inspected.issues.iter().cloned());
                    observations.extend(inspected.observations);
                    sources.push(inspected.source);
                }
                Err(failure) => {
                    let mut source = expected;
                    source.read_status = if matches!(
                        failure.kind(),
                        QueryFailureCode::UnsupportedSource | QueryFailureCode::UnsupportedSchema
                    ) {
                        SourceReadStatus::Unsupported
                    } else {
                        SourceReadStatus::Unreadable
                    };
                    source.revision =
                        format!("{UNAVAILABLE_REVISION_PREFIX}{}", source.read_status);
                    record_unavailable(&mut coverage, &source);
                    coverage.issues.push(AvailabilityIssue {
                        code: failure_availability(&failure),
                        field: failure.location().field,
                        source: Some(source.id),
                        entity: failure.location().entity,
                        offset: failure.location().offset,
                    });
                    sources.push(source);
                }
            }
        }
        sources.sort_by_key(|source| source.id);
        let view = NativeQueryView {
            sources,
            observations,
            repository_roots,
            coverage,
        };
        view.validate(limits)?;
        Ok(QueryInput::Native(view))
    }
}

impl QuerySource for LocalQuerySource {
    fn load(
        &self,
        scope: &QueryScope,
        selection: &SourceSelection,
        limits: &QueryLimits,
        access: ContentAccess,
    ) -> Result<QueryInput, QueryFailure> {
        validate_query_scope(scope)?;
        limits.validate()?;
        selection.validate()?;
        if let QueryScope::Offline { input } = scope {
            return self.load_offline(input, limits);
        }
        if self.context.platform.is_empty() {
            return Err(QueryFailure::new(
                QueryFailureCode::InvalidArgument,
                RecoveryAction::ReadQueryHelp,
            ));
        }
        validate_bases(&self.context.bases)?;
        let (admitted, excluded) = self.admitted_registrations(selection)?;
        match scope {
            QueryScope::Offline { .. } => unreachable!("offline input returned above"),
            QueryScope::Repository { path, scope } => {
                let roots = self.repository_roots(path, *scope, limits)?;
                let candidates = self.discover_repository(&admitted, limits)?;
                self.load_candidates(candidates, roots, limits, access, excluded)
            }
            QueryScope::Source {
                selector: SourceSelector::Path { path, adapter },
            } => {
                let candidates = self.discover_explicit_path(path, adapter.as_ref(), &admitted)?;
                self.load_candidates(candidates, Vec::new(), limits, access, excluded)
            }
            QueryScope::Source {
                selector: SourceSelector::Id(id),
            } => {
                let mut candidates = self.discover_repository(&admitted, limits)?;
                candidates.retain(|candidate| {
                    source_id(candidate, &self.registrations[candidate.registration]) == *id
                });
                if candidates.is_empty() {
                    return Err(missing_source());
                }
                self.load_candidates(candidates, Vec::new(), limits, access, excluded)
            }
        }
    }
}

#[derive(Clone)]
struct Candidate {
    registration: usize,
    adapter: AdapterId,
    locator: SourceLocator,
    path: Option<PathBuf>,
    status: SourceReadStatus,
}

impl Candidate {
    fn unavailable(
        registration: usize,
        adapter: AdapterId,
        locator: SourceLocator,
        status: SourceReadStatus,
    ) -> Self {
        Self {
            registration,
            adapter,
            locator,
            path: None,
            status,
        }
    }
}

enum WalkFailure {
    Limit,
    Read,
}

fn walk_matching(
    root: &Path,
    matcher: &GlobMatcher,
    max_entries: usize,
) -> Result<Vec<(PathBuf, SourceReadStatus)>, WalkFailure> {
    let metadata = fs::metadata(root).map_err(|_| WalkFailure::Read)?;
    if !metadata.is_dir() {
        return Err(WalkFailure::Read);
    }
    let mut stack = vec![root.to_path_buf()];
    let mut found = Vec::new();
    let mut entries_seen = 0usize;
    while let Some(directory) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&directory)
            .map_err(|_| WalkFailure::Read)?
            .collect::<Result<_, _>>()
            .map_err(|_| WalkFailure::Read)?;
        entries.sort_unstable_by_key(|entry| entry.path());
        for entry in entries.into_iter().rev() {
            entries_seen = entries_seen.checked_add(1).ok_or(WalkFailure::Limit)?;
            if entries_seen > max_entries {
                return Err(WalkFailure::Limit);
            }
            let path = entry.path();
            let relative = path.strip_prefix(root).map_err(|_| WalkFailure::Read)?;
            let kind = entry.file_type().map_err(|_| WalkFailure::Read)?;
            if kind.is_dir() {
                stack.push(path);
            } else if matcher.is_match(relative) {
                found.push((
                    path,
                    if kind.is_file() {
                        SourceReadStatus::Readable
                    } else {
                        SourceReadStatus::Unsupported
                    },
                ));
            }
        }
    }
    found.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    Ok(found)
}

fn compile_glob(pattern: &str) -> Result<GlobMatcher, globset::Error> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map(|glob| glob.compile_matcher())
}

fn platform_matches(platforms: &[&str], platform: &str) -> bool {
    platforms.iter().any(|candidate| {
        *candidate == platform
            || (*candidate == "unix" && matches!(platform, "macos" | "linux" | "unix"))
    })
}

fn validate_bases(bases: &BTreeMap<String, PathBuf>) -> Result<(), QueryFailure> {
    if bases
        .iter()
        .any(|(name, path)| name.is_empty() || !path.is_absolute() || path.to_str().is_none())
    {
        return Err(QueryFailure::new(
            QueryFailureCode::InvalidArgument,
            RecoveryAction::ReadQueryHelp,
        ));
    }
    Ok(())
}

fn push_candidate(
    candidates: &mut Vec<Candidate>,
    candidate: Candidate,
    limits: &QueryLimits,
) -> Result<(), QueryFailure> {
    if candidates.len() == limits.max_sources {
        return Err(QueryFailure::limit(LimitKind::Sources));
    }
    candidates.push(candidate);
    Ok(())
}

fn locator_bytes(locator: &SourceLocator) -> &[u8] {
    match locator {
        SourceLocator::LocalPath(path) => path.as_os_str().as_encoded_bytes(),
        SourceLocator::Provided(value) => value.as_bytes(),
    }
}

fn source_id(candidate: &Candidate, registration: &QueryRegistration) -> SourceId {
    let representation = registration.representation.name();
    // The selector remains the authorization boundary; identity uses the
    // normalized physical path so aliases do not create duplicate sources.
    let identity = candidate
        .path
        .as_deref()
        .map(|path| path.as_os_str().as_encoded_bytes())
        .unwrap_or_else(|| locator_bytes(&candidate.locator));
    SourceId::derive([
        candidate.adapter.as_str().as_bytes(),
        registration.harness.as_str().as_bytes(),
        representation.as_bytes(),
        identity,
    ])
}

fn seed_identity(candidate: &Candidate, registration: &QueryRegistration) -> SourceEvidence {
    SourceEvidence {
        id: source_id(candidate, registration),
        adapter: candidate.adapter.clone(),
        harness: registration.harness.clone(),
        representation: registration.representation.name().into_owned(),
        locator: candidate.locator.clone(),
        revision: format!("{UNAVAILABLE_REVISION_PREFIX}{}", candidate.status),
        query_policy_version: registration.query_policy_version.clone(),
        read_status: candidate.status,
        associations: Vec::new(),
        available_fields: BTreeSet::new(),
    }
}

fn pipeline_failure_evidence(
    mut source: SourceEvidence,
    failure: &PipelineError,
) -> SourceEvidence {
    source.read_status = match failure.kind() {
        PipelineErrorKind::Unsupported => SourceReadStatus::Unsupported,
        _ => SourceReadStatus::Unreadable,
    };
    source.revision = format!("{UNAVAILABLE_REVISION_PREFIX}{}", source.read_status);
    source
}

fn pipeline_issue(source: &SourceEvidence, failure: &PipelineError) -> AvailabilityIssue {
    AvailabilityIssue {
        code: match failure.kind() {
            PipelineErrorKind::Unsupported => AvailabilityCode::NotSupported,
            PipelineErrorKind::SourceChanged => AvailabilityCode::Stale,
            PipelineErrorKind::Read => AvailabilityCode::Partial,
            PipelineErrorKind::InvalidInput | PipelineErrorKind::InvalidData => {
                AvailabilityCode::NotSupported
            }
            PipelineErrorKind::RecordLimit
            | PipelineErrorKind::BatchLimit
            | PipelineErrorKind::ListingLimit
            | PipelineErrorKind::OutputLimit
            | PipelineErrorKind::Write => AvailabilityCode::Partial,
        },
        field: None,
        source: Some(source.id),
        entity: None,
        offset: failure.offset(),
    }
}

fn failure_availability(failure: &QueryFailure) -> AvailabilityCode {
    match failure.kind() {
        QueryFailureCode::UnsupportedSource | QueryFailureCode::UnsupportedSchema => {
            AvailabilityCode::NotSupported
        }
        QueryFailureCode::StaleCursor(_) => AvailabilityCode::Stale,
        QueryFailureCode::MissingSource => AvailabilityCode::Absent,
        QueryFailureCode::AmbiguousIdentity | QueryFailureCode::AmbiguousBranch => {
            AvailabilityCode::Ambiguous
        }
        _ => AvailabilityCode::Partial,
    }
}

fn validate_inspected(
    seed: &SourceEvidence,
    inspected: &InspectedSource,
    limits: &QueryLimits,
) -> Result<(), QueryFailure> {
    inspected.validate(limits)?;
    if inspected.source.id != seed.id
        || inspected.source.adapter != seed.adapter
        || inspected.source.harness != seed.harness
        || inspected.source.representation != seed.representation
        || inspected.source.locator != seed.locator
        || inspected.source.revision != seed.revision
        || inspected.source.query_policy_version != seed.query_policy_version
        || inspected.source.read_status != SourceReadStatus::Readable
    {
        return Err(QueryFailure::invalid_data());
    }
    Ok(())
}

fn record_unavailable(coverage: &mut Coverage, source: &SourceEvidence) {
    coverage.source_read_complete = false;
    increment(&mut coverage.source_status, source.read_status);
    coverage.issues.push(AvailabilityIssue {
        code: match source.read_status {
            SourceReadStatus::Absent => AvailabilityCode::Absent,
            SourceReadStatus::Unsupported => AvailabilityCode::NotSupported,
            SourceReadStatus::Unreadable | SourceReadStatus::Partial => AvailabilityCode::Partial,
            SourceReadStatus::Readable => return,
        },
        field: None,
        source: Some(source.id),
        entity: None,
        offset: None,
    });
}

fn increment<K: Ord>(counts: &mut BTreeMap<K, u64>, key: K) {
    let value = counts.entry(key).or_default();
    *value = value.saturating_add(1);
}

fn add_input_bytes(current: usize, added: usize, maximum: usize) -> Result<usize, QueryFailure> {
    let total = current
        .checked_add(added)
        .ok_or_else(|| QueryFailure::limit(LimitKind::TotalInputBytes))?;
    if total > maximum {
        return Err(QueryFailure::limit(LimitKind::TotalInputBytes));
    }
    Ok(total)
}

fn normalize_explicit_path(path: &Path) -> (PathBuf, SourceReadStatus) {
    let status = explicit_path_status(path);
    if status != SourceReadStatus::Readable {
        return (path.to_path_buf(), status);
    }
    let normalized = path
        .parent()
        .and_then(|parent| fs::canonicalize(parent).ok())
        .and_then(|parent| path.file_name().map(|name| parent.join(name)))
        .unwrap_or_else(|| path.to_path_buf());
    (normalized, status)
}

fn explicit_path_status(path: &Path) -> SourceReadStatus {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => SourceReadStatus::Readable,
        Ok(_) => SourceReadStatus::Unsupported,
        Err(failure) if failure.kind() == io::ErrorKind::NotFound => SourceReadStatus::Absent,
        Err(_) => SourceReadStatus::Unreadable,
    }
}
fn validate_query_scope(scope: &QueryScope) -> Result<(), QueryFailure> {
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
        return Err(QueryFailure::new(
            QueryFailureCode::InvalidArgument,
            RecoveryAction::ReadQueryHelp,
        ));
    }
    Ok(())
}

fn canonical_directory(path: &Path) -> Result<PathBuf, QueryFailure> {
    match fs::canonicalize(path) {
        Ok(path) if path.is_dir() => Ok(path),
        Ok(_) => Err(unreadable_source(SourceProblem::InvalidData)),
        Err(failure) if failure.kind() == io::ErrorKind::NotFound => Err(missing_source()),
        Err(failure) if failure.kind() == io::ErrorKind::PermissionDenied => {
            Err(unreadable_source(SourceProblem::Permissions))
        }
        Err(_) => Err(unreadable_source(SourceProblem::InvalidData)),
    }
}

fn read_saved_file(path: &Path, maximum: usize) -> Result<Vec<u8>, QueryFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|failure| match failure.kind() {
        io::ErrorKind::NotFound => missing_source(),
        io::ErrorKind::PermissionDenied => unreadable_source(SourceProblem::Permissions),
        _ => unreadable_source(SourceProblem::InvalidData),
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(QueryFailure::new(
            QueryFailureCode::UnsupportedSource,
            RecoveryAction::FixSource {
                reason: SourceProblem::UnsupportedDialect,
                source: None,
            },
        ));
    }
    let mut file = open_read_no_follow(path).map_err(|failure| match failure.kind() {
        io::ErrorKind::NotFound => missing_source(),
        io::ErrorKind::PermissionDenied => unreadable_source(SourceProblem::Permissions),
        _ => unreadable_source(SourceProblem::InvalidData),
    })?;
    let opened = file
        .metadata()
        .map_err(|_| unreadable_source(SourceProblem::InvalidData))?;
    if !same_file_generation(&metadata, &opened) {
        return Err(unreadable_source(SourceProblem::ChangedDuringRead));
    }
    if opened.len() > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(QueryFailure::limit(LimitKind::SourceBytes));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable_source(SourceProblem::InvalidData))?;
    if bytes.len() > maximum {
        return Err(QueryFailure::limit(LimitKind::SourceBytes));
    }
    let final_opened = file
        .metadata()
        .map_err(|_| unreadable_source(SourceProblem::ChangedDuringRead))?;
    let final_path = fs::symlink_metadata(path)
        .map_err(|_| unreadable_source(SourceProblem::ChangedDuringRead))?;
    if !same_file_generation(&opened, &final_opened) || !same_file_generation(&opened, &final_path)
    {
        return Err(unreadable_source(SourceProblem::ChangedDuringRead));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn same_file_generation(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.is_file()
        && right.is_file()
        && left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.len() == right.len()
        && left.mtime() == right.mtime()
        && left.mtime_nsec() == right.mtime_nsec()
        && left.ctime() == right.ctime()
        && left.ctime_nsec() == right.ctime_nsec()
}

#[cfg(not(unix))]
fn same_file_generation(left: &Metadata, right: &Metadata) -> bool {
    left.is_file()
        && right.is_file()
        && left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
}

#[cfg(unix)]
fn open_read_no_follow(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_read_no_follow(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

fn missing_source() -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::MissingSource,
        RecoveryAction::FixSource {
            reason: SourceProblem::Missing,
            source: None,
        },
    )
}

fn unreadable_source(reason: SourceProblem) -> QueryFailure {
    QueryFailure::new(
        QueryFailureCode::UnreadableSource,
        RecoveryAction::FixSource {
            reason,
            source: None,
        },
    )
    .retryable_after_recovery(true)
}

fn git_unavailable() -> QueryFailure {
    unreadable_source(SourceProblem::GitUnavailable)
}

fn unsupported_source(mut allowed: Vec<AdapterId>) -> QueryFailure {
    allowed.sort();
    allowed.dedup();
    QueryFailure::new(
        QueryFailureCode::UnsupportedSource,
        RecoveryAction::ChooseAdapter { allowed },
    )
}
