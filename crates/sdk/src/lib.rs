//! In-process inspection of explicitly supplied source-root configuration.
//!
//! This foundation validates configuration; it does not read sessions or collect
//! telemetry. There is no daemon, async runtime, ambient configuration lookup,
//! environment expansion, or network client. Only an explicit [`ConfigSource::File`]
//! invokes the selected reader.
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

mod fs;
mod service;

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
