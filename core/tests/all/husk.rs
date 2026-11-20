//! SPEC §3's husks and laws 71 to 76, over the letter schema
//! (`../schemas/letter.rs`), whose bodies hold parts sealed under named
//! keys.
//!
//! A history of letters is appended to a store on disk as one run. A set of
//! its objects is husked in a copy of the store. The laws compare the two
//! stores, and a replica that pulls from the copy, as a holder with no key
//! reads them: each sealed part read as the name of its key.

use crate::common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use prodrome::dag::{Finding, Unread};
use prodrome::event::{canonical_envelope, seal_hash, Actor, Envelope, Hash};
use prodrome::genesis::mk_genesis;
use prodrome::laws::Registers;
use prodrome::literal::{Datetime, ProdromeError};
use prodrome::policy::Untrusted;
use prodrome::reference::Todo;
use prodrome::store::{sync, EventStore, HuskObject, MemoryStore, Replica, Stopped};
use proptest::prelude::*;

use common::vectors::{each, text_at, vectors};

#[path = "../schemas/letter.rs"]
mod letter;

use letter::{Id, Letter, Part, Says};

type Store = EventStore<Letter>;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A directory for one case's stores, removed when the case ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "prodrome-husk-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        Scratch(root)
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// A store begun with a genesis whose nonce is fixed, so that every
    /// store of a case is one prodrome.
    fn store(&self, name: &str) -> Store {
        let store = Store::new(self.at(name), Untrusted::none());
        let genesis: Envelope<Letter> =
            Envelope::Genesis(mk_genesis("letters", &"0".repeat(32)).expect("a genesis"));
        let digest = seal_hash(&genesis);
        store
            .adopt_objects(
                &[(digest.clone(), canonical_envelope(&genesis))].into(),
                &digest,
            )
            .expect("begins");
        store
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One drawn event: a letter written in parts (each clear, or boxed under
/// one of three keys), or titled.
#[derive(Debug, Clone)]
enum Draft {
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

fn a_history() -> impl Strategy<Value = Vec<Draft>> {
    prop::collection::vec(a_draft(), 1..12)
}

/// Which objects of a history a case husks: a bit for each, by name order.
fn a_choice() -> impl Strategy<Value = Vec<bool>> {
    prop::collection::vec(any::<bool>(), 16)
}

/// The events of `drafts`, a minute apart, all by `ada`.
fn events(drafts: &[Draft]) -> Vec<Letter> {
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

/// Every object file of `from` copied into `to`'s `objects/`.
fn copy_objects(from: &Path, to: &Path) {
    let objects = to.join("objects");
    fs::create_dir_all(&objects).expect("creates objects/");
    for entry in fs::read_dir(from.join("objects")).expect("lists") {
        let path = entry.expect("an entry").path();
        fs::copy(&path, objects.join(path.file_name().expect("a file"))).expect("copies");
    }
}

/// The files of `objects/`, by name, with their bytes.
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root.join("objects"))
        .expect("lists")
        .map(|entry| {
            let path = entry.expect("an entry").path();
            let name = path
                .file_name()
                .expect("a file")
                .to_string_lossy()
                .into_owned();
            (name, fs::read(&path).expect("reads"))
        })
        .collect()
}

/// What a holder with no key reads of a replica: its registers by object
/// name, and each object's event with every sealed part read as its key.
#[derive(Debug, PartialEq)]
struct Keyless {
    registers: Registers<Letter>,
    events: BTreeMap<Hash, Option<Letter>>,
    tips: BTreeSet<Hash>,
}

fn keyless(replica: &impl Replica<Letter>) -> Keyless {
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

/// The names of the changes of `store` whose events hold a box.
fn boxed(store: &Store) -> BTreeSet<Hash> {
    store
        .dag()
        .expect("reads")
        .objects()
        .iter()
        .filter(|(_, object)| object.event().is_some_and(Letter::boxed))
        .map(|(name, _)| name.clone())
        .collect()
}

/// Does any event that `replica` reads, or the fold that it keeps, hold a
/// box under one of `names`?
fn opens_any(replica: &impl Replica<Letter>, names: &BTreeSet<Hash>) -> bool {
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

/// The store `drafts` makes, appended as one run, and the names chosen
/// from its objects by `choice`.
fn made(
    scratch: &Scratch,
    name: &str,
    drafts: &[Draft],
    choice: &[bool],
) -> (Store, BTreeSet<Hash>) {
    let store = scratch.store(name);
    store.append_all(events(drafts)).expect("appends");
    let chosen = store
        .dag()
        .expect("reads")
        .objects()
        .keys()
        .zip(choice.iter().cycle())
        .filter(|(_, chosen)| **chosen)
        .map(|(name, _)| name.clone())
        .collect();
    (store, chosen)
}

/// A copy of `store`'s files, at `name`, opened afresh.
fn copied(scratch: &Scratch, name: &str, store: &Store) -> Store {
    copy_objects(store.root(), &scratch.at(name));
    Store::new(scratch.at(name), Untrusted::none())
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
        let (whole, chosen) = made(&scratch, "whole", &drafts, &choice);
        let husked = copied(&scratch, "husked", &whole);
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

    /// Law 72, the prune law: `husk` changes only the files of the named
    /// objects that hold a box, each to its husk's print; every other file
    /// is byte-identical, and the heads, the linear order and every
    /// object's parents do not change. A second `husk` of the same names
    /// changes nothing.
    #[test]
    fn a_husk_changes_only_the_named_files(drafts in a_history(), choice in a_choice()) {
        let scratch = Scratch::new();
        let (store, chosen) = made(&scratch, "store", &drafts, &choice);
        let dag = store.dag().expect("reads");
        let order = dag.linearise().expect("orders").to_vec();
        let heads = store.heads().expect("heads");
        let parents: BTreeMap<Hash, Vec<Hash>> = dag
            .objects()
            .iter()
            .map(|(name, object)| (name.clone(), prodrome::event::parents_of(object)))
            .collect();
        let before = files(store.root());
        let done: BTreeSet<Hash> = store.husk(chosen).expect("husks").into_iter().collect();
        let after = files(store.root());
        prop_assert_eq!(before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
        for (file, bytes) in &after {
            let name = Hash::new(file.trim_end_matches(".py")).expect("an object's name");
            if done.contains(&name) {
                let Envelope::Change(change) = dag.get(&name).expect("held") else {
                    return Err(TestCaseError::fail("a husk of no change"));
                };
                let husk = HuskObject::of(change).expect("a box to drop");
                prop_assert_eq!(bytes, &husk.print().into_bytes());
            } else {
                prop_assert_eq!(bytes, &before[file]);
            }
        }
        let dag = store.dag().expect("reads");
        prop_assert_eq!(dag.linearise().expect("orders"), &order[..]);
        prop_assert_eq!(store.heads().expect("heads"), heads.clone());
        prop_assert_eq!(Store::new(store.root(), Untrusted::none()).heads().expect("heads"), heads);
        for (name, object) in dag.objects() {
            prop_assert_eq!(&prodrome::event::parents_of(object), &parents[name]);
        }
        prop_assert!(store.husk(done).expect("husks").is_empty(), "a husk is passed over");
        prop_assert_eq!(files(store.root()), after);
        prop_assert_eq!(store.verify(), Vec::<Finding>::new());
    }

    /// Law 73, never take back: after `husk`, no sequence of the store's
    /// own operations makes a sealed part of a husked object readable from
    /// it: not an append of each whole event again (one by one and as a
    /// run), and not a sync, a receipt or an adoption from a replica that
    /// holds every whole. Each husked name still holds its husk's print,
    /// and its `print` answers nothing.
    #[test]
    fn a_dropped_part_is_never_taken_back(drafts in a_history(), choice in a_choice()) {
        let scratch = Scratch::new();
        let (whole, chosen) = made(&scratch, "whole", &drafts, &choice);
        let husked = copied(&scratch, "husked", &whole);
        let done: BTreeSet<Hash> = husked.husk(chosen).expect("husks").into_iter().collect();
        let kept: BTreeMap<Hash, Vec<u8>> = done
            .iter()
            .map(|name| (name.clone(), husked.kept(name).expect("kept")))
            .collect();
        for event in events(&drafts) {
            husked.append(event).expect("appends");
        }
        husked.append_all(events(&drafts)).expect("appends");
        sync(&whole, &husked).expect("syncs");
        husked
            .receive(done.clone(), &|name| whole.kept(name))
            .expect("receives");
        for name in &done {
            husked.adopt(&whole, name).expect("adopts");
            prop_assert_eq!(husked.print(name), None);
        }
        prop_assert!(!opens_any(&husked, &done));
        prop_assert!(!opens_any(&Store::new(husked.root(), Untrusted::none()), &done));
        for (name, print) in &kept {
            let now = husked.kept(name);
            prop_assert_eq!(now.as_ref(), Some(print));
        }
    }

    /// Law 74, two stores agree: a replica that pulls from a store holding
    /// husks, on disk or in memory, holds the husks and reads as the store
    /// does, and `verify` is clean on both stores.
    #[test]
    fn a_replica_of_a_husked_store_reads_as_it(drafts in a_history(), choice in a_choice()) {
        let scratch = Scratch::new();
        let (store, chosen) = made(&scratch, "store", &drafts, &choice);
        let done: BTreeSet<Hash> = store.husk(chosen).expect("husks").into_iter().collect();
        let disk = Store::new(scratch.at("disk"), Untrusted::none());
        sync(&store, &disk).expect("syncs");
        let memory = MemoryStore::<Letter>::default();
        sync(&store, &memory).expect("syncs");
        prop_assert_eq!(keyless(&disk), keyless(&store));
        prop_assert_eq!(keyless(&memory), keyless(&store));
        let on_disk = disk.dag().expect("reads");
        prop_assert_eq!(on_disk.husks(), &done);
        let in_memory = memory.held().expect("reads");
        prop_assert_eq!(in_memory.dag.husks(), &done);
        prop_assert_eq!(store.verify(), Vec::<Finding>::new());
        prop_assert_eq!(disk.verify(), Vec::<Finding>::new());
        prop_assert!(sync(&store, &disk).expect("syncs").is_empty());
    }

    /// Law 76: a run of appends writes the objects that one append after
    /// another writes, and answers the same names.
    #[test]
    fn a_run_writes_what_appends_one_by_one_write(drafts in a_history()) {
        let scratch = Scratch::new();
        let run = scratch.store("run");
        let one = scratch.store("one");
        let names = run.append_all(events(&drafts)).expect("appends");
        let each: Vec<Hash> = events(&drafts)
            .into_iter()
            .map(|event| one.append(event).expect("appends"))
            .collect();
        prop_assert_eq!(names, each);
        prop_assert_eq!(files(run.root()), files(one.root()));
        prop_assert_eq!(run.heads().expect("heads"), one.heads().expect("heads"));
    }
}

/// Law 76: a run stops at the first event refused, and places the events
/// decided before it. Here the refusal is a store with no genesis, which
/// refuses the first event and places nothing.
#[test]
fn a_run_places_what_it_decided_before_a_refusal() {
    let scratch = Scratch::new();
    let empty = Store::new(scratch.at("empty"), Untrusted::none());
    let drafts = [Draft::Titled {
        letter: 0,
        title: 0,
    }];
    match empty.append_all(events(&drafts)) {
        Err(Stopped { placed, why }) => {
            assert!(placed.is_empty());
            assert!(why.to_string().contains("no genesis"), "{why}");
        }
        Ok(names) => panic!("a store with no genesis appended {names:?}"),
    }
    let store = scratch.store("store");
    let drafts = [
        Draft::Titled {
            letter: 0,
            title: 0,
        },
        Draft::Titled {
            letter: 1,
            title: 1,
        },
    ];
    let names = store.append_all(events(&drafts)).expect("appends");
    assert_eq!(names.len(), 2);
    assert_eq!(store.verify(), Vec::<Finding>::new());
}

/// What a stop leaves: the file under the name, and the temp, if one.
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

/// Law 75: a stop at each step of the replacement of one file leaves under
/// the name a file that reads as the whole or as the husk, never one that
/// reads as neither. The steps are the husk written to a temp file (in
/// full, or cut short before its sync), the rename over the whole, and the
/// directory sync; a stop before the rename reaches the disk is the whole.
#[test]
fn a_stop_at_each_step_leaves_the_whole_or_the_husk() {
    let scratch = Scratch::new();
    let drafts = [Draft::Wrote {
        letter: 0,
        parts: vec![Some(0), None],
    }];
    let (store, _) = made(&scratch, "store", &drafts, &[]);
    let name = boxed(&store).into_iter().next().expect("a boxed change");
    let Envelope::Change(change) = store.load(&name).expect("loads") else {
        panic!("a change");
    };
    let whole =
        fs::read(store.root().join(format!("objects/{}.py", name.as_str()))).expect("reads");
    let husk = HuskObject::of(&change)
        .expect("a box to drop")
        .print()
        .into_bytes();
    let temp = "00000000000000aa.tmp";
    // Each state a stop can leave: before the temp is written; the temp cut
    // short before its sync; the temp synced, before the rename reaches the
    // disk; and after the rename, which the directory sync makes durable.
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
        let root = scratch.at(&format!("stop-{at}"));
        copy_objects(store.root(), &root);
        fs::write(root.join(format!("objects/{}.py", name.as_str())), file).expect("writes");
        if let Some(staged) = staged {
            fs::write(root.join("objects").join(temp), staged).expect("stages");
        }
        let reader = Store::new(&root, Untrusted::none());
        let dag = reader.dag().expect("reads");
        assert!(dag.get(&name).is_some(), "step {at}: the name reads");
        assert!(dag.unread().is_empty(), "step {at}: {:?}", dag.unread());
        let read = if dag.husks().contains(&name) {
            Reads::Husk
        } else {
            Reads::Whole
        };
        assert_eq!(read, reads, "step {at}");
        let expected: Vec<String> = staged
            .map(|_| format!("objects/{temp} is not an object: a temp an interrupted write left, or a stray (SPEC §3); delete it"))
            .into_iter()
            .collect();
        let findings: Vec<String> = reader.verify().iter().map(ToString::to_string).collect();
        assert_eq!(findings, expected, "step {at}");
    }
    // The husking itself, from the whole: the name reads as the husk.
    assert_eq!(
        store.husk([name.clone()]).expect("husks"),
        vec![name.clone()]
    );
    assert_eq!(store.kept(&name), Some(husk));
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
    let (store, _) = made(&scratch, "store", &drafts, &[]);
    let dag = store.dag().expect("reads");
    for (name, object) in dag.objects() {
        let Envelope::Change(change) = object else {
            continue;
        };
        let path = store.root().join(format!("objects/{}.py", name.as_str()));
        let whole = fs::read_to_string(&path).expect("reads");
        // A box kept beside a dropped one, or a title with nothing dropped.
        let other = if change.event.boxed() {
            whole.replacen("Boxed(key='k0', box='0000')", "Dropped(key='k0')", 1)
        } else {
            whole.replace("title 0", "title 9")
        };
        assert_ne!(other, whole);
        fs::write(&path, &other).expect("writes");
        let reader = Store::new(store.root(), Untrusted::none());
        let read = reader.dag().expect("reads");
        assert!(
            matches!(read.unread().get(name), Some(Unread::Tampered(_))),
            "{other}"
        );
        assert!(reader.append(events(&drafts)[1].clone()).is_err());
        fs::write(&path, &whole).expect("restores");
    }
}

/// SPEC §3: a schema without husks reads a husk as tampered, and a replica
/// at it refuses one, as it refuses any print that fails its hash.
#[test]
fn a_schema_without_husks_reads_a_husk_as_tampered() {
    let print = "Change(genesis='aa', deps=(), event=None)";
    let name = Hash::new("ab".repeat(32)).expect("a name");
    let read = prodrome::dag::decode::<prodrome::event::TodoEvent<Todo>>(
        &Default::default(),
        &name,
        print.as_bytes(),
    );
    assert!(matches!(read, Err(Unread::Tampered(_))));
    let refused: Result<Vec<Hash>, ProdromeError> =
        MemoryStore::<prodrome::event::TodoEvent<Todo>>::default()
            .receive([name.clone()].into(), &|_| Some(print.as_bytes().to_vec()));
    assert!(refused.is_err());
}

/// The vectors of `conformance/husk.py`: how the letter schema, which has
/// husks, and the todo schema, which has none, read each print under its
/// name.
#[test]
fn the_husk_vectors_read_as_frozen() {
    let data = vectors("husk.py");
    let cases = each(&data, "cases");
    assert!(!cases.is_empty());
    for case in cases {
        let name = Hash::new(text_at(case, "name")).expect("a name");
        let print = text_at(case, "print");
        let letter =
            match prodrome::dag::decode::<Letter>(&Default::default(), &name, print.as_bytes()) {
                Ok(prodrome::store::Stored::Whole(_)) => "whole",
                Ok(prodrome::store::Stored::Husk(husk)) => {
                    assert_eq!(husk.print(), print, "a husk reads back as its print");
                    "husk"
                }
                Err(Unread::Tampered(_)) => "tampered",
                Err(Unread::NotText | Unread::Unparsed(_) | Unread::Io(_)) => "unparsed",
            };
        assert_eq!(letter, text_at(case, "letter"), "{print}");
        let todo = match prodrome::dag::decode::<prodrome::event::TodoEvent<Todo>>(
            &Default::default(),
            &name,
            print.as_bytes(),
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
    let (store, _) = made(&scratch, "store", &drafts, &[]);
    let name = boxed(&store).into_iter().next().expect("a boxed change");
    assert_eq!(
        store.husk([name.clone()]).expect("husks"),
        vec![name.clone()]
    );
    let Envelope::Change(change) = store.load(&name).expect("loads") else {
        panic!("a change");
    };
    let parent = change.deps.first().expect("a dep").clone();
    let root = scratch.at("partial");
    copy_objects(store.root(), &root);
    let parent_file = root.join(format!("objects/{}.py", parent.as_str()));
    let parent_print = fs::read(&parent_file).expect("reads");
    fs::remove_file(&parent_file).expect("removes");
    let partial = Store::new(&root, Untrusted::none());
    let dag = partial.dag().expect("reads");
    assert!(dag.get(&name).is_none(), "the husk waits");
    assert!(dag.husks().contains(&name), "and is held as a husk");
    let findings: Vec<String> = partial.verify().iter().map(ToString::to_string).collect();
    assert_eq!(
        findings,
        vec![format!("chain broke at {parent}: missing object {parent}")]
    );
    fs::write(&parent_file, parent_print).expect("restores");
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
    let (store, _) = made(&scratch, "store", &drafts, &[]);
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
