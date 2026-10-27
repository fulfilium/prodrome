//! The laws of SPEC §9 that ask nothing of a schema, as functions a program
//! runs over its own schemas and stores.
//!
//! An application that defines a schema checks it with the same laws this
//! crate checks its reference schemas with. Each function takes data (a
//! store's objects, an event, replicas) and answers `Ok` or the law it found
//! [`Broken`], naming what differs. A caller drives it from its own tests:
//! over histories that [`a_shuffle`] and its own event strategy draw, or over
//! the objects a store already holds.
//!
//! - [`printed`]: law 1, `print ∘ parse` is the identity on every object, and
//!   the hash of its print is its name.
//! - [`free`], [`routed`] and [`valued`]: law 33, a reading is a function of
//!   the object set, and each write joins exactly the registers it names.
//! - [`synced`]: the join clauses of law 35 (a cut sync takes nothing, a
//!   down-set then the whole reads as the union, a second sync takes nothing,
//!   and either order into a third replica reads the same), between replicas
//!   of any two kinds. The kit does not check law 35's clauses on appends in
//!   memory and on overlays.
//!
//! [`history`] builds a history from events, [`holding`] a store in memory
//! that holds a history, and [`Shuffle`] says how [`free`] reorders and
//! splits one. The module is behind the `laws` feature, which is off by
//! default; a program enables it for its tests.

#![warn(clippy::pedantic)]

mod history;
mod holding;
mod printed;
mod reading;
mod schema;
mod shuffle;
mod synced;

pub use history::history;
pub use holding::holding;
pub use printed::printed;
pub use schema::{free, routed, valued};
pub use shuffle::{a_shuffle, Shuffle};
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
