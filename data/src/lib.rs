//! The data of a Prodrome store: literals, events, the objects that hold
//! them, and their names (SPEC §2 to §4). It has no store and no clock.
//!
//! - A schema is the type of a store's events: implement
//!   [`schema::headed::Body`] and use [`schema::headed::Event`], or
//!   implement [`schema::Schema`].
//! - [`event::mk_sealed`] and [`change::mk_change`] put an event in an object.
//! - [`event::canonical_envelope`] prints an object: the bytes that a store
//!   keeps. [`event::parse_envelope`] reads them back and checks each field.
//! - [`event::seal_hash`] names an object: the SHA-256 of its print.
//! - A type derives [`literal::codec::Literal`] with
//!   `#[literal(crate = prodrome_data)]`.

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
