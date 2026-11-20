//! An overlay (design §6.1): writes to memory, reads the union with another
//! replica, and is flushed into it by a sync.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use super::memory::{self, Memory, Verified};
use super::replica::{accept, Held, Replica};
use crate::dag::Dag;
use crate::event::Hash;
use crate::literal::ProdromeError;
use crate::schema::Schema;
use crate::store::Sender;

/// A REPLICA OVER ANOTHER: what it appends or receives is held in memory
/// (its OWN objects) and never reaches `base` until [`Overlay::flush`],
/// while everything it reads is the reading of `own ∪ base`.
///
/// THE UNION BY CONSTRUCTION. Its memory is the base's ([`Replica::held`]:
/// the same objects and the same fold, shared and not copied) with its own
/// objects admitted, which reads as the fold of the union (§1, and the
/// memory's incremental-equals-cold law). When the base has changed since,
/// the next read takes the base's memory again and admits its own objects
/// over it. Nothing is copied to read it: not the base's files, and not its
/// memory until the overlay holds an object of its own, when the memory it
/// extends is copied once per change of the base.
///
/// An overlay decides nothing: it has no lock another writer takes and
/// writes to memory, so it is never the store a coordinated decision is
/// asked of ([`super::Decision`]).
#[derive(Debug)]
pub struct Overlay<E: Schema, B> {
    base: B,
    layer: Mutex<Layer<E>>,
    genesis: Option<Hash>,
}

#[derive(Debug)]
struct Layer<E: Schema> {
    /// `own ∪ base`, as of `under`.
    union: Memory<E>,
    /// The base's objects the union was taken over.
    under: Option<Arc<Dag<E>>>,
    /// What this overlay appended or received, each with its print.
    own: BTreeMap<Hash, Vec<u8>>,
}

impl<E: Schema, B: Replica<E>> Overlay<E, B> {
    /// An overlay holding nothing of its own over `base`, which reads as
    /// `base` reads.
    pub fn new(base: B) -> Overlay<E, B> {
        Overlay {
            base,
            layer: Mutex::new(Layer {
                union: Memory::empty(),
                under: None,
                own: BTreeMap::new(),
            }),
            genesis: None,
        }
    }

    /// This overlay, writing into the prodrome `genesis` begins.
    #[must_use]
    pub fn in_genesis(self, genesis: Hash) -> Overlay<E, B> {
        Overlay {
            genesis: Some(genesis),
            ..self
        }
    }

    pub fn base(&self) -> &B {
        &self.base
    }

    /// `sync(self, base)`: the base takes in this overlay's own objects it
    /// lacks, verified, and reads the union this overlay reads. Answers
    /// what it took. [`Overlay::accept`] with everything chosen.
    ///
    /// # Errors
    ///
    /// [`Overlay::accept`]'s; the base takes nothing then, and a flush again
    /// completes.
    pub fn flush(&self) -> Result<Vec<Hash>, ProdromeError> {
        self.accept(self.tips()?)
    }

    /// THE RESTRICTED JOIN (design §6.3): the base takes in the objects
    /// `chosen` rest on that it lacks, verified, and nothing else of this
    /// overlay; a batch accepted object by object. Where `chosen` is closed
    /// under parents over the base, the base reads as it did plus exactly
    /// `chosen`. What is not chosen stays this overlay's own, read over the
    /// base as before, and a later flush takes it. Answers what the base
    /// took, parents first.
    ///
    /// # Errors
    ///
    /// [`accept`]'s, a chosen name this overlay does not hold among them;
    /// the base takes nothing then.
    ///
    /// The overlay is its base's own layer, so the base takes its husks
    /// as its keeper's ([`Sender::Keeper`]): each one the overlay took
    /// from a keeper of its own.
    pub fn accept(&self, chosen: BTreeSet<Hash>) -> Result<Vec<Hash>, ProdromeError> {
        accept(self, &self.base, chosen, Sender::Keeper)
    }

    fn lock(&self) -> MutexGuard<'_, Layer<E>> {
        self.layer
            .lock()
            .expect("an overlay outlives no panic while it is held")
    }

    /// The layer, its union level with the base as the base reads now.
    fn layer(&self) -> Result<MutexGuard<'_, Layer<E>>, ProdromeError> {
        let Held { dag, folded, .. } = self.base.held()?;
        let mut layer = self.lock();
        if layer
            .under
            .as_ref()
            .is_none_or(|under| !Arc::ptr_eq(under, &dag))
        {
            let mut union = Memory::over(dag.clone(), folded);
            let mut own = Vec::new();
            for (name, print) in layer.own.iter().filter(|(name, _)| !union.holds(name)) {
                own.push((
                    Verified::read(self.base.schema(), name, print, Sender::Keeper)
                        .map_err(|why| why.refusal(name))?,
                    None,
                ));
            }
            union.admit(&[], own);
            layer.union = union;
            layer.under = Some(dag);
        }
        Ok(layer)
    }
}

impl<E: Schema, B: Replica<E>> Replica<E> for Overlay<E, B> {
    /// The base's: an overlay reads the union, so it parses as its base.
    fn schema(&self) -> &E::Vocabulary {
        self.base.schema()
    }

    fn held(&self) -> Result<Held<E>, ProdromeError> {
        let mut layer = self.layer()?;
        let folded = layer.union.folded()?.clone();
        Ok(Held {
            dag: layer.union.dag().clone(),
            folded,
            tips: layer.union.tips().clone(),
        })
    }

    fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError> {
        Ok(self.layer()?.union.tips().clone())
    }

    /// The print the union reads under `name`. Where the base holds the
    /// name as an object, whole or husk, it is the base's: a husk of the
    /// overlay's own whole hides the whole, and a dropped part is never
    /// answered again (SPEC §3). Elsewhere, where the base holds no object
    /// under the name (it lacks it, or its file is unread), it is the
    /// overlay's own, and else the base's. The base is the one the union
    /// was last taken over. Never waits on the base: a flush reads it while
    /// the base holds its lock.
    fn kept(&self, name: &Hash) -> Option<Vec<u8>> {
        let (base_holds, own) = {
            let layer = self.lock();
            let holds = layer.under.as_ref().is_some_and(|base| base.holds(name));
            (holds, layer.own.get(name).cloned())
        };
        if base_holds {
            self.base.kept(name)
        } else {
            own.or_else(|| self.base.kept(name))
        }
    }

    fn append(&self, event: E) -> Result<Hash, ProdromeError> {
        let mut layer = self.layer()?;
        let (name, sealed) = layer.union.change(self.genesis.as_ref(), event)?;
        if let Some((verified, print)) = sealed {
            layer.own.insert(name.clone(), print);
            layer.union.admit(&[], vec![(verified, None)]);
        }
        Ok(name)
    }

    /// Walked over the union as it read when it began, without the lock, as
    /// [`super::MemoryStore`] walks.
    fn receive(
        &self,
        seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
        sender: Sender,
    ) -> Result<Vec<Hash>, ProdromeError> {
        let dag = self.layer()?.union.dag().clone();
        let taken = memory::receive(
            self.schema(),
            |name| dag.get(name).is_some(),
            seeds,
            print_of,
            sender,
        )?;
        let mut layer = self.lock();
        let mut names = Vec::new();
        let mut fresh = Vec::new();
        for (verified, print) in layer.union.unkept(taken) {
            names.push(verified.name().clone());
            layer.own.insert(verified.name().clone(), print);
            fresh.push((verified, None));
        }
        layer.union.admit(&[], fresh);
        Ok(names)
    }
}
