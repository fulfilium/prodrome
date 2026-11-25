//! The readings of a Prodrome store: the DAG of its objects as a value, the
//! fold of the DAG into each entity's stream of events, the policies that
//! decide which events bind, and the proof of signatures (SPEC §3, §5, §6).
//! It has no files and no evaluator.
//!
//! - Hold: [`dag::Dag`] collects `(Hash, Envelope)` pairs.
//! - Fold: [`registers::fold`] gives a [`registers::Folded`] over
//!   [`dag::Dag::nodes`].
//! - Read: [`fold::read`] gives an entity's registers at an instant under a
//!   [`policy::Policy`].
//! - Prove and check: [`sign::Proof`] reads which objects a registered key
//!   signed, and [`dag::Dag::verify`] lists what is wrong with a history.

pub mod dag;
pub mod fold;
pub mod policy;
pub mod registers;
pub mod sign;
#[doc(hidden)]
pub mod spread;
