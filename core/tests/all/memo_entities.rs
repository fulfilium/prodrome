//! A worked example (design §6.2): an application's map of one value for
//! each entity, held as entries of the one cache.
//!
//! An application often keeps a map of its own beside a cache: for each
//! entity, a value that it computed from that entity's history, with the
//! last event that it read, so that it can tell when to compute again. Here
//! the value is the total of the amounts that an entity's events carry.
//!
//! In the one cache, each value of that map is an entry:
//!
//! - The function's name is the algebra's name, `memo::name(b"total", [])`.
//!   It covers the code that computes a total. A program that changes that
//!   code, or reads one more input, gives the algebra another name.
//! - The argument's name is the entity's node in a tree: [`memo::name`]
//!   over the entity's id and the root of the balanced tree of its events'
//!   names. The name changes exactly when the entity's history changes, so
//!   no entry is ever stale and no program checks a head.
//! - "Look up by entity" becomes "build the entity's node from the events
//!   that the store holds for it, and fold the algebra over the node". A
//!   warm cache answers with one hit at the node. A cold one computes the
//!   entity's events, and after an append it computes only the spine from
//!   the new event to the entity's node.
//!
//! The entities hang under one node for the whole ledger, so one fold over
//! the ledger fills the entry of every entity, and after an append it hits
//! every other entity at its node. The interface did not grow for this.

use std::collections::BTreeMap;
use std::fs;

use prodrome::event::Hash;
use prodrome::memo::{self, fold, Algebra, Cache, File, Keep, Outcome, Tree};
use proptest::prelude::*;

use crate::memory::Scratch;

/// What a node of the ledger is.
#[derive(Debug, Clone)]
enum Own {
    /// The whole ledger, its entities as children.
    Ledger,
    /// One entity, the tree of its events as its child.
    Entity,
    /// A chunk of events, made by `memo::balance`.
    Chunk,
    /// One event, with the amount it carries.
    Event(u64),
}

#[derive(Debug, Clone)]
struct Node {
    name: Hash,
    own: Own,
    kids: Vec<Node>,
}

impl Tree for Node {
    fn name(&self) -> &Hash {
        &self.name
    }

    fn children(&self) -> impl Iterator<Item = &Node> {
        self.kids.iter()
    }
}

/// The total of the amounts under a node.
struct Total(Hash);

impl Algebra<Node> for Total {
    type Out = u64;
    fn name(&self) -> &Hash {
        &self.0
    }
    fn apply(&self, node: &Node, children: Vec<u64>) -> u64 {
        match node.own {
            Own::Event(amount) => amount,
            Own::Ledger | Own::Entity | Own::Chunk => children.into_iter().sum(),
        }
    }
}

fn total() -> Total {
    Total(memo::name(b"total", []))
}

/// The `n`th event of `entity`, as a store would name it.
fn event(entity: &str, n: usize, amount: u64) -> Node {
    Node {
        name: memo::name(format!("{entity} {n} {amount}").as_bytes(), []),
        own: Own::Event(amount),
        kids: Vec::new(),
    }
}

/// The node of `entity` over its events.
fn entity(id: &str, amounts: &[u64]) -> Node {
    let events = amounts
        .iter()
        .enumerate()
        .map(|(n, amount)| event(id, n, *amount))
        .collect();
    let kids: Vec<Node> = memo::balance(events, |kids: Vec<Node>| Node {
        name: memo::name(b"chunk", kids.iter().map(|kid| &kid.name)),
        own: Own::Chunk,
        kids,
    })
    .into_iter()
    .collect();
    Node {
        name: memo::name(
            format!("entity {id}").as_bytes(),
            kids.iter().map(|kid| &kid.name),
        ),
        own: Own::Entity,
        kids,
    }
}

/// The ledger over every entity, in the order of their ids.
fn ledger(histories: &BTreeMap<String, Vec<u64>>) -> Node {
    let kids: Vec<Node> = histories
        .iter()
        .map(|(id, amounts)| entity(id, amounts))
        .collect();
    Node {
        name: memo::name(b"ledger", kids.iter().map(|kid| &kid.name)),
        own: Own::Ledger,
        kids,
    }
}

fn histories() -> BTreeMap<String, Vec<u64>> {
    [("ada", 40), ("ben", 30), ("cy", 20)]
        .into_iter()
        .map(|(id, events)| {
            let amounts = (0..events).map(|n| (n * 7 + id.len()) as u64).collect();
            (id.to_string(), amounts)
        })
        .collect()
}

/// A process fills the cache with one fold over the ledger. A new process
/// looks up one entity with one hit. After an event is appended to `ada`,
/// a fold over the ledger hits `ben` and `cy` at their nodes and computes
/// only the spine from the new event to the root.
#[test]
fn a_map_of_entities_is_entries_of_the_one_cache() {
    let scratch = Scratch::new("entities");
    fs::create_dir_all(&scratch.0).expect("a scratch directory");
    let path = scratch.0.join("cache");
    let mut histories = histories();
    {
        let mut cache = Cache::held(File::open(&path).expect("opens"), Keep);
        let (value, _) = fold(&total(), &ledger(&histories), &mut cache);
        assert_eq!(value, histories.values().flatten().sum::<u64>());
        cache.settle().expect("settles");
    }

    let mut cache = Cache::held(File::open(&path).expect("opens"), Keep);
    let ben = entity("ben", &histories["ben"]);
    let (value, report) = fold(&total(), &ben, &mut cache);
    assert_eq!(value, histories["ben"].iter().sum::<u64>());
    assert_eq!(report.visits().len(), 1);
    assert_eq!(report.visits()[0].outcome, Outcome::Hit);

    histories.get_mut("ada").expect("ada").push(1000);
    let after = ledger(&histories);
    let (value, report) = fold(&total(), &after, &mut cache);
    assert_eq!(value, histories.values().flatten().sum::<u64>());
    let hits: Vec<&Hash> = report.names(Outcome::Hit).collect();
    for id in ["ben", "cy"] {
        assert!(
            hits.contains(&&entity(id, &histories[id]).name),
            "{id} hits"
        );
    }
    let computed = report
        .by_depth()
        .iter()
        .map(|counts| counts.computed)
        .sum::<usize>();
    let depth = report.by_depth().len();
    assert!(
        computed <= depth,
        "{computed} nodes computed over {depth} levels"
    );
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// Through a file opened again before each round, the entry of each
    /// entity is the total that a map of the application's own would hold,
    /// over histories that grow by random appends.
    #[test]
    fn each_entity_reads_as_the_map_would(
        rounds in prop::collection::vec(prop::collection::vec((0..3usize, 0..100u64), 0..6), 1..4),
    ) {
        let scratch = Scratch::new("entities-map");
        fs::create_dir_all(&scratch.0).expect("a scratch directory");
        let path = scratch.0.join("cache");
        let ids = ["ada", "ben", "cy"];
        let mut histories: BTreeMap<String, Vec<u64>> =
            ids.iter().map(|id| (id.to_string(), Vec::new())).collect();
        for appends in rounds {
            for (at, amount) in appends {
                histories.get_mut(ids[at]).expect("an entity").push(amount);
            }
            let mut cache = Cache::held(File::open(&path).expect("opens"), Keep);
            let _ = fold(&total(), &ledger(&histories), &mut cache);
            for (id, amounts) in &histories {
                let (value, _) = fold(&total(), &entity(id, amounts), &mut cache);
                prop_assert_eq!(value, amounts.iter().sum::<u64>());
            }
            cache.settle().expect("settles");
        }
    }
}
