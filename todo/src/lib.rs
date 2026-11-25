//! The todo schema as data (SPEC §4, §6). This crate reads no history and
//! evaluates no term.
//!
//! - Build: the constructors of [`event`] return a [`event::TodoEvent`].
//! - Print and parse: a [`event::TodoEvent`] is a
//!   [`schema::Schema`](prodrome_data::schema::Schema) with the vocabulary
//!   [`todo::TodoVocabulary`].
//! - Payload: a host defines its record's fields with a
//!   [`payload::Payload`]. `reference` is the payload of the conformance
//!   vectors.
//!
//! `prodrome-core` re-exports [`event`], [`payload`] and `reference` at
//! the same paths, and the items of [`todo`](mod@todo) at `prodrome::todo` and
//! `prodrome::fold`.

pub mod event;
pub mod payload;
/// The reference payload, with which `conformance/*.py` was taken. A host
/// defines its own payload and can turn off this default feature.
#[cfg(feature = "reference")]
pub mod reference;
pub mod todo;
