//! SPEC §3's husks and laws 71, 78 and 80, over the letter schema
//! (`../schemas/letter.rs`), whose bodies hold parts sealed under named
//! keys. Laws 72 to 77 and 79 run from the suite in `store_laws/`, over
//! every store of record and every replica where a law applies, with the
//! drafts and readings of this file.
//!
//! A history of letters is appended to a store on disk as one run. A set of
//! its objects is husked in a copy of the store. The laws compare the two
//! stores, and a replica that pulls from the copy, as a holder with no key
//! reads them: each sealed part read as the name of its key.

use crate::common;
use crate::store_laws::{copy_prints, directory, Scratch, StoreOfRecord};

use std::collections::{BTreeMap, BTreeSet};

use prodrome::dag::decode;
use prodrome::dag::{Finding, Unread};
use prodrome::event::{canonical_envelope, Actor, Envelope, Hash};
use prodrome::laws::Registers;
use prodrome::literal::{Datetime, ProdromeError};
use prodrome::policy::Untrusted;
use prodrome::reference::Todo;
use prodrome::sign::Secret;
use prodrome::store::{
    accept, sync, EventStore, HuskObject, MemoryStore, Overlay, Replica, Sender, Stored,
};
use proptest::prelude::*;

use common::vectors::{each, text_at, vectors};

#[path = "../schemas/letter.rs"]
pub(crate) mod letter;

use letter::{Id, Letter, Part, Says};

type Store = EventStore<Letter>;

/// A new store of files for letters, begun with the genesis that every
/// store of a case shares.
fn store(scratch: &Scratch) -> Store {
    let store = directory::<Letter>(scratch, &Default::default());
    crate::store_laws::begin(&store, "letters");
    store
}

/// One drawn event: a letter written in parts (each clear, or boxed under
/// one of three keys), or titled.
#[derive(Debug, Clone)]
pub(crate) enum Draft {
    Wrote { letter: u8, parts: Vec<Option<u8>> },
    Titled { letter: u8, title: u8 },
}

fn a_draft() -> impl Strategy<Value = Draft> {
    prop_oneof![
        3 => (0..3_u8, prop::collection::vec(prop::option::of(0..3_u8), 1..4))
            .prop_map(|(letter, parts)| Draft::Wrote { letter, parts }),
        1 => (0..3_u8, 0..4_u8).prop_map(|(letter, title)| Draft::Titled { letter, title }),
    ]
}

pub(crate) fn a_history() -> impl Strategy<Value = Vec<Draft>> {
    prop::collection::vec(a_draft(), 1..12)
}

/// Which objects of a history a case husks: a bit for each, by name order.
pub(crate) fn a_choice() -> impl Strategy<Value = Vec<bool>> {
    prop::collection::vec(any::<bool>(), 16)
}

/// The events of `drafts`, a minute apart, all by `ada`.
pub(crate) fn events(drafts: &[Draft]) -> Vec<Letter> {
    drafts
        .iter()
        .enumerate()
        .map(|(at, draft)| {
            let minute = u32::try_from(at).expect("a short history");
            let at = Datetime::new(2026, 10, 1, 9, minute, 0, 0).expect("an instant");
            let actor = Actor::new("ada").expect("an actor");
            match draft {
                Draft::Wrote { letter, parts } => Letter {
                    letter: Id(format!("l{letter}")),
                    at,
                    actor,
                    says: Says::Wrote(
                        parts
                            .iter()
                            .enumerate()
                            .map(|(at, part)| match part {
                                Some(key) => Part::Boxed {
                                    key: format!("k{key}"),
                                    sealed: format!("{minute:02x}{at:02x}"),
                                },
                                None => Part::Clear(format!("t{minute}.{at}")),
                            })
                            .collect(),
                    ),
                },
                Draft::Titled { letter, title } => Letter {
                    letter: Id(format!("l{letter}")),
                    at,
                    actor,
                    says: Says::Titled(format!("title {title}")),
                },
            }
        })
        .collect()
}

