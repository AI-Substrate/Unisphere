use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use unisphere_sdk::query::*;
use unisphere_testkit::query::{FakeQuerySource, shared_query_fixture};

fn request(dataset: Dataset, operation: Operation) -> QueryRequest {
    QueryRequest {
        dataset,
        operation,
        scope: QueryScope::Repository {
            path: "/fixtures/project".into(),
            scope: RepoScope::Tree,
        },
        filters: Vec::new(),
        time: TimeWindow::default(),
        branch: None,
        turn_range: None,
        sort: Vec::new(),
        columns: None,
        limit: None,
        cursor: None,
        context: ContextWindow::default(),
        include_content: false,
        allow_partial: true,
        unresolved: UnresolvedPolicy::Reject,
        limits: QueryLimits::default(),
    }
}

fn native_view() -> NativeQueryView {
    let mut fixture = shared_query_fixture();
    let inspected = fixture.remove(0);
    let mut source_status = BTreeMap::new();
    source_status.insert(SourceReadStatus::Readable, 1);
    let mut association_status = BTreeMap::new();
    association_status.insert(AssociationStatus::Matched, 1);
    NativeQueryView {
        sources: vec![inspected.source],
        observations: inspected.observations,
        repository_roots: vec!["/fixtures/project".into()],
        coverage: Coverage {
            discovered_sources: 1,
            loaded_sources: 1,
            selected_sources: 1,
            source_status,
            association_status,
            excluded_adapters: Vec::new(),
            source_read_complete: true,
            issues: Vec::new(),
        },
    }
}

fn source_ref(view: &NativeQueryView, offset: u64, subrecord: &str) -> SourceRef {
    SourceRef {
        source_id: view.sources[0].id,
        revision: view.sources[0].revision.clone(),
        locator: NativeLocator::Jsonl { offset },
        subrecord: subrecord.into(),
    }
}

fn session_key(view: &NativeQueryView) -> SessionEvidenceKey {
    view.observations[0]
        .session
        .clone()
        .expect("fixture session")
}

fn partition(view: &NativeQueryView) -> PartitionId {
    view.observations[0].branch.partition()
}

fn timestamp(value: &str) -> Timestamp {
    Timestamp::parse(value, TimestampBasis::SourceReported).expect("valid timestamp")
}

fn message_observation(
    view: &NativeQueryView,
    offset: u64,
    native_id: &str,
    text: &str,
    turn_id: &str,
    marker: RequestMarker,
    at: &str,
) -> Observation {
    Observation {
        source_ref: source_ref(view, offset, native_id),
        native_record_id: Some(native_id.into()),
        session: Some(session_key(view)),
        branch: BranchEvidence::Linear {
            partition: partition(view),
        },
        parent_ids: Vec::new(),
        sequence: NativeSequence {
            version: 1,
            key: offset.to_be_bytes().to_vec(),
        },
        timestamp: Some(timestamp(at)),
        facets: vec![ObservationFacet::Message {
            native_id: Some(native_id.into()),
            role: MessageRole::User,
            parts: vec![ObservationPart::Text(text.into())],
            request_marker: marker,
            turn_id: Some(turn_id.into()),
        }],
        diagnostics: Vec::new(),
    }
}

fn tool_observations(view: &NativeQueryView, outcome: Outcome) -> [Observation; 2] {
    let common = |offset, subrecord: &str, at: &str, facet| Observation {
        source_ref: source_ref(view, offset, subrecord),
        native_record_id: Some(subrecord.into()),
        session: Some(session_key(view)),
        branch: BranchEvidence::Linear {
            partition: partition(view),
        },
        parent_ids: Vec::new(),
        sequence: NativeSequence {
            version: 1,
            key: offset.to_be_bytes().to_vec(),
        },
        timestamp: Some(timestamp(at)),
        facets: vec![facet],
        diagnostics: Vec::new(),
    };
    [
        common(
            256,
            "tool-call",
            "2026-09-01T12:00:02Z",
            ObservationFacet::ToolCall {
                native_call_id: "call-1".into(),
                native_name: "Bash".into(),
                family: Some("shell".into()),
                input: vec![ObservationPart::Structured(
                    serde_json::json!({"command":"printf sentinel"}),
                )],
                turn_id: Some("turn-1".into()),
            },
        ),
        common(
            384,
            "tool-result",
            "2026-09-01T12:00:03Z",
            ObservationFacet::ToolResult {
                native_call_id: "call-1".into(),
                native_name: Some("Bash".into()),
                output: vec![ObservationPart::Text("sentinel".into())],
                outcome,
                exit_code: Some(if outcome == Outcome::Succeeded { 0 } else { 1 }),
                reported_duration_ms: None,
                turn_id: Some("turn-1".into()),
            },
        ),
    ]
}

