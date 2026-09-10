use std::{collections::BTreeMap, fs, sync::Arc};
use tempfile::tempdir;
use unisphere_core::{
    AdapterCapabilities, AdapterDescriptor, LocationHint,
    query::{
        AdapterId, ContentAccess, HarnessId, InspectedSource, NativeQueryInput, QueryAdapter,
        QueryInput, QueryLimits, QueryScope, QuerySource, RepoScope, SavedFormat, SourceReadStatus,
        SourceSelection,
    },
};
use unisphere_loader_query::{
    LocalQueryContext, LocalQuerySource, NativeRepresentation, ProvidedInput, QueryRegistration,
};

const CAPABILITIES: AdapterCapabilities = AdapterCapabilities {
    export_platforms: &["unix"],
    output_formats: &["query-json"],
    sdk_caller_owned_cursor: false,
    cursor_source_assumption: "whole_source_revision",
    cli_persisted_resume: false,
    delayed_revision_reconciliation: false,
    lossless_archive: false,
};

macro_rules! descriptor {
    ($id:literal, $path:literal, $platforms:expr) => {
        AdapterDescriptor {
            id: $id,
            application: "fixture",
            description: "fixture query source",
            locations: &[LocationHint {
                platforms: $platforms,
                base: "home",
                path: $path,
                session_glob: "*.jsonl",
                storage_format: "jsonl",
            }],
            capabilities: CAPABILITIES,
        }
    };
}

const VALID: AdapterDescriptor = descriptor!("valid", "stores/valid", &["macos", "linux"]);
const BLOCKED: AdapterDescriptor = descriptor!("blocked", "stores/blocked", &["macos", "linux"]);
const ABSENT: AdapterDescriptor = descriptor!("absent", "stores/absent", &["macos", "linux"]);
const UNREADABLE: AdapterDescriptor =
    descriptor!("unreadable", "stores/unreadable", &["macos", "linux"]);
const UNSUPPORTED: AdapterDescriptor = descriptor!("unsupported", "stores/windows", &["windows"]);

struct EchoAdapter;

impl QueryAdapter for EchoAdapter {
    fn inspect(
        &self,
        input: NativeQueryInput<'_>,
        _access: ContentAccess,
        _limits: &QueryLimits,
    ) -> Result<InspectedSource, unisphere_core::query::QueryFailure> {
        let source = match input {
            NativeQueryInput::Records { source, .. }
            | NativeQueryInput::Snapshot { source, .. }
            | NativeQueryInput::ProvidedObject { source, .. } => source.clone(),
        };
        Ok(InspectedSource {
            source,
            partitions: Vec::new(),
            observations: Vec::new(),
            issues: Vec::new(),
        })
    }
}

fn registration(descriptor: AdapterDescriptor) -> QueryRegistration {
    QueryRegistration::new(
        descriptor,
        HarnessId::new("fixture").unwrap(),
        NativeRepresentation::Jsonl,
        Arc::new(EchoAdapter),
    )
}

fn context(home: &std::path::Path) -> LocalQueryContext {
    LocalQueryContext::new(
        "macos",
        BTreeMap::from([("home".into(), home.to_path_buf())]),
    )
}

fn native(input: QueryInput) -> unisphere_core::query::NativeQueryView {
    match input {
        QueryInput::Native(view) => view,
        QueryInput::Saved { .. } => panic!("expected native input"),
    }
}

#[test]
fn explicit_source_aliases_preserve_identity_and_authorized_selector() {
    let temporary = tempdir().unwrap();
    let directory = temporary.path().join("stores/valid");
    fs::create_dir_all(directory.join("nested")).unwrap();
    fs::write(directory.join("session.jsonl"), b"{}\n").unwrap();
    fs::write(directory.join("other.jsonl"), b"{}\n").unwrap();
    let canonical = fs::canonicalize(directory.join("session.jsonl")).unwrap();
    let alias = directory.join("nested/../session.jsonl");
    let source = LocalQuerySource::new(vec![registration(VALID)], context(temporary.path()));
    let load = |path: &std::path::Path| {
        native(
            source
                .load(
                    &QueryScope::Source {
                        selector: unisphere_core::query::SourceSelector::Path {
                            path: path.to_path_buf(),
                            adapter: Some(AdapterId::new("valid").unwrap()),
                        },
                    },
                    &SourceSelection::default(),
                    &QueryLimits::default(),
                    ContentAccess::default(),
                )
                .unwrap(),
        )
    };
    let direct = load(&canonical);
    let aliased = load(&alias);
    assert_eq!(direct.sources[0].id, aliased.sources[0].id);
    assert_eq!(direct.sources[0].revision, aliased.sources[0].revision);
    assert!(matches!(
        &aliased.sources[0].locator,
        unisphere_core::query::SourceLocator::LocalPath(path) if path == &alias
    ));
    assert_ne!(
        aliased.sources[0].id,
        load(&directory.join("other.jsonl")).sources[0].id
    );
}

