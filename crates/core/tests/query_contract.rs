use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use unisphere_core::query::*;

fn base_request() -> QueryRequest {
    QueryRequest {
        dataset: Dataset::Sessions,
        operation: Operation::List,
        scope: QueryScope::Repository {
            path: PathBuf::from("/fixtures/project"),
            scope: RepoScope::Exact,
        },
        filters: Vec::new(),
        time: TimeWindow::default(),
        branch: None,
        turn_range: None,
        sort: Vec::new(),
        columns: None,
        limit: Some(50),
        cursor: None,
        context: ContextWindow::default(),
        include_content: false,
        allow_partial: false,
        unresolved: UnresolvedPolicy::Reject,
        limits: QueryLimits::default(),
    }
}

fn assert_limit(error: QueryFailure, expected: LimitKind) {
    assert_eq!(error.kind(), QueryFailureCode::ResourceLimit);
    assert!(matches!(
        error.recovery(),
        RecoveryAction::NarrowQuery { limit } if *limit == expected
    ));
}

fn assert_request_error(request: &QueryRequest, expected: QueryFailureCode) {
    assert_eq!(request.validate().unwrap_err().kind(), expected);
}

fn assert_unsupported_operation(request: &QueryRequest, dataset: Dataset) {
    let error = request.validate().unwrap_err();
    assert_eq!(error.kind(), QueryFailureCode::UnsupportedOperation);
    assert_eq!(error.recovery(), &RecoveryAction::ConsultSchema { dataset });
}

fn view_basis() -> ViewDigestBasis {
    let source_id = SourceId::derive([&b"fixture-source"[..]]);
    let coverage = Coverage {
        discovered_sources: 1,
        loaded_sources: 1,
        selected_sources: 1,
        source_status: BTreeMap::from([(SourceReadStatus::Readable, 1)]),
        association_status: BTreeMap::from([(AssociationStatus::Matched, 1)]),
        excluded_adapters: Vec::new(),
        source_read_complete: true,
        issues: Vec::new(),
    };
    ViewDigestBasis {
        view_schema_version: 1,
        reconstruction_version: 1,
        admitted_scope: QueryScope::Repository {
            path: PathBuf::from("/fixtures/project"),
            scope: RepoScope::Exact,
        },
        admitted_repository_roots: vec![PathBuf::from("/fixtures/project")],
        source_selection: SourceSelection::default(),
        sources: vec![ViewSourceBinding {
            source_id,
            revision: "revision-a".into(),
            representation: "claude-jsonl-v1".into(),
            query_policy_version: "policy-v1".into(),
        }],
        selected_associations: Vec::new(),
        source_read_facts: SourceReadFacts::from_coverage(&coverage),
        retained: RetainedCapability {
            access: ContentAccess::default(),
            fields_by_source: BTreeMap::from([(
                source_id,
                BTreeSet::from([FieldId::Id, FieldId::SourceRefs]),
            )]),
        },
        input: ViewInputBasis::LiveNative,
    }
}
fn two_source_view_basis() -> ViewDigestBasis {
    let mut basis = view_basis();
    let second_source_id = SourceId::derive([&b"second-fixture-source"[..]]);
    basis
        .admitted_repository_roots
        .push(PathBuf::from("/fixtures/other-project"));
    basis.sources.push(ViewSourceBinding {
        source_id: second_source_id,
        revision: "revision-z".into(),
        representation: "codex-jsonl-v1".into(),
        query_policy_version: "policy-v2".into(),
    });
    basis.retained.fields_by_source.insert(
        second_source_id,
        BTreeSet::from([FieldId::Id, FieldId::SourceRefs]),
    );
    basis
}
fn native_view_with_unresolved_parent() -> NativeQueryView {
    let source_id = SourceId::derive([&b"provenance-source"[..]]);
    let revision = "revision-a".to_owned();
    let partition = PartitionId::derive(source_id, b"session-a");
    let source = SourceEvidence {
        id: source_id,
        adapter: AdapterId::new("fixture-adapter").unwrap(),
        harness: HarnessId::new("fixture-harness").unwrap(),
        representation: "fixture-jsonl-v1".into(),
        locator: SourceLocator::Provided("supplied-fixture".into()),
        revision: revision.clone(),
        query_policy_version: "policy-v1".into(),
        read_status: SourceReadStatus::Readable,
        associations: Vec::new(),
        available_fields: BTreeSet::from([FieldId::Id, FieldId::SourceRefs]),
    };
    let observation = Observation {
        source_ref: SourceRef {
            source_id,
            revision,
            locator: NativeLocator::Jsonl { offset: 0 },
            subrecord: "record-a".into(),
        },
        native_record_id: Some("record-a".into()),
        session: None,
        branch: BranchEvidence::Linear { partition },
        parent_ids: vec!["unresolved-native-parent".into()],
        sequence: NativeSequence {
            version: 1,
            key: vec![0],
        },
        timestamp: None,
        facets: vec![ObservationFacet::Control {
            kind: ControlKind::Other,
            links: Vec::new(),
        }],
        diagnostics: Vec::new(),
    };
    NativeQueryView {
        sources: vec![source],
        observations: vec![observation],
        repository_roots: vec![PathBuf::from("/fixtures/project")],
        coverage: Coverage {
            discovered_sources: 1,
            loaded_sources: 1,
            selected_sources: 1,
            source_status: BTreeMap::from([(SourceReadStatus::Readable, 1)]),
            association_status: BTreeMap::new(),
            excluded_adapters: Vec::new(),
            source_read_complete: true,
            issues: Vec::new(),
        },
    }
}
#[test]
fn identifier_hashing_frames_components_and_keeps_kinds_distinct() {
    let split_after_two = SourceId::derive([&b"ab"[..], &b"c"[..]]);
    let split_after_one = SourceId::derive([&b"a"[..], &b"bc"[..]]);
    assert_ne!(split_after_two, split_after_one);

    let session = EntityId::derive(EntityKind::Session, [&b"same-native-id"[..]]);
    let message = EntityId::derive(EntityKind::Message, [&b"same-native-id"[..]]);
    assert_ne!(session, message);

    let partition = PartitionId::derive(split_after_two, b"main");
    assert!(partition.to_string().parse::<SourceId>().is_err());
    assert!(
        format!("q1:source:{}", "A".repeat(64))
            .parse::<SourceId>()
            .is_err()
    );
}

