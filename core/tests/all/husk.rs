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

use prodrome::dag::decode;
use prodrome::dag::{Finding, Unread};
use prodrome::event::{canonical_envelope, seal_hash, Actor, Envelope, Hash};
use prodrome::genesis::mk_genesis;
use prodrome::laws::Registers;
use prodrome::literal::{Datetime, ProdromeError};
use prodrome::policy::Untrusted;
use prodrome::reference::Todo;
use prodrome::sign::{KeyAdded, Proof, Registrar, Secret};
use prodrome::store::{
    accept_from, sync, EventStore, HuskObject, MemoryStore, Overlay, Replica, Sender, Stopped,
    Stored,
};
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

    /// Law 73, never take back: after `husk`, no sequence of operations of
    /// any replica the law kit has makes a sealed part of a husked object
    /// readable from it. The store on disk appends each whole event again,
    /// one by one and as a run, and syncs, receives and adopts every whole
    /// from a replica that holds them, as a stranger and as its keeper. A
    /// store in memory that took the husks from the store does the same. An
    /// overlay whose own layer holds the wholes, over a base that then takes
    /// the husks, reads and answers the husks, and its flush keeps them.
    /// Each husked name keeps its husk's print, and its `print` answers
    /// nothing.
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
        let everything = whole.tips().expect("tips");

        for event in events(&drafts) {
            husked.append(event).expect("appends");
        }
        husked.append_all(events(&drafts)).expect("appends");
        sync(&whole, &husked).expect("syncs");
        accept_from(&whole, &husked, everything.clone(), Sender::Keeper).expect("accepts");
        husked
            .receive_from(done.clone(), &|name| whole.kept(name), Sender::Keeper)
            .expect("receives");
        for name in &done {
            husked.adopt_from(&whole, name, Sender::Keeper).expect("adopts");
        }
        never_taken_back(&husked, &kept)?;
        never_taken_back(&Store::new(husked.root(), Untrusted::none()), &kept)?;

        let memory = MemoryStore::<Letter>::default();
        accept_from(&husked, &memory, husked.tips().expect("tips"), Sender::Keeper)
            .expect("accepts");
        for event in events(&drafts) {
            memory.append(event).expect("appends");
        }
        sync(&whole, &memory).expect("syncs");
        accept_from(&whole, &memory, everything.clone(), Sender::Keeper).expect("accepts");
        never_taken_back(&memory, &kept)?;

        let base = scratch.store("base");
        let overlay = Overlay::new(&base);
        accept_from(&whole, &overlay, everything, Sender::Keeper).expect("takes the wholes");
        accept_from(&husked, &base, husked.tips().expect("tips"), Sender::Keeper)
            .expect("the base takes the husks");
        never_taken_back(&overlay, &kept)?;
        overlay.flush().expect("flushes");
        never_taken_back(&base, &kept)?;
    }

    /// Law 74, two stores agree: a replica that pulls from a store holding
    /// husks, as from its keeper of record, on disk or in memory, holds the
    /// husks and reads as the store does, and `verify` is clean on both
    /// stores. A pull from the same store as from a stranger refuses every
    /// husk and takes nothing.
    #[test]
    fn a_replica_of_a_husked_store_reads_as_it(drafts in a_history(), choice in a_choice()) {
        let scratch = Scratch::new();
        let (store, chosen) = made(&scratch, "store", &drafts, &choice);
        let done: BTreeSet<Hash> = store.husk(chosen).expect("husks").into_iter().collect();
        let tips = store.tips().expect("tips");
        let disk = Store::new(scratch.at("disk"), Untrusted::none());
        accept_from(&store, &disk, tips.clone(), Sender::Keeper).expect("pulls");
        let memory = MemoryStore::<Letter>::default();
        accept_from(&store, &memory, tips.clone(), Sender::Keeper).expect("pulls");
        prop_assert_eq!(keyless(&disk), keyless(&store));
        prop_assert_eq!(keyless(&memory), keyless(&store));
        let on_disk = disk.dag().expect("reads");
        prop_assert_eq!(on_disk.husks(), &done);
        let in_memory = memory.held().expect("reads");
        prop_assert_eq!(in_memory.dag.husks(), &done);
        prop_assert_eq!(store.verify(), Vec::<Finding>::new());
        prop_assert_eq!(disk.verify(), Vec::<Finding>::new());
        prop_assert!(accept_from(&store, &disk, tips, Sender::Keeper).expect("pulls").is_empty());
        let stranger = MemoryStore::<Letter>::default();
        prop_assert_eq!(sync(&store, &stranger).is_err(), !done.is_empty());
        if !done.is_empty() {
            prop_assert!(stranger.held().expect("reads").dag.objects().is_empty());
        }
    }

    /// Law 76: a run of appends writes the objects that one append after
    /// another writes, and answers the same names. A run with a refused
    /// event at any place stops there: the events before it are placed, as
    /// one append after another places them, and none after it is.
    #[test]
    fn a_run_writes_what_appends_one_by_one_write(drafts in a_history(), at in 0..13_usize) {
        let scratch = Scratch::new();
        let run = scratch.store("run");
        let one = scratch.store("one");
        let names = run.append_all(events(&drafts)).expect("appends");
        let each: Vec<Hash> = events(&drafts)
            .into_iter()
            .map(|event| one.append(event).expect("appends"))
            .collect();
        prop_assert_eq!(names, each.clone());
        prop_assert_eq!(files(run.root()), files(one.root()));
        prop_assert_eq!(run.heads().expect("heads"), one.heads().expect("heads"));

        let at = at.min(drafts.len());
        let mut refused = events(&drafts);
        let mut refusal = refused[0].clone();
        refusal.says = Says::Titled(letter::REFUSED.to_owned());
        refused.insert(at, refusal);
        let stopped = scratch.store("stopped");
        let before = scratch.store("before");
        for event in events(&drafts).into_iter().take(at) {
            before.append(event).expect("appends");
        }
        match stopped.append_all(refused) {
            Err(Stopped { placed, why }) => {
                prop_assert_eq!(placed, each[..at].to_vec());
                prop_assert!(why.to_string().contains("refused"), "{}", why);
            }
            Ok(names) => {
                return Err(TestCaseError::fail(format!("a refused run answered {names:?}")));
            }
        }
        prop_assert_eq!(files(stopped.root()), files(before.root()));
        prop_assert_eq!(stopped.heads().expect("heads"), before.heads().expect("heads"));
        prop_assert_eq!(stopped.verify(), Vec::<Finding>::new());
    }

    /// Law 77: for any sequence of receipts from a party whose word is not
    /// taken, no object reads as proven, or vouched for, whose bytes the
    /// receiver did not hash to its name. The party holds a true signature
    /// of every change, by the key registered to its actor, and forged
    /// husks under some of their names. From the keeper of record the same
    /// husks are taken, and a signature vouches for each and proves none.
    #[test]
    fn a_signature_proves_no_husk(drafts in a_history(), choice in a_choice()) {
        let scratch = Scratch::new();
        let (store, chosen) = made(&scratch, "store", &drafts, &choice);
        let ada = Secret::new(&"1".repeat(64)).expect("a seed");
        let dag = store.dag().expect("reads");
        let genesis = dag.geneses().into_iter().next().expect("a genesis");
        let mut prints: BTreeMap<Hash, Vec<u8>> = dag
            .objects()
            .keys()
            .map(|name| (name.clone(), store.kept(name).expect("kept")))
            .collect();
        let added: Envelope<Letter> = Envelope::KeyAdded(KeyAdded {
            genesis,
            actor: Actor::new("ada").expect("an actor"),
            key: ada.public(),
        });
        prints.insert(seal_hash(&added), canonical_envelope(&added).into_bytes());
        let mut forged = BTreeSet::new();
        for (name, object) in dag.objects() {
            let Some(event) = object.event() else { continue };
            let signed: Envelope<Letter> = Envelope::Signed(ada.sign(name));
            prints.insert(seal_hash(&signed), canonical_envelope(&signed).into_bytes());
            let Envelope::Change(change) = object else { continue };
            if !chosen.contains(name) || !event.boxed() {
                continue;
            }
            let husk = HuskObject::of(change).expect("a box").print();
            let lie = husk.replacen(&format!("letter='{}'", event.letter.0), "letter='forged'", 1);
            prints.insert(name.clone(), lie.into_bytes());
            forged.insert(name.clone());
        }
        let stranger = MemoryStore::<Letter>::default();
        for name in prints.keys() {
            let _ = stranger.receive([name.clone()].into(), &|name| prints.get(name).cloned());
        }
        let held = stranger.held().expect("reads");
        let proof = Proof::of(&held.dag, &Registrar::Anyone);
        for name in held.dag.objects().keys() {
            if proof.proves(name) || proof.vouches(name) {
                prop_assert!(stranger.print(name).is_some(), "{} stands unhashed", name);
            }
            prop_assert!(!forged.contains(name), "a stranger's husk was taken");
        }

        let keeper = MemoryStore::<Letter>::default();
        let tips: BTreeSet<Hash> = prints.keys().cloned().collect();
        keeper
            .receive_from(tips, &|name| prints.get(name).cloned(), Sender::Keeper)
            .expect("takes the keeper's word");
        let held = keeper.held().expect("reads");
        let proof = Proof::of(&held.dag, &Registrar::Anyone);
        for name in &forged {
            prop_assert!(held.dag.husks().contains(name));
            prop_assert!(!proof.proves(name), "a signature proves a husk");
            prop_assert!(proof.vouches(name), "a signature vouches for the keeper's husk");
        }
    }

    /// Law 78: a husk is exactly one print. A husk's print with a comment
    /// before it, a space after any comma, or two of its fields swapped
    /// still parses, and is tampered, even from the keeper of record.
    #[test]
    fn a_husk_is_exactly_one_print(drafts in a_history(), comma in 0..64_usize) {
        let scratch = Scratch::new();
        let (store, _) = made(&scratch, "store", &drafts, &[]);
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

/// The checks of law 73 at one replica: no event of its DAG or its fold
/// under a husked name holds a box, each husked name keeps its husk's
/// print, and its `print` answers nothing.
fn never_taken_back(
    replica: &impl Replica<Letter>,
    kept: &BTreeMap<Hash, Vec<u8>>,
) -> Result<(), TestCaseError> {
    let names: BTreeSet<Hash> = kept.keys().cloned().collect();
    prop_assert!(!opens_any(replica, &names));
    for (name, print) in kept {
        let now = replica.kept(name);
        prop_assert_eq!(now.as_ref(), Some(print));
        prop_assert_eq!(replica.print(name), None);
    }
    Ok(())
}

/// Law 79: after a placement fails, the memory reads as a cold read of the
/// files. A run of two appends whose second name is taken by a directory
/// places the first and fails on the second; the decision's memory is a
/// fresh handle's, the same append again places nothing it did not, and
/// once the name is free the run completes as a store that never failed.
#[test]
fn a_failed_placement_leaves_the_memory_as_a_cold_read() {
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
    ];
    let clean = scratch.store("clean");
    let names = clean.append_all(events(&drafts)).expect("appends");
    let store = scratch.store("store");
    let blocked = store
        .root()
        .join(format!("objects/{}.py", names[1].as_str()));
    {
        let mut decision = store.decide().expect("decides");
        fs::create_dir(&blocked).expect("blocks the second name");
        let stopped = decision.append_all(events(&drafts));
        assert!(matches!(stopped, Err(Stopped { ref placed, .. }) if placed.is_empty()));
        let cold = Store::new(store.root(), Untrusted::none());
        assert_eq!(**decision.dag(), *cold.dag().expect("reads"));
        assert_eq!(
            decision.append(events(&drafts)[0].clone()).expect("a twin"),
            names[0]
        );
        assert!(decision.append(events(&drafts)[1].clone()).is_err());
        assert_eq!(**decision.dag(), *cold.dag().expect("reads"));
    }
    fs::remove_dir(&blocked).expect("unblocks");
    assert_eq!(store.append_all(events(&drafts)).expect("appends"), names);
    let objects = |root: &Path| {
        let mut held = files(root);
        held.retain(|file, _| file.ends_with(".py"));
        held
    };
    assert_eq!(objects(store.root()), objects(clean.root()));
    // Each failed rename left its temp, which `verify` names, as after any
    // write cut short.
    assert!(store
        .verify()
        .iter()
        .all(|finding| matches!(finding, Finding::Garbage(file) if file.ends_with(".tmp"))));
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

/// Law 75, read at the store: each state that a stop of the replacement of
/// one file can leave (`store.rs` stops the replacement itself) leaves under
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
    // The husking itself, from the whole: the name reads as the husk, in a
    // file that replaced the whole by a rename and was not written over in
    // place. The stops inside the replacement are law 75's test in
    // `store.rs`.
    let inode = |store: &Store| {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(store.root().join(format!("objects/{}.py", name.as_str())))
            .expect("a file")
            .ino()
    };
    let before = inode(&store);
    assert_eq!(
        store.husk([name.clone()]).expect("husks"),
        vec![name.clone()]
    );
    assert_eq!(store.kept(&name), Some(husk));
    assert_ne!(inode(&store), before, "a rename, not a write in place");
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
    let read = decode::<prodrome::event::TodoEvent<Todo>>(
        &Default::default(),
        &name,
        print.as_bytes(),
        Sender::Keeper,
    );
    assert!(matches!(read, Err(Unread::Tampered(_))));
    let refused: Result<Vec<Hash>, ProdromeError> =
        MemoryStore::<prodrome::event::TodoEvent<Todo>>::default()
            .receive([name.clone()].into(), &|_| Some(print.as_bytes().to_vec()));
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
