//! A store's memory (design §6.1, stage 7): what a store has read, held and
//! extended by the objects it has not, reads exactly as the same objects
//! read cold.
//!
//! The fold first, with no store in the way: [`Folded::insert`] in ANY order
//! that puts parents first is `fold` of the union, so the state a store holds
//! is a function of its objects and not of the order they reached it.
//!
//! Then the store: law 7's pieces where the memory meets them. An adoption
//! through a handle that has read is idempotent and order-free, an
//! interrupted one completes on the next, and what it reads after is the
//! reading of the union of the two directories. A handle that lives through
//! a script of writes and other doors reads as a fresh one: that is law 45,
//! in `store_laws/script.rs`.

use crate::common;
use crate::store_laws::{copy_prints, StoreOfRecord};

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use prodrome::dag::Excluded;
use prodrome::event::{Actor, Hash, TodoEvent, TodoId};
use prodrome::fold::{self as folds, Kind, Product};
use prodrome::policy::Untrusted;
use prodrome::reference::Todo;
use prodrome::registers::{fold, Folded, Genesis, Node};
use prodrome::store::EventStore;
use prodrome::store::Sender;
use proptest::prelude::*;

use common::{a_log, seal, two_writers};

type Event = TodoEvent<Todo>;
type Store = EventStore<Event, Untrusted>;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A directory of stores, removed when the case ends.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn new(what: &str) -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "prodrome-memory-{what}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        Scratch(root)
    }

    pub(crate) fn store(&self, name: &str) -> Store {
        Store::new(self.0.join(name), roster())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn roster() -> Untrusted {
    Untrusted::of([Actor::new("triage").expect("valid")])
}

/// Two writers' changes over one genesis, each on its own replica: a shared
/// log appended, then each side's own.
pub(crate) fn replicas(
    scratch: &Scratch,
    (shared, mine, theirs): &(Vec<Event>, Vec<Event>, Vec<Event>),
) -> (Store, Store) {
    let here = scratch.store("here");
    here.init("memory").expect("begins");
    for event in shared {
        here.append(event.clone()).expect("appends");
    }
    let there = scratch.store("there");
    for tip in here.tips().expect("the tips derive") {
        there.adopt(&here, &tip, Sender::Stranger).expect("adopts");
    }
    for event in mine {
        here.append(event.clone()).expect("appends");
    }
    for event in theirs {
        there.append(event.clone()).expect("appends");
    }
    (here, there)
}

/// [`replicas`], with each side's tips adopted by the other.
fn changes(scratch: &Scratch, logs: &(Vec<Event>, Vec<Event>, Vec<Event>)) -> Store {
    let (here, there) = replicas(scratch, logs);
    for tip in there.tips().expect("the tips derive") {
        here.adopt(&there, &tip, Sender::Stranger).expect("adopts");
    }
    here
}

/// The same logs as legacy objects: each sealed on every tip of its writer's
/// replica, so the second head is a `Woven` once both are adopted.
fn sealed(
    scratch: &Scratch,
    (shared, mine, theirs): &(Vec<Event>, Vec<Event>, Vec<Event>),
) -> Store {
    let here = scratch.store("here");
    for event in shared {
        seal(&here, event.clone());
    }
    let there = scratch.store("there");
    for tip in here.tips().expect("the tips derive") {
        there.adopt(&here, &tip, Sender::Stranger).expect("adopts");
    }
    for event in mine {
        seal(&here, event.clone());
    }
    for event in theirs {
        seal(&there, event.clone());
    }
    for tip in there.tips().expect("the tips derive") {
        here.adopt(&there, &tip, Sender::Stranger).expect("adopts");
    }
    here
}

/// `nodes` in the topological order `keys` draws: of the objects whose
/// parents are all placed, the one with the least key, then name.
fn shuffled(nodes: &[Node<Event>], keys: &[u32]) -> Vec<Node<Event>> {
    let names: BTreeSet<&Hash> = nodes.iter().map(Node::name).collect();
    let mut placed: BTreeSet<&Hash> = BTreeSet::new();
    let mut left: Vec<(u32, &Node<Event>)> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (keys.get(i).copied().unwrap_or(0), node))
        .collect();
    let mut out = Vec::with_capacity(nodes.len());
    while !left.is_empty() {
        let ready = left
            .iter()
            .enumerate()
            .filter(|(_, (_, node))| {
                node.parents()
                    .iter()
                    .all(|p| placed.contains(p) || !names.contains(p))
            })
            .min_by_key(|(_, (key, node))| (*key, node.name()))
            .map(|(i, _)| i)
            .expect("a DAG always has a ready node");
        let (_, node) = left.remove(ready);
        placed.insert(node.name());
        out.push(node.clone());
    }
    out
}

