//! One script of writes, by the store's handles and by other doors, and the
//! laws that hold after every step of it: SPEC law 45 (a handle that lived
//! through the steps reads as a fresh one, and its appends write what a
//! fresh handle's append writes), and laws 60, 62, 63 and 65 (the record of
//! heads).
//!
//! A step is a write of the store (an append, a run of appends under one
//! hold, a receipt, a quarantine, a `record_heads`) by one of three handles
//! of one store, or a door (a print arrives, a print is taken away, the
//! record is removed), or a read. Each write of the store holds the lock,
//! so after it the record of heads is the derived heads. A door leaves the
//! record as it was, so after it the store serves the older record, or the
//! derived heads where the record names a tip that the store no longer
//! keeps.

use crate::common;

use std::collections::BTreeSet;

use prodrome::dag::{Dag, Excluded, Finding, Heads};
use prodrome::event::{Hash, TodoEvent};
use prodrome::reference::Todo;
use prodrome::registers::Folded;
use prodrome::store::{sync, Sender};
use proptest::prelude::*;
use proptest::sample::Index;

use super::kind::{derived, stores_of_record, Scratch, StoreOfRecord};
use common::a_log;

type Event = TodoEvent<Todo>;

/// Which handle of the store makes a step.
#[derive(Debug, Clone, Copy)]
enum Handle {
    /// The handle that lives through every step.
    First,
    /// A second handle that lives through every step, as a second process
    /// does.
    Second,
    /// A new handle for this step alone.
    Fresh,
}

fn a_handle() -> impl Strategy<Value = Handle> {
    prop_oneof![
        Just(Handle::First),
        Just(Handle::Second),
        Just(Handle::Fresh)
    ]
}

/// A write of the store, by one handle. Each one holds the store's lock.
#[derive(Debug, Clone)]
enum Write {
    /// One event of the pool appended.
    Append(Index),
    /// Several events of the pool appended under one hold.
    Run(Vec<Index>),
    /// A store elsewhere appends one event of its own pool, and the store
    /// receives everything it holds.
    Receive(Index),
    /// One object of the history damaged by the door, then `fsck`.
    Quarantine(Index),
    /// `record_heads`.
    RecordHeads,
}

/// A door: a change of what the store keeps, by no handle of the store.
#[derive(Debug, Clone)]
enum Door {
    /// One print that the store elsewhere keeps and the store lacks, put in
    /// the store, as a copy of files or a `git merge` does.
    Arrive(Index),
    /// One print taken away, as a host deletes a rejected proposal. The
    /// genesis stays.
    SetAside(Index),
    /// The record of heads removed.
    DropRecord,
}

#[derive(Debug, Clone)]
enum Step {
    Write(Handle, Write),
    Door(Door),
    Read(Handle),
}

fn a_step() -> impl Strategy<Value = Step> {
    let write = prop_oneof![
        4 => any::<Index>().prop_map(Write::Append),
        2 => prop::collection::vec(any::<Index>(), 1..4).prop_map(Write::Run),
        2 => any::<Index>().prop_map(Write::Receive),
        1 => any::<Index>().prop_map(Write::Quarantine),
        1 => Just(Write::RecordHeads),
    ];
    let door = prop_oneof![
        3 => any::<Index>().prop_map(Door::Arrive),
        1 => any::<Index>().prop_map(Door::SetAside),
        1 => Just(Door::DropRecord),
    ];
    prop_oneof![
        5 => (a_handle(), write).prop_map(|(by, write)| Step::Write(by, write)),
        2 => door.prop_map(Step::Door),
        1 => a_handle().prop_map(Step::Read),
    ]
}

/// What a handle reads: its history, its fold, its tips and what it leaves
/// out, or its refusal.
#[derive(Debug, PartialEq)]
struct Reading {
    dag: Dag<Event>,
    folded: Folded<Event>,
    tips: BTreeSet<Hash>,
    excluded: Vec<Excluded>,
}

fn reading(store: &dyn StoreOfRecord<Event>) -> Result<Reading, String> {
    let held = store.held().map_err(|error| error.to_string())?;
    Ok(Reading {
        excluded: held.dag.excluded(),
        dag: (*held.dag).clone(),
        folded: (*held.folded).clone(),
        tips: held.tips,
    })
}

