//! SPEC §3 and law 43: WHAT AN OBJECT RESTS ON. Every object rests on the
//! edges it names and on the genesis it names, so a `Genesis` is beneath
//! everything begun from it and is a head only of a prodrome with nothing
//! else in it.
//!
//! The order is stated here from the objects' fields, apart from
//! `parents_of`, and every reader of it is held to it over generated
//! histories of several prodromes holding changes, snapshots, keys added
//! and revoked, and signatures: the heads are exactly the objects nothing
//! rests on, the linearisation and a replica's parents-first walk are
//! linear extensions of it, a closure is its down-set, and the interior of
//! a history without a genesis holds nothing begun from it.

use std::collections::{BTreeMap, BTreeSet};

use prodrome::change::mk_change;
use prodrome::dag::Dag;
use prodrome::event::{
    canonical_envelope, mk_created, seal_hash, Actor, Envelope, Hash, TodoEvent,
};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::reference::Todo;
use prodrome::sign::{mk_key_revoked, KeyAdded, Secret};
use prodrome::snapshot::mk_snapshot;
use prodrome::store::{Held, MemoryStore, Replica};
use proptest::prelude::*;

type Event = TodoEvent<Todo>;

fn created(todo: &str) -> Event {
    let at = Datetime::new(2026, 10, 3, 12, 0, 0, 0).expect("an instant");
    mk_created(todo, at, "writer", "", "").expect("valid")
}

/// A store in memory holding one genesis, whose name sorts AFTER the change
/// `created("first")` writes into it: the case a linearisation ordering
/// only by name would put backwards.
fn begun() -> (MemoryStore<Event>, Hash) {
    for nonce in 0u32.. {
        let genesis: Envelope<Event> =
            Envelope::Genesis(mk_genesis("rests-on", &format!("{nonce:032x}")).expect("a genesis"));
        let name = seal_hash(&genesis);
        let print = canonical_envelope(&genesis).into_bytes();
        let store = MemoryStore::default().in_genesis(name.clone());
        store
            .receive([name.clone()].into(), &|held| {
                (*held == name).then(|| print.clone())
            })
            .expect("receives its genesis");
        let probe = MemoryStore::default().in_genesis(name.clone());
        probe
            .receive([name.clone()].into(), &|held| {
                (*held == name).then(|| print.clone())
            })
            .expect("receives its genesis");
        if probe.append(created("first")).expect("appends") < name {
            return (store, name);
        }
    }
    unreachable!("some nonce names a genesis after its first change")
}

#[test]
fn a_genesis_is_beneath_its_first_change() {
    let (store, genesis) = begun();
    let change = store.append(created("first")).expect("appends");
    assert!(change < genesis, "the case under test");
    let Held { dag, tips, .. } = store.held().expect("reads");
    let only: BTreeSet<Hash> = [change.clone()].into();
    assert_eq!(tips, only, "the store's heads");
    assert_eq!(dag.tips(), only, "the DAG's tips");
    assert_eq!(
        dag.tips_in(&Some(genesis.clone())),
        only,
        "the prodrome's heads"
    );
    assert_eq!(
        dag.linearise().expect("orders"),
        vec![genesis.clone(), change.clone()],
        "the genesis first"
    );
    assert_eq!(
        dag.closure([change.clone()]),
        [genesis.clone(), change.clone()].into(),
        "a change's down-set holds its genesis"
    );
    let alone: Dag<Event> = dag
        .objects()
        .iter()
        .filter(|(name, _)| **name == change)
        .map(|(name, object)| (name.clone(), object.clone()))
        .collect();
    assert!(
        alone.interior().objects().is_empty(),
        "a change without its genesis is outside the history"
    );
}

/// A snapshot written before a genesis was beneath its changes names it
/// among its tips. It still verifies and attests what the snapshot written
/// now attests: the same down-set, so no stored object reads otherwise.
#[test]
fn a_snapshot_naming_its_genesis_as_a_tip_attests_as_it_did() {
    let (store, genesis) = begun();
    let change = store.append(created("first")).expect("appends");
    let snapshot = |tips: Vec<Hash>| {
        let object: Envelope<Event> =
            Envelope::Snapshot(mk_snapshot(genesis.clone(), tips, None).expect("a snapshot"));
        (seal_hash(&object), object)
    };
    let (then, written_then) = snapshot(vec![genesis.clone(), change.clone()]);
    let (now, written_now) = snapshot(vec![change.clone()]);
    let Held { dag, .. } = store.held().expect("reads");
    let held: Dag<Event> = dag
        .objects()
        .iter()
        .map(|(name, object)| (name.clone(), object.clone()))
        .chain([(then.clone(), written_then), (now.clone(), written_now)])
        .collect();
    assert_eq!(held.verify(&prodrome::policy::Everything), Vec::new());
    let attests = |name: &Hash| {
        let mut down = held.closure([name.clone()]);
        down.remove(name);
        down
    };
    assert_eq!(attests(&then), attests(&now));
    assert_eq!(attests(&now), [genesis, change].into());
}

