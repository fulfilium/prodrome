//! An actor's replica, held in memory, as an overlay or on disk.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::step::Kind;
use crate::dag::{Finding, Heads};
use crate::event::Hash;
use crate::literal::ProdromeError;
use crate::policy::Everything;
use crate::schema::Schema;
use crate::store::{EventStore, Held, MemoryStore, Overlay, Replica, Sender};

/// An actor's replica.
#[derive(Debug)]
pub(super) enum Kept<E: Schema> {
    Memory(MemoryStore<E>),
    Overlay(Box<Overlay<E, MemoryStore<E>>>),
    Disk(EventStore<E, Everything>),
}

/// The directory of a script's stores on disk, removed when the script
/// ends.
#[derive(Debug)]
pub(super) struct Scratch(PathBuf);

static NEXT: AtomicUsize = AtomicUsize::new(0);

impl Scratch {
    pub(super) fn new() -> Scratch {
        Scratch(std::env::temp_dir().join(format!(
            "prodrome-script-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A directory that cannot be removed is left in the temporary
        // directory; the script's verdict does not depend on it.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl<E: Schema> Kept<E> {
    /// An empty replica of kind `kind` at the schema `vocabulary`, the
    /// `n`-th of the script whose stores on disk are in `scratch`.
    pub(super) fn new(kind: Kind, vocabulary: &E::Vocabulary, scratch: &Scratch, n: usize) -> Self {
        match kind {
            Kind::Memory => Kept::Memory(MemoryStore::at(vocabulary.clone())),
            Kind::Overlay => {
                Kept::Overlay(Box::new(Overlay::new(MemoryStore::at(vocabulary.clone()))))
            }
            Kind::Disk => Kept::Disk(EventStore::at(
                scratch.0.join(n.to_string()),
                Everything,
                vocabulary.clone(),
            )),
        }
    }

    /// The base an overlay is flushed into.
    pub(super) fn base(&self) -> Option<&MemoryStore<E>> {
        match self {
            Kept::Overlay(overlay) => Some(overlay.base()),
            Kept::Memory(_) | Kept::Disk(_) => None,
        }
    }

    /// An overlay's flush; `None` for another kind.
    pub(super) fn flush(&self) -> Option<Result<Vec<Hash>, ProdromeError>> {
        match self {
            Kept::Overlay(overlay) => Some(overlay.flush()),
            Kept::Memory(_) | Kept::Disk(_) => None,
        }
    }

    /// What `verify` finds: a store on disk's own `verify`, which also
    /// reads its files, and the DAG's elsewhere.
    pub(super) fn verify(&self) -> Result<Vec<Finding>, ProdromeError> {
        match self {
            Kept::Disk(store) => Ok(store.verify()),
            Kept::Memory(_) | Kept::Overlay(_) => Ok(self.held()?.dag.verify(&Everything)),
        }
    }
}

impl<E: Schema> Replica<E> for Kept<E> {
    fn schema(&self) -> &E::Vocabulary {
        match self {
            Kept::Memory(store) => store.schema(),
            Kept::Overlay(overlay) => overlay.schema(),
            Kept::Disk(store) => Replica::schema(store),
        }
    }

    fn held(&self) -> Result<Held<E>, ProdromeError> {
        match self {
            Kept::Memory(store) => store.held(),
            Kept::Overlay(overlay) => overlay.held(),
            Kept::Disk(store) => store.held(),
        }
    }

    fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError> {
        match self {
            Kept::Memory(store) => store.tips(),
            Kept::Overlay(overlay) => overlay.tips(),
            Kept::Disk(store) => Replica::tips(store),
        }
    }

    fn heads(&self) -> Result<Heads, ProdromeError> {
        match self {
            Kept::Memory(store) => store.heads(),
            Kept::Overlay(overlay) => overlay.heads(),
            Kept::Disk(store) => Replica::heads(store),
        }
    }

    fn kept(&self, name: &Hash) -> Option<Vec<u8>> {
        match self {
            Kept::Memory(store) => store.kept(name),
            Kept::Overlay(overlay) => overlay.kept(name),
            Kept::Disk(store) => store.kept(name),
        }
    }

    fn append(&self, event: E) -> Result<Hash, ProdromeError> {
        match self {
            Kept::Memory(store) => store.append(event),
            Kept::Overlay(overlay) => overlay.append(event),
            Kept::Disk(store) => Replica::append(store, event),
        }
    }

    fn receive_from(
        &self,
        seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
        sender: Sender,
    ) -> Result<Vec<Hash>, ProdromeError> {
        match self {
            Kept::Memory(store) => store.receive_from(seeds, print_of, sender),
            Kept::Overlay(overlay) => overlay.receive_from(seeds, print_of, sender),
            Kept::Disk(store) => store.receive_from(seeds, print_of, sender),
        }
    }
}
