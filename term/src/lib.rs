//! The terms of FPL as data (SPEC §7): a term, its smart constructors, its
//! canonical print and its parse, and the environment that a term reads.
//! This crate evaluates nothing; `prodrome-fpl` holds the evaluator.
//!
//! - Build: the `mk_*` functions of [`fpl`], such as [`fpl::mk_flat`] and
//!   [`fpl::mk_decay`], check the bounds of SPEC §7 and return a
//!   [`term::Term`]. A span is a [`fpl::Delta`], and an instant an
//!   [`fpl::Instant`].
//! - Print and parse: [`fpl::print_term`] and [`fpl::parse_term`]. A term is
//!   a [`literal::codec::Literal`](prodrome_data::literal::codec::Literal),
//!   so a stored type can hold one in a field.
//! - Read the shape: [`term::TermF`] is one layer of a term, and
//!   [`term::schema`] names the fields of each layer.
//!
//! `prodrome-core` re-exports [`term`] and [`schedule`] at the same paths,
//! and each item of [`fpl`] at `prodrome::fpl`.

pub mod fpl;
pub mod schedule;
pub mod term;
