//! A holder read through a cache in memory, such as memory in front of a
//! file (design §6.2).
//!
//! A [`Front`] answers a key from its memory when the memory holds it, and
//! otherwise asks the back holder and copies the whole answer into the
//! memory. It writes every pair to the back only, and drops the key from
//! the memory, so the memory holds only what the back answered. In front of
//! a holder that drops nothing by itself, every answer of the front is the
//! back's answer (law 57). In front of any holder, every value that the
//! front answers is one that the back held at that key. Unlike
//! [`super::Cache::join`], a front adds no pair.
//!
//! The front reads what the back held when the front first asked. A
//! second process that writes another value at a key that the front holds
//! makes a finding that the front does not see until its memory drops the
//! key. The front never answers a value that no writer wrote.

use std::collections::BTreeSet;

use super::cache::{Cache, Evict, Finding, Key, Lookup};
use super::holder::Holder;
use prodrome_data::literal::ProdromeError;

/// The holder `B`, read through a cache in memory under the policy `E`.
#[derive(Debug, Clone)]
pub struct Front<V, E, B> {
    memory: Cache<V, E>,
    back: B,
}

impl<V: Clone + Eq, E: Evict, B: Holder<V>> Front<V, E, B> {
    /// `back`, read through an empty memory under `evict`.
    pub fn new(evict: E, back: B) -> Self {
        Front {
            memory: Cache::new(evict),
            back,
        }
    }
}

impl<V: Clone + Eq, E: Evict, B: Holder<V>> Holder<V> for Front<V, E, B> {
    fn get(&mut self, key: &Key) -> Lookup<V> {
        match self.memory.get(key) {
            Lookup::Miss => {}
            held @ (Lookup::Hit(_) | Lookup::Finding(_)) => return held,
        }
        let answer = self.back.get(key);
        let values = match &answer {
            Lookup::Miss => return answer,
            Lookup::Hit(value) => std::slice::from_ref(value),
            Lookup::Finding(values) => values.as_slice(),
        };
        for value in values {
            // The memory copies what the back holds, so a second value here
            // is one the back holds as well, and the answer says so.
            let _copied = self.memory.insert(key.clone(), value.clone());
        }
        answer
    }

    fn add(&mut self, key: Key, value: V) -> Result<(), Finding> {
        self.memory.remove(&key);
        self.back.add(key, value)
    }

    fn remove(&mut self, key: &Key) -> bool {
        self.memory.remove(key);
        self.back.remove(key)
    }

    fn forget(&mut self, doomed: &BTreeSet<Key>) -> Result<(), ProdromeError> {
        let memory = self.memory.forget(doomed);
        self.back.forget(doomed).and(memory)
    }

    fn keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.back.keys()
    }

    fn len(&self) -> usize {
        self.back.len()
    }

    fn settle(&mut self) -> Result<(), ProdromeError> {
        self.back.settle()
    }
}
