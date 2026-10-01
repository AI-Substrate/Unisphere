//! Immutable supplied-evidence query views and the injected query service.
//!
//! [`QueryService`] is the imperative source-acquisition shell. [`QueryView`]
//! and [`execute_view`] are the reusable in-process functional core: no source
//! is reread while a retained view is queried, and each response contains only
//! validated privacy-aware projections.

mod cursor;
mod engine;
mod rows;
mod saved;
mod service;
mod view;

pub use engine::execute_view;
pub use rows::{EventRow, MessageRow, SessionRow, SourceRow, ToolRow, TurnRow};
pub use service::QueryService;
pub use unisphere_core::query::*;
pub use view::QueryView;