fn tree_observation(
    view: &NativeQueryView,
    offset: u64,
    native_id: &str,
    parent: Option<&str>,
    facet: ObservationFacet,
) -> Observation {
    Observation {
        source_ref: source_ref(view, offset, native_id),
        native_record_id: Some(native_id.into()),
        session: Some(session_key(view)),
        branch: BranchEvidence::Node {
            partition: partition(view),
            native_id: native_id.into(),
            parent: parent.map(str::to_owned),
            declared_branch: None,
            links: Vec::new(),
        },
        parent_ids: parent.into_iter().map(str::to_owned).collect(),
        sequence: NativeSequence {
            version: 1,
            key: offset.to_be_bytes().to_vec(),
        },
        timestamp: None,
        facets: vec![facet],
        diagnostics: Vec::new(),
    }
}

#[test]
fn service_extracts_source_selection_before_io_and_reconstructs_all_datasets() {
    let mut native = native_view();
    native
        .observations
        .extend(tool_observations(&native, Outcome::Succeeded));
    let source = FakeQuerySource::new(Ok(QueryInput::Native(native)));
    let service = QueryService::new(source);
    let mut sessions = request(Dataset::Sessions, Operation::List);
    sessions.filters.push(Filter {
        field: FieldId::Adapter,
        predicate: Predicate::Equal,
        values: vec![FieldValue::String("claude-jsonl".into())],
        ignore_case: false,
    });
    let response = service.execute(&sessions).expect("session query succeeds");
    assert_eq!(response.matched, 1);
    let calls = service.source().calls();
    assert_eq!(calls.len(), 1);
    assert!(
        calls[0]
            .selection
            .include_adapters
            .contains(&AdapterId::new("claude-jsonl").unwrap())
    );

    let view = service.open_view(&sessions).expect("open retained view");
    assert_eq!(view.sources().len(), 1);
    assert_eq!(view.sessions().len(), 1);
    assert_eq!(view.turns().len(), 1);
    assert_eq!(view.messages().len(), 2);
    assert_eq!(view.tools().len(), 1);
    assert_eq!(view.events().len(), 4);
    assert_eq!(view.tools()[0].duration_ms, Some(1000.0));
    assert_eq!(
        view.tools()[0].duration_basis,
        Some(DurationBasis::PairedClock)
    );
    assert_eq!(view.tools()[0].status, Outcome::Succeeded);
    assert_eq!(view.turns()[0].ordinal, 1);
    assert!(view.messages().iter().all(|message| matches!(
        message.parts.as_slice(),
        [ObservationPart::Unavailable(
            AvailabilityCode::SensitiveOmitted
        )]
    )));
    for (dataset, expected) in [
        (Dataset::Sources, 1),
        (Dataset::Sessions, 1),
        (Dataset::Turns, 1),
        (Dataset::Messages, 2),
        (Dataset::Tools, 1),
        (Dataset::Events, 4),
    ] {
        let mut dataset_request = sessions.clone();
        dataset_request.dataset = dataset;
        let response = execute_view(&view, &dataset_request).expect("typed dataset query");
        assert_eq!(response.matched, expected);
    }

    let widened = request(Dataset::Sessions, Operation::List);
    let failure = execute_view(&view, &widened)
        .err()
        .expect("source selection widening refused");
    assert_eq!(failure.kind(), QueryFailureCode::ViewScopeMismatch);
}

#[test]
fn query_api_rejects_stats_only_columns_and_sorts_before_loading() {
    let service = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native_view()))));
    let metric_fields = [
        FieldId::Count,
        FieldId::MeasuredCount,
        FieldId::MissingDurationCount,
        FieldId::Succeeded,
        FieldId::Failures,
        FieldId::Cancelled,
        FieldId::Incomplete,
        FieldId::Unknown,
        FieldId::FailureRate,
        FieldId::MeanMs,
        FieldId::MinMs,
        FieldId::MaxMs,
        FieldId::P50Ms,
        FieldId::P95Ms,
        FieldId::InputTokens,
        FieldId::OutputTokens,
        FieldId::CacheReadTokens,
        FieldId::CacheWriteTokens,
    ];

    for field in metric_fields {
        let mut columns = request(Dataset::Tools, Operation::List);
        columns.columns = Some(vec![field]);
        let error = QueryApi::execute(&service, &columns)
            .err()
            .expect("stats-only column must be rejected");
        assert_eq!(error.kind(), QueryFailureCode::UnsupportedOperation);

        let mut sort = request(Dataset::Tools, Operation::List);
        sort.sort.push(SortKey {
            field,
            direction: SortDirection::Ascending,
        });
        let error = QueryApi::execute(&service, &sort)
            .err()
            .expect("stats-only sort must be rejected");
        assert_eq!(error.kind(), QueryFailureCode::UnsupportedOperation);
    }

    assert!(service.source().calls().is_empty());

    let mut duration = request(Dataset::Tools, Operation::List);
    duration.columns = Some(vec![FieldId::DurationMs]);
    QueryApi::execute(&service, &duration).expect("per-call duration remains a row projection");
    assert_eq!(service.source().calls().len(), 1);
}