/// What a holder with no key reads of a replica: its registers by object
/// name, and each object's event with every sealed part read as its key.
#[derive(Debug, PartialEq)]
pub(crate) struct Keyless {
    registers: Registers<Letter>,
    events: BTreeMap<Hash, Option<Letter>>,
    tips: BTreeSet<Hash>,
}

pub(crate) fn keyless(replica: &(impl Replica<Letter> + ?Sized)) -> Keyless {
    let held = replica.held().expect("reads");
    Keyless {
        registers: Registers::of(&held.folded),
        events: held
            .dag
            .objects()
            .iter()
            .map(|(name, object)| (name.clone(), object.event().map(|e| e.read_by(&[]))))
            .collect(),
        tips: held.tips,
    }
}

/// The names of the changes of `replica` whose events hold a box.
pub(crate) fn boxed(replica: &(impl Replica<Letter> + ?Sized)) -> BTreeSet<Hash> {
    replica
        .held()
        .expect("reads")
        .dag
        .objects()
        .iter()
        .filter(|(_, object)| object.event().is_some_and(Letter::boxed))
        .map(|(name, _)| name.clone())
        .collect()
}

/// Does any event that `replica` reads, or the fold that it keeps, hold a
/// box under one of `names`?
pub(crate) fn opens_any(replica: &(impl Replica<Letter> + ?Sized), names: &BTreeSet<Hash>) -> bool {
    let held = replica.held().expect("reads");
    let in_objects = names
        .iter()
        .filter_map(|name| held.dag.get(name))
        .any(|object| object.event().is_some_and(Letter::boxed));
    let in_fold = held.folded.prodromes().values().any(|prodrome| {
        prodrome
            .values()
            .flatten()
            .any(|stamp| names.contains(&stamp.name) && stamp.event.boxed())
    });
    in_objects || in_fold
}

/// `store` begun, the history of `drafts` appended to it as one run, and
/// the names chosen from its objects by `choice`.
pub(crate) fn made(
    store: &dyn StoreOfRecord<Letter>,
    drafts: &[Draft],
    choice: &[bool],
) -> BTreeSet<Hash> {
    crate::store_laws::begin(store, "letters");
    store.append_all(events(drafts)).expect("appends");
    store
        .held()
        .expect("reads")
        .dag
        .objects()
        .keys()
        .zip(choice.iter().cycle())
        .filter(|(_, chosen)| **chosen)
        .map(|(name, _)| name.clone())
        .collect()
}

/// A new store of files with the history of `drafts`, and the names
/// chosen from its objects by `choice`.
fn written(scratch: &Scratch, drafts: &[Draft], choice: &[bool]) -> (Store, BTreeSet<Hash>) {
    let store = directory::<Letter>(scratch, &Default::default());
    let chosen = made(&store, drafts, choice);
    (store, chosen)
}

