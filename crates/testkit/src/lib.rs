//! Explicit fakes, fixtures, and subprocess isolation for development proof.
#![forbid(unsafe_code)]

pub mod collection;
pub mod fakes;
pub mod fixtures;
#[cfg(unix)]
pub mod git_notes;
pub mod prep;
pub mod query;
pub mod sealed;
pub mod status;

pub use fakes::{FakeInspector, FakeReader, ReadCall};
pub use sealed::sealed_command;