#[test]
fn projected_fields_preserve_present_null_and_absent_as_different_states() {
    let fields = BTreeMap::from([(FieldId::Name, FieldValue::Null)]);
    let row = ProjectedRow::new(
        Dataset::Sessions,
        EntityId::derive(EntityKind::Session, [&b"session"[..]]),
        Vec::new(),
        fields,
        &ContentAccess {
            inspect_fields: BTreeSet::new(),
            emit_content: true,
        },
    )
    .unwrap();
    let value = serde_json::to_value(row).unwrap();
    let projected = value["fields"].as_object().unwrap();
    assert_eq!(projected.get("name"), Some(&serde_json::Value::Null));
    assert_eq!(projected.get("timestamp"), None);
}

#[test]
fn responses_require_semantic_actions_and_rendered_actions_require_readable_summaries() {
    let response = QueryResponse {
        schema_version: 1,
        dataset: Dataset::Sessions,
        query: QueryDescription {
            dataset: Dataset::Sessions,
            operation: OperationKind::List,
            scope_digest: Digest::of_bytes(b"action-scope"),
        },
        rows: Vec::new(),
        coverage: Coverage {
            discovered_sources: 0,
            loaded_sources: 0,
            selected_sources: 0,
            source_status: BTreeMap::new(),
            association_status: BTreeMap::new(),
            excluded_adapters: Vec::new(),
            source_read_complete: true,
            issues: Vec::new(),
        },
        universe: ResultUniverse {
            source_view_digest: None,
            selection_digest: Digest::of_bytes(b"action-selection"),
            columns_digest: Digest::of_bytes(b"action-columns"),
            columns: Vec::new(),
            applied_limit: Some(50),
            rows_complete_for_selection: Completeness::Complete,
            partitions_complete: Completeness::Complete,
            basis: UniverseBasis::LiveView,
            bounded_by_input: false,
        },
        matched: 0,
        emitted: 0,
        next_cursor: None,
        next_action: QueryAction::ReadSchema {
            dataset: Dataset::Sessions,
            reason: ActionReason::EmptySelection,
        },
    };
    let serialized = serde_json::to_value(response).unwrap();
    assert_eq!(serialized["next_action"]["kind"], "read_schema");
    assert_eq!(serialized["next_action"]["reason"], "empty_selection");

    let options = |summary: &str| QueryOutputOptions {
        format: OutputFormat::Json,
        csv_safety: CsvSafety::Spreadsheet,
        max_output_bytes: QueryLimits::default().max_output_bytes,
        next_action: RenderedAction {
            summary: summary.into(),
            argv: Vec::new(),
            required_inputs: Vec::new(),
        },
    };
    for summary in ["", " \t\n "] {
        assert_eq!(
            options(summary)
                .validate(&QueryLimits::default())
                .unwrap_err()
                .kind(),
            QueryFailureCode::InvalidData
        );
    }
    assert!(
        options("Inspect the session schema")
            .validate(&QueryLimits::default())
            .is_ok()
    );
}

