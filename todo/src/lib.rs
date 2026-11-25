//! The todo schema as data (SPEC §4, §6): its events, the record kind with
//! the host's payload, its vocabulary and its registers. This crate reads
//! no history and evaluates no term.
//!
//! - Build: the constructors of [`event`], such as [`event::mk_created`],
//!   [`event::mk_completed`] and [`event::mk_spec_revised`], return a
//!   [`event::TodoEvent`]. A spec is a term of `prodrome-term`.
//! - Print and parse: a [`event::TodoEvent`] is a
//!   [`schema::Schema`](prodrome_data::schema::Schema) with the vocabulary
//!   [`todo::TodoVocabulary`], so the functions of `prodrome_data::event`
//!   print, parse and name its envelopes.
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