/// A copy of `store`'s prints in a new store of files.
fn copied(scratch: &Scratch, store: &Store) -> Store {
    let copy = directory::<Letter>(scratch, &Default::default());
    copy_prints(store, &copy);
    copy
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// Law 71, the husk law: the store with any set of its objects husked
    /// reads, for a holder with no key, as the store with them whole: the
    /// same registers by name, the same tips, and the same keyless event
    /// under every name. The handle that husked, which kept its fold and
    /// its DAG's facts, holds what a fresh handle reads.
    #[test]
    fn a_husked_store_reads_as_its_whole_for_a_keyless_holder(
        drafts in a_history(),
        choice in a_choice(),
    ) {
        let scratch = Scratch::new();
        let (whole, chosen) = written(&scratch, &drafts, &choice);
        let husked = copied(&scratch, &whole);
        let before = keyless(&husked);
        let done = husked.husk(chosen.clone()).expect("husks");
        prop_assert_eq!(
            done.iter().cloned().collect::<BTreeSet<_>>(),
            chosen.intersection(&boxed(&whole)).cloned().collect::<BTreeSet<_>>()
        );
        prop_assert_eq!(keyless(&husked), keyless(&whole));
        prop_assert_eq!(&before, &keyless(&whole));
        let fresh = Store::new(husked.root(), Untrusted::none());
        prop_assert_eq!(keyless(&fresh), keyless(&whole));
        let (warm, cold) = (husked.dag().expect("reads"), fresh.dag().expect("reads"));
        prop_assert_eq!(&*warm, &*cold);
        prop_assert_eq!(warm.linearise().expect("orders"), cold.linearise().expect("orders"));
        prop_assert_eq!(warm.tips(), cold.tips());
        prop_assert_eq!(&*husked.folded().expect("folds"), &*fresh.folded().expect("folds"));
        prop_assert_eq!(
            husked.dag().expect("reads").husks().iter().cloned().collect::<Vec<_>>(),
            done
        );
    }

    /// Law 80: no signature is made over a name whose whole the signer
    /// does not hold. `Secret::sign_whole` signs each object the history
    /// holds whole, and refuses each name held as a husk, in the history or
    /// waiting, and each name not held.
    #[test]
    fn no_signature_over_a_name_whose_whole_is_not_held(
        drafts in a_history(),
        choice in a_choice(),
    ) {
        let scratch = Scratch::new();
        let (store, chosen) = written(&scratch, &drafts, &choice);
        let done: BTreeSet<Hash> = store.husk(chosen).expect("husks").into_iter().collect();
        let ada = Secret::new(&"1".repeat(64)).expect("a seed");
        let dag = store.dag().expect("reads");
        for name in dag.objects().keys() {
            let signed = ada.sign_whole(&dag, name);
            prop_assert_eq!(signed.is_ok(), !done.contains(name), "{}", name);
        }
        let absent = Hash::new("ab".repeat(32)).expect("a name");
        prop_assert!(ada.sign_whole(&dag, &absent).is_err());
    }

    /// Law 78: a husk is exactly one print. A husk's print with a comment
    /// before it, a space after any comma, or two of its fields swapped
    /// still parses, and is tampered, even from the keeper of record.
    #[test]
    fn a_husk_is_exactly_one_print(drafts in a_history(), comma in 0..64_usize) {
        let scratch = Scratch::new();
        let (store, _) = written(&scratch, &drafts, &[]);
        for name in boxed(&store) {
            let Envelope::Change(change) = store.load(&name).expect("loads") else {
                return Err(TestCaseError::fail("a boxed change"));
            };
            let print = HuskObject::of(&change).expect("a box").print();
            prop_assert!(matches!(
                decode::<Letter>(&Default::default(), &name, print.as_bytes(), Sender::Keeper),
                Ok(Stored::Husk(_))
            ));
            let commas: Vec<usize> = print.match_indices(", ").map(|(at, _)| at + 1).collect();
            let at = commas[comma % commas.len()];
            let genesis = format!("genesis='{}'", change.genesis);
            let deps = &print[print.find("deps=").expect("deps")..print.find(", event=").expect("an event")];
            let changed = [
                format!("# Boxed(key='k0', box='00ff')\n{print}"),
                format!("{} {}", &print[..at], &print[at..]),
                print.replacen(&format!("{genesis}, {deps}"), &format!("{deps}, {genesis}"), 1),
            ];
            for other in changed {
                prop_assert_ne!(&other, &print);
                prop_assert!(
                    matches!(
                        decode::<Letter>(&Default::default(), &name, other.as_bytes(), Sender::Keeper),
                        Err(Unread::Tampered(_))
                    ),
                    "{}", other
                );
            }
        }
    }
}

