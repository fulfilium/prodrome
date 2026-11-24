//! The store of a Prodrome: a directory of objects on disk, with its lock,
//! its durable writes and its record of heads, and the other replicas, in
//! memory and over another replica (SPEC §3). It has no evaluator.
//!
//! - Open and write: [`store::EventStore::new`] opens a store under a
//!   policy, [`store::EventStore::init`] writes its genesis, and
//!   [`store::EventStore::append`] writes an event durably.
//! - Read: [`store::EventStore::dag`] gives the store's DAG, and
//!   [`store::EventStore::verify`] lists what is wrong with it. The crate
//!   `prodrome-dag` folds the DAG and reads an entity's registers under the
//!   store's policy.
//! - Replicate: [`store::Replica`] is what every replica does, and
//!   [`store::sync`] and [`store::accept`] move objects between replicas.
//!   A door ([`store::Doors`]) takes objects from a [`store::Sender`], and
//!   [`store::frontier`] names what a replica has reached.
//! - Attest: [`snapshot::attest`] gives the snapshot that a writer attests
//!   of a prodrome.
//!
//! `prodrome-core` re-exports each module at the same path under
//! `prodrome::`.

pub mod snapshot;
pub mod store;

// The property tests' case counts, one rule with the core's tests.
#[cfg(test)]
#[path = "../../core/tests/common/cases.rs"]
mod cases;