#[test]
fn tool_responses_do_not_create_extra_turns_and_ambiguous_calls_are_not_guessed() {
    let mut native = native_view();
    native.observations[1].facets = vec![ObservationFacet::Message {
        native_id: Some("message-tool-response".into()),
        role: MessageRole::User,
        parts: vec![ObservationPart::Structured(
            serde_json::json!({"status":"ok"}),
        )],
        request_marker: RequestMarker::ToolResponse,
        turn_id: None,
    }];
    let mut duplicate = tool_observations(&native, Outcome::Failed)[0].clone();
    duplicate.source_ref = source_ref(&native, 300, "tool-call-retry");
    duplicate.sequence.key = 300_u64.to_be_bytes().to_vec();
    native
        .observations
        .push(tool_observations(&native, Outcome::Failed)[0].clone());
    native.observations.push(duplicate);

    native
        .observations
        .push(tool_observations(&native, Outcome::Failed)[1].clone());

    let view = QueryView::from_input_for(
        QueryInput::Native(native),
        request(Dataset::Turns, Operation::List).scope,
        SourceSelection::default(),
        ContentAccess::default(),
        &QueryLimits::default(),
    )
    .expect("build view");
    assert_eq!(view.turns().len(), 1);
    assert_eq!(view.tools().len(), 3);
    assert!(view.tools().iter().all(|tool| {
        tool.status_reason == Some(AvailabilityCode::Ambiguous) || tool.status == Outcome::Unknown
    }));
}
#[test]
fn invalid_limits_refuse_before_source_io() {
    let service = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native_view()))));
    let mut query = request(Dataset::Sessions, Operation::List);
    query.limits.max_patterns = 0;
    let failure = service
        .execute(&query)
        .err()
        .expect("invalid limit refused");
    assert_eq!(failure.kind(), QueryFailureCode::ResourceLimit);
    assert!(service.source().calls().is_empty());

    let mut query = request(Dataset::Messages, Operation::List);
    query.filters.push(Filter {
        field: FieldId::Role,
        predicate: Predicate::Equal,
        values: vec![FieldValue::Unsigned(1)],
        ignore_case: false,
    });
    let failure = service
        .execute(&query)
        .err()
        .expect("wrong filter type refused");
    assert_eq!(failure.kind(), QueryFailureCode::InvalidArgument);
    assert!(service.source().calls().is_empty());
}

#[test]
fn filters_time_order_and_continuation_share_one_deterministic_selection() {
    let mut native = native_view();
    let observations = [
        message_observation(
            &native,
            10,
            "m1",
            "Alpha first",
            "t1",
            RequestMarker::Initiating,
            "2026-09-01T12:00:00Z",
        ),
        message_observation(
            &native,
            20,
            "m2",
            "beta second",
            "t2",
            RequestMarker::Initiating,
            "2026-09-01T12:01:00Z",
        ),
        message_observation(
            &native,
            30,
            "m3",
            "BETA third",
            "t3",
            RequestMarker::Initiating,
            "2026-09-01T12:02:00Z",
        ),
    ];
    native.observations = observations.into();
    let service = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native))));
    let mut query = request(Dataset::Messages, Operation::List);
    query.filters.push(Filter {
        field: FieldId::Text,
        predicate: Predicate::Contains,
        values: vec![
            FieldValue::String("beta".into()),
            FieldValue::String("third".into()),
        ],
        ignore_case: true,
    });
    query.time.since = Some(timestamp("2026-09-01T12:01:00Z"));
    query.time.until = Some(timestamp("2026-09-01T12:03:00Z"));
    query.limit = Some(1);
    let first = service.execute(&query).expect("first page");
    assert_eq!(first.matched, 2);
    assert_eq!(first.emitted, 1);
    assert!(first.next_cursor.is_some());
    assert!(matches!(first.next_action, QueryAction::Continue { .. }));

    query.cursor = first.next_cursor;
    let second = service.execute(&query).expect("continuation");
    assert_eq!(second.emitted, 1);
    assert!(second.next_cursor.is_none());
    assert_ne!(first.rows[0].id(), second.rows[0].id());

    query.limit = Some(2);
    let error = service.execute(&query).err().expect("stale continuation");
    assert_eq!(
        error.cursor_reason(),
        Some(CursorMismatchReason::QueryOptionsChanged)
    );
}

#[test]
fn content_search_does_not_authorize_content_projection() {
    let service = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native_view()))));
    let mut query = request(Dataset::Messages, Operation::List);
    query.filters.push(Filter {
        field: FieldId::Text,
        predicate: Predicate::Contains,
        values: vec![FieldValue::String("fixture user".into())],
        ignore_case: false,
    });
    query.columns = Some(vec![FieldId::Text]);
    let error = service
        .execute(&query)
        .err()
        .expect("content consent required");
    assert_eq!(error.kind(), QueryFailureCode::ContentConsentRequired);
    assert!(matches!(
        error.recovery(),
        RecoveryAction::UseMetadataOrConsent { fields } if fields == &[FieldId::Text]
    ));

    query.include_content = true;
    let response = service
        .execute(&query)
        .expect("explicit content projection");
    assert!(matches!(

        response.rows[0].field(FieldId::Text),
        Some(FieldValue::String(value)) if value == "fixture user request"
    ));
}
fn usage_observation(view: &NativeQueryView, offset: u64, input_tokens: u64) -> Observation {
    Observation {
        source_ref: source_ref(view, offset, "usage"),
        native_record_id: Some(format!("usage-{offset}")),
        session: Some(session_key(view)),
        branch: BranchEvidence::Linear {
            partition: partition(view),
        },
        parent_ids: Vec::new(),
        sequence: NativeSequence {
            version: 1,
            key: offset.to_be_bytes().to_vec(),
        },
        timestamp: None,
        facets: vec![ObservationFacet::Usage {
            owner: Some("session-readable".into()),
            scope: UsageScope::CumulativeSnapshot,
            counters: UsageCounters {
                input_tokens: Some(input_tokens),
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
            },
        }],
        diagnostics: Vec::new(),
    }
}

