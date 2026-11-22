//! The laws of SPEC §9 that ask nothing of a schema, as functions that a
//! program runs over its own schemas and stores, from its own tests.
//!
//! Each function answers `Ok` or the [`Broken`] law, with what differs.
//!
//! - [`printed`]: law 1, `print ∘ parse` is the identity on every object, and
//!   the hash of its print is its name.
//! - [`free`], [`routed`] and [`valued`]: law 33, a reading is a function of
//!   the object set, and each write joins exactly the registers it names.
//! - [`synced`]: the join clauses of law 35, between replicas of any two
//!   kinds. The kit does not check the clauses of law 35 on appends in memory
//!   and on overlays.
//! - [`script`]: law 68, a random script of steps over several replicas,
//!   with every invariant checked after every step.
//!
//! The module is behind the `laws` feature, which is off by default.

#![warn(clippy::pedantic)]

mod history;
mod holding;
mod printed;
mod reading;
mod schema;
pub mod script;
mod shuffle;
mod synced;

pub use history::history;
pub use holding::holding;
pub use printed::printed;
pub use reading::Registers;
pub use schema::{free, routed, valued};
pub use shuffle::{a_shuffle, Shuffle};
pub use synced::synced;

/// A law that does not hold.
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
