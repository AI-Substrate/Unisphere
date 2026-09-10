use std::sync::Mutex;

use unisphere_core::query::{
    ContentAccess, QueryApi, QueryFailure, QueryInput, QueryLimits, QueryRequest, QueryResponse,
    QueryScope, QuerySource, SourceSelection,
};

#[derive(Clone, PartialEq, Eq)]
pub struct QueryLoadCall {
    pub scope: QueryScope,
    pub selection: SourceSelection,
    pub limits: QueryLimits,
    pub access: ContentAccess,
}

/// Deterministic query source with an observable pre-I/O call boundary.
pub struct FakeQuerySource {
    result: Result<QueryInput, QueryFailure>,
    calls: Mutex<Vec<QueryLoadCall>>,
}

impl FakeQuerySource {
    pub fn new(result: Result<QueryInput, QueryFailure>) -> Self {
        Self {
            result,
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<QueryLoadCall> {
        self.calls
            .lock()
            .expect("fake query source call log poisoned")
            .clone()
    }
}

impl QuerySource for FakeQuerySource {
    fn load(
        &self,
        scope: &QueryScope,
        selection: &SourceSelection,
        limits: &QueryLimits,
        access: ContentAccess,
    ) -> Result<QueryInput, QueryFailure> {
        self.calls
            .lock()
            .expect("fake query source call log poisoned")
            .push(QueryLoadCall {
                scope: scope.clone(),
                selection: selection.clone(),
                limits: *limits,
                access,
            });
        self.result.clone()
    }
}

/// Deterministic public API fake for CLI, writer, and external-consumer tests.
pub struct FakeQueryApi {
    result: Result<QueryResponse, QueryFailure>,
    requests: Mutex<Vec<QueryRequest>>,
}

impl FakeQueryApi {
    pub fn new(result: Result<QueryResponse, QueryFailure>) -> Self {
        Self {
            result,
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn requests(&self) -> Vec<QueryRequest> {
        self.requests
            .lock()
            .expect("fake query API request log poisoned")
            .clone()
    }
}

impl QueryApi for FakeQueryApi {
    fn execute(&self, request: &QueryRequest) -> Result<QueryResponse, QueryFailure> {
        self.requests
            .lock()
            .expect("fake query API request log poisoned")
            .push(request.clone());
        self.result.clone()
    }
}

pub const SHARED_QUERY_FIXTURE: &[u8] = include_bytes!("../fixtures/query/shared-v1.json");
