//! One script of writes, by the store's handles and by other doors, and the
//! laws that hold after every step of it: SPEC law 45 (a handle that lived
//! through the steps reads as a fresh one, and its appends write what a
//! fresh handle's append writes), and laws 60, 62, 63 and 65 (the record of
//! heads).
//!
//! A step is a write of the store (an append, a run of appends under one
//! hold, a receipt, a quarantine, a `record_heads`) by one of three handles
//! of one store, or a door (a print arrives, a print is taken away, the
//! record is removed), or a read by one handle. After a step the script
//! reads only the handle that made it, so a long-lived handle makes its
//! next write or read from a memory that is several writes and doors
//! behind. Each write of the store holds the lock, so after it the record
//! of heads is the derived heads. A door leaves the record as it was, so
//! after it the store serves the older record, or the derived heads where
//! the record names a tip that the store no longer keeps.

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
    /// One print that a third replica keeps and the store lacks, put in
    /// the store, as a copy of files or a `git merge` does. The third
    /// replica is never synced, so its prints arrive one at a time and in
    /// any order, a child before its parent.
    Arrive(Index),
    /// One print taken away, as a host deletes a rejected proposal. The
    /// genesis stays.
    TakeAway(Index),
    /// The record of heads removed.
    DropRecord,
}

#[derive(Debug, Clone)]
enum Step {
    Write(Handle, Write),
    Door(Door),
    Read(Handle),
}

/// A step, with the weights of the scripts that this one replaces: an
/// append, an arrival, a removal and a read in the ratio 4 : 4 : 2 : 3, and
/// each other write and door as often as one in eighteen steps.
fn a_step() -> impl Strategy<Value = Step> {
    let write = |write: BoxedStrategy<Write>| {
        (a_handle(), write).prop_map(|(by, write)| Step::Write(by, write))
    };
    prop_oneof![
        4 => write(any::<Index>().prop_map(Write::Append).boxed()),
        1 => write(prop::collection::vec(any::<Index>(), 1..4).prop_map(Write::Run).boxed()),
        1 => write(any::<Index>().prop_map(Write::Receive).boxed()),
        1 => write(any::<Index>().prop_map(Write::Quarantine).boxed()),
        1 => write(Just(Write::RecordHeads).boxed()),
        4 => any::<Index>().prop_map(|pick| Step::Door(Door::Arrive(pick))),
        2 => any::<Index>().prop_map(|pick| Step::Door(Door::TakeAway(pick))),
        1 => Just(Step::Door(Door::DropRecord)),
        3 => a_handle().prop_map(Step::Read),
    ]
}

/// What a handle reads: its history, its fold, its tips and what it leaves
/// out, or its refusal.
#[derive(Debug, Clone, PartialEq)]
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

/// One store, its two long-lived handles, a store elsewhere of the same
/// prodrome that the store receives from, and a third replica whose prints
/// arrive by a door, which a script of [`Step`]s writes to.
struct Script<'s> {
    first: &'s dyn StoreOfRecord<Event>,
    second: Box<dyn StoreOfRecord<Event>>,
    elsewhere: Box<dyn StoreOfRecord<Event>>,
    third: Box<dyn StoreOfRecord<Event>>,
    genesis: Hash,
    pool: Vec<Event>,
    theirs: Vec<Event>,
    /// How many holds of the lock the script has made. Each one writes a
    /// fresh mark of the last hold.
    holds: u64,
    /// For the first and the second handle, the number of holds when the
    /// handle last listed the prints, and what it read then. A handle reads
    /// that until a hold of the lock passes, and lists at its first read
    /// after it.
    level: [Option<(u64, Result<Reading, String>)>; 2],
}

/// Where `heads()` takes the heads from.
enum Served {
    /// The record, with no listing of the prints.
    Record,
    /// The prints, listed into the handle's memory.
    Listing,
}

