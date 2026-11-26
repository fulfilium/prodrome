//! The cache as a relation from keys to values, joined by union and read as
//! a partial function (design §6.2; SPEC §6, "A cache is a tabulation of a
//! pure function").
//!
//! [`Cache::join`] merges two caches by the union of their pairs (law 56). A
//! key that comes to hold two values is a [`Finding`], which the cache keeps
//! as evidence and never chooses between. An [`Evict`] policy decides which
//! entries go, and it means the same over every [`Holder`]: over a file, it
//! bounds the keys and so the file (law 58). [`Cache::forget`] also erases
//! the bytes of the values (law 55).
//!
//! A cache is not a replica of a store (`store::Replica`): an entry is
//! deleted freely, is verified by recomputing and stays out of every
//! checkpoint. The two never share a type, so that no entry can be received
//! into a history and no object can be evicted from one.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::marker::PhantomData;

use super::holder::{Holder, Memory};
use prodrome_data::event::Hash;
use prodrome_data::literal::ProdromeError;

/// What an entry tabulates: the function's name (its code and everything
/// that code reads) and its argument's name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    /// The function's name.
    pub function: Hash,
    /// The argument's name.
    pub argument: Hash,
}

/// A key that holds more than one value: the function was not pure, its key
/// left out something that it read, or a writer forged an entry. A reader
/// reports it and never picks one of the values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The key that holds the values.
    pub key: Key,
}

/// What a key holds.
#[derive(Debug, PartialEq, Eq)]
pub enum Lookup<V> {
    /// No entry: the bottom of the flat order.
    Miss,
    /// One value.
    Hit(V),
    /// Two or more distinct values, each as it arrived: a [`Finding`].
    Finding(Vec<V>),
}

/// The lookup of a key that holds `values`, each once.
impl<V> From<Vec<V>> for Lookup<V> {
    fn from(values: Vec<V>) -> Self {
        match <[V; 1]>::try_from(values) {
            Ok([value]) => Lookup::Hit(value),
            Err(values) if values.is_empty() => Lookup::Miss,
            Err(values) => Lookup::Finding(values),
        }
    }
}

impl<V> Lookup<V> {
    /// Every value held, in the order they arrived.
    #[must_use]
    pub fn values(self) -> Vec<V> {
        match self {
            Lookup::Miss => Vec::new(),
            Lookup::Hit(value) => vec![value],
            Lookup::Finding(values) => values,
        }
    }
}

/// Which entries a cache drops. The cache tells the policy of every key
/// that it touches and asks for a victim after every insert. A policy
/// decides what the cache remembers, never what a reading answers.
pub trait Evict {
    /// `key` was read or written.
    fn touched(&mut self, key: &Key);
    /// `key` is gone, whoever dropped it.
    fn forgot(&mut self, key: &Key);
    /// Learns the keys that a cache holds when it is made, oldest first. The
    /// default touches each in turn.
    fn learn(&mut self, keys: impl Iterator<Item = Key>) {
        for key in keys {
            self.touched(&key);
        }
    }
    /// The next key to drop from a cache of `held` keys, or `None` to stop.
    fn victim(&mut self, held: usize) -> Option<Key>;
}

/// The policy that drops nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Keep;

impl Evict for Keep {
    fn touched(&mut self, _: &Key) {}
    fn forgot(&mut self, _: &Key) {}
    /// A policy that drops nothing has nothing to learn, so a cache made
    /// over a large holder does not list its keys.
    fn learn(&mut self, _: impl Iterator<Item = Key>) {}
    fn victim(&mut self, _: usize) -> Option<Key> {
        None
    }
}

/// At most `capacity` keys, the least recently touched dropped first.
#[derive(Debug, Clone)]
pub struct Lru {
    capacity: usize,
    clock: u64,
    last: HashMap<Key, u64>,
    order: BTreeMap<u64, Key>,
}

impl Lru {
    /// A policy holding at most `capacity` keys.
    #[must_use]
    pub fn new(capacity: usize) -> Lru {
        Lru {
            capacity,
            clock: 0,
            last: HashMap::new(),
            order: BTreeMap::new(),
        }
    }
}

impl Evict for Lru {
    fn touched(&mut self, key: &Key) {
        self.clock += 1;
        if let Some(was) = self.last.insert(key.clone(), self.clock) {
            self.order.remove(&was);
        }
        self.order.insert(self.clock, key.clone());
    }

    fn forgot(&mut self, key: &Key) {
        if let Some(was) = self.last.remove(key) {
            self.order.remove(&was);
        }
    }

    fn victim(&mut self, held: usize) -> Option<Key> {
        if held > self.capacity {
            self.order.first_key_value().map(|(_, key)| key.clone())
        } else {
            None
        }
    }
}

/// The pairs of a [`Holder`] under an [`Evict`] policy. The holder is
/// [`Memory`] by default, and a cache is itself a holder.
#[derive(Debug, Clone)]
pub struct Cache<V, E = Keep, H = Memory<V>> {
    holder: H,
    evict: E,
    value: PhantomData<fn() -> V>,
}