#[test]
fn cumulative_usage_statistics_take_latest_native_snapshot() {
    let mut native = native_view();
    native
        .observations
        .push(usage_observation(&native, 500, 10));
    native
        .observations
        .push(usage_observation(&native, 600, 15));
    let service = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native))));
    let query = request(
        Dataset::Sessions,
        Operation::Stats {
            group_by: Vec::new(),
            metrics: vec![Metric::Count, Metric::InputTokens],
        },
    );
    let response = service.execute(&query).expect("session usage statistics");
    assert!(matches!(
        response.rows[0].field(FieldId::InputTokens),
        Some(FieldValue::Unsigned(15))
    ));
}

#[test]
fn context_expands_within_each_session_branch_after_matching() {
    let mut native = native_view();
    let observations = [
        message_observation(
            &native,
            10,
            "m1",
            "before",
            "t1",
            RequestMarker::Initiating,
            "2026-08-31T23:59:00Z",
        ),
        message_observation(
            &native,
            20,
            "m2",
            "needle",
            "t2",
            RequestMarker::Initiating,
            "2026-09-01T12:00:00Z",
        ),
        message_observation(
            &native,
            30,
            "m3",
            "after",
            "t3",
            RequestMarker::Initiating,
            "2026-09-02T00:01:00Z",
        ),
    ];
    native.observations = observations.into();
    let mut second_session = native.observations.clone();
    for observation in &mut second_session {
        observation
            .session
            .as_mut()
            .expect("session evidence")
            .native_id = "session-second".into();
        observation.source_ref.subrecord.push_str("-second");
        if let NativeLocator::Jsonl { offset } = &mut observation.source_ref.locator {
            *offset += 100;
        }
    }
    native.observations.extend(second_session);
    let service = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native))));
    let mut query = request(Dataset::Messages, Operation::Extract);
    query.filters.push(Filter {
        field: FieldId::Text,
        predicate: Predicate::Contains,
        values: vec![FieldValue::String("needle".into())],
        ignore_case: false,
    });
    query.time.since = Some(timestamp("2026-09-01"));
    query.time.until = Some(timestamp("2026-09-02"));
    query.context = ContextWindow {
        before: 1,
        after: 1,
    };
    query.columns = Some(vec![FieldId::Role]);
    let view = service.open_view(&query).expect("open context view");
    let source_view_digest = view.digest();
    let response = execute_view(&view, &query).expect("context extraction");
    assert_eq!(response.matched, 2);
    assert_eq!(response.emitted, 6);
    let matched_rows = response
        .rows
        .iter()
        .filter(|row| matches!(row.field(FieldId::IsContext), Some(FieldValue::Bool(false))))
        .count();
    let context_rows = response
        .rows
        .iter()
        .filter(|row| matches!(row.field(FieldId::IsContext), Some(FieldValue::Bool(true))))
        .count();
    assert_eq!(matched_rows, 2);
    assert_eq!(context_rows, 4);
    let effective_columns = vec![
        FieldId::Id,
        FieldId::SourceRefs,
        FieldId::Role,
        FieldId::IsContext,
    ];
    let columns_digest = Digest::framed(
        b"unisphere/query-columns/v1",
        effective_columns
            .iter()
            .map(|field| field.as_str().as_bytes()),
    );
    assert_eq!(response.universe.columns, effective_columns);
    assert_eq!(response.universe.columns_digest, columns_digest);
    assert_eq!(
        response.universe.source_view_digest,
        Some(source_view_digest)
    );
    assert_eq!(view.digest(), source_view_digest);

    let mut turns = request(Dataset::Turns, Operation::Extract);
    turns.filters.push(Filter {
        field: FieldId::Ordinal,
        predicate: Predicate::Equal,
        values: vec![FieldValue::Unsigned(2)],
        ignore_case: false,
    });
    turns.context = ContextWindow {
        before: 1,
        after: 1,
    };
    turns.columns = Some(vec![FieldId::SessionId]);
    let turn_response = execute_view(&view, &turns).expect("turn context extraction");
    assert_eq!(turn_response.matched, 2);
    assert_eq!(turn_response.emitted, 6);
    assert_eq!(
        turn_response
            .rows
            .iter()
            .filter(|row| matches!(row.field(FieldId::IsContext), Some(FieldValue::Bool(false))))
            .count(),
        2
    );
    assert_eq!(
        turn_response
            .rows
            .iter()
            .filter(|row| matches!(row.field(FieldId::IsContext), Some(FieldValue::Bool(true))))
            .count(),
        4
    );
    assert_eq!(
        turn_response.universe.columns,
        vec![
            FieldId::Id,
            FieldId::SourceRefs,
            FieldId::SessionId,
            FieldId::IsContext,
        ]
    );
}

