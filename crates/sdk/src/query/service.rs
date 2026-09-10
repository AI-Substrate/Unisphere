use unisphere_core::query::{QueryApi, QueryFailure, QueryRequest, QueryResponse, QuerySource};

use super::{QueryView, engine};

/// Injected query application service. It acquires one bounded source view per
/// [`QueryApi::execute`] call and has no ambient filesystem or process access.
pub struct QueryService<S> {
    source: S,
}

impl<S> QueryService<S> {
    pub const fn new(source: S) -> Self {
        Self { source }
    }

    pub const fn source(&self) -> &S {
        &self.source
    }
}

impl<S: QuerySource> QueryService<S> {
    /// Open one immutable view for repeated in-process queries.
    pub fn open_view(&self, request: &QueryRequest) -> Result<QueryView, QueryFailure> {
        request.validate()?;
        let selection = engine::source_selection(request)?;
        let access = request.content_access()?;
        let input =
            self.source
                .load(&request.scope, &selection, &request.limits, access.clone())?;
        QueryView::from_input_for(
            input,
            request.scope.clone(),
            selection,
            access,
            &request.limits,
        )
    }
}

impl<S: QuerySource> QueryApi for QueryService<S> {
    fn execute(&self, request: &QueryRequest) -> Result<QueryResponse, QueryFailure> {
        let view = self.open_view(request)?;
        super::execute_view(&view, request)
    }
}
