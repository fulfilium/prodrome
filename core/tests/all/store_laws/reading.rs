//! What a store of record reads of the prints it keeps: SPEC law 40 (a
//! store reads the largest down-set it holds, and quarantine is that
//! reading) and law 45 (the store of record reads totally, as a replica
//! does).
//!
//! Each law lays out a set of prints in a store through the doors of
//! [`StoreOfRecord`], reads it by a fresh handle and by one that held other
//! prints first, and compares the reading with that of a replica handed the
//! same prints. The interior operator of law 40, which reads no store, is
//! `down_set.rs::the_interior_is_the_largest_down_set`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::sync::OnceLock;

use prodrome::dag::{Dag, Excluded, Finding, Unread};
use prodrome::declared::{Declaration, Declared};
use prodrome::event::{canonical_envelope, parents_of, Actor, Hash, TodoEvent};
use prodrome::reference::Todo;
use prodrome::registers::{fold, Node};
use prodrome::schema::Schema;
use prodrome::store::{accept, Sender};
use proptest::prelude::*;
use sha2::{Digest, Sha256};

use super::kind::{begin, copy_prints, stores_of_record, Scratch, StoreOfRecord};
use crate::common::draw::{a_draw, history, Draw};
use crate::common::{a_log, two_writers};
use crate::declared::proposal::{a_proposal, Id, Proposal, Says, DECLARED};
use crate::declared::proposals;
use crate::down_set::{changes, chosen, legacy, names, without};

type Event = TodoEvent<Todo>;

/// `store` brought to keep exactly `prints`, through the doors: each name
/// it keeps and `prints` lacks taken away, each name whose bytes differ
/// replaced, and each new name placed.
fn lay_out<E: Schema>(store: &dyn StoreOfRecord<E>, prints: &BTreeMap<Hash, Vec<u8>>) {
    let kept = store.prints();
    for name in kept.keys().filter(|name| !prints.contains_key(*name)) {
        store.take_away(name);
    }
    for (name, bytes) in prints {
        match kept.get(name) {
            Some(held) if held == bytes => {}
            Some(_) => store.overwrite(name, bytes),
            None => store.arrive(name, bytes),
        }
    }
}