#[test]
fn parent_tree_preserves_shared_prefix_and_fork_scopes() {
    let mut native = native_view();
    native.observations = vec![
        tree_observation(
            &native,
            10,
            "root-user",
            None,
            ObservationFacet::Message {
                native_id: Some("root-user".into()),
                role: MessageRole::User,
                parts: Vec::new(),
                request_marker: RequestMarker::Initiating,
                turn_id: None,
            },
        ),
        tree_observation(
            &native,
            20,
            "a-user",
            Some("root-user"),
            ObservationFacet::Message {
                native_id: Some("a-user".into()),
                role: MessageRole::User,
                parts: Vec::new(),
                request_marker: RequestMarker::Initiating,
                turn_id: None,
            },
        ),
        tree_observation(
            &native,
            21,
            "a-call",
            Some("a-user"),
            ObservationFacet::ToolCall {
                native_call_id: "same-call".into(),
                native_name: "Bash".into(),
                family: Some("shell".into()),
                input: Vec::new(),
                turn_id: None,
            },
        ),
        tree_observation(
            &native,
            22,
            "a-result",
            Some("a-call"),
            ObservationFacet::ToolResult {
                native_call_id: "same-call".into(),
                native_name: Some("Bash".into()),
                output: Vec::new(),
                outcome: Outcome::Succeeded,
                exit_code: Some(0),
                reported_duration_ms: None,
                turn_id: None,
            },
        ),
        tree_observation(
            &native,
            30,
            "b-user",
            Some("root-user"),
            ObservationFacet::Message {
                native_id: Some("b-user".into()),
                role: MessageRole::User,
                parts: Vec::new(),
                request_marker: RequestMarker::Initiating,
                turn_id: None,
            },
        ),
        tree_observation(
            &native,
            31,
            "b-call",
            Some("b-user"),
            ObservationFacet::ToolCall {
                native_call_id: "same-call".into(),
                native_name: "Bash".into(),
                family: Some("shell".into()),
                input: Vec::new(),
                turn_id: None,
            },
        ),
        tree_observation(
            &native,
            32,
            "b-result",
            Some("b-call"),
            ObservationFacet::ToolResult {
                native_call_id: "same-call".into(),
                native_name: Some("Bash".into()),
                output: Vec::new(),
                outcome: Outcome::Failed,
                exit_code: Some(1),
                reported_duration_ms: None,
                turn_id: None,
            },
        ),
    ];
    let view = QueryView::from_input_for(
        QueryInput::Native(native),
        request(Dataset::Messages, Operation::List).scope,
        SourceSelection::default(),
        ContentAccess::default(),
        &QueryLimits::default(),
    )
    .expect("validated fork reconstructs");

    let root = view
        .messages()
        .iter()
        .find(|row| row.native_id.as_deref() == Some("root-user"))
        .expect("shared root message");
    let branch_a = view
        .messages()
        .iter()
        .find(|row| row.native_id.as_deref() == Some("a-user"))
        .expect("branch A message");
    let branch_b = view
        .messages()
        .iter()
        .find(|row| row.native_id.as_deref() == Some("b-user"))
        .expect("branch B message");
    assert_eq!(view.sessions()[0].branch_ids.len(), 2);
    assert_eq!(root.branch_ids.len(), 2);
    assert_eq!(branch_a.branch_ids.len(), 1);
    assert_eq!(branch_b.branch_ids.len(), 1);
    assert_ne!(branch_a.branch_ids, branch_b.branch_ids);
    assert!(root.branch_ids.contains(&branch_a.branch_ids[0]));
    assert!(root.branch_ids.contains(&branch_b.branch_ids[0]));

    let root_turn = view
        .turns()
        .iter()
        .find(|row| row.message_ids.contains(&root.id))
        .expect("shared-prefix turn");
    let turn_a = view
        .turns()
        .iter()
        .find(|row| row.message_ids.contains(&branch_a.id))
        .expect("branch A turn");
    let turn_b = view
        .turns()
        .iter()
        .find(|row| row.message_ids.contains(&branch_b.id))
        .expect("branch B turn");
    assert_eq!(root_turn.branch_ids.len(), 2);
    assert_eq!(root_turn.ordinal, 1);
    assert_eq!(turn_a.ordinal, 2);
    assert_eq!(turn_b.ordinal, 2);
    assert_ne!(turn_a.id, turn_b.id);

    assert_eq!(view.tools().len(), 2);
    assert_ne!(view.tools()[0].id, view.tools()[1].id);
    assert_ne!(view.tools()[0].branch_ids, view.tools()[1].branch_ids);
    for tool in view.tools() {
        let expected_turn = if tool.branch_ids == branch_a.branch_ids {
            turn_a.id
        } else {
            turn_b.id
        };
        assert_eq!(tool.turn_id, Some(expected_turn));
    }
    assert!(
        view.tools()
            .iter()
            .any(|row| row.status == Outcome::Succeeded)
    );
    assert!(view.tools().iter().any(|row| row.status == Outcome::Failed));

    let mut context = request(Dataset::Messages, Operation::Extract);
    context.filters.push(Filter {
        field: FieldId::Id,
        predicate: Predicate::Equal,
        values: vec![FieldValue::Id(branch_a.id)],
        ignore_case: false,
    });
    context.context.before = 1;
    context.branch = Some(branch_a.branch_ids[0]);
    let response = execute_view(&view, &context).expect("branch-local context");
    assert_eq!(response.matched, 1);
    assert_eq!(response.emitted, 2);
    assert_eq!(
        response
            .rows
            .iter()
            .map(|row| row.id())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([root.id, branch_a.id])
    );
}