type Objects = BTreeMap<Hash, Envelope<Event>>;

/// WHAT AN OBJECT RESTS ON, read off its fields as SPEC §3 states it: the
/// edges it names and the genesis it names.
fn rests_on(object: &Envelope<Event>) -> BTreeSet<Hash> {
    match object {
        Envelope::Genesis(_) => BTreeSet::new(),
        Envelope::Change(change) => change
            .deps
            .iter()
            .chain([&change.genesis])
            .cloned()
            .collect(),
        Envelope::Snapshot(snapshot) => snapshot
            .tips
            .iter()
            .chain(&snapshot.previous)
            .chain([&snapshot.genesis])
            .cloned()
            .collect(),
        Envelope::KeyAdded(added) => [added.genesis.clone()].into(),
        Envelope::KeyRevoked(revoked) => revoked
            .deps
            .iter()
            .chain([&revoked.genesis])
            .cloned()
            .collect(),
        Envelope::Signed(signed) => [signed.object.clone()].into(),
        Envelope::Sealed { prev, .. } => prev.iter().cloned().collect(),
        Envelope::Woven { parents, .. } => parents.iter().cloned().collect(),
    }
}

/// `seeds` and everything they rest on, transitively, by [`rests_on`].
fn down(objects: &Objects, seeds: impl IntoIterator<Item = Hash>) -> BTreeSet<Hash> {
    let mut found = BTreeSet::new();
    let mut pending: Vec<Hash> = seeds.into_iter().collect();
    while let Some(name) = pending.pop() {
        if found.insert(name.clone()) {
            pending.extend(objects.get(&name).map(rests_on).unwrap_or_default());
        }
    }
    found
}

/// One drawn object: its kind, the prodrome it is in, and raw picks among
/// that prodrome's objects before it.
type Drawn = (u8, usize, Vec<usize>);

fn a_history() -> impl Strategy<Value = (usize, Vec<Drawn>)> {
    (
        1usize..4,
        prop::collection::vec(
            (0u8..5, 0usize..4, prop::collection::vec(0usize..64, 0..4)),
            0..24,
        ),
    )
}

/// The drawn history as objects: `count` prodromes, then each drawn
/// object in the prodrome it names, resting on the picks among what that
/// prodrome held before it.
fn realise(count: usize, drawn: &[Drawn]) -> (Vec<Hash>, Objects) {
    let mut objects = Objects::new();
    let mut held: Vec<Vec<Hash>> = Vec::new();
    let put = |objects: &mut Objects, object: Envelope<Event>| {
        let name = seal_hash(&object);
        objects.insert(name.clone(), object);
        name
    };
    for g in 0..count {
        let genesis = put(
            &mut objects,
            Envelope::Genesis(mk_genesis("laws", &format!("{g:032x}")).expect("a genesis")),
        );
        held.push(vec![genesis]);
    }
    let origins: Vec<Hash> = held.iter().map(|names| names[0].clone()).collect();
    for (i, (kind, prodrome, picks)) in drawn.iter().enumerate() {
        let prodrome = prodrome % origins.len();
        let genesis = origins[prodrome].clone();
        let before = &held[prodrome];
        let picked: BTreeSet<Hash> = picks
            .iter()
            .map(|pick| before[pick % before.len()].clone())
            .collect();
        let others: Vec<Hash> = picked
            .iter()
            .filter(|name| **name != genesis)
            .cloned()
            .collect();
        let actor = Actor::new(format!("writer{i}")).expect("an actor");
        let key = Secret::new(&format!("{i:064x}")).expect("a seed");
        let object = match kind {
            0 => Envelope::Change(
                mk_change(genesis, others, created(&format!("t{i}"))).expect("a change"),
            ),
            1 => {
                let previous = picked
                    .iter()
                    .find(|name| matches!(objects.get(*name), Some(Envelope::Snapshot(_))))
                    .cloned();
                let tips: Vec<Hash> = if picked.is_empty() {
                    vec![genesis.clone()]
                } else {
                    picked.into_iter().collect()
                };
                Envelope::Snapshot(mk_snapshot(genesis, tips, previous).expect("a snapshot"))
            }
            2 => Envelope::KeyAdded(KeyAdded {
                genesis,
                actor,
                key: key.public(),
            }),
            3 => Envelope::KeyRevoked(
                mk_key_revoked(genesis, others, actor, key.public()).expect("a revocation"),
            ),
            _ => Envelope::Signed(key.sign(&before[i % before.len()])),
        };
        let name = put(&mut objects, object);
        held[prodrome].push(name);
    }
    (origins, objects)
}

