//! A write of a store of record stopped after each of its steps, or failed
//! at one: SPEC law 61 (a stop of a post leaves no record but the derived
//! one), law 75 (a stop at each step of a replacement leaves the whole or
//! the husk), law 79, with the second clause of law 61 (a failed
//! placement leaves the memory as a cold read, and no record), and law 82
//! (the next hold removes what a stopped write left).
//!
//! Each stop is stated through the doors of [`StoreOfRecord`]: the steps
//! that a write made before the stop are the doors it opened, and
//! [`StoreOfRecord::interrupt`] is what it leaves of the step that it did
//! not finish.

use std::collections::BTreeSet;

use prodrome::dag::Finding;
use prodrome::event::{mk_reopened, Hash, TodoEvent};
use prodrome::literal::Datetime;
use prodrome::reference::Todo;
use prodrome::store::{sync, HuskObject, Sender, Stopped};
use proptest::prelude::*;

use super::kind::{begin, copy_prints, derived, stores_of_record, Scratch, StoreOfRecord};
use crate::husk::letter::Letter;
use crate::husk::{boxed, events, Draft};

type Event = TodoEvent<Todo>;

fn reopened(entity: usize, day: usize) -> Event {
    let day = u32::try_from(day % 28).expect("a day of the month");
    let at = Datetime::new(2026, 9, 1 + day, 12, 0, 0, 0).expect("a real instant");
    mk_reopened(&format!("todo-{entity}"), at, "ada", "").expect("a valid event")
}

/// Law 61 at one store: `before` objects written, then a post of `new`
/// objects stopped after each of its steps. The steps are the record
/// removed, each object placed, parents first (each one staged before it
/// takes its name), and the record written, which a power loss can lose or
/// cut at any byte.
fn a_stop_serves_the_derived_heads(
    writer: &dyn StoreOfRecord<Event>,
    before: usize,
    new: usize,
    order: Order,
) -> Result<(), TestCaseError> {
    begin(writer, "heads");
    for n in 0..before {
        writer.append(reopened(n, n)).expect("appends");
    }
    let source = writer.sibling();
    sync(writer, &*source, Sender::Stranger).expect("syncs");
    let placed: Vec<(Hash, Vec<u8>)> = (0..new)
        .map(|n| {
            let name = source.append(reopened(n + 10, n + 20)).expect("appends");
            let print = source.kept(&name).expect("kept");
            (name, print)
        })
        .collect();
    prop_assert_eq!(
        writer.heads().expect("reads"),
        derived(writer),
        "the old record"
    );
    let record = writer.record().expect("recorded");
    writer.set_record(None);
    prop_assert_eq!(writer.open().heads().expect("derives"), derived(writer));
    match order {
        Order::EachThenItsRename => {}
        Order::AllStagedFirst => {
            for (_, print) in &placed {
                let _ = writer.interrupt(print);
                prop_assert_eq!(writer.open().heads().expect("derives"), derived(writer));
            }
        }
    }
    for (name, print) in &placed {
        match order {
            Order::EachThenItsRename => {
                let _ = writer.interrupt(&print[..print.len() / 2]);
                prop_assert_eq!(writer.open().heads().expect("derives"), derived(writer));
            }
            Order::AllStagedFirst => {}
        }
        writer.arrive(name, print);
        prop_assert_eq!(writer.open().heads().expect("derives"), derived(writer));
    }
    let heads = derived(writer);
    let new_record = heads.print();
    prop_assert_ne!(&new_record, &record.print());
    for cut in 0..new_record.len() {
        writer.set_record(Some(&new_record.as_bytes()[..cut]));
        prop_assert_eq!(writer.record(), None, "a cut record is not served");
        prop_assert_eq!(writer.open().heads().expect("derives"), heads.clone());
    }
    writer.set_record(Some(new_record.as_bytes()));
    prop_assert_eq!(writer.record(), Some(heads));
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(48))]

    /// Law 61, over every store of record: every state that a stop of the
    /// store's own post can leave serves no record but the derived one. A
    /// killed process leaves the old record with the old objects, or no
    /// record with any prefix of the new objects placed parents first and
    /// what the next one left when it stopped. A power loss can also lose
    /// the new record, or cut it at any byte, after every new object is
    /// placed.
    #[test]
    fn a_served_record_is_the_derived_one(
        before in 0usize..4,
        new in 1usize..4,
        order in prop_oneof![Just(Order::EachThenItsRename), Just(Order::AllStagedFirst)],
    ) {
        let scratch = Scratch::new();
        for store in stores_of_record::<Event>(&scratch, &Default::default()) {
            a_stop_serves_the_derived_heads(&*store, before, new, order)?;
        }
    }
}

