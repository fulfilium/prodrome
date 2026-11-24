//! A replica in memory (design §6.1), as a page holds one.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

use super::memory::{self, Memory};
use super::replica::{Held, Replica};
use crate::store::Sender;
use prodrome_data::event::Hash;
use prodrome_data::literal::ProdromeError;
use prodrome_data::schema::Schema;

/// A replica that holds its objects and their prints in memory (SPEC §3,
/// "A replica").
///
/// It holds no lock that another writer takes, so it makes only monotone
/// writes, and they reach a store of record by [`super::sync`].
#[derive(Debug)]
pub struct MemoryStore<E: Schema> {
    schema: E::Vocabulary,
    objects: Mutex<Objects<E>>,
    genesis: Option<Hash>,
}

#[derive(Debug)]
struct Objects<E: Schema> {
    memory: Memory<E>,
    prints: BTreeMap<Hash, Vec<u8>>,
}

impl<E: Schema<Vocabulary: Default>> Default for MemoryStore<E> {
    fn default() -> Self {
        MemoryStore::at(E::Vocabulary::default())
    }
}

impl<E: Schema> MemoryStore<E> {
    /// A store in memory that holds nothing, opened at the schema `schema`.
    pub fn at(schema: E::Vocabulary) -> Self {
        MemoryStore {
            schema,
            objects: Mutex::new(Objects {
                memory: Memory::empty(),
                prints: BTreeMap::new(),
            }),
            genesis: None,
        }
    }

    /// This store, writing into the prodrome that `genesis` begins; without
    /// one, it writes into its only genesis.
    #[must_use]
    pub fn in_genesis(self, genesis: Hash) -> MemoryStore<E> {
        MemoryStore {
            genesis: Some(genesis),
            ..self
        }
    }

    /// The objects. A panic while they were held may have left them half
    /// admitted, and there is no directory to read them again from.
    fn objects(&self) -> MutexGuard<'_, Objects<E>> {
        self.objects
            .lock()
            .expect("a store in memory outlives no panic while it is held")
    }
}

impl<E: Schema> Replica<E> for MemoryStore<E> {
    fn schema(&self) -> &E::Vocabulary {
        &self.schema
    }

    fn held(&self) -> Result<Held<E>, ProdromeError> {
        let mut objects = self.objects();
        let folded = objects.memory.folded()?.clone();
        Ok(Held {
            dag: objects.memory.dag().clone(),
            folded,
            tips: objects.memory.tips().clone(),
        })
    }

    fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError> {
        Ok(self.objects().memory.tips().clone())
    }

    fn kept(&self, name: &Hash) -> Option<Vec<u8>> {
        self.objects().prints.get(name).cloned()
    }

    fn append(&self, event: E) -> Result<Hash, ProdromeError> {
        let mut objects = self.objects();
        let (name, sealed) = objects.memory.change(self.genesis.as_ref(), event)?;
        if let Some((verified, print)) = sealed {
            objects.prints.insert(name.clone(), print);
            objects.memory.admit(&[], vec![(verified, None)]);
        }
        Ok(name)
    }

    /// Walked without the lock, over what was held when it began, so a
    /// `print_of` that reads this store again does not wait on it; what
    /// another receipt took meanwhile is not taken twice.
    fn receive(
        &self,
        seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
        sender: Sender,
    ) -> Result<Vec<Hash>, ProdromeError> {
        let dag = self.objects().memory.dag().clone();
        let taken = memory::receive(
            &self.schema,
            |name| dag.get(name).is_some(),
            seeds,
            print_of,
            sender,
        )?;
        let mut objects = self.objects();
        let mut names = Vec::new();
        let mut fresh = Vec::new();
        for (verified, print) in objects.memory.unkept(taken) {
            names.push(verified.name().clone());
            objects.prints.insert(verified.name().clone(), print);
            fresh.push((verified, None));
        }
        objects.memory.admit(&[], fresh);
        Ok(names)
    }
}