#[test]
fn schemas_reject_foreign_fields_and_classify_nested_payloads_as_sensitive() {
    let messages = schema(Dataset::Messages);
    let invalid = messages
        .validate_field(FieldId::ToolName, Predicate::Equal)
        .unwrap_err();
    assert_eq!(invalid.kind(), QueryFailureCode::InvalidField);
    assert!(matches!(
        invalid.recovery(),
        RecoveryAction::ChooseField { dataset: Dataset::Messages, allowed }
            if !allowed.contains(&FieldId::ToolName)
    ));

    for (dataset, fields) in [
        (
            Dataset::Sources,
            &[
                FieldId::NativeId,
                FieldId::Association,
                FieldId::ProjectPath,
                FieldId::SourcePath,
            ][..],
        ),
        (
            Dataset::Messages,
            &[FieldId::Text, FieldId::Parts, FieldId::Model][..],
        ),
        (
            Dataset::Tools,
            &[
                FieldId::ToolName,
                FieldId::Command,
                FieldId::Input,
                FieldId::Output,
            ][..],
        ),
    ] {
        for field in fields {
            assert_eq!(
                schema(dataset).field(*field).unwrap().sensitivity,
                Sensitivity::Sensitive
            );
        }
    }
}

#[test]
fn structured_source_association_cannot_bypass_content_consent() {
    let fields = BTreeMap::from([(
        FieldId::Association,
        FieldValue::Structured(serde_json::json!({
            "private_native_metadata": "SENSITIVE-ASSOCIATION-CANARY"
        })),
    )]);
    let denied = match ProjectedRow::new(
        Dataset::Sources,
        EntityId::derive(EntityKind::Source, [&b"source"[..]]),
        Vec::new(),
        fields,
        &ContentAccess::default(),
    ) {
        Ok(_) => panic!("structured association escaped without content consent"),
        Err(error) => error,
    };
    assert_eq!(denied.kind(), QueryFailureCode::ContentConsentRequired);
    assert!(matches!(
        denied.recovery(),
        RecoveryAction::UseMetadataOrConsent { fields }
            if fields.as_slice() == [FieldId::Association]
    ));
}

#[test]
fn message_event_extract_and_default_metadata_projection_are_supported() {
    for dataset in [Dataset::Messages, Dataset::Events] {
        assert!(
            schema(dataset)
                .permitted_operations
                .contains(&OperationKind::Extract)
        );
        let mut request = base_request();
        request.dataset = dataset;
        request.operation = Operation::Extract;
        assert!(request.validate().is_ok());
    }

    let sessions = schema(Dataset::Sessions);
    assert!(!sessions.default_columns.contains(&FieldId::Name));
    assert!(
        sessions
            .default_columns
            .iter()
            .all(|field| { sessions.field(*field).unwrap().sensitivity == Sensitivity::Metadata })
    );
}