/// Law 79, with the second clause of law 61, at one store: a run of two
/// appends whose second placement fails places the first, and the hold's
/// memory then reads as a fresh handle reads. The same append again places
/// nothing it did not place, and a later write of the same hold succeeds.
/// The hold records no heads, because the failed placement's objects can be
/// kept and not in memory. Once the fault is gone, the run completes as in
/// a store where it never failed, and each failed step left only what a
/// stop leaves.
fn a_failed_placement_reads_as_cold(clean: &dyn StoreOfRecord<Letter>) {
    let drafts = [0, 1, 2].map(|n| Draft::Titled {
        letter: n,
        title: n,
    });
    let events = events(&drafts);
    begin(clean, "letters");
    let names = clean.append_all(events[..2].to_vec()).expect("appends");
    let store = clean.sibling();
    begin(&*store, "letters");
    let later = {
        let mut hold = store.hold().expect("holds");
        store.obstruct(&names[1]);
        let stopped = hold.append_all(events[..2].to_vec());
        assert!(matches!(stopped, Err(Stopped { ref placed, .. }) if placed.is_empty()));
        assert_eq!(store.record(), None, "removed before the placement");
        let cold = store.open();
        assert_eq!(*hold.dag(), *cold.held().expect("reads").dag);
        assert_eq!(hold.append(events[0].clone()).expect("a twin"), names[0]);
        assert!(hold.append(events[1].clone()).is_err());
        let later = hold
            .append(events[2].clone())
            .expect("a later write succeeds");
        assert_eq!(*hold.dag(), *cold.held().expect("reads").dag);
        later
    };
    assert_eq!(store.record(), None, "the hold records nothing");
    store.clear(&names[1]);
    assert_eq!(store.heads().expect("derives"), derived(&*store));
    assert_eq!(
        store.append_all(events[..2].to_vec()).expect("appends"),
        names
    );
    let mut kept = store.prints();
    kept.remove(&later);
    assert_eq!(kept, clean.prints());
    assert!(store
        .verify()
        .iter()
        .all(|finding| store.left_by_a_stop(finding)));
}

/// Law 79 and the second clause of law 61, over every store of record.
#[test]
fn a_failed_placement_leaves_the_memory_as_a_cold_read() {
    let scratch = Scratch::new();
    for store in stores_of_record::<Letter>(&scratch, &Default::default()) {
        a_failed_placement_reads_as_cold(&*store);
    }
}

/// The order of the steps of a post in law 61's model: each print staged
/// and then renamed, or every print staged before the first rename, as one
/// sync of a batch stages them.
#[derive(Debug, Clone, Copy)]
enum Order {
    EachThenItsRename,
    AllStagedFirst,
}

/// What a stop of a replacement leaves under the name, and beside it.
struct Stop<'b> {
    file: &'b [u8],
    staged: Option<&'b [u8]>,
    reads: Reads,
}

/// What a name reads as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reads {
    Whole,
    Husk,
}

