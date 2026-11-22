//! A schema: the type of a store's events, and what each event writes
//! (SPEC §4, `docs/design-register-types.md` §5).

pub mod headed;

use std::fmt::Debug;

use crate::event::Actor;
use crate::fold::Product;
#[cfg(doc)]
use crate::husk::Husking;
use crate::husk::{HuskObject, Other};
use crate::literal::{Datetime, ProdromeError, Value, Vocabulary};

/// A store's event type: its stored shape, its key, and the registers each
/// event writes (SPEC §8).
///
/// No name in the vocabulary may be an envelope's name
/// ([`crate::event::ENVELOPE_SIGNATURES`]), because a parse looks up the
/// envelopes first. [`Schema::at`] is the writer's stamp: data, never an
/// order (SPEC §1).
pub trait Schema: Clone + PartialEq + Debug + Send + Sync + 'static {
    /// The whitelist of SPEC §2 for this schema's events, as a value.
    type Vocabulary: Vocabulary + Clone + Debug + Send + Sync;

    /// The entity that an event is about, such as a todo.
    type Key: Clone + Ord + Debug + Send + Sync;

    /// The name of a register that a write supersedes in.
    type Register: Copy + Ord + Debug + Send + Sync + 'static;

    /// Every register that a write supersedes in, in the order that a
    /// reading lists them.
    fn registers(vocabulary: &Self::Vocabulary) -> Vec<Self::Register>;

    /// The registers of one entity (SPEC §6, "A schema's registers").
    type Registers<'a>: Product<'a, Schema = Self>;

    /// The event as a literal: every field, in declared order.
    fn to_value(&self) -> Value;

    /// Parses a literal into an event at `vocabulary`, through every smart
    /// constructor.
    ///
    /// # Errors
    ///
    /// A constructor the vocabulary does not name, or a field a smart
    /// constructor refuses.
    fn from_value(vocabulary: &Self::Vocabulary, value: &Value) -> Result<Self, ProdromeError>;

    fn key(&self) -> &Self::Key;

    fn at(&self) -> Datetime;

    fn actor(&self) -> &Actor;

    /// The registers whose frontier this event joins, and so whose writes
    /// it supersedes, each once (SPEC §6). A register that only
    /// accumulates, such as the todo's tendings, is not among them.
    fn writes(&self) -> impl Iterator<Item = Self::Register>;

    /// Whether this event is about standing (SPEC §5).
    fn asks(&self) -> bool {
        true
    }

    /// The husk that `print` holds, where `print` does not hash to its name
    /// (SPEC §3, "A husk").
    ///
    /// A store calls this method only after a file fails its hash. The
    /// default reads no husk, so such a print is tampered. A schema that has
    /// husks implements [`Husking`] and answers
    /// `HuskObject::of_other(print).ok()`.
    fn husk(_print: &Other<'_>) -> Option<HuskObject<Self>> {
        None
    }
}
