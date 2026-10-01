#![forbid(unsafe_code)]

use std::path::PathBuf;
use unisphere_sdk::query::{
    ContentAccess, ContextWindow, Dataset, Operation, QueryApi, QueryFailure, QueryFailureCode,
    QueryInput, QueryLimits, QueryRequest, QueryScope, QueryService, QuerySource, RecoveryAction,
    RepoScope, SourceSelection, TimeWindow, UnresolvedPolicy,
};

// A real QueryService receives an explicitly unavailable source. The example
// exercises SDK validation and typed recovery without touching machine data.
struct NoSource;

impl QuerySource for NoSource {
    fn load(
        &self,
        _scope: &QueryScope,
        _selection: &SourceSelection,
        _limits: &QueryLimits,
        _access: ContentAccess,
    ) -> Result<QueryInput, QueryFailure> {
        Err(QueryFailure::new(
            QueryFailureCode::MissingSource,
            RecoveryAction::FixSource {
                reason: unisphere_sdk::query::SourceProblem::Missing,
                source: None,
            },
        ))
    }
}

fn main() {
    let request = QueryRequest {
        dataset: Dataset::Sessions,
        operation: Operation::List,
        scope: QueryScope::Repository {
            path: PathBuf::from("/fixtures/project"),
            scope: RepoScope::Tree,
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
    };

    let failure = match QueryService::new(NoSource).execute(&request) {
        Err(failure) => failure,
        Ok(_) => panic!("fixture has no source"),
    };
    assert_eq!(failure.code(), "UNI-QUERY-SOURCE-MISSING");
    assert!(!failure.retryable());
    println!(
        "{}",
        serde_json::to_string(&failure).expect("typed failure is serializable")
    );
}
