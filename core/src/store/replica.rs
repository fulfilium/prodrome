//! A replica (design §1, §6.1): a set of objects closed under their parents,
//! wherever it is held, and sync, the semilattice's join of two.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::dag::{Dag, Heads};
use crate::event::Hash;
use crate::literal::ProdromeError;
use crate::registers::Folded;
use crate::schema::Schema;

/// What a replica holds, of one look: its objects, their fold and their
/// tips.
#[derive(Debug, Clone)]
pub struct Held<E: Schema> {
    pub dag: Arc<Dag<E>>,
    pub folded: Arc<Folded<E>>,
    pub tips: BTreeSet<Hash>,
}

/// A REPLICA OF THE FREE SEMILATTICE: objects named by the hash of their
/// prints, closed under parents, on disk ([`super::EventStore`]), in memory
/// ([`super::MemoryStore`]) or over another ([`super::Overlay`]).
///
/// Every replica reads, appends a MONOTONE write (a `Change` decided on
/// what it holds, by the same rules wherever it is held) and receives. None
/// of that needs coordination, so none of it is the store of record's
/// alone; a decision is ([`super::Decision`]).
pub trait Replica<E: Schema> {
    /// The schema it is opened at: what every object it holds or receives
    /// parses at (§5). No object carries it.
    fn schema(&self) -> &E::Vocabulary;

    /// Its objects, their fold and their tips, of one look: TOTAL over what
    /// it holds, a print that is no object left out with what rests on it,
    /// and named by its `dag`'s [`Dag::excluded`] (SPEC laws 44 and 45).
    ///
    /// # Errors
    ///
    /// Where it holds them cannot be read at all: a store's `objects/` not
    /// listed.
    fn held(&self) -> Result<Held<E>, ProdromeError>;

    /// Its tips: what a sync from it starts from.
    ///
    /// # Errors
    ///
    /// [`Replica::held`]'s.
    fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError>;

    /// Its tips and the count of the objects of its history. A replica in
    /// memory answers them from what it holds. A store on disk reads its
    /// record of them ([`super::EventStore::heads`]).
    ///
    /// # Errors
    ///
    /// [`Replica::held`]'s.
    fn heads(&self) -> Result<Heads, ProdromeError> {
        let held = self.held()?;
        Ok(Heads::new(held.tips, held.dag.objects().len()))
    }

    /// The print it holds under `name`, the bytes that name is the hash of,
    /// unchecked: whoever receives it verifies it.
    fn print(&self, name: &Hash) -> Option<Vec<u8>>;

    /// Write `event` as a `Change` over its entity's heads in this
    /// replica, or nothing where it holds a twin (SPEC §3): the decision is
    /// the same wherever the objects are held, so two replicas holding the
    /// same objects write the same bytes.
    ///
    /// # Errors
    ///
    /// The append's refusals (SPEC §3), and the replica's own.
    fn append(&self, event: E) -> Result<Hash, ProdromeError>;

    /// Take in what `seeds` rest on that it lacks, each object read by
    /// `print_of` and VERIFIED on receipt (it hashes to its name and
    /// parses), nothing taken unless all of it verified, and held parents
    /// first. Answers what it took, parents first.
    ///
    /// # Errors
    ///
    /// A print missing, or one that is not the object its name says.
    fn receive(
        &self,
        seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
    ) -> Result<Vec<Hash>, ProdromeError>;
}

/// A replica borrowed is that replica: an overlay over `&store` reads and
/// flushes into a store its caller keeps.
impl<E: Schema, R: Replica<E> + ?Sized> Replica<E> for &R {
    fn schema(&self) -> &E::Vocabulary {
        (**self).schema()
    }

    fn held(&self) -> Result<Held<E>, ProdromeError> {
        (**self).held()
    }

    fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError> {
        (**self).tips()
    }

    fn heads(&self) -> Result<Heads, ProdromeError> {
        (**self).heads()
    }

    fn print(&self, name: &Hash) -> Option<Vec<u8>> {
        (**self).print(name)
    }

    fn append(&self, event: E) -> Result<Hash, ProdromeError> {
        (**self).append(event)
    }

    fn receive(
        &self,
        seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
    ) -> Result<Vec<Hash>, ProdromeError> {
        (**self).receive(seeds, print_of)
    }
}

/// THE RESTRICTED JOIN (design §6.3): `to ∪ ↓chosen`, adopting into `to`
/// the objects of `from` that `chosen` rest on and `to` lacks, each verified
/// on adoption, and answering them, parents first.
///
/// Every way one replica takes objects from another is this, with a choice
/// of `chosen`: a sync chooses everything `from` holds (its tips), an
/// overlay's flush chooses all of its own objects ([`super::Overlay::flush`])
/// and its acceptance chooses some ([`super::Overlay::accept`]), and an
/// adoption chooses one tip ([`super::EventStore::adopt`]). Where `chosen`
/// is closed under parents over what `to` holds, `to` reads as it did plus
/// exactly `chosen`; elsewhere it takes what they rest on too, since a
/// replica is closed under parents. No object is renamed or rewritten: what
/// arrives is the bytes `from` holds under each name.
///
/// # Errors
///
/// [`Replica::receive`] of `to`, a chosen name `from` does not hold among
/// them; `to` takes nothing then.
pub fn accept<E: Schema>(
    from: &(impl Replica<E> + ?Sized),
    to: &(impl Replica<E> + ?Sized),
    chosen: BTreeSet<Hash>,
) -> Result<Vec<Hash>, ProdromeError> {
    to.receive(chosen, &|name| from.print(name))
}

/// `to ∪ from`: adopt into `to` every object of `from` that `to` lacks,
/// each verified on adoption, and answer them, parents first. [`accept`]
/// with everything chosen.
///
/// THE JOIN, AND NOTHING ELSE. It reads no register and asks no schema:
/// idempotent (an object adopted twice is one object), order-free (union
/// commutes), and safe to cut short and repeat, since what one sync placed
/// is a down-set of `from` and the next adopts the rest. Between any two
/// replicas at one schema, whatever each is.
///
/// # Errors
///
/// [`Replica::tips`] of `from` and [`Replica::receive`] of `to`; `to` takes
/// nothing then.
pub fn sync<E: Schema>(
    from: &(impl Replica<E> + ?Sized),
    to: &(impl Replica<E> + ?Sized),
) -> Result<Vec<Hash>, ProdromeError> {
    accept(from, to, from.tips()?)
}