#[test]
fn explicit_adapter_selects_declared_document_or_journal_framing_outside_hint_roots() {
    const DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
        id: "documents",
        application: "fixture",
        description: "Two declared native framings",
        locations: &[
            LocationHint { platforms: &["macos"], base: "home", path: "store",
                session_glob: "*.json", storage_format: "json_document" },
            LocationHint { platforms: &["macos"], base: "home", path: "store",
                session_glob: "*.jsonl", storage_format: "json_journal" },
        ],
        capabilities: CAPABILITIES,
    };
    let temporary = tempdir().unwrap();
    let registrations = [NativeRepresentation::JsonDocument, NativeRepresentation::JsonJournal]
        .into_iter()
        .map(|representation| QueryRegistration::new(
            DESCRIPTOR, HarnessId::new("fixture").unwrap(), representation, Arc::new(EchoAdapter)
        )).collect();
    let source = LocalQuerySource::new(registrations, context(temporary.path()));
    for (name, bytes) in [
        ("copied.json", include_bytes!("../../adapter-vscode-copilot/tests/fixtures/session-v3.json").as_slice()),
        ("copied.jsonl", include_bytes!("../../adapter-vscode-copilot/tests/fixtures/session-journal.jsonl").as_slice()),
    ] {
        let path = temporary.path().join(name);
        fs::write(&path, bytes).unwrap();
        let view = native(source.load(
            &QueryScope::Source { selector: unisphere_core::query::SourceSelector::Path {
                path, adapter: Some(AdapterId::new("documents").unwrap()),
            }},
            &SourceSelection::default(), &QueryLimits::default(), ContentAccess::default(),
        ).expect("explicit copied source retains its declared native framing"));
        assert_eq!(view.coverage.loaded_sources, 1);
        assert_eq!(view.sources[0].read_status, SourceReadStatus::Readable);
    }
    let failure = source.load(
        &QueryScope::Source { selector: unisphere_core::query::SourceSelector::Path {
            path: temporary.path().join("ambiguous.native"),
            adapter: Some(AdapterId::new("documents").unwrap()),
        }},
        &SourceSelection::default(), &QueryLimits::default(), ContentAccess::default(),
    ).err().expect("unrecognised framing remains ambiguous before opening");
    assert_eq!(failure.kind(), unisphere_core::query::QueryFailureCode::UnsupportedSource);
}

#[test]
fn source_selection_precedes_discovery_and_io() {
    let temporary = tempdir().unwrap();
    let repository = temporary.path().join("repo");
    let valid = temporary.path().join("stores/valid");
    let blocked = temporary.path().join("stores/blocked");
    fs::create_dir_all(&repository).unwrap();
    fs::create_dir_all(&valid).unwrap();
    fs::create_dir_all(&blocked).unwrap();
    fs::write(valid.join("session.jsonl"), b"{}\n").unwrap();
    fs::write(blocked.join("oversized.jsonl"), vec![b'x'; 4096]).unwrap();

    let source = LocalQuerySource::new(
        vec![registration(VALID), registration(BLOCKED)],
        context(temporary.path()),
    );
    let mut selection = SourceSelection::default();
    selection
        .include_adapters
        .insert(AdapterId::new("valid").unwrap());
    let limits = QueryLimits {
        max_source_bytes: 64,
        max_total_input_bytes: 128,
        ..QueryLimits::default()
    };

    let view = native(
        source
            .load(
                &QueryScope::Repository {
                    path: repository,
                    scope: RepoScope::Tree,
                },
                &selection,
                &limits,
                ContentAccess::default(),
            )
            .unwrap(),
    );
    assert_eq!(view.sources.len(), 1);
    assert_eq!(view.sources[0].adapter.as_str(), "valid");
    assert_eq!(view.sources[0].read_status, SourceReadStatus::Readable);
    assert_eq!(view.coverage.discovered_sources, 1);
    assert_eq!(view.coverage.loaded_sources, 1);
    assert_eq!(
        view.coverage.excluded_adapters,
        vec![AdapterId::new("blocked").unwrap()]
    );
}