impl<'s> Script<'s> {
    /// The store begun; the store elsewhere, which shares the genesis; and
    /// the third replica, which shares the genesis and holds every second
    /// event of both pools, so that what arrives from it is concurrent with
    /// the store's writes, conflicts with them and twins of them.
    fn new(
        store: &'s dyn StoreOfRecord<Event>,
        pool: Vec<Event>,
        theirs: Vec<Event>,
    ) -> Script<'s> {
        let genesis = super::kind::begin(store, "script");
        let elsewhere = store.sibling();
        sync(store, &*elsewhere, Sender::Stranger).expect("syncs");
        let third = store.sibling();
        sync(store, &*third, Sender::Stranger).expect("syncs");
        for event in pool.iter().chain(&theirs).step_by(2) {
            let _ = third.append(event.clone());
        }
        let first = Some((0, reading(store)));
        Script {
            first: store,
            second: store.open(),
            elsewhere,
            third,
            genesis,
            pool,
            theirs,
            holds: 0,
            level: [first, None],
        }
    }

    /// Run `step`, then check every law after it. A refused write changes
    /// nothing, and the laws say what the store then reads.
    fn run(&mut self, step: &Step) -> Result<(), TestCaseError> {
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
        if let Step::Write(..) = step {
            // Every write holds the lock, and every hold writes a fresh
            // mark.
            self.holds += 1;
        }
        let recorded = match step {
            Step::Write(..) => Some(derived(self.first)),
            Step::Door(Door::DropRecord) => None,
            Step::Door(Door::Arrive(_) | Door::TakeAway(_)) | Step::Read(_) => before,
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
                    self.first.overwrite(pick.get(&names), b"damaged");
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
                    .third
                    .prints()
                    .into_iter()
                    .filter(|(name, _)| !kept.contains_key(name))
                    .collect();
                if !lacking.is_empty() {
                    let (name, bytes) = pick.get(&lacking);
                    self.first.arrive(name, bytes);
                }
            }
            Door::TakeAway(pick) => {
                let kept: Vec<Hash> = self
                    .first
                    .prints()
                    .into_keys()
                    .filter(|name| *name != self.genesis)
                    .collect();
                if !kept.is_empty() {
                    self.first.take_away(pick.get(&kept));
                }
            }
            Door::DropRecord => self.first.set_record(None),
        }
    }

    /// The laws after `step`, where `recorded` is the record that the step
    /// leaves.
    fn holds(&mut self, step: &Step, recorded: Option<Heads>) -> Result<(), TestCaseError> {
        let fresh = self.first.open();
        let cold = reading(&*fresh);
        // Law 45: incremental equals cold, for the handle that made the
        // step, as of the last hold of the lock: a handle that reads with no
        // hold since its last listing reads what it read then, and a door's
        // change since is read at the next hold. The other long-lived handle
        // is not read, so it lags.
        let checked: Vec<usize> = match step {
            Step::Write(Handle::First, _) | Step::Read(Handle::First) => vec![0],
            Step::Write(Handle::Second, _) | Step::Read(Handle::Second) => vec![1],
            Step::Write(Handle::Fresh, _) | Step::Read(Handle::Fresh) | Step::Door(_) => vec![],
        };
        let handles: [&dyn StoreOfRecord<Event>; 2] = [self.first, &*self.second];
        for &at in &checked {
            let expected = match (step, &self.level[at]) {
                (Step::Read(_), Some((holds, read))) if *holds == self.holds => read.clone(),
                (Step::Read(_) | Step::Write(..) | Step::Door(_), _) => cold.clone(),
            };
            prop_assert_eq!(&reading(handles[at]), &expected, "after {:?}", step);
            self.level[at] = Some((self.holds, expected));
        }
        let by: Vec<&dyn StoreOfRecord<Event>> = checked.iter().map(|&at| handles[at]).collect();
        // Law 60: after a write of the store, the record is the derived
        // heads; after a door, it is the record before the door.
        let derived = derived(self.first);
        prop_assert_eq!(self.first.record(), recorded.clone(), "after {:?}", step);
        // Laws 62 and 63: the store serves the record while every tip it
        // names is kept, and otherwise the derived heads.
        let kept = self.first.prints();
        let (served, from) = match &recorded {
            Some(record) if record.tips().iter().all(|tip| kept.contains_key(tip)) => {
                (record.clone(), Served::Record)
            }
            Some(_) | None => (derived.clone(), Served::Listing),
        };
        for handle in by.iter().copied().chain([&*fresh]) {
            prop_assert_eq!(
                handle.heads().expect("heads"),
                served.clone(),
                "after {:?}",
                step
            );
        }
        // Heads that are not the record's are listed into the handle's
        // memory, so the handle is level with the prints from here.
        match from {
            Served::Record => {}
            Served::Listing => {
                for &at in &checked {
                    self.level[at] = Some((self.holds, cold.clone()));
                }
            }
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
        let held = fresh.held().expect("reads");
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
    /// appends under one hold, receipts, quarantines, `record_heads`), of
    /// doors (a print of a third replica arrives, a print is taken away, the
    /// record is removed) and of reads, the handle that made the step reads
    /// what a fresh handle reads, from a memory that was behind; each append
    /// writes what a fresh handle's append to a copy writes; the record is
    /// the derived heads after a write and the older record after a door;
    /// the store serves the record while every tip it names is kept;
    /// `verify` reports a record that is not the derived heads; and a
    /// `Dag`'s heads are the derived heads.
    #[test]
    fn every_step_of_a_script_keeps_the_laws_of_a_store_of_record(
        pool in a_log(),
        theirs in a_log(),
        steps in prop::collection::vec(a_step(), 1..40),
    ) {
        prop_assume!(!pool.is_empty());
        let scratch = Scratch::new();
        for store in stores_of_record::<Event>(&scratch, &Default::default()) {
            let mut script = Script::new(&*store, pool.clone(), theirs.clone());
            for step in &steps {
                script.run(step)?;
            }
        }
    }
}

/// An index into any list: the fixed scripts below draw from lists whose
/// members are all alike for what each one checks.
fn any_index() -> Index {
    use proptest::strategy::ValueTree;
    any::<Index>()
        .new_tree(&mut proptest::test_runner::TestRunner::deterministic())
        .expect("an index")
        .current()
}

/// Events about `count` different todos, each its entity's first write, so
/// that each change rests on the genesis alone.
fn apart(count: usize) -> Vec<Event> {
    const TODOS: [&str; 6] = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"];
    let drafts: Vec<(common::Draft, i64)> = TODOS
        .iter()
        .take(count)
        .zip(0..)
        .map(|(todo, at)| {
            (
                common::Draft {
                    todo,
                    actor: "ada",
                    roll: 1,
                    spec: None,
                    items: 0,
                    text: 7,
                },
                at,
            )
        })
        .collect();
    common::realise(&drafts, 0)
}

/// Law 62, over every store of record, on fixed steps of the script: a
/// print of the third replica arrives, and the store serves the older
/// record, which is not the derived heads; then a hold of the lock, a
/// `record_heads` or a removal of the record ends the lag, and the store
/// serves the derived heads.
#[test]
fn another_door_s_objects_read_as_the_older_heads() -> Result<(), TestCaseError> {
    let scratch = Scratch::new();
    for store in stores_of_record::<Event>(&scratch, &Default::default()) {
        let mut script = Script::new(&*store, apart(6), Vec::new());
        let ends = [
            Step::Write(Handle::First, Write::Run(Vec::new())),
            Step::Write(Handle::Second, Write::RecordHeads),
            Step::Door(Door::DropRecord),
        ];
        for end in &ends {
            let older = store.record().expect("recorded");
            script.run(&Step::Door(Door::Arrive(any_index())))?;
            prop_assert_eq!(
                store.heads().expect("reads"),
                older.clone(),
                "the record lags"
            );
            prop_assert_ne!(older, derived(&*store), "the arrival changed the heads");
            script.run(end)?;
            prop_assert_eq!(
                store.heads().expect("reads"),
                derived(&*store),
                "after {:?}",
                end
            );
        }
    }
    Ok(())
}

/// Law 63, over every store of record, on fixed steps of the script: the
/// one change of the store is taken away while the record names it as a
/// tip; the store serves the derived heads, and `verify` reports the record
/// and the derived heads.
#[test]
fn a_record_naming_an_absent_head_is_not_served() -> Result<(), TestCaseError> {
    let scratch = Scratch::new();
    for store in stores_of_record::<Event>(&scratch, &Default::default()) {
        let mut script = Script::new(&*store, apart(1), Vec::new());
        script.run(&Step::Write(Handle::First, Write::Append(any_index())))?;
        let recorded = store.record().expect("recorded");
        let tip = recorded.tips().iter().next().expect("a tip").clone();
        prop_assert_ne!(&tip, &script.genesis, "the tip is the change");
        script.run(&Step::Door(Door::TakeAway(any_index())))?;
        prop_assert!(!store.prints().contains_key(&tip), "the tip is taken away");
        prop_assert_eq!(
            store.record(),
            Some(recorded.clone()),
            "the record names it"
        );
        let derived = derived(&*store);
        prop_assert_ne!(&derived, &recorded);
        prop_assert_eq!(store.heads().expect("derives"), derived.clone());
        let stale = Finding::StaleHeads { recorded, derived };
        prop_assert!(store.verify().contains(&stale));
    }
    Ok(())
}
