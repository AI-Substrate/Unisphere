//! In-process explicit configuration inspection and injected session collection.
//!
//! Configuration inspection never reads session roots. [`Collector`] separately
//! composes caller-selected loading, pure mapping and output ports. There is no
//! daemon, async runtime or implicit environment/global configuration lookup.
//! Source-derived collection is not a lossless or complete-session claim.
//!
//! ```
//! use unisphere_sdk::{ConfigSource, InspectionRequest, inspect};
//!
//! let request = InspectionRequest {
//!     source: ConfigSource::Inline(br#"{"source_roots":["~/literal","relative"]}"#.to_vec()),
//!     ..InspectionRequest::default()
//! };
//! let report = inspect(&request)?;
//! assert_eq!(report.configuration.source_roots, ["~/literal", "relative"]);
//! # Ok::<(), unisphere_sdk::Failure>(())
//! ```
//!
//! Failures expose stable typed categories, fixed diagnostic copy, and safe
//! structural locations, never raw parser errors or configuration values.
//!
//! ```
//! use unisphere_sdk::{ConfigSource, FailureKind, InspectionRequest, inspect};
//!
//! let request = InspectionRequest {
//!     source: ConfigSource::Inline(br#"{"source_roots":[" "]}"#.to_vec()),
//!     ..InspectionRequest::default()
//! };
//! let failure = inspect(&request).unwrap_err();
//! assert_eq!(failure.kind(), FailureKind::InvalidConfiguration);
//! assert_eq!(failure.code(), "UNI-CONFIG-INVALID");
//! assert_eq!(failure.location().unwrap().field.as_deref(), Some("source_roots[0]"));
//! assert!(!failure.fix().is_empty());
//! ```
//!
//! For injected I/O or caller defaults use [`Inspector`] through [`InspectionApi`].
//! Precedence is defaults < document < explicit overrides. Every supplied layer
//! is validated, even when a higher-priority layer replaces it; `Some(vec![])`
//! clears roots, whereas `None` retains the lower-priority roots.
#![forbid(unsafe_code)]

pub mod collection;
mod fs;
pub mod prep;
pub mod query;
mod service;
pub mod snapshot;
pub mod status;
pub use collection::{Collector, collect_batch};
pub use query::{QueryService, QueryView, execute_view};
pub use status::{StatusCursor, StatusService};
pub mod git_notes;
pub use git_notes::GitNotesCollector;
pub use snapshot::SnapshotCollector;
pub use unisphere_core::collection::*;
pub use unisphere_core::git_notes::*;
pub use unisphere_core::{
    MappedSnapshot, NativeSnapshot, SnapshotAdapter, SnapshotCheckpoint, SnapshotCollection,
    SnapshotCollectionApi, SnapshotDiagnostic, SnapshotFormat, SnapshotLimits, SnapshotLoader,
    SnapshotRecord, SnapshotRef, SnapshotRequest,
};

pub use fs::StdConfigReader;
pub use service::Inspector;
pub use unisphere_core::{
    ConfigOverrides, ConfigReader, ConfigSource, Configuration, Failure, FailureKind,
    InspectionApi, InspectionReport, InspectionRequest, Location, MAX_CONFIG_BYTES, ReadFailure,
};

/// Inspect explicit configuration with empty defaults and [`StdConfigReader`].
///
/// `Defaults` and `Inline` perform no file reads. `File` must be an absolute
/// caller-supplied path. Inline and file documents are limited to
/// [`MAX_CONFIG_BYTES`]; an oversized inline document is invalid configuration,
/// while an oversized file produces [`ReadFailure::TooLarge`].
pub fn inspect(request: &InspectionRequest) -> Result<InspectionReport, Failure> {
    Inspector::new(StdConfigReader).inspect(request)
}