#[test]
fn discovery_keeps_absent_unreadable_unsupported_and_unassociated_distinct() {
    let temporary = tempdir().unwrap();
    let repository = temporary.path().join("repo");
    let valid = temporary.path().join("stores/valid");
    fs::create_dir_all(&repository).unwrap();
    fs::create_dir_all(&valid).unwrap();
    fs::create_dir_all(temporary.path().join("stores")).unwrap();
    fs::write(valid.join("session.jsonl"), b"{}\n").unwrap();
    fs::write(
        temporary.path().join("stores/unreadable"),
        b"not a directory",
    )
    .unwrap();

    let source = LocalQuerySource::new(
        vec![
            registration(VALID),
            registration(ABSENT),
            registration(UNREADABLE),
            registration(UNSUPPORTED),
        ],
        context(temporary.path()),
    );
    let view = native(
        source
            .load(
                &QueryScope::Repository {
                    path: repository,
                    scope: RepoScope::Exact,
                },
                &SourceSelection::default(),
                &QueryLimits::default(),
                ContentAccess::default(),
            )
            .unwrap(),
    );

    let statuses: BTreeMap<_, _> = view
        .sources
        .iter()
        .map(|source| (source.adapter.as_str(), source.read_status))
        .collect();
    assert_eq!(statuses["valid"], SourceReadStatus::Readable);
    assert_eq!(statuses["absent"], SourceReadStatus::Absent);
    assert_eq!(statuses["unreadable"], SourceReadStatus::Unreadable);
    assert_eq!(statuses["unsupported"], SourceReadStatus::Unsupported);
    assert!(!view.coverage.source_read_complete);
    assert!(
        view.coverage
            .association_status
            .contains_key(&unisphere_core::query::AssociationStatus::Unassociated)
    );
}

#[test]
fn supplied_stdin_is_bounded_and_never_opens_registered_sources() {
    let mut supplied = context(std::path::Path::new("/definitely/not/read"));
    supplied.stdin = Some(ProvidedInput {
        bytes: Arc::from(&b"{\"schema_version\":1}"[..]),
        format: SavedFormat::QueryJsonV1,
    });
    let source = LocalQuerySource::new(vec![registration(VALID)], supplied);
    let input = source
        .load(
            &QueryScope::Offline {
                input: unisphere_core::query::OfflineRef::Stdin,
            },
            &SourceSelection::default(),
            &QueryLimits::default(),
            ContentAccess::default(),
        )
        .unwrap();
    match input {
        QueryInput::Saved { bytes, format } => {
            assert_eq!(&*bytes, b"{\"schema_version\":1}");
            assert_eq!(format, SavedFormat::QueryJsonV1);
        }
        QueryInput::Native(_) => panic!("offline input must remain saved input"),
    }
}

#[cfg(unix)]
#[test]
fn worktree_scope_transports_git_verified_roots() {
    use std::os::unix::fs::PermissionsExt;

    let temporary = tempdir().unwrap();
    let repository = temporary.path().join("repo");
    let linked = temporary.path().join("linked");
    fs::create_dir_all(&repository).unwrap();
    fs::create_dir_all(&linked).unwrap();
    let git = temporary.path().join("git-fixture");
    let quote = |path: &std::path::Path| path.display().to_string().replace('\'', "'\\''");
    fs::write(
        &git,
        format!(
            "#!/bin/sh\nprintf '%s\\0' 'worktree {}' 'HEAD a' '' 'worktree {}' 'HEAD b' ''\n",
            quote(&repository),
            quote(&linked),
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&git).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&git, permissions).unwrap();

    let mut supplied = context(temporary.path());
    supplied.git_executable = Some(git);
    let source = LocalQuerySource::new(Vec::new(), supplied);
    let view = native(
        source
            .load(
                &QueryScope::Repository {
                    path: repository.clone(),
                    scope: RepoScope::Worktrees,
                },
                &SourceSelection::default(),
                &QueryLimits::default(),
                ContentAccess::default(),
            )
            .unwrap(),
    );
    assert_eq!(
        view.repository_roots,
        vec![
            fs::canonicalize(linked).unwrap(),
            fs::canonicalize(repository).unwrap(),
        ]
    );
}
