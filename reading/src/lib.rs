//! The readings of a schema's history (SPEC §6), on the DAG of
//! `prodrome-dag`, the terms of `prodrome-term` and the evaluator of
//! `prodrome-fpl`, and their instances for the todo schema of
//! `prodrome-todo`. This crate reads no store.
//!
//! - Ask: a schema implements the traits of [`schema`]: [`schema::Price`],
//!   [`schema::History`], [`schema::Bind`] and [`schema::Row`].
//! - Price: [`fold::price`], [`fold::flatten`], [`fold::env`] and
//!   [`fold::link_specs`].
//! - List: [`view::entries`] and [`view::list_order`].
//!
//! [`todo`](mod@todo) implements the four traits for the todo schema.
//! `prodrome-core` re-exports each module of this crate at the same path.

pub mod fold;
pub mod schema;
pub mod todo;
pub mod view;