/// One store, its two long-lived handles, and a store elsewhere of the same
/// prodrome, which a script of [`Step`]s writes to.
struct Script<'s> {
    first: &'s dyn StoreOfRecord<Event>,
    second: Box<dyn StoreOfRecord<Event>>,
    elsewhere: Box<dyn StoreOfRecord<Event>>,
    genesis: Hash,
    pool: Vec<Event>,
    theirs: Vec<Event>,
}

impl<'s> Script<'s> {
    /// The store begun, and the store elsewhere, which shares the genesis
    /// and holds every second event of both pools, so that what arrives
    /// from it is concurrent with the store's writes, conflicts with them
    /// and twins of them.
    fn new(
        store: &'s dyn StoreOfRecord<Event>,
        pool: Vec<Event>,
        theirs: Vec<Event>,
    ) -> Script<'s> {
        let genesis = super::kind::begin(store, "script");
        let elsewhere = store.sibling();
        sync(store, &*elsewhere, Sender::Stranger).expect("syncs");
        for event in pool.iter().chain(&theirs).step_by(2) {
            let _ = elsewhere.append(event.clone());
        }
        Script {
            first: store,
            second: store.open(),
            elsewhere,
            genesis,
            pool,
            theirs,
        }
    }

    /// Run `step`, then check every law after it. A refused write changes
    /// nothing, and the laws say what the store then reads.
    fn run(&self, step: &Step) -> Result<(), TestCaseError> {
        let before = self.first.record();
        match step {
            Step::Write(by, write) => match by {
                Handle::First => self.write(self.first, write)?,
                Handle::Second => self.write(&*self.second, write)?,
                Handle::Fresh => self.write(&*self.first.open(), write)?,
            },
            Step::Door(door) => self.open(door),
            Step::Read(by) => {
                let _ = match by {
                    Handle::First => self.first.held(),
                    Handle::Second => self.second.held(),
                    Handle::Fresh => self.first.open().held(),
                };
            }
        }
        let recorded = match step {
            Step::Write(..) => Some(derived(self.first)),
            Step::Door(Door::DropRecord) => None,
            Step::Door(Door::Arrive(_) | Door::SetAside(_)) | Step::Read(_) => before,
        };
        self.holds(step, recorded)
    }

    /// `write`, by the handle `by`.
    fn write(&self, by: &dyn StoreOfRecord<Event>, write: &Write) -> Result<(), TestCaseError> {
        match write {
            Write::Append(pick) => {
                let event = pick.get(&self.pool).clone();
                // Law 45: the append writes what a fresh handle's append to
                // a copy of the store writes, byte for byte.
                let copy = self.first.sibling();
                super::kind::copy_prints(self.first, &*copy);
                let cold = copy.append(event.clone());
                let written = by.append(event);
                prop_assert_eq!(
                    written.as_ref().map_err(ToString::to_string),
                    cold.as_ref().map_err(ToString::to_string)
                );
                if let Ok(name) = written {
                    prop_assert_eq!(self.first.kept(&name), copy.kept(&name));
                }
            }
            Write::Run(picks) => {
                if let Ok(mut hold) = by.hold() {
                    for pick in picks {
                        let _ = hold.append(pick.get(&self.pool).clone());
                    }
                }
            }
            Write::Receive(pick) => {
                if !self.theirs.is_empty() {
                    let _ = self.elsewhere.append(pick.get(&self.theirs).clone());
                }
                let _ = sync(&*self.elsewhere, by, Sender::Stranger);
            }
            Write::Quarantine(pick) => {
                let names: Vec<Hash> = history(self.first);
                if !names.is_empty() {
                    self.first.replace(pick.get(&names), b"damaged");
                }
                let _ = by.fsck();
            }
            Write::RecordHeads => {
                let _ = by.record_heads();
            }
        }
        Ok(())
    }

    /// `door`, opened past every handle.
    fn open(&self, door: &Door) {
        match door {
            Door::Arrive(pick) => {
                let kept = self.first.prints();
                let lacking: Vec<(Hash, Vec<u8>)> = self
                    .elsewhere
                    .prints()
                    .into_iter()
                    .filter(|(name, _)| !kept.contains_key(name))
                    .collect();
                if !lacking.is_empty() {
                    let (name, bytes) = pick.get(&lacking);
                    self.first.place(name, bytes);
                }
            }
            Door::SetAside(pick) => {
                let kept: Vec<Hash> = self
                    .first
                    .prints()
                    .into_keys()
                    .filter(|name| *name != self.genesis)
                    .collect();
                if !kept.is_empty() {
                    self.first.set_aside(pick.get(&kept));
                }
            }
            Door::DropRecord => self.first.set_record(None),
        }
    }

    /// The laws after `step`, where `recorded` is the record that the step
    /// leaves.
    fn holds(&self, step: &Step, recorded: Option<Heads>) -> Result<(), TestCaseError> {
        let fresh = self.first.open();
        let cold = reading(&*fresh);
        // Law 45: incremental equals cold.
        prop_assert_eq!(&reading(self.first), &cold, "after {:?}", step);
        prop_assert_eq!(&reading(&*self.second), &cold, "after {:?}", step);
        // Law 60: after a write of the store, the record is the derived
        // heads; after a door, it is the record before the door.
        let derived = derived(self.first);
        prop_assert_eq!(self.first.record(), recorded.clone(), "after {:?}", step);
        // Laws 62 and 63: the store serves the record while every tip it
        // names is kept, and otherwise the derived heads.
        let kept = self.first.prints();
        let served = match &recorded {
            Some(record) if record.tips().iter().all(|tip| kept.contains_key(tip)) => {
                record.clone()
            }
            Some(_) | None => derived.clone(),
        };
        for handle in [self.first, &*self.second, &*fresh] {
            prop_assert_eq!(
                handle.heads().expect("heads"),
                served.clone(),
                "after {:?}",
                step
            );
        }
        // Law 63: `verify` reports a record that is not the derived heads.
        let stale: Vec<Finding> = self
            .first
            .verify()
            .into_iter()
            .filter(|finding| matches!(finding, Finding::StaleHeads { .. }))
            .collect();
        let expected: Vec<Finding> = recorded
            .filter(|record| *record != derived)
            .map(|record| Finding::StaleHeads {
                recorded: record,
                derived: derived.clone(),
            })
            .into_iter()
            .collect();
        prop_assert_eq!(stale, expected, "after {:?}", step);
        // Law 65: the heads of the history are its tips and the count of
        // its objects, and a `Dag` read fresh from exactly its objects has
        // the same heads.
        let held = self.first.held().expect("reads");
        prop_assert_eq!(derived.tips(), &held.tips);
        prop_assert_eq!(derived.count(), held.dag.objects().len());
        let dag = held.dag;
        prop_assert_eq!(dag.heads(), derived.clone());
        let again: Dag<Event> = dag.objects().clone().into_iter().collect();
        prop_assert_eq!(again.heads(), derived);
        Ok(())
    }
}

