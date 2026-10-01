//! Pure supplied-data contracts for bounded session queries.
//!
//! Native observations are not output rows. Query services must explicitly
//! project them before handing a response to a writer. This module acquires no
//! filesystem, environment, clock, process, or network capability.

use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryNameError;

impl fmt::Display for QueryNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unrecognized query value")
    }
}
impl Error for QueryNameError {}

macro_rules! query_enum {
    ($(#[$attribute:meta])* pub enum $name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$attribute])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire),+ }
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.as_str()) }
        }
        impl std::str::FromStr for $name {
            type Err = $crate::query::QueryNameError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value { $($wire => Ok(Self::$variant),)+ _ => Err($crate::query::QueryNameError) }
            }
        }
        impl Ord for $name {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.as_str().cmp(other.as_str()) }
        }
        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
        }
    };
}

mod error;
mod fingerprint;
mod identity;
mod model;
mod schema;
mod source;

pub use error::*;
pub use fingerprint::*;
pub use identity::*;
pub use model::*;
pub use schema::*;
pub use source::*;