impl<V> Default for Cache<V> {
    fn default() -> Self {
        Cache::new(Keep)
    }
}

impl<V, E> Cache<V, E> {
    /// An empty cache in memory under `evict`.
    pub fn new(evict: E) -> Self {
        Cache {
            holder: Memory::default(),
            evict,
            value: PhantomData,
        }
    }
}

impl<V: Clone + Eq, E: Evict, H: Holder<V>> Cache<V, E, H> {
    /// The pairs `holder` holds, under `evict`. The policy is told of every
    /// key in the holder's order, oldest first, as if each had just been
    /// added, and then drops what it would have dropped.
    pub fn held(holder: H, evict: E) -> Self {
        let mut cache = Cache {
            holder,
            evict,
            value: PhantomData,
        };
        cache.evict.learn(cache.holder.keys());
        cache.shrink();
        cache
    }

    /// What `key` holds. Reading a held key touches it, for the policy.
    pub fn get(&mut self, key: &Key) -> Lookup<V> {
        let lookup = self.holder.get(key);
        match lookup {
            Lookup::Miss => self.evict.forgot(key),
            Lookup::Hit(_) | Lookup::Finding(_) => self.evict.touched(key),
        }
        lookup
    }

    /// Adds the pair `(key, value)`. The policy may then drop entries, this
    /// one among them.
    ///
    /// # Errors
    ///
    /// A [`Finding`] when `key` already held a different value. The pair is
    /// held either way.
    pub fn insert(&mut self, key: Key, value: V) -> Result<(), Finding> {
        self.evict.touched(&key);
        let held = self.holder.add(key, value);
        self.shrink();
        held
    }

    /// Adds every pair of `other` to this cache. Answers every key that
    /// holds two values after the join and did not before.
    pub fn join(&mut self, other: &mut impl Holder<V>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for key in other.keys().collect::<Vec<_>>() {
            for value in other.get(&key).values() {
                if let Err(finding) = self.holder.add(key.clone(), value) {
                    if !findings.contains(&finding) {
                        findings.push(finding);
                    }
                }
            }
            self.evict.touched(&key);
        }
        self.shrink();
        findings
    }

    /// Drops `key` and every value it holds. Answers whether it held any.
    pub fn remove(&mut self, key: &Key) -> bool {
        self.evict.forgot(key);
        self.holder.remove(key)
    }

    /// Drops every key of `doomed` and erases its values, so that no byte
    /// of a forgotten value is left in the holder's storage when this
    /// answers `Ok` (law 55).
    ///
    /// # Errors
    ///
    /// The holder's failure to erase. The keys are dropped either way.
    pub fn forget(&mut self, doomed: &BTreeSet<Key>) -> Result<(), ProdromeError> {
        for key in doomed {
            self.evict.forgot(key);
        }
        self.holder.forget(doomed)
    }

    /// Every key held, oldest first where the holder keeps an order.
    pub fn keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.holder.keys()
    }

    /// Every pair held, which a sync sends.
    pub fn pairs(&mut self) -> Vec<(Key, V)> {
        let mut pairs = Vec::new();
        for key in self.holder.keys().collect::<Vec<_>>() {
            for value in self.holder.get(&key).values() {
                pairs.push((key.clone(), value));
            }
        }
        pairs
    }

    /// Every key holding more than one value.
    pub fn findings(&mut self) -> Vec<Finding> {
        let mut findings = Vec::new();
        for key in self.holder.keys().collect::<Vec<_>>() {
            if let Lookup::Finding(_) = self.holder.get(&key) {
                findings.push(Finding { key });
            }
        }
        findings
    }

    /// How many keys are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.holder.len()
    }

    /// Whether no key is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.holder.is_empty()
    }

    /// Writes every change so far to durable storage ([`Holder::settle`]).
    ///
    /// # Errors
    ///
    /// The holder's first failed write since the last `settle`.
    pub fn settle(&mut self) -> Result<(), ProdromeError> {
        self.holder.settle()
    }

    fn shrink(&mut self) {
        while let Some(victim) = self.evict.victim(self.holder.len()) {
            if !self.remove(&victim) {
                break;
            }
        }
    }
}

/// A cache is a holder, so a cache can stand behind another.
impl<V: Clone + Eq, E: Evict, H: Holder<V>> Holder<V> for Cache<V, E, H> {
    fn get(&mut self, key: &Key) -> Lookup<V> {
        Cache::get(self, key)
    }

    fn add(&mut self, key: Key, value: V) -> Result<(), Finding> {
        self.insert(key, value)
    }

    fn remove(&mut self, key: &Key) -> bool {
        Cache::remove(self, key)
    }

    fn forget(&mut self, doomed: &BTreeSet<Key>) -> Result<(), ProdromeError> {
        Cache::forget(self, doomed)
    }

    fn keys(&self) -> impl Iterator<Item = Key> + '_ {
        self.holder.keys()
    }

    fn len(&self) -> usize {
        self.holder.len()
    }

    fn settle(&mut self) -> Result<(), ProdromeError> {
        self.holder.settle()
    }
}