#[test]
fn cyclic_and_dangling_parent_paths_keep_rows_without_inventing_branches() {
    for (nodes, expected) in [
        (
            vec![("a", Some("b")), ("b", Some("a"))],
            AvailabilityCode::Conflict,
        ),
        (vec![("a", Some("missing"))], AvailabilityCode::NotCaptured),
    ] {
        let mut native = native_view();
        native.observations = nodes
            .iter()
            .enumerate()
            .map(|(index, (id, parent))| {
                tree_observation(
                    &native,
                    index as u64 + 1,
                    id,
                    *parent,
                    ObservationFacet::Message {
                        native_id: Some((*id).into()),
                        role: MessageRole::User,
                        parts: Vec::new(),
                        request_marker: RequestMarker::Initiating,
                        turn_id: None,
                    },
                )
            })
            .collect();
        let view = QueryView::from_input_for(
            QueryInput::Native(native),
            request(Dataset::Messages, Operation::List).scope,
            SourceSelection::default(),
            ContentAccess::default(),
            &QueryLimits::default(),
        )
        .expect("unresolved topology still retains supplied rows");
        assert_eq!(view.messages().len(), nodes.len());
        assert!(view.messages().iter().all(|row| row.branch_ids.is_empty()));
        assert!(
            view.messages()
                .iter()
                .any(|row| row.availability.contains(&expected))
        );
        assert!(view.sessions().iter().all(|row| row.branch_ids.is_empty()));
    }
}

#[test]
fn conflicting_branch_evidence_remains_explicit_and_blocks_exact_statistics() {
    let mut native = native_view();
    let partition = partition(&native);
    native.observations[0].branch = BranchEvidence::Node {
        partition,
        native_id: "node-1".into(),
        parent: None,
        declared_branch: Some("main".into()),
        links: Vec::new(),
    };
    native.observations[1].branch = BranchEvidence::Node {
        partition,
        native_id: "node-1".into(),
        parent: Some("different-parent".into()),
        declared_branch: Some("main".into()),
        links: Vec::new(),
    };
    let view = QueryView::from_input_for(
        QueryInput::Native(native),
        request(Dataset::Sessions, Operation::List).scope,
        SourceSelection::default(),
        ContentAccess::default(),
        &QueryLimits::default(),
    )
    .expect("conflicting evidence remains queryable");
    assert!(
        view.sessions()[0]
            .availability
            .contains(&AvailabilityCode::Conflict)
    );

    let query = request(
        Dataset::Sessions,
        Operation::Stats {
            group_by: Vec::new(),
            metrics: vec![Metric::Count],
        },
    );
    let failure = execute_view(&view, &query)
        .err()
        .expect("exact aggregate refuses conflict");
    assert_eq!(failure.kind(), QueryFailureCode::AmbiguousIdentity);
}

#[test]
fn tool_statistics_use_measured_values_and_named_terminal_denominator() {
    let mut native = native_view();
    native
        .observations
        .extend(tool_observations(&native, Outcome::Failed));
    let service = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native))));
    let query = request(
        Dataset::Tools,
        Operation::Stats {
            group_by: vec![FieldId::ToolFamily],
            metrics: vec![
                Metric::Count,
                Metric::MeasuredCount,
                Metric::MissingDurationCount,
                Metric::Failures,
                Metric::FailureRate,
                Metric::MeanMs,
                Metric::P95Ms,
            ],
        },
    );
    let response = service.execute(&query).expect("statistics");
    assert_eq!(response.matched, 1);
    assert_eq!(response.rows.len(), 1);
    let row = &response.rows[0];
    assert!(matches!(
        row.field(FieldId::Count),
        Some(FieldValue::Unsigned(1))
    ));
    assert!(matches!(
        row.field(FieldId::MeasuredCount),
        Some(FieldValue::Unsigned(1))
    ));
    assert!(matches!(
        row.field(FieldId::MissingDurationCount),
        Some(FieldValue::Unsigned(0))
    ));
    assert!(matches!(
        row.field(FieldId::Failures),
        Some(FieldValue::Unsigned(1))
    ));
    assert!(
        matches!(row.field(FieldId::FailureRate), Some(FieldValue::Float(value)) if *value == 1.0)
    );
    assert!(
        matches!(row.field(FieldId::MeanMs), Some(FieldValue::Float(value)) if *value == 1000.0)
    );
    assert!(
        matches!(row.field(FieldId::P95Ms), Some(FieldValue::Float(value)) if *value == 1000.0)
    );
}