/// Law 40 at one history and one store: its prints less `removed`, read by
/// a fresh handle and by one that read every print first, read as a store
/// given only the interior.
fn reads_its_interior(
    store: &dyn StoreOfRecord<Event>,
    whole: &Dag<Event>,
    prints: &BTreeMap<Hash, Vec<u8>>,
    removed: &BTreeSet<Hash>,
) -> Result<(), TestCaseError> {
    let interior = without(whole, removed).interior();
    let warm = store.sibling();
    lay_out(&*warm, prints);
    let _ = warm.held().expect("the whole store reads");
    let gappy = store.sibling();
    lay_out(&*gappy, prints);
    for name in removed {
        warm.take_away(name);
        gappy.take_away(name);
    }
    let only = store.sibling();
    for name in interior.objects().keys() {
        only.arrive(name, &prints[name]);
    }
    let folded = fold(&interior.nodes().expect("an interior reads"));
    for at in [&warm, &gappy, &only] {
        let held = at.held().expect("reads");
        prop_assert_eq!(names(&held.dag), names(&interior));
        prop_assert_eq!(&*held.folded, &folded);
        prop_assert_eq!(held.tips, interior.tips());
    }
    // And the stores that keep the same prints hold what waits outside it.
    for at in [&warm, &gappy] {
        prop_assert_eq!(&*at.held().expect("reads").dag, &interior);
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// Law 40, the store, over every store of record: a store missing any
    /// objects of a history (two writers' changes, or a legacy chain), read
    /// cold or by a handle that held them all, reads its objects, its fold
    /// and its tips as a store given only the interior.
    #[test]
    fn a_store_reads_its_largest_down_set(
        logs in two_writers(),
        log in a_log(),
        mask in any::<u64>(),
    ) {
        let built = crate::memory::Scratch::new("down-set");
        let scratch = Scratch::new();
        for history in [changes(&built, &logs), legacy(&built, &log)] {
            let whole = history.dag().expect("reads");
            let prints = history.prints();
            let removed = chosen(&whole, mask);
            for store in stores_of_record::<Event>(&scratch, &Default::default()) {
                reads_its_interior(&*store, &whole, &prints, &removed)?;
            }
        }
    }
}

/// The quarantine law at one history, one store, and one object of the
/// history.
fn quarantine_reads_without_it(
    store: &dyn StoreOfRecord<Event>,
    intact: &dyn StoreOfRecord<Event>,
    pick: prop::sample::Index,
) -> Result<(), TestCaseError> {
    let whole = intact.held().expect("a history reads").dag;
    copy_prints(intact, store);
    let _ = store.held().expect("the whole store reads");
    let names: Vec<&Hash> = whole.objects().keys().collect();
    let damaged = (*pick.get(&names)).clone();
    let resting: BTreeSet<Hash> = whole
        .objects()
        .keys()
        .filter(|name| whole.closure([(*name).clone()]).contains(&damaged))
        .cloned()
        .collect();
    let expected = without(&whole, &resting);
    let waiting: BTreeSet<Hash> = resting
        .iter()
        .filter(|name| **name != damaged)
        .cloned()
        .collect();
    let mut bytes = intact.kept(&damaged).expect("kept");
    bytes.push(b' ');
    store.overwrite(&damaged, &bytes);
    let read = store
        .held()
        .expect("a read leaves out what fails its hash")
        .dag;
    prop_assert_eq!(read.objects(), expected.objects());
    let lacked: Vec<(Hash, BTreeSet<Hash>)> = read
        .excluded()
        .into_iter()
        .map(|out| (out.name, out.resting))
        .collect();
    prop_assert_eq!(lacked, vec![(damaged.clone(), waiting.clone())]);

    let receipt = store.receipt(&damaged, &bytes);
    let report = store.fsck();
    prop_assert!(
        report.contains(&Finding::Quarantined(receipt.clone())),
        "{:?}",
        report
    );
    prop_assert!(
        !report
            .iter()
            .any(|finding| matches!(finding, Finding::Unread { .. })),
        "nothing failing its hash is left among the objects: {:?}",
        report
    );
    let aside = store.aside();
    prop_assert_eq!(aside.get(&receipt), Some(&bytes));
    let folded = fold(&expected.nodes().expect("a down-set reads"));
    let fresh = store.open();
    for at in [store, &*fresh] {
        let held = at.held().expect("reads");
        prop_assert_eq!(held.dag.objects(), expected.objects());
        let lacked: Vec<Excluded> = (!waiting.is_empty())
            .then(|| Excluded {
                name: damaged.clone(),
                why: None,
                resting: waiting.clone(),
            })
            .into_iter()
            .collect();
        prop_assert_eq!(
            held.dag.excluded(),
            lacked,
            "set aside, it is a parent no print is"
        );
        prop_assert_eq!(&*held.folded, &folded);
        prop_assert_eq!(held.tips, expected.tips());
    }

    // Damaged again under the same name: set aside beside the first, which
    // is never overwritten, under a receipt of its own.
    let again = b"not an object".to_vec();
    let second = store.receipt(&damaged, &again);
    prop_assert_ne!(&second, &receipt);
    store.arrive(&damaged, &again);
    let report = store.fsck();
    let aside = store.aside();
    prop_assert_eq!(aside.len(), 2);
    prop_assert_eq!(aside.get(&receipt), Some(&bytes));
    prop_assert_eq!(aside.get(&second), Some(&again));
    prop_assert!(report.contains(&Finding::Quarantined(second)));
    let held = store.held().expect("reads");
    prop_assert_eq!(held.dag.objects(), expected.objects());

    // The same bytes set aside again are the same receipt: nothing new.
    store.arrive(&damaged, &again);
    let _ = store.fsck();
    prop_assert_eq!(store.aside().len(), 2);

    // The repair is a sync from a replica that has the object: the walk
    // passes through what waited for it, and the whole history is back.
    accept(
        intact,
        store,
        intact.tips().expect("derives"),
        Sender::Stranger,
    )
    .expect("adopts");
    prop_assert_eq!(&*store.held().expect("reads").dag, &*whole);
    prop_assert_eq!(store.kept(&damaged), intact.kept(&damaged));
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// Law 40, quarantine, over every store of record: over two writers'
    /// changes and over a legacy chain, an object damaged by the door is
    /// set aside by `fsck`, named in its report and kept byte for byte, and
    /// the store, through the handle that read it whole and a fresh one,
    /// reads its objects, its fold and its tips as the history without it
    /// and everything resting on it. Damaged again, the second print is set
    /// aside beside the first; a sync from a replica that has the object
    /// brings the whole history back.
    #[test]
    fn quarantine_is_the_down_set_without_what_rests_on_it(
        logs in two_writers(),
        log in a_log(),
        pick in any::<prop::sample::Index>(),
    ) {
        prop_assume!(!log.is_empty());
        let built = crate::memory::Scratch::new("quarantine");
        let scratch = Scratch::new();
        for history in [changes(&built, &logs), legacy(&built, &log)] {
            for store in stores_of_record::<Event>(&scratch, &Default::default()) {
                quarantine_reads_without_it(&*store, &history, pick)?;
            }
        }
    }
}

/// The proposal schema after a migration: `Tagged` says why, a field no
/// `Tagged` written before has.
fn migrated() -> &'static Declaration {
    static ADMITTED: OnceLock<Declaration> = OnceLock::new();
    ADMITTED.get_or_init(|| {
        let text = DECLARED.replacen(
            "Field(name='tags', type=List(item=Text())))",
            "Field(name='tags', type=List(item=Text())), Field(name='why', type=Text()))",
            1,
        );
        assert_ne!(text, DECLARED, "the migration changes the schema");
        Declaration::admit(&text).expect("the migrated schema is lawful")
    })
}

