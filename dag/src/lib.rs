//! The readings of a Prodrome store: the DAG of its objects as a value, the
//! fold of the DAG into each entity's stream of events, the policies that
//! decide which events bind, and the proof of signatures (SPEC §3, §5, §6).
//! It has no files and no evaluator.
//!
//! - Hold: collect `(Hash, Envelope)` pairs into a [`dag::Dag`], or read
//!   named prints with [`dag::Dag::from_prints`] and the vocabulary of the
//!   schema.
//! - Fold: [`dag::Dag::nodes`] gives each object's place, and
//!   [`registers::fold`] gives a [`registers::Folded`], whose `prodromes()`
//!   hold each entity's stream of `Stamp`s.
//! - Read: [`fold::read`] gives an entity's registers at an instant under a
//!   [`policy::Policy`], such as `Untrusted` or `Everything`, which are
//!   defined in `prodrome-data` and re-exported in [`policy`].
//! - Prove and check: [`sign::Secret`] signs, [`sign::Proof`] reads which
//!   objects a registered key signed, [`policy::Proven`] binds only those,
//!   and [`dag::Dag::verify`] lists what is wrong with a history.
//!
//! `prodrome-core` re-exports each module at the same path under
//! `prodrome::`.

pub mod dag;
pub mod fold;
pub mod policy;
pub mod registers;
pub mod sign;
#[doc(hidden)]
pub mod spread;
