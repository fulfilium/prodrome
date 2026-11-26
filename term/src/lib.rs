//! The terms of FPL as data (SPEC §7): a term, its smart constructors, its
//! canonical print and its parse, and the environment that a term reads.
//! This crate evaluates nothing; `prodrome-fpl` holds the evaluator.
//!
//! - Build: the `mk_*` functions of [`fpl`] check the bounds of SPEC §7.
//! - Print and parse: [`fpl::print_term`] and [`fpl::parse_term`]. A term is
//!   a [`literal::codec::Literal`](prodrome_data::literal::codec::Literal),
//!   so a stored type can hold one in a field.
//!
//! `prodrome-core` re-exports [`term`] and [`schedule`] at the same paths,
//! and each item of [`fpl`] at `prodrome::fpl`.

pub mod fpl;
pub mod schedule;
pub mod term;
