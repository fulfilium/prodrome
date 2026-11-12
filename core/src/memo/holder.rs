//! Where a cache's pairs are held (design §6.2): the one interface of the
//! relation from keys to values, and its interpreter in memory.
//!
//! A [`Holder`] holds a finite set of (key, value) pairs. A program reads a
//! key through it as a partial function: no value, one value, or a
//! [`Lookup::Finding`] when the key holds two. Every holder obeys the same
//! laws (SPEC law 51), whatever it keeps its pairs in:
//!
//! - after `add(k, v)`, `get(k)` answers `v` among its values, unless the
//!   holder dropped the pair (a policy evicted it, or a file could not be
//!   written);
//! - `add` answers a [`Finding`] exactly when the key comes to hold a second
//!   distinct value;
//! - after `remove(k)`, `get(k)` misses, and no other key changes;
//! - after `forget(ks)` answers `Ok`, every key of `ks` misses, and no byte
//!   of their values is left in the holder's storage;
//! - `keys` is every key that holds a value, and `len` is its count.
//!
//! [`Memory`] holds the pairs in a hash map. [`super::File`] holds them in
//! one file. [`super::Cache`] is a holder under an eviction policy, and
//! [`super::Front`] is a holder read through a cache in memory, so holders
//! nest. [`super::fold()`] runs over any holder.

use std::collections::{BTreeSet, HashMap};

use super::cache::{Finding, Key, Lookup};
use crate::literal::ProdromeError;

/// A set of (key, value) pairs, read as a partial function where it is one.
pub trait Holder<V> {
    /// What `key` holds.
    fn get(&mut self, key: &Key) -> Lookup<V>;

    /// Adds the pair `(key, value)`.
    ///
    /// # Errors
    ///
    /// A [`Finding`] when the add gives `key` its second distinct value.
    /// The pair is held either way.
    fn add(&mut self, key: Key, value: V) -> Result<(), Finding>;

    /// Drops `key` and every value it holds. Answers whether it held any.
    fn remove(&mut self, key: &Key) -> bool;

    /// Drops every key of `doomed` and erases its values from the holder's
    /// storage, so that no byte of them is left there when this answers
    /// `Ok`.
    ///
    /// # Errors
    ///
    /// The failure to erase. The keys are dropped either way.
    fn forget(&mut self, doomed: &BTreeSet<Key>) -> Result<(), ProdromeError>;

    /// Every key held. A holder that keeps an order of arrival gives the
    /// oldest first.
    fn keys(&self) -> impl Iterator<Item = Key> + '_;

    /// How many keys are held.
    fn len(&self) -> usize;

    /// Whether no key is held.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Writes every change so far to durable storage. A holder in memory
    /// has nothing to write.
    ///
    /// # Errors
    ///
    /// The first write that failed since the last `settle`, or the failure
    /// of this one. A failed write drops its pair and stops no read.
    fn settle(&mut self) -> Result<(), ProdromeError>;
}

/// The pairs in memory.
#[derive(Debug, Clone)]
pub struct Memory<V> {
    entries: HashMap<Key, Vec<V>>,
}

impl<V> Default for Memory<V> {
    fn default() -> Self {
        Memory {
            entries: HashMap::new(),
        }
    }
}

impl<V: Clone + Eq> Holder<V> for Memory<V> {
    fn get(&mut self, key: &Key) -> Lookup<V> {
        self.entries.get(key).cloned().unwrap_or_default().into()
    }

    fn add(&mut self, key: Key, value: V) -> Result<(), Finding> {
        let values = self.entries.entry(key.clone()).or_default();
        if values.contains(&value) {
            return Ok(());
        }
        values.push(value);
        if values.len() == 2 {
            Err(Finding { key })
        } else {
            Ok(())
        }
    }

    fn remove(&mut self, key: &Key) -> bool {
        self.entries.remove(key).is_some()
    }

    /// Memory a removed entry freed may still hold its bytes until the
    /// allocator reuses it. A program that must not keep a readable value
    /// in its own memory gives the cache a value sealed under a key that it
    /// can destroy.
    fn forget(&mut self, doomed: &BTreeSet<Key>) -> Result<(), ProdromeError> {
        for key in doomed {
            self.entries.remove(key);
        }
        Ok(())
    }

    fn keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.entries.keys().cloned()
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn settle(&mut self) -> Result<(), ProdromeError> {
        Ok(())
    }
}
