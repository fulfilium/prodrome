//! What a reading asks of a schema: its price, its history, its binding
//! for FPL and its row (SPEC §6, §6.7). The trait [`Schema`] itself is
//! defined in `prodrome-data` and re-exported here, with its module
//! [`headed`].

pub use prodrome_data::schema::*;

use prodrome_dag::policy::Policy;
use prodrome_term::fpl::{Env, FplError};
use prodrome_term::term::Term;

/// A schema's price: the FPL term of each candidate of an entity's reading,
/// which [`crate::fold::price`] combines as their `Least` (SPEC §6, "A
/// schema's registers"; law 34). This `Least` is not a register's join, and
/// it is not FPL's `Conj`, which aggregates different entities.
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
/// of SPEC §6.4 ([`crate::fold::flatten`]).
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