/// Law 75's last clause, at the store of files: `husk` puts the husk under
/// the name by a rename, and never writes the file of the whole in place.
/// The stops between the steps of the replacement are law 75's tests in
/// `store.rs` and `store_laws/stops.rs`.
#[test]
fn a_husk_replaces_its_file_by_a_rename() {
    use std::os::unix::fs::MetadataExt;
    let scratch = Scratch::new();
    let store = directory::<Letter>(&scratch, &Default::default());
    let drafts = [Draft::Wrote {
        letter: 0,
        parts: vec![Some(0), None],
    }];
    made(&store, &drafts, &[]);
    let name = boxed(&store).into_iter().next().expect("a boxed change");
    let inode = || {
        std::fs::metadata(store.root().join(format!("objects/{}.py", name.as_str())))
            .expect("a file")
            .ino()
    };
    let before = inode();
    assert_eq!(
        store.husk([name.clone()]).expect("husks"),
        vec![name.clone()]
    );
    assert!(store.kept(&name).is_some());
    assert_eq!(store.print(&name), None, "the husk is not the whole");
    assert_ne!(inode(), before, "a rename, not a write in place");
}

/// SPEC §3: at a husking schema a print that still holds a box, or that
/// holds no dropped part, is no husk: it is tampered, `verify` says so, and
/// a write refuses until `fsck` sets it aside.
#[test]
fn a_print_that_is_no_husk_is_tampered() {
    let scratch = Scratch::new();
    let drafts = [
        Draft::Wrote {
            letter: 0,
            parts: vec![Some(0), Some(1)],
        },
        Draft::Titled {
            letter: 1,
            title: 0,
        },
    ];
    let (store, _) = written(&scratch, &drafts, &[]);
    let dag = store.dag().expect("reads");
    for (name, object) in dag.objects() {
        let Envelope::Change(change) = object else {
            continue;
        };
        let whole = String::from_utf8(store.kept(name).expect("kept")).expect("text");
        // A box kept beside a dropped one, or a title with nothing dropped.
        let other = if change.event.boxed() {
            whole.replacen("Boxed(key='k0', box='0000')", "Dropped(key='k0')", 1)
        } else {
            whole.replace("title 0", "title 9")
        };
        assert_ne!(other, whole);
        store.replace(name, other.as_bytes());
        let reader = Store::new(store.root(), Untrusted::none());
        let read = reader.dag().expect("reads");
        assert!(
            matches!(read.unread().get(name), Some(Unread::Tampered(_))),
            "{other}"
        );
        assert!(reader.append(events(&drafts)[1].clone()).is_err());
        store.replace(name, whole.as_bytes());
    }
}

/// SPEC §3: a schema without husks reads a husk as tampered, and a replica
/// at it refuses one, as it refuses any print that fails its hash.
#[test]
fn a_schema_without_husks_reads_a_husk_as_tampered() {
    let print = "Change(genesis='aa', deps=(), event=None)";
    let name = Hash::new("ab".repeat(32)).expect("a name");
    let read = decode::<prodrome::event::TodoEvent<Todo>>(
        &Default::default(),
        &name,
        print.as_bytes(),
        Sender::Keeper,
    );
    assert!(matches!(read, Err(Unread::Tampered(_))));
    let refused: Result<Vec<Hash>, ProdromeError> =
        MemoryStore::<prodrome::event::TodoEvent<Todo>>::default().receive(
            [name.clone()].into(),
            &|_| Some(print.as_bytes().to_vec()),
            Sender::Stranger,
        );
    assert!(refused.is_err());
}