#[test]
fn requests_reject_relative_scopes_unsupported_context_and_unsupported_time() {
    for scope in [
        QueryScope::Repository {
            path: PathBuf::from("fixtures/project"),
            scope: RepoScope::Exact,
        },
        QueryScope::Source {
            selector: SourceSelector::Path {
                path: PathBuf::from("fixtures/source.jsonl"),
                adapter: None,
            },
        },
        QueryScope::Offline {
            input: OfflineRef::File(PathBuf::from("fixtures/query.json")),
        },
    ] {
        let mut request = base_request();
        request.scope = scope;
        assert_request_error(&request, QueryFailureCode::InvalidArgument);
    }

    let mut unsupported_context = base_request();
    unsupported_context.context.before = 1;
    assert_request_error(&unsupported_context, QueryFailureCode::UnsupportedOperation);

    for dataset in [Dataset::Turns, Dataset::Messages] {
        let mut supported_context = base_request();
        supported_context.dataset = dataset;
        supported_context.context.after = 1;
        assert!(supported_context.validate().is_ok());
    }

    let mut sources = base_request();
    sources.dataset = Dataset::Sources;
    sources.time.include_undated = true;
    assert!(sources.validate().is_ok());
    sources.time.since = Some(Timestamp::parse("2026-09-01", TimestampBasis::Native).unwrap());
    assert_unsupported_operation(&sources, Dataset::Sources);

    let mut sessions = base_request();
    sessions.time.since = Some(Timestamp::parse("2026-09-01", TimestampBasis::Native).unwrap());
    assert!(sessions.validate().is_ok());

    for invalid_field in [FieldId::Name, FieldId::Timestamp] {
        let mut invalid = base_request();
        invalid.time.field = Some(invalid_field);
        assert_unsupported_operation(&invalid, Dataset::Sessions);
    }

    let mut alternate_time = base_request();
    alternate_time.time.field = Some(FieldId::FirstEventAt);
    assert!(alternate_time.validate().is_ok());
}

#[cfg(unix)]
#[test]
fn requests_reject_non_utf8_path_scopes() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let non_utf8 = PathBuf::from(OsString::from_vec(vec![b'/', 0xff]));
    for scope in [
        QueryScope::Repository {
            path: non_utf8.clone(),
            scope: RepoScope::Exact,
        },
        QueryScope::Source {
            selector: SourceSelector::Path {
                path: non_utf8.clone(),
                adapter: None,
            },
        },
        QueryScope::Offline {
            input: OfflineRef::File(non_utf8.clone()),
        },
    ] {
        let mut request = base_request();
        request.scope = scope;
        assert_request_error(&request, QueryFailureCode::InvalidArgument);
    }
}

