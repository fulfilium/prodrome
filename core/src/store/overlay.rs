//! An overlay (design §6.1; SPEC §3, "An overlay").

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use super::memory::{self, Memory, Verified};
use super::replica::{accept, Held, Replica};
use crate::dag::Dag;
use crate::event::Hash;
use crate::literal::ProdromeError;
use crate::schema::Schema;
use crate::store::Sender;

/// A replica over `base`: it holds what it appends or receives (its own
/// objects) in memory, and reads `own ∪ base`; its own objects reach `base`
/// only by [`Overlay::flush`] or [`Overlay::accept`].
///
/// Its memory is the base's memory ([`Replica::held`], shared and not
/// copied) with its own objects admitted over it. When the base changes, the
/// next read takes the base's memory again and admits its own objects over
/// it. A read copies none of the base's files. The memory that it extends
/// is copied once for each change of the base, and only while the overlay
/// holds an object of its own.
///
/// It has no lock that another writer takes, so no [`super::Decision`] is
/// asked of it.
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
    /// An overlay over `base` that holds nothing of its own.
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

    /// This overlay, writing into the prodrome that `genesis` begins.
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

    /// `sync(self, base)`: [`Overlay::accept`] with everything chosen.
    /// Answers what the base took.
    ///
    /// # Errors
    ///
    /// [`Overlay::accept`]'s; the base takes nothing then, and a flush again
    /// completes.
    pub fn flush(&self) -> Result<Vec<Hash>, ProdromeError> {
        self.accept(self.tips()?)
    }

    /// The restricted join (design §6.3): the base takes in the objects that
    /// `chosen` rest on and that it lacks, verified, and answers them,
    /// parents first.
    ///
    /// Where `chosen` is closed under parents over the base, the base reads
    /// as it did plus exactly `chosen`. What is not chosen stays this
    /// overlay's own, and a later flush takes it.
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

    /// The print the union reads under `name`. Where the base keeps a
    /// print under the name that reads as an object, whole or husk, it is
    /// the base's: a husk of the overlay's own whole hides the whole, and a
    /// dropped part is never answered again (SPEC §3). Elsewhere (the base
    /// lacks the name, or its print there is no object) it is the
    /// overlay's own. The base's print is read as it is now, whatever the
    /// overlay read before, and is read as the base reads its own prints.
    /// Never waits on the base's lock: a flush reads it while the base
    /// holds its lock.
    fn kept(&self, name: &Hash) -> Option<Vec<u8>> {
        self.base
            .kept(name)
            .filter(|print| {
                Verified::<E>::read(self.base.schema(), name, print, Sender::Keeper).is_ok()
            })
            .or_else(|| self.lock().own.get(name).cloned())
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