/// The names of the history of `store`, as a fresh handle reads it.
fn history(store: &dyn StoreOfRecord<Event>) -> Vec<Hash> {
    store
        .open()
        .held()
        .expect("a fresh handle reads")
        .dag
        .objects()
        .keys()
        .cloned()
        .collect()
}

proptest! {
    #![proptest_config(crate::common::cases::cases(48))]

    /// Laws 45, 60, 62, 63 and 65, over every store of record: after every
    /// step of a script of writes by three handles (appends, runs of
    /// appends under one hold, receipts, quarantines, `record_heads`) and
    /// of doors (a print arrives, a print is taken away, the record is
    /// removed), every long-lived handle reads what a fresh handle reads,
    /// each append writes what a fresh handle's append to a copy writes,
    /// the record is the derived heads after a write and the older record
    /// after a door, the store serves the record while every tip it names
    /// is kept, `verify` reports a record that is not the derived heads,
    /// and a `Dag`'s heads are the derived heads.
    #[test]
    fn every_step_of_a_script_keeps_the_laws_of_a_store_of_record(
        pool in a_log(),
        theirs in a_log(),
        steps in prop::collection::vec(a_step(), 1..24),
    ) {
        prop_assume!(!pool.is_empty());
        let scratch = Scratch::new();
        for store in stores_of_record::<Event>(&scratch, &Default::default()) {
            let script = Script::new(&*store, pool.clone(), theirs.clone());
            for step in &steps {
                script.run(step)?;
            }
        }
    }
}