/// The vectors of `conformance/husk.py`: how the letter schema, which has
/// husks, and the todo schema, which has none, read each print under its
/// name from the keeper of record. From a stranger, the letter schema reads
/// each print the vectors call a husk as tampered.
#[test]
fn the_husk_vectors_read_as_frozen() {
    let data = vectors("husk.py");
    let cases = each(&data, "cases");
    assert!(!cases.is_empty());
    for case in cases {
        let name = Hash::new(text_at(case, "name")).expect("a name");
        let print = text_at(case, "print");
        let letter =
            match decode::<Letter>(&Default::default(), &name, print.as_bytes(), Sender::Keeper) {
                Ok(Stored::Whole(_)) => "whole",
                Ok(Stored::Husk(husk)) => {
                    assert_eq!(husk.print(), print, "a husk reads back as its print");
                    "husk"
                }
                Err(Unread::Tampered(_)) => "tampered",
                Err(Unread::NotText | Unread::Unparsed(_) | Unread::Io(_)) => "unparsed",
            };
        assert_eq!(letter, text_at(case, "letter"), "{print}");
        let from_a_stranger = decode::<Letter>(
            &Default::default(),
            &name,
            print.as_bytes(),
            Sender::Stranger,
        );
        if letter == "husk" {
            assert!(
                matches!(from_a_stranger, Err(Unread::Tampered(_))),
                "{print}"
            );
        }
        let todo = match decode::<prodrome::event::TodoEvent<Todo>>(
            &Default::default(),
            &name,
            print.as_bytes(),
            Sender::Keeper,
        ) {
            Ok(_) => "read",
            Err(Unread::Tampered(_)) => "tampered",
            Err(Unread::NotText | Unread::Unparsed(_) | Unread::Io(_)) => "unparsed",
        };
        assert_eq!(todo, text_at(case, "todo"), "{print}");
    }
}

/// SPEC §3: a husk's parents are checked as a whole's. A store that holds a
/// husk and not the change it rests on holds it outside the history, and
/// `verify` reports the break; with the parent back, the husk joins.
#[test]
fn a_husk_whose_parent_is_absent_waits() {
    let scratch = Scratch::new();
    let drafts = [
        Draft::Wrote {
            letter: 0,
            parts: vec![None],
        },
        Draft::Wrote {
            letter: 0,
            parts: vec![Some(0)],
        },
    ];
    let (store, _) = written(&scratch, &drafts, &[]);
    let name = boxed(&store).into_iter().next().expect("a boxed change");
    assert_eq!(
        store.husk([name.clone()]).expect("husks"),
        vec![name.clone()]
    );
    let Envelope::Change(change) = store.load(&name).expect("loads") else {
        panic!("a change");
    };
    let parent = change.deps.first().expect("a dep").clone();
    let partial = copied(&scratch, &store);
    let parent_print = partial.kept(&parent).expect("kept");
    partial.set_aside(&parent);
    let dag = partial.dag().expect("reads");
    assert!(dag.get(&name).is_none(), "the husk waits");
    assert!(dag.husks().contains(&name), "and is held as a husk");
    let findings: Vec<String> = partial.verify().iter().map(ToString::to_string).collect();
    assert_eq!(
        findings,
        vec![format!("chain broke at {parent}: missing object {parent}")]
    );
    partial.place(&parent, &parent_print);
    assert!(
        partial.dag().expect("reads").get(&name).is_some(),
        "the husk joins"
    );
    assert_eq!(partial.verify(), Vec::<Finding>::new());
}

/// SPEC §3: a frontier's answer sends a husked object as its husk, the only
/// print the store keeps under its name.
#[test]
fn a_frontier_sends_a_husk() {
    use prodrome::store::frontier::{Frontiers, Since};
    let scratch = Scratch::new();
    let drafts = [Draft::Wrote {
        letter: 0,
        parts: vec![Some(1)],
    }];
    let (store, _) = written(&scratch, &drafts, &[]);
    let name = boxed(&store).into_iter().next().expect("a boxed change");
    store.husk([name.clone()]).expect("husks");
    let dag = store.dag().expect("reads");
    let frontiers = Frontiers::remembered(std::num::NonZeroUsize::new(4).expect("four"));
    let lacking = frontiers
        .answer("letters", &Since::Nothing, &dag)
        .expect("answers");
    let (_, object) = lacking
        .lacking()
        .iter()
        .find(|(sent, _)| **sent == name)
        .expect("the husk is lacking");
    assert_eq!(
        Some(canonical_envelope(object).into_bytes()),
        store.kept(&name)
    );
    assert_eq!(store.print(&name), None);
}

