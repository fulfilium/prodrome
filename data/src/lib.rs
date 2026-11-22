//! The data of a Prodrome store: values, events, objects and their names
//! (SPEC §2 to §4). It has no store, no evaluator and no clock.
//!
//! - A schema is a [`schema::headed::Event`] over a [`schema::headed::Body`]. A
//!   type derives [`literal::codec::Literal`] with `#[literal(crate = prodrome_data)]`.
//! - An event's instant is a [`literal::Datetime`]. The module [`instant`]
//!   converts it to a `chrono` time and to ISO text, and back.
//! - [`event::canonical`] prints an event, and [`event::parse_event`] reads it.
//! - [`change::mk_change`] puts an event in an object. [`event::canonical_envelope`]
//!   prints the object as a store keeps it, and [`event::parse_envelope`] reads it.
//! - [`event::seal_hash`] names an object: the SHA-256 of its print.

#[doc(hidden)]
pub mod bitset;
pub mod change;
pub mod event;
pub mod fold;
pub mod genesis;
pub mod husk;
pub mod instant;
pub mod literal;
pub mod payload;
pub mod schema;
pub mod sign;
pub mod snapshot;
pub mod stamp;
pub mod topo;
