//! Explicit fakes, fixtures, and subprocess isolation for development proof.
#![forbid(unsafe_code)]

pub mod fakes;
pub mod fixtures;
pub mod sealed;

pub use fakes::{FakeInspector, FakeReader, ReadCall};
pub use sealed::sealed_command;
