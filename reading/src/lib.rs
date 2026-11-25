//! The readings of a schema's history (SPEC §6), on the DAG of
//! `prodrome-dag`, the terms of `prodrome-term` and the evaluator of
//! `prodrome-fpl`, and their instances for the todo schema of
//! `prodrome-todo`. This crate reads no store.
//!
//! - Ask: a schema implements the traits of [`schema`] to say what a
//!   reading prices ([`schema::Price`]), how its price changes with its
//!   history ([`schema::History`]), what a `Ref` reads of it
//!   ([`schema::Bind`]) and what its row shows ([`schema::Row`]).
//! - Price: [`fold::price`] prices one entity's registers, and
//!   [`fold::flatten`] gives each entity's price as a function of time.
//!   [`fold::env`] and [`fold::link_specs`] give what a term reads and
//!   links against.
//! - List: [`view::entries`] gives one [`view::Entry`] for each entity of a
//!   DAG at a moment, with its price and its value, and
//!   [`view::list_order`] sorts the entries.
//!
//! [`todo`](mod@todo) implements the four traits for the todo schema.
//! `prodrome-core` re-exports each module of this crate at the same path.

pub mod fold;
pub mod schema;
pub mod todo;
pub mod view;
