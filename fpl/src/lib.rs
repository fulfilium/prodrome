//! The evaluator of FPL (SPEC §7) on the terms of `prodrome-term`: the
//! fulfillment of a closed term, its explanation, the linking of
//! references, the chain compiler, breakpoints, and when the value that a
//! view observes next changes.
//!
//! - Evaluate: [`fpl::link`] binds each `Ref` of a term and returns a
//!   [`fpl::Closed`] term, and [`fpl::fulfillment`] reads it at an instant
//!   against an [`fpl::Env`].
//! - Explain: [`fpl::explained`] returns the term's shape with the value of
//!   each node.
//! - Observe: [`observe::next_change`] gives the first instant at which an
//!   [`observe::Observation`] of the value changes.
//!
//! [`fpl`] re-exports each item of `prodrome_term::fpl`, so a program that
//! uses this crate names a constructor and the evaluator at one path.
//! `prodrome-core` re-exports each module of this crate at the same path.

pub mod breaks;
pub mod chain;
pub mod fpl;
pub mod observe;
