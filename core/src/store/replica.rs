//! Replicas and sync (design §1, §6.1; SPEC §3, "A replica").

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::dag::{Dag, Heads};
use crate::event::Hash;
use crate::literal::ProdromeError;
use crate::registers::Folded;
use crate::schema::Schema;
use crate::store::Sender;

/// What a replica holds, as of one look.
#[derive(Debug, Clone)]
pub struct Held<E: Schema> {
    pub dag: Arc<Dag<E>>,
    pub folded: Arc<Folded<E>>,
    pub tips: BTreeSet<Hash>,
}

/// A set of objects named by the hash of their prints and closed under
/// parents: on disk ([`super::EventStore`]), in memory
/// ([`super::MemoryStore`]) or over another replica ([`super::Overlay`]).
pub trait Replica<E: Schema> {
    /// The schema that every object it holds or receives parses at (SPEC
    /// §4).
    fn schema(&self) -> &E::Vocabulary;

    /// Its objects, their fold and their tips, as of one look; a print that
    /// is no object is left out, and named in [`Dag::excluded`] (SPEC laws 44
    /// and 45).
    ///
    /// # Errors
    ///
    /// Where it holds them cannot be read at all: a store's `objects/` not
    /// listed.
    fn held(&self) -> Result<Held<E>, ProdromeError>;

    /// Its tips, from which a sync from it starts.
    ///
    /// # Errors
    ///
    /// [`Replica::held`]'s.
    fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError>;

    /// Its tips and the count of the objects of its history; a store on disk
    /// reads its record of them ([`super::EventStore::heads`]).
    ///
    /// # Errors
    ///
    /// [`Replica::held`]'s.
    fn heads(&self) -> Result<Heads, ProdromeError> {
        Ok(self.held()?.dag.heads())
    }

    /// The print it keeps under `name`, unchecked: the whole or a husk of it
    /// (SPEC §3). The receiver must rehash the bytes, and read bytes that are
    /// not the whole as a husk or refuse them.
    fn kept(&self, name: &Hash) -> Option<Vec<u8>>;

    /// What [`Replica::kept`] answers where its hash is `name`, and nothing
    /// for a husk.
    fn print(&self, name: &Hash) -> Option<Vec<u8>> {
        self.kept(name)
            .filter(|bytes| Hash::of_bytes(bytes) == *name)
    }

    /// Write `event` as a `Change` over its entity's heads in this
    /// replica, or nothing where it holds a twin (SPEC §3).
    ///
    /// # Errors
    ///
    /// The append's refusals (SPEC §3), and the replica's own.
    fn append(&self, event: E) -> Result<Hash, ProdromeError>;

    /// Take in what `seeds` rest on that it lacks, each object read by
    /// `print_of`, and answer what it took, parents first.
    ///
    /// It takes nothing unless every object hashes to its name and parses,
    /// or, from its keeper of record, reads as a husk; and it never replaces
    /// a name that it keeps, whole or husk.
    ///
    /// # Errors
    ///
    /// A print missing, or one that is not the object its name says.
    fn receive(
        &self,
        seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
        sender: Sender,
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

    fn kept(&self, name: &Hash) -> Option<Vec<u8>> {
        (**self).kept(name)
    }

    fn append(&self, event: E) -> Result<Hash, ProdromeError> {
        (**self).append(event)
    }

    fn receive(
        &self,
        seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
        sender: Sender,
    ) -> Result<Vec<Hash>, ProdromeError> {
        (**self).receive(seeds, print_of, sender)
    }
}

/// The restricted join `to ∪ ↓chosen` (design §6.3; SPEC §3,
/// "`accept(from, to, chosen)`"): adopts into `to` the objects of `from`
/// that `chosen` rest on and `to` lacks, each verified, and answers them,
/// parents first.
///
/// Where `sender` is [`Sender::Keeper`], `to` takes the husks that `from`
/// keeps; from a stranger it refuses them.
///
/// # Errors
///
/// [`Replica::receive`] of `to`, a chosen name `from` does not hold among
/// them; `to` takes nothing then.
pub fn accept<E: Schema>(
    from: &(impl Replica<E> + ?Sized),
    to: &(impl Replica<E> + ?Sized),
    chosen: BTreeSet<Hash>,
    sender: Sender,
) -> Result<Vec<Hash>, ProdromeError> {
    to.receive(chosen, &|name| from.kept(name), sender)
}

/// The join `to ∪ from` (SPEC §3, "`sync(from, to)`"; law 35): [`accept`]
/// with everything chosen.
///
/// A sync from a stranger that keeps a husk takes nothing, also of the
/// objects that do not rest on the husk, and its refusal names the husk.
///
/// # Errors
///
/// [`Replica::tips`] of `from` and [`Replica::receive`] of `to`; `to` takes
/// nothing then.
pub fn sync<E: Schema>(
    from: &(impl Replica<E> + ?Sized),
    to: &(impl Replica<E> + ?Sized),
    sender: Sender,
) -> Result<Vec<Hash>, ProdromeError> {
    accept(from, to, from.tips()?, sender)
}
