//! A holder read through a cache in memory (design §6.2): memory in front of
//! a file.
//!
//! A [`Front`] is the memoisation of its back holder's reads. It answers a
//! key from its memory when the memory holds it, and otherwise asks the
//! back and copies the whole answer into the memory. It writes every pair
//! to the back only, and drops the key from the memory, so the memory holds
//! only what the back answered. Its meaning is the back's (`memo f = f`
//! again, with `f` the back's `get`), and its memory is a cost:
//!
//! - in front of a holder that drops nothing by itself, every answer of the
//!   front is the back's answer (SPEC law 57);
//! - in front of any holder, every value the front answers is one that the
//!   back held at that key.
//!
//! A front is not [`super::Cache::join`]. A join adds the pairs of one
//! holder to another. A front adds no pair: its memory holds a subset of
//! what its back held, so the union of the two is the back.
//!
//! The front reads what the back held when the front first asked. A
//! second process that writes another value at a key that the front holds
//! makes a finding that the front does not see until its memory drops the
//! key. The front never answers a value that no writer wrote.

use std::collections::BTreeSet;

use super::cache::{Cache, Evict, Finding, Key, Lookup};
use super::holder::Holder;
use crate::literal::ProdromeError;

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