#[test]
fn turn_ranges_require_turns_and_one_exact_session_but_not_a_branch() {
    let mut malformed = base_request();
    malformed.turn_range = Some(InclusiveRange { start: 0, end: 1 });
    assert_request_error(&malformed, QueryFailureCode::InvalidArgument);

    let range = InclusiveRange::new(2, 4).unwrap();
    let mut wrong_dataset = base_request();
    wrong_dataset.turn_range = Some(range);
    assert_request_error(&wrong_dataset, QueryFailureCode::InvalidArgument);

    let mut unscoped_turns = base_request();
    unscoped_turns.dataset = Dataset::Turns;
    unscoped_turns.operation = Operation::Extract;
    unscoped_turns.turn_range = Some(range);
    assert_request_error(&unscoped_turns, QueryFailureCode::InvalidArgument);

    let session = EntityId::derive(EntityKind::Session, [&b"session-a"[..]]);
    let branch = EntityId::derive(EntityKind::Branch, [&b"branch-a"[..]]);
    let mut filtered = base_request();
    filtered.dataset = Dataset::Turns;
    filtered.operation = Operation::Extract;
    filtered.turn_range = Some(range);
    filtered.filters = vec![Filter {
        field: FieldId::SessionId,
        predicate: Predicate::Equal,
        values: vec![FieldValue::Id(session)],
        ignore_case: false,
    }];
    assert!(filtered.validate().is_ok());

    let mut with_branch = filtered.clone();
    with_branch.branch = Some(branch);
    assert!(with_branch.validate().is_ok());

    let mut via_in = filtered.clone();
    via_in.filters[0].predicate = Predicate::In;
    assert!(via_in.validate().is_ok());

    let mut multiple_sessions = filtered.clone();
    multiple_sessions.filters[0]
        .values
        .push(FieldValue::Id(EntityId::derive(
            EntityKind::Session,
            [&b"session-b"[..]],
        )));
    assert_request_error(&multiple_sessions, QueryFailureCode::InvalidArgument);

    let mut wrong_session_kind = filtered.clone();
    wrong_session_kind.filters[0].values = vec![FieldValue::Id(EntityId::derive(
        EntityKind::Message,
        [&b"message-a"[..]],
    ))];
    assert_request_error(&wrong_session_kind, QueryFailureCode::InvalidArgument);

    let mut non_id_session = filtered.clone();
    non_id_session.filters[0].values = vec![FieldValue::String("session-a".into())];
    assert_request_error(&non_id_session, QueryFailureCode::InvalidArgument);

    let mut wrong_operation = filtered;
    wrong_operation.operation = Operation::Check {
        source: SourceId::derive([&b"source-a"[..]]),
    };
    assert_unsupported_operation(&wrong_operation, Dataset::Turns);
}

#[test]
fn requests_reject_wrong_operation_and_branch_entity_kinds() {
    let message = EntityId::derive(EntityKind::Message, [&b"message"[..]]);

    let mut wrong_show = base_request();
    wrong_show.operation = Operation::Show { entity: message };
    assert_request_error(&wrong_show, QueryFailureCode::InvalidArgument);

    let mut wrong_tree = base_request();
    wrong_tree.operation = Operation::Tree { session: message };
    assert_request_error(&wrong_tree, QueryFailureCode::InvalidArgument);

    let mut wrong_branch = base_request();
    wrong_branch.branch = Some(message);
    assert_request_error(&wrong_branch, QueryFailureCode::InvalidArgument);
}

#[test]
fn sensitive_search_access_does_not_authorize_projected_content() {
    let mut request = base_request();
    request.dataset = Dataset::Messages;
    request.columns = Some(vec![FieldId::Text]);
    let denied_request = request.validate().unwrap_err();
    assert_eq!(
        denied_request.kind(),
        QueryFailureCode::ContentConsentRequired
    );
    assert!(matches!(
        denied_request.recovery(),
        RecoveryAction::UseMetadataOrConsent { fields }
            if fields.as_slice() == [FieldId::Text]
    ));

    let mut inspected_only = ContentAccess::default();
    inspected_only.inspect_fields.insert(FieldId::Text);
    assert!(inspected_only.permits_payload(FieldId::Text));
    let fields = BTreeMap::from([(
        FieldId::Text,
        FieldValue::String("SENSITIVE-PROJECTION-CANARY".into()),
    )]);
    let denied_row = match ProjectedRow::new(
        Dataset::Messages,
        EntityId::derive(EntityKind::Message, [&b"message"[..]]),
        Vec::new(),
        fields,
        &inspected_only,
    ) {
        Ok(_) => panic!("inspected content escaped into a projected row"),
        Err(error) => error,
    };
    assert_eq!(denied_row.kind(), QueryFailureCode::ContentConsentRequired);
    assert!(matches!(
        denied_row.recovery(),
        RecoveryAction::UseMetadataOrConsent { fields }
            if fields.as_slice() == [FieldId::Text]
    ));
}
#[test]
fn zero_over_ceiling_impossible_and_overflowing_limits_are_rejected() {
    let zero = QueryLimits {
        max_sources: 0,
        ..QueryLimits::default()
    };
    assert_limit(zero.validate().unwrap_err(), LimitKind::Sources);

    let over_ceiling = QueryLimits {
        max_sources: QueryLimits::HARD.max_sources + 1,
        ..QueryLimits::default()
    };
    assert_limit(over_ceiling.validate().unwrap_err(), LimitKind::Sources);

    let impossible = QueryLimits {
        max_total_input_bytes: 1,
        max_source_bytes: 2,
        ..QueryLimits::default()
    };
    assert_limit(impossible.validate().unwrap_err(), LimitKind::SourceBytes);

    let mut request = base_request();
    request.context = ContextWindow {
        before: usize::MAX,
        after: 1,
    };
    assert_limit(
        request.validate().unwrap_err(),
        LimitKind::ContextNeighbours,
    );
}