/// What becomes of a print chosen to be damaged.
#[derive(Debug, Clone, Copy)]
enum Damage {
    /// Its bytes changed under its name: it fails its hash.
    Tampered,
    /// Taken away: what rests on it names a parent no print is.
    Gone,
}

fn a_damage() -> impl Strategy<Value = Damage> {
    prop_oneof![Just(Damage::Tampered), Just(Damage::Gone)]
}

/// The name `bytes` would have, computed apart from the crate.
fn name_of(bytes: &[u8]) -> Hash {
    let hex = Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    Hash::new(hex).expect("a sha256 is a name")
}

/// A draw's prints, written at the proposal schema as written.
fn prints(draw: &Draw<Proposal>) -> BTreeMap<Hash, Vec<u8>> {
    let events: Vec<(Declared, u64)> = draw
        .events
        .iter()
        .map(|(event, bits)| {
            let event = Declared::from_value(proposals(), &event.to_value())
                .expect("a proposal is a declared one");
            (event, *bits)
        })
        .collect();
    history(&events)
        .into_iter()
        .map(|(name, object)| (name, canonical_envelope(&object).into_bytes()))
        .collect()
}

/// Law 45 at one set of prints: every read of a store keeping them, by
/// `warm` (which held other prints first) and by a fresh handle, is the
/// reading of a replica handed them, and a write refuses exactly when that
/// replica holds a print that is no object, with that print's refusal.
fn reads_as_its_replica(
    schema: &Declaration,
    warm: &dyn StoreOfRecord<Declared>,
    prints: &BTreeMap<Hash, Vec<u8>>,
) -> Result<(), TestCaseError> {
    let replica = Dag::<Declared>::from_prints(
        schema,
        prints
            .iter()
            .map(|(name, bytes)| (name.clone(), Ok(bytes.clone()))),
        Sender::Stranger,
    )
    .interior();
    let nodes = replica.nodes().expect("a history reads");
    let folded = fold(&nodes);
    let events: Vec<Declared> = nodes.into_iter().filter_map(Node::into_event).collect();
    let fresh = warm.open();
    for store in [warm, &*fresh] {
        let held = store.held().expect("a read is total");
        prop_assert_eq!(
            &*held.dag,
            &replica,
            "the store holds what the replica holds"
        );
        prop_assert_eq!(
            held.dag.excluded(),
            replica.excluded(),
            "and leaves out the same"
        );
        prop_assert_eq!(&*held.folded, &folded);
        prop_assert_eq!(held.tips, replica.tips());
        prop_assert_eq!(store.events().expect("reads"), events.clone());
        for tip in replica.tips() {
            prop_assert_eq!(
                store.ancestors(&tip).expect("a head has ancestors"),
                replica.closure(replica.get(&tip).map(parents_of).unwrap_or_default())
            );
        }
    }
    // A write: refused, with the first unread print's own refusal, exactly
    // when something is unread. With nothing unread it is written over the
    // history's heads, and the store and a replica handed its prints, the
    // written one among them, still hold the same.
    let proposal = Proposal {
        proposal: Id("p-written".to_owned()),
        at: crate::common::moment(0),
        actor: Actor::new("ana").expect("an actor"),
        says: Says::Proposed("written".to_owned()),
    };
    let event = Declared::from_value(schema, &proposal.to_value()).expect("a proposal");
    let written = fresh.append(event);
    if let Some(first) = replica.unread().keys().next() {
        let refusal = written.expect_err("a write does not guess");
        let named = replica
            .whole()
            .map(|_| ())
            .expect_err("something is unread");
        prop_assert_eq!(&refusal, &named);
        prop_assert!(refusal.to_string().contains(first.as_str()), "{}", refusal);
    } else if replica.geneses().is_empty() {
        // The genesis itself is gone: there is no prodrome to write into,
        // a refusal of its own, and nothing unread is its cause.
        prop_assert!(
            written.is_err_and(|refusal| refusal.to_string().contains("no genesis")),
            "a store with no genesis refuses as one"
        );
    } else {
        let name = written.expect("nothing unread: a write is written");
        let mut after = prints.clone();
        after.insert(name.clone(), fresh.kept(&name).expect("the write is kept"));
        let replica = Dag::<Declared>::from_prints(
            schema,
            after
                .iter()
                .map(|(name, bytes)| (name.clone(), Ok(bytes.clone()))),
            Sender::Stranger,
        )
        .interior();
        prop_assert!(replica.get(&name).is_some(), "the write is in the history");
        for store in [warm, &*fresh] {
            let held = store.held().expect("reads");
            prop_assert_eq!(&*held.dag, &replica, "and they still agree");
            prop_assert_eq!(held.tips, replica.tips());
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 45, over every store of record: over generated histories read
    /// at the schema they were written at or at a newer one, with random
    /// prints damaged in place or taken away and prints that are no object
    /// added, a store, read cold and by a handle that held the intact
    /// prints, holds what a replica handed the same prints holds: its
    /// history, its fold, its heads, its events and ancestry, and what it
    /// leaves out and why. A write refuses, with the first unread print's
    /// refusal, exactly when one is held; otherwise it is written, and the
    /// store and the replica still agree.
    #[test]
    fn a_store_reads_as_a_replica_holding_its_prints(
        draw in a_draw(a_proposal()),
        newer in any::<bool>(),
        damage in prop::collection::vec((any::<prop::sample::Index>(), a_damage()), 0..4),
        junk in 0usize..3,
    ) {
        let schema = if newer { migrated() } else { proposals() };
        let intact = prints(&draw);
        let mut prints = intact.clone();
        let names: Vec<Hash> = prints.keys().cloned().collect();
        for (pick, how) in &damage {
            let name = pick.get(&names);
            match how {
                Damage::Tampered => {
                    if let Some(bytes) = prints.get_mut(name) {
                        bytes.push(b' ');
                    }
                }
                Damage::Gone => {
                    prints.remove(name);
                }
            }
        }
        for n in 0..junk {
            let bytes = if n % 2 == 0 {
                vec![0xff, 0xfe, u8::try_from(n).expect("small")]
            } else {
                format!("Nonsense(n={n})").into_bytes()
            };
            prints.insert(name_of(&bytes), bytes);
        }
        let scratch = Scratch::new();
        for warm in stores_of_record::<Declared>(&scratch, schema) {
            lay_out(&*warm, &intact);
            let _ = warm.held().expect("reads the intact prints");
            lay_out(&*warm, &prints);
            reads_as_its_replica(schema, &*warm, &prints)?;
        }
    }
}

/// Law 45 at one store: a handle that has read every object leaves one out
/// once its print no longer holds its bytes, and says so, as a fresh handle
/// does, and refuses a write; it reads as before once the bytes are back.
fn a_changed_print_is_read_again(store: &dyn StoreOfRecord<Event>) {
    begin(store, "changed");
    let event = crate::common::realise(
        &[(
            crate::common::Draft {
                todo: "alpha",
                actor: "ada",
                roll: 1,
                spec: None,
                items: 0,
                text: 7,
            },
            0,
        )],
        0,
    );
    let name = store.append(event[0].clone()).expect("appends");
    let read = |store: &dyn StoreOfRecord<Event>| {
        let held = store.held().expect("reads");
        ((*held.dag).clone(), (*held.folded).clone(), held.tips)
    };
    let before = read(store);
    let bytes = store.kept(&name).expect("kept");
    let mut damaged = bytes.clone();
    let last = damaged.len() - 2;
    damaged[last] ^= 1;
    store.overwrite(&name, &damaged);
    // The overwrite is a door: the handle reads it at the next hold of the
    // lock, here the write that refuses.
    assert!(store.append(event[0].clone()).is_err(), "a write refuses");
    let left_out = read(store);
    assert!(
        left_out.0.get(&name).is_none(),
        "the held handle reads the print again"
    );
    assert!(matches!(
        &left_out.0.excluded()[..],
        [Excluded { name: out, why: Some(Unread::Tampered(_)), .. }] if *out == name
    ));
    assert_eq!(left_out, read(&*store.open()));
    store.overwrite(&name, &bytes);
    // A refused hold leaves no count of holds, so the handle lists again.
    assert_eq!(read(store), before);
}

/// Law 45, a print changed under a held name, over every store of record.
#[test]
fn a_print_changed_under_a_held_name_is_read_again() {
    let scratch = Scratch::new();
    for store in stores_of_record::<Event>(&scratch, &Default::default()) {
        a_changed_print_is_read_again(&*store);
    }
}