/// SPEC §3: an overlay answers the base's print only where the base holds
/// the name as an object. Where the base's file under a name is damaged and
/// the overlay's own layer holds the whole, the union reads the whole, the
/// overlay answers it, and a sync from the overlay takes it.
#[test]
fn an_overlay_answers_its_own_whole_over_a_damaged_base() {
    let scratch = Scratch::new();
    let drafts = [Draft::Wrote {
        letter: 0,
        parts: vec![None],
    }];
    let (whole, _) = written(&scratch, &drafts, &[]);
    let name = whole
        .tips()
        .expect("tips")
        .into_iter()
        .next()
        .expect("a tip");
    let base = copied(&scratch, &whole);
    base.replace(&name, b"damaged");
    let overlay = Overlay::new(&base);
    sync(&whole, &overlay, Sender::Stranger).expect("the overlay takes the whole");
    assert_eq!(overlay.kept(&name), whole.kept(&name));
    let fresh = MemoryStore::<Letter>::default();
    sync(&overlay, &fresh, Sender::Stranger).expect("syncs from the overlay");
    assert_eq!(fresh.print(&name), whole.print(&name));
}

/// A write of an object the store holds outside its history, waiting for a
/// parent, is no write where the bytes are the same: a merge over a waiting
/// parent, run twice, answers one name and writes one file.
#[test]
fn writing_a_waiting_object_again_writes_nothing() {
    let scratch = Scratch::new();
    let drafts = [
        Draft::Titled {
            letter: 0,
            title: 0,
        },
        Draft::Titled {
            letter: 1,
            title: 1,
        },
        Draft::Titled {
            letter: 1,
            title: 2,
        },
    ];
    let (whole, _) = written(&scratch, &drafts, &[]);
    let [first, middle, last] = drafts.map(|draft| {
        let event = events(&[draft])[0].clone();
        whole
            .dag()
            .expect("reads")
            .objects()
            .iter()
            .find(|(_, object)| object.event().is_some_and(|held| held.says == event.says))
            .map(|(name, _)| name.clone())
            .expect("appended")
    });
    let store = copied(&scratch, &whole);
    store.set_aside(&middle);
    assert!(store.dag().expect("reads").get(&last).is_none(), "it waits");
    let merged = store
        .merge(Some(&[first.clone(), last.clone()]), None)
        .expect("merges");
    let files_once = store.prints();
    let again = store
        .merge(Some(&[first, last]), None)
        .expect("a merge again writes nothing");
    assert_eq!(again, merged);
    assert_eq!(store.prints(), files_once);
}

/// SPEC §3: an overlay whose own layer holds a whole answers the base's husk
/// once the base takes it, with no read of the overlay between: an `accept`
/// from the overlay hands out the husk, never the whole.
#[test]
fn an_overlay_answers_the_base_s_husk_without_a_read_between() {
    let scratch = Scratch::new();
    let drafts = [Draft::Wrote {
        letter: 0,
        parts: vec![Some(0)],
    }];
    let (whole, _) = written(&scratch, &drafts, &[]);
    let name = boxed(&whole).into_iter().next().expect("a boxed change");
    let husked = copied(&scratch, &whole);
    husked.husk([name.clone()]).expect("husks");
    let base = store(&scratch);
    let overlay = Overlay::new(&base);
    sync(&whole, &overlay, Sender::Stranger).expect("the overlay takes the whole");
    let tips = husked.tips().expect("tips");
    accept(&husked, &base, tips.clone(), Sender::Keeper).expect("the base takes the husk");
    let fresh = MemoryStore::<Letter>::default();
    accept(&overlay, &fresh, tips, Sender::Keeper).expect("accepts from the overlay");
    assert_eq!(fresh.kept(&name), husked.kept(&name));
    assert_eq!(fresh.print(&name), None);
}
