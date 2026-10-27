//! A schema: what a store's events are, and what each one writes
//! (`docs/design-register-types.md` §5).
//!
//! THE EVENT TYPE IS THE SCHEMA. Its values are the events the schema names,
//! a closed sum of constructors with frozen fields, so an event outside the
//! vocabulary is not a value of the type: §2's parse refuses it at the
//! boundary. The impl says the rest: the entity each event is about, the
//! registers it writes, and which events a policy is asked about.
//!
//! A schema is the STORE'S type (`EventStore<S>`), as the payload was. No
//! object carries it: a stored print is parsed against the schema the store
//! was opened at, so `Genesis(label, nonce)` stays frozen.
//!
//! [`crate::event::TodoEvent`] is the reference schema ([`crate::todo`]):
//! §4's six kinds and the record kind, and every legacy envelope is read at it.
//! A schema MAY price its entities ([`Price`]); one that does not has no
//! price at all, which is not FPL's `Absent`.

pub mod headed;

use std::fmt::Debug;

use crate::event::Actor;
use crate::fold::Product;
use crate::fpl::{Env, FplError};
use crate::literal::{Datetime, ProdromeError, Value, Vocabulary};
use crate::policy::Policy;
use crate::term::Term;

/// A store's events and what they mean.
///
/// # The contract
///
/// - [`Schema::Vocabulary`], [`Schema::to_value`] and [`Schema::from_value`]
///   are the STORED SHAPE: every constructor an event may print, with its
///   declared field order, the print in that order, and the parse that refuses
///   what a smart constructor would. None of the vocabulary's names may be an
///   envelope's (`Sealed`, `Woven`, `Genesis`, `Change`, `Snapshot`), which are
///   looked up first.
/// - The vocabulary is a VALUE, the schema as data: what a parse reads and
///   what a boundary names registers by. A Rust schema's is a unit its type
///   already says everything of (`Default`); a declared one's is the
///   declaration a host admitted at run time ([`crate::declared`]). A store
///   is opened at one, and no object carries it.
/// - [`Schema::key`], [`Schema::at`] and [`Schema::actor`] are what every
///   event says: about what, when its writer thought it was (data, never an
///   order), and who.
/// - [`Schema::Registers`] is an entity's REGISTERS, a [`Product`] of
///   semilattices: its `join` is the route from an event to the writes it
///   makes, each register a [`crate::fold::Frontier`] or an accumulating set,
///   read under its [`crate::fold::RegisterType`].
/// - [`Schema::writes`] is the route's NAMES: the registers whose frontier
///   this event joins and whose writes it therefore supersedes (§6). A register
///   that only accumulates, such as the todo's tendings, is not among them.
/// - [`Schema::asks`] is whether an event is about standing: the reference
///   policy binds one it answers `false` for whoever wrote it, and a policy
///   that requires signatures asks of every event (§5).
pub trait Schema: Clone + PartialEq + Debug + Send + Sync + 'static {
    /// §2's whitelist for this schema's events, as a value: the schema as
    /// data, what a store is opened at.
    type Vocabulary: Vocabulary + Clone + Debug + Send + Sync;

    /// What an event is about: a todo, a review.
    type Key: Clone + Ord + Debug + Send + Sync;

    /// A register's name, among those a write supersedes in.
    type Register: Copy + Ord + Debug + Send + Sync + 'static;

    /// Every register a write supersedes in, in the order a reading lists
    /// them, at the schema `vocabulary`.
    fn registers(vocabulary: &Self::Vocabulary) -> Vec<Self::Register>;

    /// An entity's registers: a product of semilattices.
    type Registers<'a>: Product<'a, Schema = Self>;

    /// The event as a literal: every field, in declared order.
    fn to_value(&self) -> Value;

    /// The parse at the schema `vocabulary`: a literal into an event,
    /// through every smart constructor.
    ///
    /// # Errors
    ///
    /// A constructor the vocabulary does not name, or a field a smart
    /// constructor refuses.
    fn from_value(vocabulary: &Self::Vocabulary, value: &Value) -> Result<Self, ProdromeError>;

    fn key(&self) -> &Self::Key;

    fn at(&self) -> Datetime;

    fn actor(&self) -> &Actor;

    /// The registers this event writes, each once.
    fn writes(&self) -> impl Iterator<Item = Self::Register>;

    /// Is this event about standing? The reference policy binds every other
    /// whoever wrote it (§5).
    fn asks(&self) -> bool {
        true
    }
}

/// A schema's PRICE (design §4): the §7 term each candidate of an entity's
/// reading prices as. Optional: a schema without it has no price at all,
/// which is not FPL's `Absent`.
///
/// A reading in conflict holds more than one candidate. The core prices the
/// reading as `Least` over their terms ([`crate::fold::price`]): the meet in
/// the fulfillment order, "price a conflict as its most urgent candidate",
/// whose top is `∅`, so a candidate that prices `∅` never lowers one that
/// prices a number (law 34). It is not a register's join, and FPL's `Conj`,
/// which aggregates different entities, is neither.
pub trait Price: Schema {
    /// The term each candidate of the reading prices as, none for one that
    /// prices nothing. Empty: the entity has no price here.
    ///
    /// # Errors
    ///
    /// A term a smart constructor refuses.
    fn terms(registers: &Self::Registers<'_>) -> Result<Vec<Term>, FplError>;
}

/// A price that CHANGES WITH THE ENTITY'S HISTORY: §6.4's fulfillment
/// function, a piece at every instant one of its registers was written
/// ([`crate::fold::flatten`]). A schema whose price is only its reading's
/// has none.
pub trait History: Price {
    /// The term each candidate prices as at one MOMENT of the history: `now`,
    /// the reading there, with a register unwritten yet read as it was first
    /// written (`first`), and `head`, the history's price before its first
    /// moment, for a candidate that prices nothing there.
    ///
    /// With nothing first written and no head, a moment is its reading:
    /// `moment(r, &Default::default(), None)` is `Price::terms(r)`.
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

/// What FPL's `Ref`s read of an entity in this store (§6.1, §7.2): its key is
/// the name a `Ref` gives it, so it reads as a string, and [`Bind::bind`]
/// records what the environment holds of it. An entity in ANOTHER store is
/// a host's environment, passed in, never a method of a schema.
pub trait Bind: Schema<Key: AsRef<str>> {
    /// What FPL's terms read of an entity at a reading, recorded in `env`.
    fn bind(key: &Self::Key, registers: &Self::Registers<'_>, env: &mut Env);
}

/// What a row of the list shows of an entity (§6.7), and when it tells a
/// reader the policy refused or does not confirm a write.
/// [`crate::view::entries`] asks for it, beside a [`History`] and a
/// [`Bind`], and nothing else does.
pub trait Row: Schema {
    /// What a row shows of an entity's registers.
    type Reading: Clone + PartialEq + std::fmt::Debug;

    fn reading(registers: &Self::Registers<'_>) -> Self::Reading;

    /// Does the CLAIMED reading, under every writer, dispute the confirmed
    /// one where a reader should be told?
    fn disputes(confirmed: &Self::Reading, claimed: &Self::Reading) -> bool;

    /// Does the reading show a write the policy binds and does not confirm?
    fn unconfirmed(registers: &Self::Registers<'_>, policy: &impl Policy<Self>) -> bool;
}