/// The first `split` objects of `order` folded cold, in the linearisation's
/// order, and the rest inserted in `order`'s.
fn inserted(nodes: &[Node<Event>], order: &[Node<Event>], split: usize) -> Folded<Event> {
    let split = split.min(order.len());
    let first: BTreeSet<&Hash> = order[..split].iter().map(Node::name).collect();
    let prefix: Vec<Node<Event>> = nodes
        .iter()
        .filter(|node| first.contains(node.name()))
        .cloned()
        .collect();
    let mut state = fold(&prefix);
    for node in &order[split..] {
        state
            .insert(node)
            .expect("every object rests within its scope");
    }
    state
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// THE FOLD IS AN ACTION OF THE SET. Over two writers' changes and over
    /// the same logs as legacy objects, a down-set folded cold and the rest
    /// inserted in any order that puts parents first is the fold of the
    /// whole; inserting what is held changes nothing.
    #[test]
    fn inserting_in_any_order_folds_as_the_union(
        logs in two_writers(),
        keys in prop::collection::vec(0u32..1000, 0..60),
        split in 0usize..60,
    ) {
        let scratch = Scratch::new("insert");
        for store in [changes(&scratch, &logs), sealed(&Scratch::new("insert"), &logs)] {
            let nodes = store.dag().and_then(|dag| dag.nodes()).expect("reads");
            let whole = fold(&nodes);
            prop_assert!(whole.local(), "an append's store rests within its scopes");
            let order = shuffled(&nodes, &keys);
            let mut state = inserted(&nodes, &order, split);
            prop_assert_eq!(&state, &whole);
            for node in &nodes {
                state.insert(node).expect("held");
            }
            prop_assert_eq!(&state, &whole);
        }
    }
}

/// Each register's reading, by entity: the writes it reads as, by name.
type Readings = BTreeMap<(Genesis, TodoId), Vec<Vec<Hash>>>;

fn readings(state: &Folded<Event>) -> Readings {
    let mut out = Readings::new();
    for (genesis, prodrome) in state.prodromes() {
        for (todo, stream) in prodrome {
            let registers = folds::read(stream, None, &roster());
            let reading = [Kind::State, Kind::Spec, Kind::Content]
                .into_iter()
                .map(|kind| {
                    registers
                        .reading(kind)
                        .into_iter()
                        .map(|stamp| stamp.name.clone())
                        .collect()
                })
                .collect();
            out.insert((genesis.clone(), todo.clone()), reading);
        }
    }
    out
}

/// What a store reads: its objects, its fold, each register's reading, its
/// heads and what it left out, or the refusal.
#[derive(Debug, PartialEq)]
struct Read {
    objects: BTreeSet<Hash>,
    folded: Folded<Event>,
    readings: Readings,
    tips: BTreeSet<Hash>,
    excluded: Vec<Excluded>,
}

fn read(store: &Store) -> Result<Read, String> {
    let dag = store.dag().map_err(|e| e.to_string())?;
    let folded = store.folded().map_err(|e| e.to_string())?;
    Ok(Read {
        objects: dag.objects().keys().cloned().collect(),
        readings: readings(&folded),
        folded: (*folded).clone(),
        tips: store.tips().map_err(|e| e.to_string())?,
        excluded: dag.excluded(),
    })
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// LAW 7 WHERE THE MEMORY MEETS IT. A handle that has read adopts
    /// another replica's tips, first a non-tip (an interrupted sync), then
    /// every tip in a drawn order and then again: it reads what a fresh
    /// handle reads of its directory, which is what a fresh handle reads of
    /// a directory holding the union of both replicas' files, and the same
    /// whatever the order.
    #[test]
    fn an_adoption_through_the_memory_reads_as_the_union(
        logs in two_writers(),
        keys in prop::collection::vec(0u32..1000, 0..8),
        partial in any::<prop::sample::Index>(),
    ) {
        let scratch = Scratch::new("sync");
        let (base, there) = replicas(&scratch, &logs);
        let mut reads = Vec::new();
        for (n, order) in [keys.clone(), keys.iter().rev().copied().collect()].iter().enumerate() {
            let here = scratch.store(&format!("into-{n}"));
            copy_prints(&base, &here);
            read(&here).expect("reads");
            let theirs: Vec<Hash> = there.dag().expect("reads").objects().keys().cloned().collect();
            here.adopt(&there, partial.get(&theirs), Sender::Stranger).expect("a partial sync");
            let mut tips: Vec<(u32, Hash)> = there
                .tips()
                .expect("derives")
                .into_iter()
                .enumerate()
                .map(|(i, tip)| (order.get(i).copied().unwrap_or(0), tip))
                .collect();
            tips.sort();
            for _ in 0..2 {
                for (_, tip) in &tips {
                    here.adopt(&there, tip, Sender::Stranger).expect("adopts");
                }
            }
            let union = scratch.store(&format!("union-{n}"));
            copy_prints(&base, &union);
            copy_prints(&there, &union);
            let warm = read(&here);
            prop_assert_eq!(&warm, &read(&scratch.store(&format!("into-{n}"))));
            prop_assert_eq!(&warm, &read(&union));
            reads.push(warm);
        }
        prop_assert_eq!(&reads[0], &reads[1]);
    }
}

