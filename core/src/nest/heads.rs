//! A register whose value is a history (design §6.3), held by its heads.

use std::collections::BTreeSet;
use std::fmt;
use std::marker::PhantomData;

use crate::event::{hashes, hashes_value, sorted_distinct, Hash};
use crate::fold::{read, Order, Product};
use crate::literal::{Call, ProdromeError, Value};
use crate::policy::Everything;
use crate::registers::Stamp;
use crate::schema::Schema;

/// The value of a register that holds a history of the schema `I`: the
/// history's heads. The history is everything that the heads rest on, in a
/// replica of `I`'s objects.
///
/// A register is declared to hold histories of one schema by a
/// [`crate::fold::RegisterType`] whose values are `&Heads<I>`, never by a
/// field of the object, which carries only the names as a sorted tuple.
///
/// The order is inclusion of the heads. It is sound because one set of
/// heads within another names a history within the other's. A pointer
/// whose maximal writes the order cannot compare is never read as one of
/// them: its reading is the union of their histories ([`fn@pointer`]),
/// which needs the inner objects and so is not the order's join. The order
/// is not inflationary: a write moves the pointer to whatever its writer
/// saw, and a writer that saw less is not refused, because the outer
/// history holds everything that any of its writes named.
pub struct Heads<I> {
    names: BTreeSet<Hash>,
    inner: PhantomData<fn() -> I>,
}

impl<I> Heads<I> {
    pub fn new(names: impl IntoIterator<Item = Hash>) -> Heads<I> {
        Heads {
            names: names.into_iter().collect(),
            inner: PhantomData,
        }
    }

    /// The heads, each an object of `I`.
    #[must_use]
    pub fn names(&self) -> &BTreeSet<Hash> {
        &self.names
    }

    /// The field's print: a tuple of the names, sorted.
    #[must_use]
    pub fn to_value(&self) -> Value {
        hashes_value(&self.names.iter().cloned().collect::<Vec<_>>())
    }

    /// The field `name` of `call`, refused where it names a head twice.
    ///
    /// # Errors
    ///
    /// Not a tuple of names, or one named twice.
    pub fn from_call(call: &Call, name: &str) -> Result<Heads<I>, ProdromeError> {
        let names = sorted_distinct(
            &format!("{}.{name}", call.name),
            "head",
            hashes(call, name)?,
        )?;
        Ok(Heads::new(names))
    }
}

impl<I> Clone for Heads<I> {
    fn clone(&self) -> Self {
        Heads::new(self.names.iter().cloned())
    }
}

impl<I> PartialEq for Heads<I> {
    fn eq(&self, other: &Self) -> bool {
        self.names == other.names
    }
}

impl<I> Eq for Heads<I> {}

impl<I> fmt::Debug for Heads<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Heads").field(&self.names).finish()
    }
}

/// Inclusion of the heads.
impl<I> Order for &Heads<I> {
    fn le(&self, other: &Self) -> bool {
        self.names.is_subset(&other.names)
    }
}

/// A schema of which some registers hold histories, as a nest reads it
/// ([`super::Level`]). Each held register is a register type whose values
/// are [`Heads`] of its inner schema.
///
/// A held register's history sits at the path `key/segment` of the entity
/// that holds it, and each of its objects at that path and then its own
/// key. So a nest refuses an entity's key that is not a path segment.
pub trait Nests: Schema<Key: AsRef<str>> {
    /// The registers whose values are histories.
    const HELD: &'static [Self::Register];

    /// A held register's name in a path.
    fn segment(register: Self::Register) -> &'static str;

    /// The heads this event writes to the held `register`, if it writes it.
    fn heads(&self, register: Self::Register) -> Option<&BTreeSet<Hash>>;
}

/// The reading of a held register: the union of the heads of its maximal
/// writes, which name the history that it holds now. Empty where it is
/// unwritten.
pub fn pointer<E: Nests>(stream: &[Stamp<E>], register: E::Register) -> BTreeSet<Hash> {
    read(stream, None, &Everything)
        .reading(register)
        .into_iter()
        .filter_map(|stamp| stamp.event.heads(register))
        .flatten()
        .cloned()
        .collect()
}
