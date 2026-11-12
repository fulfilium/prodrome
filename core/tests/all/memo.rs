//! Law 8 (design §6.2, §7): a cache is a tabulation of a pure function.
//!
//! The store first. A cache is a set of (key, value) pairs: its join is
//! union, so it is commutative, associative and idempotent whatever the
//! pairs; a key holding one value reads as that value, a key holding two is
//! a finding and never reads as either; a bounded policy keeps the cache
//! within its bound, dropping the least recently used first.

use std::collections::BTreeSet;

use prodrome::event::Hash;
use prodrome::memo::{Cache, Finding, Key, Lookup, Lru};
use proptest::prelude::*;

/// A name for a small number, so generated keys collide.
fn hash(n: u8) -> Hash {
    Hash::new(format!("{n:064x}")).expect("64 hex")
}

fn key(function: u8, argument: u8) -> Key {
    Key {
        function: hash(function),
        argument: hash(argument),
    }
}

/// A cache of the pairs drawn: keys from a few functions and arguments,
/// values from a few, so two pairs often share a key.
fn cache_of(pairs: &[(u8, u8, u8)]) -> Cache<u8> {
    let mut cache = Cache::default();
    for &(function, argument, value) in pairs {
        let _ = cache.insert(key(function, argument), value);
    }
    cache
}

fn pairs() -> impl Strategy<Value = Vec<(u8, u8, u8)>> {
    prop::collection::vec((0..2u8, 0..4u8, 0..3u8), 0..12)
}

/// A cache as the set it is.
fn graph(cache: &Cache<u8>) -> BTreeSet<(Key, u8)> {
    cache.clone().pairs().into_iter().collect()
}

fn joined(a: &Cache<u8>, b: &Cache<u8>) -> Cache<u8> {
    let mut a = a.clone();
    let _ = a.join(&mut b.clone());
    a
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// Union is the join: commutative, associative, idempotent, and the
    /// pairs of a join are the union of the pairs.
    #[test]
    fn join_is_union(a in pairs(), b in pairs(), c in pairs()) {
        let (a, b, c) = (cache_of(&a), cache_of(&b), cache_of(&c));
        prop_assert_eq!(graph(&joined(&a, &b)), graph(&joined(&b, &a)));
        prop_assert_eq!(
            graph(&joined(&joined(&a, &b), &c)),
            graph(&joined(&a, &joined(&b, &c)))
        );
        prop_assert_eq!(graph(&joined(&a, &a)), graph(&a));
        let union: BTreeSet<_> = graph(&a).union(&graph(&b)).cloned().collect();
        prop_assert_eq!(graph(&joined(&a, &b)), union);
    }

    /// A key reads as a value exactly when it holds one; a key holding two
    /// reads as a finding with both, and the join that made it so says so.
    #[test]
    fn two_values_are_a_finding(a in pairs(), b in pairs()) {
        let (a, b) = (cache_of(&a), cache_of(&b));
        let mut both = a.clone();
        let reported: BTreeSet<Key> =
            both.join(&mut b.clone()).into_iter().map(|f| f.key).collect();
        let pairs = graph(&both);
        for k in both.keys().collect::<Vec<_>>() {
            let values: BTreeSet<u8> =
                pairs.iter().filter(|(at, _)| *at == k).map(|(_, v)| *v).collect();
            let finding_before = a.clone().findings().iter().any(|f| f.key == k);
            match both.get(&k) {
                Lookup::Hit(value) => prop_assert_eq!(values, BTreeSet::from([value])),
                Lookup::Finding(held) => {
                    prop_assert!(values.len() > 1);
                    prop_assert_eq!(values, held.iter().copied().collect::<BTreeSet<_>>());
                    prop_assert!(finding_before || reported.contains(&k));
                }
                Lookup::Miss => prop_assert!(false, "a held key misses"),
            }
        }
    }

    /// A bounded cache holds at most its bound, whatever is inserted, and
    /// what it drops first is what was touched least recently.
    #[test]
    fn lru_is_bounded(capacity in 1..5usize, inserted in pairs()) {
        let mut cache = Cache::new(Lru::new(capacity));
        for &(function, argument, value) in &inserted {
            let _ = cache.insert(key(function, argument), value);
            prop_assert!(cache.len() <= capacity);
        }
    }
}

#[test]
fn a_second_value_is_reported_and_never_read() {
    let mut cache = Cache::default();
    assert_eq!(cache.insert(key(0, 0), 1u8), Ok(()));
    assert_eq!(
        cache.insert(key(0, 0), 1),
        Ok(()),
        "the same pair twice is one"
    );
    assert_eq!(cache.insert(key(0, 0), 2), Err(Finding { key: key(0, 0) }));
    assert_eq!(cache.get(&key(0, 0)), Lookup::Finding(vec![1, 2]));
    assert!(cache.remove(&key(0, 0)));
    assert_eq!(cache.get(&key(0, 0)), Lookup::Miss);
}

#[test]
fn lru_drops_the_least_recently_used() {
    let mut cache = Cache::new(Lru::new(2));
    cache.insert(key(0, 0), 0u8).expect("one value");
    cache.insert(key(0, 1), 1).expect("one value");
    assert_eq!(cache.get(&key(0, 0)), Lookup::Hit(0));
    cache.insert(key(0, 2), 2).expect("one value");
    assert_eq!(cache.get(&key(0, 1)), Lookup::Miss);
    assert_eq!(cache.get(&key(0, 0)), Lookup::Hit(0));
    assert_eq!(cache.get(&key(0, 2)), Lookup::Hit(2));
}
