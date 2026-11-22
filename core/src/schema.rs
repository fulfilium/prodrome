//! A schema: the type of a store's events, and what each event writes
//! (SPEC §4, `docs/design-register-types.md` §5).
//!
//! The event type is the schema, so the parse refuses an event outside its
//! vocabulary at the boundary (SPEC §2). A store is opened at a schema
//! (`EventStore<S>`), and no object carries it, so `Genesis(label, nonce)`
//! does not change. [`crate::event::TodoEvent`] is the reference schema
//! ([`crate::todo`]), and a store reads every legacy envelope at it.

pub mod headed;

use std::fmt::Debug;

use crate::event::Actor;
use crate::fold::Product;
use crate::fpl::{Env, FplError};
use crate::literal::{Datetime, ProdromeError, Value, Vocabulary};
use crate::policy::Policy;
#[cfg(doc)]
use crate::store::Husking;
use crate::store::{HuskObject, Other};
use crate::term::Term;

/// A store's event type: its stored shape, its key, and the registers each
/// event writes.
///
/// - [`Schema::to_value`] prints every field in declared order, and
///   [`Schema::from_value`] refuses what a smart constructor refuses. No
///   name in the vocabulary may be an envelope's name
///   ([`crate::event::ENVELOPE_SIGNATURES`]), because a parse looks up the
///   envelopes first.
/// - The vocabulary is a value: a unit (`Default`) for a Rust schema, or
///   the declaration that a host admitted at run time ([`crate::declared`]).
/// - [`Schema::at`] is the writer's stamp: data, never an order (SPEC §1).
/// - [`Schema::Registers`] is a [`Product`] of semilattices; each register is
///   a [`crate::fold::Frontier`] or an accumulating set, read under its
///   [`crate::fold::RegisterType`] (SPEC §6, "A schema's registers").
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

    /// The registers of one entity.
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

    /// Whether this event is about standing. The reference policy binds an
    /// event that is not, whoever wrote it, and a policy that requires
    /// signatures asks of every event (SPEC §5).
    fn asks(&self) -> bool {
        true
    }

    /// The husk that `print` holds, where `print` does not hash to its name
    /// (SPEC §3, "A husk").
    ///
    /// The default reads no husk, so such a print is tampered. A store calls
    /// this method only after a file fails its hash. A schema that has husks
    /// implements [`Husking`] and answers `HuskObject::of_other(print).ok()`;
    /// only a [`Husking`] schema can make a [`HuskObject`].
    fn husk(_print: &Other<'_>) -> Option<HuskObject<Self>> {
        None
    }
}

/// A schema's price (design §4): the FPL term of each candidate of an
/// entity's reading.
///
/// A schema without this trait has no price, which differs from FPL's
/// `Absent`. The core prices a reading in conflict as `Least` over the terms
/// of its candidates ([`crate::fold::price`]), so a conflict takes the price
/// of its most urgent candidate (law 34: a candidate that prices `∅` never
/// lowers one that prices a number). This `Least` is not a register's join,
/// and it is not FPL's `Conj`, which aggregates different entities.
pub trait Price: Schema {
    /// The term of each candidate of the reading, and none for a candidate
    /// that prices nothing. An empty vector means that the entity has no
    /// price.
    ///
    /// # Errors
    ///
    /// A term a smart constructor refuses.
    fn terms(registers: &Self::Registers<'_>) -> Result<Vec<Term>, FplError>;
}

/// A price that changes with the entity's history: the fulfillment function
/// of SPEC §6.4, with a piece at every instant that one of its registers was
/// written ([`crate::fold::flatten`]).
pub trait History: Price {
    /// The term of each candidate at one moment of the history.
    ///
    /// `now` is the reading at the moment. A register that is not written
    /// yet reads as `first`, its first write. `head` is the price before the
    /// first moment, for a candidate that prices nothing at the moment.
    /// An implementation must keep `moment(r, &Default::default(), None)`
    /// equal to `Price::terms(r)`.
    ///
    /// # Errors
    ///
    /// A term a smart constructor refuses.
    fn moment(
        now: &Self::Registers<'_>,
        first: &Self::Registers<'_>,
        head: Option<&Term>,
    ) -> Result<Vec<Term>, FplError>;
}

/// What an FPL `Ref` reads of an entity in this store (SPEC §6.1, §7.2); a
/// `Ref` names the entity by its key as a string.
///
/// An entity in another store reaches FPL through an environment that the
/// host passes in, not through this trait.
pub trait Bind: Schema<Key: AsRef<str>> {
    /// Records in `env` what FPL terms read of the entity at a reading.
    fn bind(key: &Self::Key, registers: &Self::Registers<'_>, env: &mut Env);
}

/// What a row of the list shows of an entity (SPEC §6.7), and when the row
/// tells a reader that the policy refused or does not confirm a write.
/// Only [`crate::view::entries`] uses it.
pub trait Row: Schema {
    /// What a row shows of an entity's registers.
    type Reading: Clone + PartialEq + std::fmt::Debug;

    fn reading(registers: &Self::Registers<'_>) -> Self::Reading;

    /// Whether the claimed reading, under every writer, disputes the
    /// confirmed reading so that a reader must be told.
    fn disputes(confirmed: &Self::Reading, claimed: &Self::Reading) -> bool;

    /// Whether the reading shows a write that the policy binds and does not
    /// confirm.
    fn unconfirmed(registers: &Self::Registers<'_>, policy: &impl Policy<Self>) -> bool;
}