#[test]
fn saved_statistics_keep_absent_null_and_literal_null_groups_distinct() {
    let mut native = native_view();
    native
        .observations
        .extend(tool_observations(&native, Outcome::Succeeded));
    let live = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native))));
    let response = live
        .execute(&request(Dataset::Tools, Operation::List))
        .unwrap();
    let mut data = serde_json::to_value(response).unwrap();
    let original = data["rows"][0].clone();
    let rows = [
        ("absent", None),
        ("null", Some(serde_json::Value::Null)),
        ("literal", Some(serde_json::json!("null"))),
    ]
    .into_iter()
    .map(|(name, value)| {
        let mut row = original.clone();
        row["id"] =
            serde_json::to_value(EntityId::derive(EntityKind::Tool, [name.as_bytes()])).unwrap();
        let fields = row["fields"].as_object_mut().unwrap();
        fields.remove("tool_family");
        if let Some(value) = value {
            fields.insert("tool_family".into(), value);
        }
        row
    })
    .collect::<Vec<_>>();
    data["rows"] = serde_json::json!(rows);
    data["matched"] = serde_json::json!(3);
    data["emitted"] = serde_json::json!(3);
    let bytes = serde_json::to_vec(&serde_json::json!({
        "ok": true, "command": "tools list", "v": 1, "data": data,
        "next_action": {"summary": "Inspect a saved tool"}
    }))
    .unwrap()
    .into();
    let service = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Saved {
        bytes,
        format: SavedFormat::QueryJsonV1,
    })));
    let mut query = request(
        Dataset::Tools,
        Operation::Stats {
            group_by: vec![FieldId::ToolFamily],
            metrics: vec![Metric::Count],
        },
    );
    query.scope = QueryScope::Offline {
        input: OfflineRef::Stdin,
    };
    let grouped = service.execute(&query).expect("complete saved statistics");
    assert_eq!(grouped.matched, 3);
    assert_eq!(grouped.rows.len(), 3);
    assert_eq!(
        grouped
            .rows
            .iter()
            .map(ProjectedRow::id)
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    for expected in [
        None,
        Some(FieldValue::Null),
        Some(FieldValue::String("null".into())),
    ] {
        let group = grouped
            .rows
            .iter()
            .find(|row| row.field(FieldId::ToolFamily) == expected.as_ref())
            .expect("each availability state retains its own group");
        assert!(matches!(
            group.field(FieldId::Count),
            Some(FieldValue::Unsigned(1))
        ));
    }
}

#[test]
fn c25_view_digest_excludes_response_and_request_options_but_binds_view_inputs() {
    let native = native_view();
    let scope = request(Dataset::Sessions, Operation::List).scope;
    let base = QueryView::from_input_for(
        QueryInput::Native(native.clone()),
        scope.clone(),
        SourceSelection::default(),
        ContentAccess::default(),
        &QueryLimits::default(),
    )
    .expect("base view");
    let digest = base.digest();

    let mut first_request = request(Dataset::Sessions, Operation::List);
    first_request.limit = Some(1);
    let first = execute_view(&base, &first_request).expect("first response");
    let mut second_request = first_request.clone();
    second_request.limit = Some(25);
    second_request.columns = Some(vec![FieldId::Id, FieldId::Harness]);
    let second = execute_view(&base, &second_request).expect("second response");
    assert_eq!(base.digest(), digest);
    assert_eq!(first.universe.source_view_digest, Some(digest));
    assert_eq!(second.universe.source_view_digest, Some(digest));
    assert_ne!(
        first.universe.selection_digest,
        second.universe.selection_digest
    );
    let _response_only_state = (
        first.universe,
        first.next_action,
        first.next_cursor,
        second.universe,
        second.next_action,
        second.next_cursor,
    );
    assert_eq!(base.digest(), digest);

    let mut revised_native = native.clone();
    revised_native.sources[0].revision = "readable-r2".into();
    for observation in &mut revised_native.observations {
        observation.source_ref.revision = "readable-r2".into();
    }
    let revised = QueryView::from_input_for(
        QueryInput::Native(revised_native),
        scope.clone(),
        SourceSelection::default(),
        ContentAccess::default(),
        &QueryLimits::default(),
    )
    .expect("revised view");
    assert_ne!(revised.digest(), digest);
    let mut paged = request(Dataset::Events, Operation::List);
    paged.limit = Some(1);
    let page = execute_view(&base, &paged).expect("event page");
    paged.cursor = page.next_cursor;
    let stale = execute_view(&revised, &paged)
        .err()
        .expect("changed source view refuses continuation");
    assert_eq!(
        stale.cursor_reason(),
        Some(CursorMismatchReason::SourceViewChanged)
    );

    let selected = QueryView::from_input_for(
        QueryInput::Native(native.clone()),
        scope.clone(),
        SourceSelection {
            include_adapters: BTreeSet::from([AdapterId::new("claude-jsonl").unwrap()]),
            ..SourceSelection::default()
        },
        ContentAccess::default(),
        &QueryLimits::default(),
    )
    .expect("selected view");
    assert_ne!(selected.digest(), digest);

    let retained_content = QueryView::from_input_for(
        QueryInput::Native(native),
        scope,
        SourceSelection::default(),
        ContentAccess {
            inspect_fields: BTreeSet::from([FieldId::Parts]),
            emit_content: false,
        },
        &QueryLimits::default(),
    )
    .expect("content-retaining view");
    assert_ne!(retained_content.digest(), digest);
}

