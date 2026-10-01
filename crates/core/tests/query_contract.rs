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

fn empty_response(dataset: Dataset, operation: OperationKind) -> QueryResponse {
    QueryResponse {
        schema_version: 1,
        dataset,
        query: QueryDescription {
            dataset,
            operation,
            scope_digest: Digest::of_bytes(b"response-scope"),
        },
        rows: Vec::new(),
        coverage: Coverage::default(),
        universe: ResultUniverse {
            source_view_digest: None,
            selection_digest: Digest::of_bytes(b"response-selection"),
            columns_digest: Digest::of_bytes(b"response-columns"),
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
            dataset,
            reason: ActionReason::EmptySelection,
        },
    }
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
    let response = empty_response(Dataset::Sessions, OperationKind::List);
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

#[test]
fn every_dataset_projects_common_metadata_and_protects_native_ids() {
    let common_fields = [
        FieldId::NativeId,
        FieldId::Harness,
        FieldId::Adapter,
        FieldId::Availability,
    ];
    let source_schema = schema(Dataset::Sources);
    for &dataset in Dataset::ALL {
        for field in common_fields {
            assert_eq!(
                schema(dataset).field(field),
                source_schema.field(field),
                "{dataset} must preserve the common field contract for {field}"
            );
        }
        assert!(schema(dataset).default_columns.iter().all(|field| {
            schema(dataset).field(*field).unwrap().sensitivity == Sensitivity::Metadata
        }));
    }

    for (dataset, kind) in [
        (Dataset::Sources, EntityKind::Source),
        (Dataset::Sessions, EntityKind::Session),
        (Dataset::Turns, EntityKind::Turn),
        (Dataset::Messages, EntityKind::Message),
        (Dataset::Tools, EntityKind::Tool),
        (Dataset::Events, EntityKind::Event),
    ] {
        let metadata = BTreeMap::from([
            (FieldId::Harness, FieldValue::String("omp".into())),
            (
                FieldId::Adapter,
                FieldValue::String("fixture-adapter".into()),
            ),
            (
                FieldId::Availability,
                FieldValue::Strings(vec!["not_captured".into()]),
            ),
        ]);
        let id = EntityId::derive(kind, [dataset.as_str().as_bytes()]);
        assert!(
            ProjectedRow::new(
                dataset,
                id,
                Vec::new(),
                metadata.clone(),
                &ContentAccess::default(),
            )
            .is_ok()
        );

        let mut with_native_id = metadata;
        with_native_id.insert(
            FieldId::NativeId,
            FieldValue::String("sensitive-native-id".into()),
        );
        let denied = match ProjectedRow::new(
            dataset,
            id,
            Vec::new(),
            with_native_id,
            &ContentAccess::default(),
        ) {
            Ok(_) => panic!("native ID escaped without content consent for {dataset}"),
            Err(error) => error,
        };
        assert_eq!(denied.kind(), QueryFailureCode::ContentConsentRequired);
    }
}

#[test]
fn context_marker_is_non_filterable_metadata_and_round_trips() {
    for (dataset, kind) in [
        (Dataset::Turns, EntityKind::Turn),
        (Dataset::Messages, EntityKind::Message),
    ] {
        let dataset_schema = schema(dataset);
        let marker = dataset_schema.field(FieldId::IsContext).unwrap();
        assert_eq!(marker.field_type, FieldType::Bool);
        assert!(!marker.nullable);
        assert_eq!(marker.unit, None);
        assert_eq!(marker.sensitivity, Sensitivity::Metadata);
        assert_eq!(marker.availability, Availability::ProjectedOptional);
        assert!(marker.allowed_predicates.is_empty());
        assert!(!dataset_schema.default_columns.contains(&FieldId::IsContext));

        let mut circular_filter = base_request();
        circular_filter.dataset = dataset;
        circular_filter.filters = vec![Filter {
            field: FieldId::IsContext,
            predicate: Predicate::Equal,
            values: vec![FieldValue::Bool(true)],
            ignore_case: false,
        }];
        let error = circular_filter.validate().unwrap_err();
        assert_eq!(error.kind(), QueryFailureCode::UnsupportedOperation);
        assert_eq!(error.recovery(), &RecoveryAction::ConsultSchema { dataset });

        for is_context in [false, true] {
            let row = ProjectedRow::new(
                dataset,
                EntityId::derive(kind, [dataset.as_str().as_bytes(), &[u8::from(is_context)]]),
                Vec::new(),
                BTreeMap::from([(FieldId::IsContext, FieldValue::Bool(is_context))]),
                &ContentAccess::default(),
            )
            .unwrap();
            let wire = serde_json::to_value(&row).unwrap();
            assert_eq!(wire["fields"]["is_context"], is_context);

            let untrusted: UntrustedProjectedRow = serde_json::from_value(wire).unwrap();
            let restored =
                ProjectedRow::from_untrusted(untrusted, &ContentAccess::default()).unwrap();
            assert!(matches!(
                restored.field(FieldId::IsContext),
                Some(FieldValue::Bool(value)) if *value == is_context
            ));
        }
    }

    for dataset in [
        Dataset::Sources,
        Dataset::Sessions,
        Dataset::Tools,
        Dataset::Events,
    ] {
        assert!(schema(dataset).field(FieldId::IsContext).is_none());
    }
}

#[test]
fn declared_statistics_are_real_projectable_fields_with_units_and_null_boundaries() {
    struct MetricContract {
        metric: Metric,
        field: FieldId,
        field_type: FieldType,
        nullable: bool,
        unit: FieldUnit,
        sample: FieldValue,
    }

    let contracts = [
        MetricContract {
            metric: Metric::Count,
            field: FieldId::Count,
            field_type: FieldType::U64,
            nullable: false,
            unit: FieldUnit::Count,
            sample: FieldValue::Unsigned(3),
        },
        MetricContract {
            metric: Metric::MeasuredCount,
            field: FieldId::MeasuredCount,
            field_type: FieldType::U64,
            nullable: false,
            unit: FieldUnit::Count,
            sample: FieldValue::Unsigned(2),
        },
        MetricContract {
            metric: Metric::MissingDurationCount,
            field: FieldId::MissingDurationCount,
            field_type: FieldType::U64,
            nullable: false,
            unit: FieldUnit::Count,
            sample: FieldValue::Unsigned(1),
        },
        MetricContract {
            metric: Metric::Succeeded,
            field: FieldId::Succeeded,
            field_type: FieldType::U64,
            nullable: false,
            unit: FieldUnit::Count,
            sample: FieldValue::Unsigned(1),
        },
        MetricContract {
            metric: Metric::Failures,
            field: FieldId::Failures,
            field_type: FieldType::U64,
            nullable: false,
            unit: FieldUnit::Count,
            sample: FieldValue::Unsigned(1),
        },
        MetricContract {
            metric: Metric::Cancelled,
            field: FieldId::Cancelled,
            field_type: FieldType::U64,
            nullable: false,
            unit: FieldUnit::Count,
            sample: FieldValue::Unsigned(0),
        },
        MetricContract {
            metric: Metric::Incomplete,
            field: FieldId::Incomplete,
            field_type: FieldType::U64,
            nullable: false,
            unit: FieldUnit::Count,
            sample: FieldValue::Unsigned(0),
        },
        MetricContract {
            metric: Metric::Unknown,
            field: FieldId::Unknown,
            field_type: FieldType::U64,
            nullable: false,
            unit: FieldUnit::Count,
            sample: FieldValue::Unsigned(1),
        },
        MetricContract {
            metric: Metric::FailureRate,
            field: FieldId::FailureRate,
            field_type: FieldType::FiniteF64,
            nullable: true,
            unit: FieldUnit::Ratio,
            sample: FieldValue::Float(0.5),
        },
        MetricContract {
            metric: Metric::MeanMs,
            field: FieldId::MeanMs,
            field_type: FieldType::FiniteF64,
            nullable: true,
            unit: FieldUnit::Milliseconds,
            sample: FieldValue::Float(10.0),
        },
        MetricContract {
            metric: Metric::MinMs,
            field: FieldId::MinMs,
            field_type: FieldType::FiniteF64,
            nullable: true,
            unit: FieldUnit::Milliseconds,
            sample: FieldValue::Float(5.0),
        },
        MetricContract {
            metric: Metric::MaxMs,
            field: FieldId::MaxMs,
            field_type: FieldType::FiniteF64,
            nullable: true,
            unit: FieldUnit::Milliseconds,
            sample: FieldValue::Float(15.0),
        },
        MetricContract {
            metric: Metric::P50Ms,
            field: FieldId::P50Ms,
            field_type: FieldType::FiniteF64,
            nullable: true,
            unit: FieldUnit::Milliseconds,
            sample: FieldValue::Float(10.0),
        },
        MetricContract {
            metric: Metric::P95Ms,
            field: FieldId::P95Ms,
            field_type: FieldType::FiniteF64,
            nullable: true,
            unit: FieldUnit::Milliseconds,
            sample: FieldValue::Float(14.0),
        },
        MetricContract {
            metric: Metric::InputTokens,
            field: FieldId::InputTokens,
            field_type: FieldType::U64,
            nullable: true,
            unit: FieldUnit::Tokens,
            sample: FieldValue::Unsigned(100),
        },
        MetricContract {
            metric: Metric::OutputTokens,
            field: FieldId::OutputTokens,
            field_type: FieldType::U64,
            nullable: true,
            unit: FieldUnit::Tokens,
            sample: FieldValue::Unsigned(50),
        },
        MetricContract {
            metric: Metric::CacheReadTokens,
            field: FieldId::CacheReadTokens,
            field_type: FieldType::U64,
            nullable: true,
            unit: FieldUnit::Tokens,
            sample: FieldValue::Unsigned(25),
        },
        MetricContract {
            metric: Metric::CacheWriteTokens,
            field: FieldId::CacheWriteTokens,
            field_type: FieldType::U64,
            nullable: true,
            unit: FieldUnit::Tokens,
            sample: FieldValue::Unsigned(10),
        },
    ];

    for dataset in [Dataset::Sessions, Dataset::Turns, Dataset::Tools] {
        let dataset_schema = schema(dataset);
        let mut projected = BTreeMap::new();
        for &metric in dataset_schema.metrics {
            let contract = contracts
                .iter()
                .find(|contract| contract.metric == metric)
                .unwrap();
            let field = dataset_schema.field(contract.field).unwrap();
            assert_eq!(field.field_type, contract.field_type);
            assert_eq!(field.nullable, contract.nullable);
            assert_eq!(field.unit, Some(contract.unit));
            assert_eq!(field.sensitivity, Sensitivity::Metadata);
            let availability = match contract.metric {
                Metric::InputTokens
                | Metric::OutputTokens
                | Metric::CacheReadTokens
                | Metric::CacheWriteTokens => Availability::SourceQualified,
                _ => Availability::ProjectedOptional,
            };
            assert_eq!(field.availability, availability);
            assert!(field.allowed_predicates.is_empty());
            projected.insert(contract.field, contract.sample.clone());
        }

        let group_id = EntityId::derive(EntityKind::Group, [dataset.as_str().as_bytes()]);
        let row = ProjectedRow::new(
            dataset,
            group_id,
            Vec::new(),
            projected,
            &ContentAccess::default(),
        )
        .unwrap();
        for &metric in dataset_schema.metrics {
            let contract = contracts
                .iter()
                .find(|contract| contract.metric == metric)
                .unwrap();
            assert!(row.field(contract.field) == Some(&contract.sample));
        }

        let nullable = contracts
            .iter()
            .filter(|contract| {
                contract.nullable && dataset_schema.metrics.contains(&contract.metric)
            })
            .map(|contract| (contract.field, FieldValue::Null))
            .collect();
        let nullable_row = ProjectedRow::new(
            dataset,
            group_id,
            Vec::new(),
            nullable,
            &ContentAccess::default(),
        )
        .unwrap();
        for contract in contracts.iter().filter(|contract| {
            contract.nullable && dataset_schema.metrics.contains(&contract.metric)
        }) {
            assert!(matches!(
                nullable_row.field(contract.field),
                Some(FieldValue::Null)
            ));
        }

        for contract in contracts.iter().filter(|contract| {
            !contract.nullable && dataset_schema.metrics.contains(&contract.metric)
        }) {
            let error = match ProjectedRow::new(
                dataset,
                group_id,
                Vec::new(),
                BTreeMap::from([(contract.field, FieldValue::Null)]),
                &ContentAccess::default(),
            ) {
                Ok(_) => panic!("non-null statistic accepted null for {}", contract.field),
                Err(error) => error,
            };
            assert_eq!(error.kind(), QueryFailureCode::InvalidData);
        }
    }

    let non_finite = match ProjectedRow::new(
        Dataset::Tools,
        EntityId::derive(EntityKind::Group, [&b"non-finite"[..]]),
        Vec::new(),
        BTreeMap::from([(FieldId::FailureRate, FieldValue::Float(f64::NAN))]),
        &ContentAccess::default(),
    ) {
        Ok(_) => panic!("non-finite failure rate entered a projected statistics row"),
        Err(error) => error,
    };
    assert_eq!(non_finite.kind(), QueryFailureCode::InvalidData);

    for (dataset, kind) in [
        (Dataset::Sources, EntityKind::Source),
        (Dataset::Messages, EntityKind::Message),
        (Dataset::Events, EntityKind::Event),
    ] {
        let dataset_schema = schema(dataset);
        for contract in &contracts {
            assert!(dataset_schema.field(contract.field).is_none());
            let error = match ProjectedRow::new(
                dataset,
                EntityId::derive(
                    kind,
                    [
                        dataset.as_str().as_bytes(),
                        contract.field.as_str().as_bytes(),
                    ],
                ),
                Vec::new(),
                BTreeMap::from([(contract.field, contract.sample.clone())]),
                &ContentAccess::default(),
            ) {
                Ok(_) => panic!(
                    "non-statistics dataset {dataset} accepted statistic field {}",
                    contract.field
                ),
                Err(error) => error,
            };
            assert_eq!(error.kind(), QueryFailureCode::InvalidField);
        }
    }
}

#[test]
fn non_stats_datasets_declare_no_grouping_fields() {
    for dataset in [Dataset::Sources, Dataset::Messages, Dataset::Events] {
        let dataset_schema = schema(dataset);
        assert!(
            !dataset_schema
                .permitted_operations
                .contains(&OperationKind::Stats)
        );
        assert!(dataset_schema.grouping_fields.is_empty());
        assert!(dataset_schema.metrics.is_empty());
        let mut request = base_request();
        request.dataset = dataset;
        request.operation = Operation::Stats {
            group_by: vec![FieldId::Harness],
            metrics: vec![Metric::Count],
        };
        let error = request.validate().unwrap_err();
        assert_eq!(error.kind(), QueryFailureCode::UnsupportedOperation);
        assert_eq!(error.recovery(), &RecoveryAction::ConsultSchema { dataset });
    }
}

#[test]
fn source_selection_applies_or_within_groups_and_across_group_exclusions() {
    let claude = AdapterId::new("claude-jsonl").unwrap();
    let codex = AdapterId::new("codex-jsonl").unwrap();
    let cursor = AdapterId::new("cursor-sqlite").unwrap();
    let omp = HarnessId::new("omp").unwrap();
    let vscode = HarnessId::new("vscode").unwrap();

    let all = SourceSelection::default();
    assert!(all.admits(&claude, &omp));
    assert!(all.admits(&cursor, &vscode));

    let mut selected = SourceSelection {
        include_adapters: BTreeSet::from([claude.clone(), codex.clone()]),
        include_harnesses: BTreeSet::from([omp.clone()]),
        ..SourceSelection::default()
    };
    assert!(selected.admits(&claude, &omp));
    assert!(selected.admits(&codex, &omp));
    assert!(!selected.admits(&cursor, &omp));
    assert!(!selected.admits(&claude, &vscode));

    selected.exclude_adapters.insert(claude.clone());
    assert!(!selected.admits(&claude, &omp));
    assert!(selected.admits(&codex, &omp));
    selected.exclude_harnesses.insert(omp.clone());
    assert!(!selected.admits(&codex, &omp));

    let conflict = selected.validate().unwrap_err();
    assert_eq!(conflict.kind(), QueryFailureCode::InvalidArgument);
    assert_eq!(conflict.recovery(), &RecoveryAction::ReadQueryHelp);
}

#[test]
fn view_digest_matches_closed_preimage_vector_and_repeats_stably() {
    let basis = view_basis();
    let first = basis.digest().unwrap();
    assert_eq!(
        first.to_string(),
        "23c71eb09affec419f8bf44180fcb9b975d9ead38351be921688a1b904d0ae35"
    );
    assert_eq!(first, basis.digest().unwrap());
}

#[test]
fn query_binding_changes_when_columns_change() {
    let selection = SourceSelection::default();
    let mut narrow = base_request();
    narrow.columns = Some(vec![FieldId::Id]);
    narrow.limit = Some(5);
    let mut wide = narrow.clone();
    wide.columns = Some(vec![FieldId::Id, FieldId::StartedAt]);

    assert_ne!(
        query_binding_digest(&narrow, &selection).unwrap(),
        query_binding_digest(&wide, &selection).unwrap()
    );
}

#[test]
fn untrusted_rows_reject_schema_drift_and_convert_supplied_timestamps() {
    let timestamp = "2026-09-01T12:00:00Z";
    let row = UntrustedProjectedRow {
        schema_version: 1,
        dataset: Dataset::Sessions,
        id: EntityId::derive(EntityKind::Session, [&b"untrusted-session"[..]]),
        source_refs: Vec::new(),
        fields: BTreeMap::from([(
            FieldId::StartedAt,
            serde_json::Value::String(timestamp.into()),
        )]),
    };
    let projected = ProjectedRow::from_untrusted(row.clone(), &ContentAccess::default()).unwrap();
    assert!(matches!(
        projected.field(FieldId::StartedAt),
        Some(FieldValue::Timestamp(value)) if value.basis() == TimestampBasis::SuppliedUnknown
    ));

    let mut wrong_schema = row.clone();
    wrong_schema.schema_version = 2;
    let unsupported = match ProjectedRow::from_untrusted(wrong_schema, &ContentAccess::default()) {
        Ok(_) => panic!("unsupported row schema was accepted"),
        Err(error) => error,
    };
    assert_eq!(unsupported.kind(), QueryFailureCode::UnsupportedSchema);
    assert_eq!(
        unsupported.recovery(),
        &RecoveryAction::ConsultSchema {
            dataset: Dataset::Sessions
        }
    );

    let mut foreign_field = row;
    foreign_field.fields =
        BTreeMap::from([(FieldId::ToolName, serde_json::Value::String("tool".into()))]);
    assert_eq!(
        match ProjectedRow::from_untrusted(foreign_field, &ContentAccess::default()) {
            Ok(_) => panic!("foreign field entered a session row"),
            Err(error) => error,
        }
        .kind(),
        QueryFailureCode::InvalidField
    );
}

#[test]
fn scalar_timestamp_round_trip_records_unknown_supplied_basis_and_declared_loss() {
    let source_reported =
        Timestamp::parse("2026-09-01T12:00:00Z", TimestampBasis::SourceReported).unwrap();
    let wire = serde_json::to_value(&source_reported).unwrap();
    assert_eq!(
        wire,
        serde_json::Value::String("2026-09-01T12:00:00Z".into())
    );
    let supplied: Timestamp = serde_json::from_value(wire).unwrap();
    assert_eq!(supplied.unix_nanos(), source_reported.unix_nanos());
    assert_eq!(supplied.basis(), TimestampBasis::SuppliedUnknown);

    let same_instant_native =
        Timestamp::parse("2026-09-01T12:00:00Z", TimestampBasis::Native).unwrap();
    assert_eq!(source_reported, same_instant_native);
    assert_eq!(
        source_reported.cmp(&same_instant_native),
        std::cmp::Ordering::Equal
    );
    assert_eq!(source_reported, supplied);
    assert_eq!(source_reported.cmp(&supplied), std::cmp::Ordering::Equal);

    let earlier = Timestamp::parse(
        "2026-09-01T11:59:59Z",
        TimestampBasis::DerivedFromSupportedNative,
    )
    .unwrap();
    let later = Timestamp::parse("2026-09-01T12:00:01Z", TimestampBasis::SuppliedUnknown).unwrap();
    assert!(earlier < source_reported);
    assert!(supplied < later);

    for dataset in [
        Dataset::Sessions,
        Dataset::Turns,
        Dataset::Messages,
        Dataset::Tools,
        Dataset::Events,
    ] {
        for capability in schema(dataset).formats {
            assert!(capability.losses.contains(&FormatLoss::TimestampBasis));
            assert_eq!(capability.lossy, !capability.losses.is_empty());
        }
    }
    for capability in schema(Dataset::Sources).formats {
        assert!(!capability.losses.contains(&FormatLoss::TimestampBasis));
        assert_eq!(capability.lossy, !capability.losses.is_empty());
    }
}

#[test]
fn time_bounds_compare_instants_and_preserve_contextual_recovery() {
    let instant = "2026-09-01T12:00:00Z";
    let window = TimeWindow {
        field: Some(FieldId::StartedAt),
        since: Some(Timestamp::parse(instant, TimestampBasis::Native).unwrap()),
        until: Some(Timestamp::parse(instant, TimestampBasis::SourceReported).unwrap()),
        include_undated: false,
    };
    let standalone = window.validate().unwrap_err();
    assert_eq!(standalone.kind(), QueryFailureCode::InvalidTime);
    assert_eq!(standalone.recovery(), &RecoveryAction::ReadQueryHelp);

    let mut request = base_request();
    request.time = window;
    let contextual = request.validate().unwrap_err();
    assert_eq!(contextual.kind(), QueryFailureCode::InvalidTime);
    assert_eq!(
        contextual.recovery(),
        &RecoveryAction::ConsultSchema {
            dataset: Dataset::Sessions
        }
    );
}

#[test]
fn standalone_validators_use_neutral_help_while_requests_name_their_dataset() {
    for error in [
        Timestamp::parse("not-a-time", TimestampBasis::Native).unwrap_err(),
        Timestamp::new(i128::MAX, TimestampBasis::Native).unwrap_err(),
    ] {
        assert_eq!(error.kind(), QueryFailureCode::InvalidTime);
        assert_eq!(error.recovery(), &RecoveryAction::ReadQueryHelp);
    }

    let invalid_range = InclusiveRange { start: 0, end: 1 };
    let standalone_range = invalid_range.validate().unwrap_err();
    assert_eq!(standalone_range.kind(), QueryFailureCode::InvalidArgument);
    assert_eq!(standalone_range.recovery(), &RecoveryAction::ReadQueryHelp);

    let invalid_has = Filter {
        field: FieldId::Parts,
        predicate: Predicate::Has,
        values: vec![FieldValue::String("unexpected".into())],
        ignore_case: false,
    };
    let standalone_has = invalid_has.validate(&QueryLimits::default()).unwrap_err();
    assert_eq!(standalone_has.kind(), QueryFailureCode::InvalidArgument);
    assert_eq!(standalone_has.recovery(), &RecoveryAction::ReadQueryHelp);

    let session = EntityId::derive(EntityKind::Session, [&b"range-session"[..]]);
    let mut range_request = base_request();
    range_request.dataset = Dataset::Turns;
    range_request.operation = Operation::Extract;
    range_request.turn_range = Some(invalid_range);
    range_request.filters = vec![Filter {
        field: FieldId::SessionId,
        predicate: Predicate::Equal,
        values: vec![FieldValue::Id(session)],
        ignore_case: false,
    }];
    let contextual_range = range_request.validate().unwrap_err();
    assert_eq!(contextual_range.kind(), QueryFailureCode::InvalidArgument);
    assert_eq!(
        contextual_range.recovery(),
        &RecoveryAction::ConsultSchema {
            dataset: Dataset::Turns
        }
    );

    let mut has_request = base_request();
    has_request.dataset = Dataset::Messages;
    has_request.filters = vec![invalid_has];
    let contextual_has = has_request.validate().unwrap_err();
    assert_eq!(contextual_has.kind(), QueryFailureCode::InvalidArgument);
    assert_eq!(
        contextual_has.recovery(),
        &RecoveryAction::ConsultSchema {
            dataset: Dataset::Messages
        }
    );
}

#[test]
fn universe_coverage_and_output_boundaries_reject_invalid_states() {
    let valid_coverage = Coverage {
        discovered_sources: 1,
        loaded_sources: 1,
        selected_sources: 1,
        source_status: BTreeMap::from([(SourceReadStatus::Readable, 1)]),
        association_status: BTreeMap::new(),
        excluded_adapters: Vec::new(),
        source_read_complete: true,
        issues: Vec::new(),
    };
    assert!(valid_coverage.validate().is_ok());
    let mut too_many_loaded = valid_coverage.clone();
    too_many_loaded.loaded_sources = 2;
    assert_eq!(
        too_many_loaded.validate().unwrap_err().kind(),
        QueryFailureCode::InvalidData
    );
    let mut too_many_selected = valid_coverage.clone();
    too_many_selected.selected_sources = 2;
    assert_eq!(
        too_many_selected.validate().unwrap_err().kind(),
        QueryFailureCode::InvalidData
    );
    let mut overflowing_statuses = valid_coverage;
    overflowing_statuses.discovered_sources = u64::MAX;
    overflowing_statuses.source_status = BTreeMap::from([
        (SourceReadStatus::Readable, u64::MAX),
        (SourceReadStatus::Partial, 1),
    ]);
    assert_eq!(
        overflowing_statuses.validate().unwrap_err().kind(),
        QueryFailureCode::InvalidData
    );

    let valid_universe = empty_response(Dataset::Sessions, OperationKind::List).universe;
    assert!(valid_universe.validate().is_ok());
    let mut duplicate_columns = valid_universe.clone();
    duplicate_columns.columns = vec![FieldId::Id, FieldId::Id];
    assert_eq!(
        duplicate_columns.validate().unwrap_err().kind(),
        QueryFailureCode::InvalidData
    );
    let mut impossible_complete_input = valid_universe;
    impossible_complete_input.basis = UniverseBasis::ProvidedRows;
    impossible_complete_input.bounded_by_input = true;
    assert_eq!(
        impossible_complete_input.validate().unwrap_err().kind(),
        QueryFailureCode::InvalidData
    );
    impossible_complete_input.rows_complete_for_selection = Completeness::Subset;
    assert!(impossible_complete_input.validate().is_ok());

    let limits = QueryLimits::default();
    let response = empty_response(Dataset::Sessions, OperationKind::List);
    let options = QueryOutputOptions {
        format: OutputFormat::Json,
        csv_safety: CsvSafety::Spreadsheet,
        max_output_bytes: limits.max_output_bytes,
        next_action: RenderedAction {
            summary: "Inspect the session schema".into(),
            argv: Vec::new(),
            required_inputs: Vec::new(),
        },
    };
    assert!(options.validate_for(&response, &limits).is_ok());
    let mut zero_output = options.clone();
    zero_output.max_output_bytes = 0;
    assert_limit(
        zero_output.validate(&limits).unwrap_err(),
        LimitKind::OutputBytes,
    );
    let mut over_ceiling_output = options.clone();
    over_ceiling_output.max_output_bytes = limits.max_output_bytes + 1;
    assert_limit(
        over_ceiling_output.validate(&limits).unwrap_err(),
        LimitKind::OutputBytes,
    );
    let mut unsupported = options;
    unsupported.format = OutputFormat::Text;
    let output_error = unsupported.validate_for(&response, &limits).unwrap_err();
    assert_eq!(output_error.kind(), QueryFailureCode::UnsupportedOperation);
    assert_eq!(
        output_error.recovery(),
        &RecoveryAction::ConsultSchema {
            dataset: Dataset::Sessions
        }
    );
}
