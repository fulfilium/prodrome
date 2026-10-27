//! The laws of SPEC §9 that ask nothing of a schema, as functions a program
//! runs over its own schemas and stores.
//!
//! An application that defines a schema checks it with the same laws this
//! crate checks its reference schemas with. Each function takes data (a
//! store's objects, an event, two replicas) and answers `Ok` or the law it
//! found broken, so a caller drives it from its own tests: over generated
//! histories, or over the objects a store already holds.
//!
//! - [`printed`]: law 1, `print ∘ parse` is the identity on every object, and
//!   the hash of its print is its name.
//! - [`free`], [`routed`] and [`valued`]: law 33, a reading is a function of
//!   the object set, and each write joins exactly the registers it names.
//! - [`synced`]: law 35, sync is the join.
//!
//! [`history`] builds a history from events, for a caller that generates
//! them. The module is behind the `laws` feature, which is on by default; a
//! program that turns the default features off enables it for its tests.

#![warn(clippy::pedantic)]

mod history;
mod printed;
mod schema;
mod synced;

pub use history::history;
pub use printed::printed;
pub use schema::{free, routed, valued, Shuffle};
pub use synced::synced;

/// A law that does not hold: its number in SPEC §9, and what the check saw.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("law {law}: {what}")]
pub struct Broken {
    /// The law's number in SPEC §9.
    pub law: u32,
    /// What the check saw.
    pub what: String,
}

impl Broken {
    fn of(law: u32, what: impl Into<String>) -> Broken {
        Broken {
            law,
            what: what.into(),
        }
    }
}