#[test]
fn cursor_binding_checks_query_before_view_and_preserves_typed_recovery() {
    let expected_query = Digest::of_bytes(b"expected-query");
    let expected_view = Digest::of_bytes(b"expected-view");
    let wrong_query = Digest::of_bytes(b"wrong-query");
    let wrong_view = Digest::of_bytes(b"wrong-view");

    let both_changed = CursorBinding {
        query_digest: wrong_query,
        view_digest: wrong_view,
    }
    .validate(expected_query, expected_view)
    .unwrap_err();
    assert_eq!(
        both_changed.cursor_reason(),
        Some(CursorMismatchReason::QueryOptionsChanged)
    );
    assert_eq!(both_changed.recovery(), &RecoveryAction::StartFreshQuery);

    let source_changed = CursorBinding {
        query_digest: expected_query,
        view_digest: wrong_view,
    }
    .validate(expected_query, expected_view)
    .unwrap_err();
    assert_eq!(
        source_changed.cursor_reason(),
        Some(CursorMismatchReason::SourceViewChanged)
    );
    assert_eq!(source_changed.recovery(), &RecoveryAction::ReopenView);
}

#[test]
fn query_binding_normalizes_defaults_filters_and_time_field() {
    let selection = SourceSelection::default();
    let implicit_time_field = base_request();
    let mut explicit_default_time_field = implicit_time_field.clone();
    explicit_default_time_field.time.field = Some(FieldId::StartedAt);
    assert_eq!(
        query_binding_digest(&implicit_time_field, &selection).unwrap(),
        query_binding_digest(&explicit_default_time_field, &selection).unwrap()
    );

    let mut alternate_time_field = implicit_time_field.clone();
    alternate_time_field.time.field = Some(FieldId::FirstEventAt);
    assert_ne!(
        query_binding_digest(&implicit_time_field, &selection).unwrap(),
        query_binding_digest(&alternate_time_field, &selection).unwrap()
    );

    let mut implicit_default = base_request();
    implicit_default.dataset = Dataset::Messages;
    implicit_default.limit = None;

    let mut explicit_default = implicit_default.clone();
    explicit_default.limit = Some(50);
    assert_eq!(
        query_binding_digest(&implicit_default, &selection).unwrap(),
        query_binding_digest(&explicit_default, &selection).unwrap()
    );

    let mut first_order = explicit_default.clone();
    first_order.filters = vec![
        Filter {
            field: FieldId::Role,
            predicate: Predicate::Equal,
            values: vec![FieldValue::String("user".into())],
            ignore_case: false,
        },
        Filter {
            field: FieldId::Role,
            predicate: Predicate::Equal,
            values: vec![FieldValue::String("assistant".into())],
            ignore_case: false,
        },
    ];
    let mut second_order = first_order.clone();
    second_order.filters.reverse();
    assert_eq!(
        query_binding_digest(&first_order, &selection).unwrap(),
        query_binding_digest(&second_order, &selection).unwrap()
    );

    let mut different_limit = explicit_default;
    different_limit.limit = Some(5);
    assert_ne!(
        query_binding_digest(&implicit_default, &selection).unwrap(),
        query_binding_digest(&different_limit, &selection).unwrap()
    );
}