/// Where each name stands in `order`, which must hold every object once.
fn positions(order: &[Hash], objects: &Objects) -> Result<BTreeMap<Hash, usize>, TestCaseError> {
    let at: BTreeMap<Hash, usize> = order.iter().cloned().zip(0..).collect();
    prop_assert_eq!(at.len(), order.len(), "each object once");
    prop_assert_eq!(
        at.keys().collect::<Vec<_>>(),
        objects.keys().collect::<Vec<_>>()
    );
    Ok(at)
}

/// Every object after everything it rests on.
fn extends(order: &[Hash], objects: &Objects) -> Result<(), TestCaseError> {
    let at = positions(order, objects)?;
    for (name, object) in objects {
        for beneath in rests_on(object) {
            prop_assert!(
                at[&beneath] < at[name],
                "{} before {}",
                beneath.as_str(),
                name.as_str()
            );
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// THE HEADS ARE THE MAXIMAL OBJECTS: every head is an object nothing
    /// rests on, its genesis included in what rests on means, and every
    /// such object is a head; of the DAG, of each prodrome, and of a store
    /// in memory that received the objects in any order. A genesis is a
    /// head exactly when nothing is begun from it.
    #[test]
    fn the_heads_are_what_nothing_rests_on(
        (geneses, drawn) in a_history(),
        order in prop::collection::vec(any::<prop::sample::Index>(), 0..8),
    ) {
        let (geneses, objects) = realise(geneses, &drawn);
        let beneath: BTreeSet<Hash> = objects.values().flat_map(rests_on).collect();
        let heads: BTreeSet<Hash> = objects.keys().filter(|name| !beneath.contains(*name)).cloned().collect();
        let dag: Dag<Event> = objects.clone().into_iter().collect();
        prop_assert_eq!(&dag.tips(), &heads);
        for genesis in &geneses {
            let begun = objects.values().any(|object| rests_on(object).contains(genesis));
            prop_assert_eq!(heads.contains(genesis), !begun);
            let mine: BTreeSet<Hash> = heads
                .iter()
                .filter(|head| dag.genesis_of(head).as_ref() == Some(genesis))
                .cloned()
                .collect();
            prop_assert_eq!(dag.tips_in(&Some(genesis.clone())), mine);
        }

        let store: MemoryStore<Event> = MemoryStore::default();
        let prints: BTreeMap<Hash, Vec<u8>> = objects
            .iter()
            .map(|(name, object)| (name.clone(), canonical_envelope(object).into_bytes()))
            .collect();
        let names: Vec<Hash> = objects.keys().cloned().collect();
        for index in &order {
            store
                .receive([index.get(&names).clone()].into(), &|name| prints.get(name).cloned())
                .expect("receives");
        }
        store
            .receive(names.iter().cloned().collect(), &|name| prints.get(name).cloned())
            .expect("receives");
        prop_assert_eq!(store.held().expect("reads").tips, heads);
    }

    /// THE LINEARISATION IS A LINEAR EXTENSION, and so is the order a
    /// replica takes objects in (`store::accept`'s walk, parents first).
    #[test]
    fn every_order_puts_what_an_object_rests_on_first((geneses, drawn) in a_history()) {
        let (_, objects) = realise(geneses, &drawn);
        let dag: Dag<Event> = objects.clone().into_iter().collect();
        extends(dag.linearise().expect("orders"), &objects)?;

        let store: MemoryStore<Event> = MemoryStore::default();
        let prints: BTreeMap<Hash, Vec<u8>> = objects
            .iter()
            .map(|(name, object)| (name.clone(), canonical_envelope(object).into_bytes()))
            .collect();
        let taken = store
            .receive(dag.tips(), &|name| prints.get(name).cloned())
            .expect("receives");
        extends(&taken, &objects)?;
    }

    /// A CLOSURE IS THE DOWN-SET, and the history a DAG lacking a genesis
    /// holds is the down-set of everything that does not rest on it.
    #[test]
    fn a_closure_is_the_down_set_and_a_genesis_bears_its_prodrome(
        (geneses, drawn) in a_history(),
        pick in any::<prop::sample::Index>(),
    ) {
        let (geneses, objects) = realise(geneses, &drawn);
        let dag: Dag<Event> = objects.clone().into_iter().collect();
        for name in objects.keys() {
            prop_assert_eq!(dag.closure([name.clone()]), down(&objects, [name.clone()]));
        }
        let gone = pick.get(&geneses);
        let without: Dag<Event> = objects
            .iter()
            .filter(|(name, _)| *name != gone)
            .map(|(name, object)| (name.clone(), object.clone()))
            .collect();
        let kept: BTreeSet<Hash> = objects
            .keys()
            .filter(|name| !down(&objects, [(*name).clone()]).contains(gone))
            .cloned()
            .collect();
        prop_assert_eq!(without.interior().objects().keys().cloned().collect::<BTreeSet<_>>(), kept);
    }
}