/// THE TWIN AN APPEND ANSWERS IS THE FIRST IN THE LINEARISATION. Two
/// replicas write the same event over different deps, so the union holds
/// two objects whose events print the same; appending it once more through
/// a handle that has read both writes nothing and answers the one a cold
/// read finds first. Each round's genesis is new, so the rounds draw both
/// orders of the twins' names.
#[test]
fn an_append_answers_the_first_twin() {
    let at = |day| prodrome::literal::Datetime::new(2026, 9, day, 12, 0, 0, 0).expect("an instant");
    for round in 0..16 {
        let scratch = Scratch::new(&format!("twins-{round}"));
        let here = scratch.store("here");
        here.init("twins").expect("begins");
        let there = scratch.store("there");
        for tip in here.tips().expect("derives") {
            there.adopt(&here, &tip, Sender::Stranger).expect("adopts");
        }
        let twin = prodrome::event::mk_completed("alpha", at(2), "bassel", "").expect("valid");
        here.append(prodrome::event::mk_reopened("alpha", at(1), "bassel", "").expect("valid"))
            .expect("appends");
        here.append(twin.clone()).expect("appends");
        there.append(twin.clone()).expect("appends");
        read(&here).expect("reads");
        for tip in there.tips().expect("derives") {
            here.adopt(&there, &tip, Sender::Stranger).expect("adopts");
        }
        let print = prodrome::event::canonical(&twin);
        let first = here
            .dag()
            .and_then(|dag| dag.nodes())
            .expect("reads")
            .into_iter()
            .find(|node| {
                node.event()
                    .is_some_and(|e| prodrome::event::canonical(e) == print)
            })
            .expect("a twin")
            .name()
            .clone();
        let objects = here.prints();
        assert_eq!(here.append(twin.clone()).expect("appends"), first);
        assert_eq!(scratch.store("here").append(twin).expect("appends"), first);
        assert_eq!(here.prints(), objects, "a twin writes nothing");
    }
}

/// A STORE IS SHARED BETWEEN THREADS, and its memory with it: a clone of a
/// handle shares the handle's memory.
#[test]
fn a_store_is_send_and_sync() {
    fn shared<T: Send + Sync>() {}
    shared::<Store>();
}

/// TWO WRITERS ON ONE STORE STAY CORRECT. Threads append at once, two
/// through clones of one handle (one memory) and two through handles of
/// their own (each its own memory and its own descriptor on the lock, as two
/// processes are), every one of them appending one event they all share:
/// the store holds that event once, each writer's own events once each, and
/// every handle reads what a fresh one reads.
#[test]
fn writers_at_once_write_each_event_once() {
    let scratch = Scratch::new("threads");
    let first = scratch.store("store");
    first.init("threads").expect("begins");
    let at =
        |hour| prodrome::literal::Datetime::new(2026, 9, 1, hour, 0, 0, 0).expect("an instant");
    let shared = prodrome::event::mk_created("shared", at(1), "bassel", "", "").expect("valid");
    let writers = [
        first.clone(),
        first.clone(),
        scratch.store("store"),
        scratch.store("store"),
    ];
    std::thread::scope(|threads| {
        for (n, writer) in writers.iter().enumerate() {
            let shared = shared.clone();
            threads.spawn(move || {
                for hour in 0..6 {
                    let todo = format!("todo-{n}");
                    let event = prodrome::event::mk_completed(&todo, at(hour), "bassel", "")
                        .expect("valid");
                    writer.append(event).expect("appends");
                    writer.append(shared.clone()).expect("appends");
                }
            });
        }
    });
    let cold = read(&scratch.store("store")).expect("reads");
    assert_eq!(
        cold.objects.len(),
        1 + 1 + writers.len() * 6,
        "the genesis, the shared event, each writer's own"
    );
    for writer in &writers {
        assert_eq!(read(writer).as_ref(), Ok(&cold));
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(16))]

    /// THE GENESES ARE KEPT, NOT SCANNED FOR. Two unrelated legacy replicas,
    /// one's files arriving in the other's directory one at a time while a
    /// handle reads: an append through that handle writes into the prodrome
    /// of the least legacy root, the object a fresh handle's append writes.
    #[test]
    fn an_append_writes_into_the_least_legacy_root(
        mine in a_log(),
        theirs in a_log(),
        event in a_log(),
    ) {
        prop_assume!(!mine.is_empty() && !theirs.is_empty() && !event.is_empty());
        let scratch = Scratch::new("roots");
        let here = scratch.store("here");
        for event in &mine {
            seal(&here, event.clone());
        }
        let there = scratch.store("there");
        for event in &theirs {
            seal(&there, event.clone());
        }
        read(&here).expect("reads");
        let kept = here.prints();
        for (name, bytes) in there.prints().into_iter().rev() {
            if !kept.contains_key(&name) {
                here.place(&name, &bytes);
            }
            read(&here).expect("reads");
        }
        let twin = scratch.store("twin");
        copy_prints(&here, &twin);
        let cold = twin.append(event[0].clone()).map_err(|e| e.to_string());
        prop_assert_eq!(here.append(event[0].clone()).map_err(|e| e.to_string()), cold);
        prop_assert_eq!(read(&here), read(&scratch.store("here")));
    }
}