#[test]
fn inspected_source_rejects_foreign_source_and_revision_observations() {
    let view = native_view_with_unresolved_parent();
    let valid = InspectedSource {
        source: view.sources[0].clone(),
        partitions: Vec::new(),
        observations: view.observations,
        issues: Vec::new(),
    };
    assert!(valid.validate(&QueryLimits::default()).is_ok());

    let mut foreign_source = valid.clone();
    foreign_source.observations[0].source_ref.source_id =
        SourceId::derive([&b"foreign-observation-source"[..]]);
    assert_eq!(
        foreign_source
            .validate(&QueryLimits::default())
            .unwrap_err()
            .kind(),
        QueryFailureCode::InvalidData
    );

    let mut wrong_revision = valid;
    wrong_revision.observations[0].source_ref.revision = "revision-b".into();
    assert_eq!(
        wrong_revision
            .validate(&QueryLimits::default())
            .unwrap_err()
            .kind(),
        QueryFailureCode::InvalidData
    );
}

#[test]
fn native_view_rejects_orphan_and_revision_mismatch_but_allows_native_parent_ids() {
    let valid = native_view_with_unresolved_parent();
    assert!(valid.validate(&QueryLimits::default()).is_ok());

    let mut orphan = valid.clone();
    orphan.observations[0].source_ref.source_id =
        SourceId::derive([&b"orphan-observation-source"[..]]);
    assert_eq!(
        orphan.validate(&QueryLimits::default()).unwrap_err().kind(),
        QueryFailureCode::InvalidData
    );

    let mut wrong_revision = valid;
    wrong_revision.observations[0].source_ref.revision = "revision-b".into();
    assert_eq!(
        wrong_revision
            .validate(&QueryLimits::default())
            .unwrap_err()
            .kind(),
        QueryFailureCode::InvalidData
    );
}

#[test]
fn view_digest_canonicalizes_input_order_and_binds_source_and_retention() {
    let basis = two_source_view_basis();
    let original = basis.digest().unwrap();

    let mut roots_permuted = basis.clone();
    roots_permuted.admitted_repository_roots.reverse();
    assert_eq!(original, roots_permuted.digest().unwrap());

    let mut sources_permuted = basis.clone();
    sources_permuted.sources.reverse();
    assert_eq!(original, sources_permuted.digest().unwrap());

    let mut changed_revision = basis.clone();
    changed_revision.sources[0].revision = "revision-b".into();
    assert_ne!(original, changed_revision.digest().unwrap());

    let mut changed_selection = basis.clone();
    changed_selection
        .source_selection
        .include_adapters
        .insert(AdapterId::new("claude-jsonl").unwrap());
    assert_ne!(original, changed_selection.digest().unwrap());

    let mut changed_retention = basis.clone();
    changed_retention
        .retained
        .fields_by_source
        .values_mut()
        .next()
        .unwrap()
        .insert(FieldId::Text);
    assert_ne!(original, changed_retention.digest().unwrap());
}

#[test]
fn view_digest_binds_saved_input_origin_and_completeness() {
    let mut saved = view_basis();
    saved.input = ViewInputBasis::Saved {
        format: SavedFormat::QueryJsonV1,
        input_sha256: Digest::of_bytes(b"saved-input-a"),
        rows_complete: Completeness::Complete,
        partitions_complete: Completeness::Complete,
    };
    let original = saved.digest().unwrap();

    let mut changed_bytes = saved.clone();
    if let ViewInputBasis::Saved { input_sha256, .. } = &mut changed_bytes.input {
        *input_sha256 = Digest::of_bytes(b"saved-input-b");
    }
    assert_ne!(original, changed_bytes.digest().unwrap());

    let mut changed_format = saved.clone();
    if let ViewInputBasis::Saved { format, .. } = &mut changed_format.input {
        *format = SavedFormat::QueryJsonlV1;
    }
    assert_ne!(original, changed_format.digest().unwrap());

    let mut changed_rows = saved.clone();
    if let ViewInputBasis::Saved { rows_complete, .. } = &mut changed_rows.input {
        *rows_complete = Completeness::Subset;
    }
    assert_ne!(original, changed_rows.digest().unwrap());

    let mut changed_partitions = saved;
    if let ViewInputBasis::Saved {
        partitions_complete,
        ..
    } = &mut changed_partitions.input
    {
        *partitions_complete = Completeness::Unknown;
    }
    assert_ne!(original, changed_partitions.digest().unwrap());
}
