//! The store of a Prodrome: a directory of objects on disk, with its lock,
//! its durable writes and its record of heads, and the other replicas, in
//! memory and over another replica (SPEC §3). It has no evaluator.
//!
//! - Open and write: [`store::EventStore::new`], [`store::EventStore::init`]
//!   and [`store::EventStore::append`].
//! - Read: [`store::EventStore::dag`] and [`store::EventStore::verify`].
//!   The crate `prodrome-dag` folds the DAG.
//! - Replicate: [`store::Replica`], [`store::sync`], [`store::accept`],
//!   [`store::Doors`], [`store::Sender`] and [`store::frontier`].
//! - Attest: [`snapshot::attest`].
//!
//! `prodrome-core` re-exports each module at the same path under
//! `prodrome::`.

pub mod snapshot;
pub mod store;

// The property tests' case counts, one rule with the core's tests.
#[cfg(test)]
#[path = "../../core/tests/common/cases.rs"]
mod cases;
