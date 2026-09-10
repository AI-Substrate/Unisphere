#![forbid(unsafe_code)]

use std::path::PathBuf;
use unisphere_sdk::query::{
    ContextWindow, Dataset, Operation, QueryApi, QueryFailure, QueryFailureCode, QueryLimits,
    QueryRequest, QueryResponse, QueryScope, RecoveryAction, RepoScope, TimeWindow,
    UnresolvedPolicy,
};

// An application injects a real QueryService here. This fixture uses a deterministic
// boundary double so it remains executable without filesystem or network access.
struct NoSource;

impl QueryApi for NoSource {
    fn execute(&self, request: &QueryRequest) -> Result<QueryResponse, QueryFailure> {
        request.validate()?;
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

    let failure = match NoSource.execute(&request) {
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
