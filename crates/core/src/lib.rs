//! Pure contracts for explicit configuration, collection and adapter metadata.
//!
//! This crate performs no filesystem, environment, process, or network operations.
#![forbid(unsafe_code)]

mod catalog;
pub mod collection;
mod config;
mod errors;
pub mod git_notes;
mod ports;
pub mod prep;
pub mod query;
pub mod snapshot;
mod snapshot_collection;
pub mod status;
pub use catalog::{AdapterCapabilities, AdapterDescriptor, LocationHint};
pub use collection::*;
pub use git_notes::*;
pub use snapshot::*;
pub use snapshot_collection::{
    SnapshotCheckpoint, SnapshotCollection, SnapshotCollectionApi, SnapshotRequest,
};

pub use config::{
    ConfigOverrides, ConfigSource, Configuration, InspectionReport, InspectionRequest,
    MAX_CONFIG_BYTES,
};
pub use errors::{Failure, FailureKind, Location, ReadFailure};
pub use ports::{ConfigReader, InspectionApi};
