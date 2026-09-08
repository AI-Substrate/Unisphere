//! Pure contracts for explicit configuration inspection.
//!
//! This crate performs no filesystem, environment, process, or network operations.
#![forbid(unsafe_code)]

pub mod collection;
mod config;
mod errors;
mod ports;
pub use collection::*;

pub use config::{
    ConfigOverrides, ConfigSource, Configuration, InspectionReport, InspectionRequest,
    MAX_CONFIG_BYTES,
};
pub use errors::{Failure, FailureKind, Location, ReadFailure};
pub use ports::{ConfigReader, InspectionApi};