#[test]
fn source_statuses_remain_distinct_and_strict_session_queries_refuse_partial_reads() {
    let fixture = shared_query_fixture();
    let mut source_status = BTreeMap::new();
    source_status.insert(SourceReadStatus::Readable, 1);
    source_status.insert(SourceReadStatus::Partial, 1);
    let native = NativeQueryView {
        sources: fixture.iter().map(|source| source.source.clone()).collect(),
        observations: fixture
            .iter()
            .flat_map(|source| source.observations.clone())
            .collect(),
        repository_roots: vec!["/fixtures/project".into()],
        coverage: Coverage {
            discovered_sources: 2,
            loaded_sources: 2,
            selected_sources: 2,
            source_status,
            association_status: BTreeMap::from([
                (AssociationStatus::Matched, 1),
                (AssociationStatus::Unassociated, 1),
            ]),
            excluded_adapters: Vec::new(),
            source_read_complete: false,
            issues: fixture
                .iter()
                .flat_map(|source| source.issues.clone())
                .collect(),
        },
    };
    let service = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native))));
    let sources = service
        .execute(&request(Dataset::Sources, Operation::List))
        .expect("source diagnosis succeeds");
    assert_eq!(sources.matched, 2);
    let statuses = sources
        .rows
        .iter()
        .filter_map(|row| row.field(FieldId::ReadStatus))
        .collect::<Vec<_>>();
    assert!(
        statuses
            .iter()
            .any(|value| matches!(value, FieldValue::String(status) if status == "readable"))
    );
    assert!(
        statuses
            .iter()
            .any(|value| matches!(value, FieldValue::String(status) if status == "partial"))
    );

    let mut sessions = request(Dataset::Sessions, Operation::List);
    sessions.allow_partial = false;
    let failure = service
        .execute(&sessions)
        .err()
        .expect("partial read refused");
    assert_eq!(failure.kind(), QueryFailureCode::UnreadableSource);
}

#[test]
fn offline_saved_input_is_bounded_and_never_reaches_a_live_enrichment_path() {
    let live = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native_view()))));
    let response = live
        .execute(&request(Dataset::Messages, Operation::List))
        .expect("live response");
    let bytes: Arc<[u8]> = serde_json::to_vec(&serde_json::json!({
        "ok": true,
        "command": "sessions list",
        "v": 1,
        "data": response,
        "next_action": {"summary": "inspect one result"}
    }))
    .expect("serialize saved envelope")
    .into();
    let offline_source = FakeQuerySource::new(Ok(QueryInput::Saved {
        bytes,
        format: SavedFormat::QueryJsonV1,
    }));
    let offline = QueryService::new(offline_source);
    let mut query = request(Dataset::Messages, Operation::List);
    query.scope = QueryScope::Offline {
        input: OfflineRef::Stdin,
    };
    let result = offline.execute(&query).expect("offline filtering");
    assert_eq!(result.matched, 2);
    assert_eq!(offline.source().calls().len(), 1);
    assert!(matches!(
        offline.source().calls()[0].scope,
        QueryScope::Offline { .. }
    ));
    assert!(result.universe.source_view_digest.is_some());
}

#[test]
fn malformed_saved_envelope_fails_with_schema_recovery() {
    let source = FakeQuerySource::new(Ok(QueryInput::Saved {
        bytes: Arc::from(br#"{\"data\":{}}"#.as_slice()),
        format: SavedFormat::QueryJsonV1,
    }));
    let service = QueryService::new(source);
    let mut query = request(Dataset::Messages, Operation::List);
    query.scope = QueryScope::Offline {
        input: OfflineRef::Stdin,
    };
    let failure = service
        .execute(&query)
        .err()
        .expect("malformed input refused");
    assert_eq!(failure.kind(), QueryFailureCode::UnsupportedSchema);
    assert!(matches!(
        failure.recovery(),
        RecoveryAction::UseCompleteInput
    ));
}

#[test]
fn standalone_jsonl_refuses_context_without_partition_completeness() {
    let live = QueryService::new(FakeQuerySource::new(Ok(QueryInput::Native(native_view()))));
    let response = live
        .execute(&request(Dataset::Messages, Operation::List))
        .expect("live response");
    let bytes: Arc<[u8]> = serde_json::to_vec(&response.rows[0])
        .expect("serialize row")
        .into();
    let source = FakeQuerySource::new(Ok(QueryInput::Saved {
        bytes,
        format: SavedFormat::QueryJsonlV1,
    }));
    let service = QueryService::new(source);
    let mut query = request(Dataset::Messages, Operation::Extract);
    query.scope = QueryScope::Offline {
        input: OfflineRef::Stdin,
    };
    query.context.before = 1;
    let failure = service
        .execute(&query)
        .err()
        .expect("incomplete context refused");
    assert_eq!(failure.kind(), QueryFailureCode::InputSubset);
    assert!(matches!(
        failure.recovery(),
        RecoveryAction::UseCompleteInput
    ));
}
