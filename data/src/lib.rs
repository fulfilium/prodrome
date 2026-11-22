//! The data of a Prodrome store: values, events, objects and their names
//! (SPEC §2 to §4). It has no store, no evaluator and no clock.
//!
//! - Build: a schema is a [`schema::headed::Event`] over a [`schema::headed::Body`].
//!   [`schema::headed::Event::new`] (or `of`) takes the key, the instant as a
//!   [`literal::Datetime`], the [`event::Actor`] and the body. A type derives
//!   [`literal::codec::Literal`] with `#[literal(crate = prodrome_data)]`.
//! - Stamp: [`instant::datetime_of`] turns a `chrono` time into a `Datetime`, and
//!   [`instant::instant_of`] back; [`instant::iso`] and [`instant::parse_iso`] give ISO text.
//! - Print and parse: [`event::canonical`] and [`event::parse_event`], which takes the
//!   vocabulary [`schema::headed::Constructors`] (`Constructors::<B>::default()`).
//! - Hash: [`change::mk_change`] puts an event under a [`genesis::mk_genesis`] in an
//!   [`event::Envelope::Change`]. [`event::canonical_envelope`] prints an `Envelope`,
//!   [`event::parse_envelope`] reads it, and [`event::seal_hash`] names it (SHA-256).

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