/// Law 75 at one store: each state that a stop of the replacement of one
/// print by its husk can leave reads under the name as the whole or as the
/// husk, never as neither. The steps are the husk staged (in full, or cut
/// short), and the husk under the name; a stop before the second step is
/// the whole, and leaves what [`StoreOfRecord::interrupt`] leaves.
fn a_stop_leaves_the_whole_or_the_husk(store: &dyn StoreOfRecord<Letter>) {
    begin(store, "letters");
    let drafts = [Draft::Wrote {
        letter: 0,
        parts: vec![Some(0), None],
    }];
    store.append_all(events(&drafts)).expect("appends");
    let name = boxed(store).into_iter().next().expect("a boxed change");
    let whole = store.kept(&name).expect("kept");
    let held = store.held().expect("reads");
    let Some(prodrome::event::Envelope::Change(change)) = held.dag.get(&name) else {
        panic!("a change");
    };
    let husk = HuskObject::of(change)
        .expect("a box to drop")
        .print()
        .into_bytes();
    let states = [
        Stop {
            file: &whole,
            staged: None,
            reads: Reads::Whole,
        },
        Stop {
            file: &whole,
            staged: Some(&husk[..husk.len() / 2]),
            reads: Reads::Whole,
        },
        Stop {
            file: &whole,
            staged: Some(&husk),
            reads: Reads::Whole,
        },
        Stop {
            file: &husk,
            staged: None,
            reads: Reads::Husk,
        },
    ];
    for (
        at,
        Stop {
            file,
            staged,
            reads,
        },
    ) in states.into_iter().enumerate()
    {
        let stopped = store.sibling();
        copy_prints(store, &*stopped);
        stopped.overwrite(&name, file);
        let expected: Vec<Finding> = staged
            .map(|staged| stopped.interrupt(staged))
            .into_iter()
            .collect();
        let reader = stopped.open();
        let dag = reader.held().expect("reads").dag;
        assert!(dag.get(&name).is_some(), "step {at}: the name reads");
        assert!(dag.unread().is_empty(), "step {at}: {:?}", dag.unread());
        let read = if dag.husks().contains(&name) {
            Reads::Husk
        } else {
            Reads::Whole
        };
        assert_eq!(read, reads, "step {at}");
        assert_eq!(reader.verify(), expected, "step {at}");
    }
    assert_eq!(
        store.husk(BTreeSet::from([name.clone()])).expect("husks"),
        vec![name.clone()]
    );
    assert_eq!(store.kept(&name), Some(husk));
}

/// Law 75, over every store of record.
#[test]
fn a_stop_at_each_step_leaves_the_whole_or_the_husk() {
    let scratch = Scratch::new();
    for store in stores_of_record::<Letter>(&scratch, &Default::default()) {
        a_stop_leaves_the_whole_or_the_husk(&*store);
    }
}

/// Law 82 at one store: what a stopped write left is reported by `verify`
/// until the next hold of the lock, and the hold removes it and changes no
/// print.
fn a_hold_removes_what_a_stop_left(store: &dyn StoreOfRecord<Event>) {
    begin(store, "sweep");
    store.append(reopened(1, 1)).expect("appends");
    let stray = store.stray();
    let holds: [(&str, &dyn Fn()); 3] = [
        ("a decision", &|| drop(store.hold().expect("holds"))),
        ("record_heads", &|| {
            drop(store.record_heads().expect("records"))
        }),
        ("fsck", &|| drop(store.fsck())),
    ];
    for (hold, run) in holds {
        let left = [
            store.interrupt(b"a print cut sh"),
            store.interrupt(b"a whole print, staged"),
        ];
        let found = store.verify();
        assert!(
            left.iter().all(|finding| found.contains(finding)),
            "{found:?}"
        );
        let prints = store.prints();
        run();
        assert_eq!(
            store.verify(),
            vec![stray.clone()],
            "after {hold}: the stray stays"
        );
        assert_eq!(store.prints(), prints, "after {hold}");
    }
}

/// Law 82, over every store of record.
#[test]
fn a_hold_removes_what_a_stopped_write_left() {
    let scratch = Scratch::new();
    for store in stores_of_record::<Event>(&scratch, &Default::default()) {
        a_hold_removes_what_a_stop_left(&*store);
    }
}
