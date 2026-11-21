//! Husks in a store of record and in every replica that takes them: SPEC
//! laws 72, 73, 74, 76 and 77, over the letter schema
//! (`../schemas/letter.rs`), whose bodies hold parts sealed under named
//! keys.
//!
//! A history of letters is appended to a store of record as one run, and a
//! set of its objects is husked. Laws 72 and 76 are writes that only a
//! store of record makes (`husk` and a run of appends), so they run over
//! every store of record. Laws 73, 74 and 77 are about what a replica takes
//! and keeps, so they run over every replica.

use std::collections::{BTreeMap, BTreeSet};

use prodrome::dag::Finding;
use prodrome::event::{canonical_envelope, parents_of, seal_hash, Actor, Envelope, Hash};
use prodrome::policy::{Everything, Policy, Proven, Standing};
use prodrome::sign::{KeyAdded, Proof, Proved, Registrar, Secret};
use prodrome::store::{accept, sync, HuskObject, Overlay, Replica, Sender, Stopped};
use proptest::prelude::*;

use super::kind::{
    begin, copy_prints, replicas, stores_of_record, AnyReplica, Scratch, StoreOfRecord,
};
use crate::husk::letter::{self, Letter, Says};
use crate::husk::{a_choice, a_history, events, keyless, made, opens_any, Draft};

/// The store at `store`, with the history of `drafts` and a copy of it in
/// a sibling, whose objects chosen by `choice` are husked. Answers the
/// copy and the names husked.
fn husked(
    store: &dyn StoreOfRecord<Letter>,
    drafts: &[Draft],
    choice: &[bool],
) -> (Box<dyn StoreOfRecord<Letter>>, BTreeSet<Hash>) {
    let chosen = made(store, drafts, choice);
    let copy = store.sibling();
    copy_prints(store, &*copy);
    let done = copy.husk(chosen).expect("husks").into_iter().collect();
    (copy, done)
}

/// Law 72 at one store.
fn a_husk_changes_only_the_named(
    store: &dyn StoreOfRecord<Letter>,
    drafts: &[Draft],
    choice: &[bool],
) -> Result<(), TestCaseError> {
    let chosen = made(store, drafts, choice);
    let dag = store.held().expect("reads").dag;
    let order = dag.linearise().expect("orders").to_vec();
    let heads = store.heads().expect("heads");
    let record = store.record();
    let parents: BTreeMap<Hash, Vec<Hash>> = dag
        .objects()
        .iter()
        .map(|(name, object)| (name.clone(), parents_of(object)))
        .collect();
    let before = store.prints();
    let done: BTreeSet<Hash> = store.husk(chosen).expect("husks").into_iter().collect();
    let after = store.prints();
    prop_assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (name, bytes) in &after {
        if done.contains(name) {
            let Some(Envelope::Change(change)) = dag.get(name) else {
                return Err(TestCaseError::fail("a husk of no change"));
            };
            let husk = HuskObject::of(change).expect("a box to drop");
            prop_assert_eq!(bytes, &husk.print().into_bytes());
        } else {
            prop_assert_eq!(bytes, &before[name]);
        }
    }
    let dag = store.held().expect("reads").dag;
    prop_assert_eq!(dag.linearise().expect("orders"), &order[..]);
    prop_assert_eq!(store.heads().expect("heads"), heads.clone());
    prop_assert_eq!(store.open().heads().expect("heads"), heads);
    prop_assert_eq!(
        store.record(),
        record,
        "the record of heads does not change"
    );
    for (name, object) in dag.objects() {
        prop_assert_eq!(&parents_of(object), &parents[name]);
    }
    prop_assert!(
        store.husk(done).expect("husks").is_empty(),
        "a husk is passed over"
    );
    prop_assert_eq!(store.prints(), after);
    prop_assert_eq!(store.verify(), Vec::<Finding>::new());
    Ok(())
}

/// The checks of law 73 at one replica: no event of its DAG or its fold
/// under a husked name holds a box, each husked name keeps its husk's
/// print, and its `print` answers nothing.
fn never_taken_back(
    replica: &(impl Replica<Letter> + ?Sized),
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

/// Law 73 at one store of record, and the replicas of `scratch`.
fn never_taken_back_anywhere(
    whole: &dyn StoreOfRecord<Letter>,
    scratch: &Scratch,
    drafts: &[Draft],
    choice: &[bool],
) -> Result<(), TestCaseError> {
    let (husked, done) = husked(whole, drafts, choice);
    let kept: BTreeMap<Hash, Vec<u8>> = done
        .iter()
        .map(|name| (name.clone(), husked.kept(name).expect("kept")))
        .collect();
    let everything = whole.tips().expect("tips");

    for event in events(drafts) {
        husked.append(event).expect("appends");
    }
    husked.append_all(events(drafts)).expect("appends");
    sync(whole, &*husked, Sender::Stranger).expect("syncs");
    accept(whole, &*husked, everything.clone(), Sender::Keeper).expect("accepts");
    husked
        .receive(done.clone(), &|name| whole.kept(name), Sender::Keeper)
        .expect("receives");
    for name in &done {
        accept(whole, &*husked, [name.clone()].into(), Sender::Keeper).expect("adopts");
    }
    never_taken_back(&*husked, &kept)?;
    never_taken_back(&*husked.open(), &kept)?;

    for replica in replicas::<Letter>(scratch, &Default::default()) {
        let replica = replica.replica();
        accept(
            &*husked,
            replica,
            husked.tips().expect("tips"),
            Sender::Keeper,
        )
        .expect("accepts");
        for event in events(drafts) {
            replica.append(event).expect("appends");
        }
        sync(whole, replica, Sender::Stranger).expect("syncs");
        accept(whole, replica, everything.clone(), Sender::Keeper).expect("accepts");
        never_taken_back(replica, &kept)?;
    }

    let base = whole.sibling();
    begin(&*base, "letters");
    let overlay = Overlay::new(&*base);
    accept(whole, &overlay, everything, Sender::Keeper).expect("takes the wholes");
    accept(
        &*husked,
        &*base,
        husked.tips().expect("tips"),
        Sender::Keeper,
    )
    .expect("the base takes the husks");
    never_taken_back(&overlay, &kept)?;
    overlay.flush().expect("flushes");
    never_taken_back(&*base, &kept)?;
    Ok(())
}

/// Law 74 at one store of record, and the replicas of `scratch`.
fn a_replica_reads_as_its_keeper(
    store: &dyn StoreOfRecord<Letter>,
    scratch: &Scratch,
    drafts: &[Draft],
    choice: &[bool],
) -> Result<(), TestCaseError> {
    let (store, done) = husked(store, drafts, choice);
    let tips = store.tips().expect("tips");
    for pulled in replicas::<Letter>(scratch, &Default::default()) {
        let replica = pulled.replica();
        accept(&*store, replica, tips.clone(), Sender::Keeper).expect("pulls");
        prop_assert_eq!(keyless(replica), keyless(&*store));
        let held = replica.held().expect("reads");
        prop_assert_eq!(held.dag.husks(), &done);
        prop_assert!(accept(&*store, replica, tips.clone(), Sender::Keeper)
            .expect("pulls")
            .is_empty());
        match &pulled {
            AnyReplica::OfRecord(pulled) => {
                prop_assert_eq!(pulled.verify(), Vec::<Finding>::new());
            }
            AnyReplica::Memory(_) => {}
        }
    }
    prop_assert_eq!(store.verify(), Vec::<Finding>::new());
    for stranger in replicas::<Letter>(scratch, &Default::default()) {
        let stranger = stranger.replica();
        prop_assert_eq!(
            sync(&*store, stranger, Sender::Stranger).is_err(),
            !done.is_empty()
        );
        if !done.is_empty() {
            prop_assert!(stranger.held().expect("reads").dag.objects().is_empty());
        }
    }
    Ok(())
}

/// Law 76 at one store of record.
fn a_run_is_appends_one_by_one(
    store: &dyn StoreOfRecord<Letter>,
    drafts: &[Draft],
    at: usize,
) -> Result<(), TestCaseError> {
    let run = store;
    let one = store.sibling();
    begin(run, "letters");
    begin(&*one, "letters");
    let names = run.append_all(events(drafts)).expect("appends");
    let each: Vec<Hash> = events(drafts)
        .into_iter()
        .map(|event| one.append(event).expect("appends"))
        .collect();
    prop_assert_eq!(names, each.clone());
    prop_assert_eq!(run.prints(), one.prints());
    prop_assert_eq!(run.heads().expect("heads"), one.heads().expect("heads"));
    // `prints` holds no temp and no stray, and `verify` reports each.
    prop_assert_eq!(run.verify(), Vec::<Finding>::new());
    prop_assert_eq!(one.verify(), Vec::<Finding>::new());

    let at = at.min(drafts.len());
    let mut refused = events(drafts);
    let mut refusal = refused[0].clone();
    refusal.says = Says::Titled(letter::REFUSED.to_owned());
    refused.insert(at, refusal);
    let stopped = store.sibling();
    let before = store.sibling();
    begin(&*stopped, "letters");
    begin(&*before, "letters");
    for event in events(drafts).into_iter().take(at) {
        before.append(event).expect("appends");
    }
    match stopped.append_all(refused) {
        Err(Stopped { placed, why }) => {
            prop_assert_eq!(placed, each[..at].to_vec());
            prop_assert!(why.to_string().contains("refused"), "{}", why);
        }
        Ok(names) => {
            return Err(TestCaseError::fail(format!(
                "a refused run answered {names:?}"
            )));
        }
    }
    prop_assert_eq!(stopped.prints(), before.prints());
    prop_assert_eq!(
        stopped.heads().expect("heads"),
        before.heads().expect("heads")
    );
    prop_assert_eq!(stopped.verify(), Vec::<Finding>::new());
    Ok(())
}

/// Law 77 at the replicas of `scratch`, from the prints of `store`.
fn no_husk_is_proven(
    store: &dyn StoreOfRecord<Letter>,
    scratch: &Scratch,
    drafts: &[Draft],
    choice: &[bool],
) -> Result<(), TestCaseError> {
    let chosen = made(store, drafts, choice);
    let ada = Secret::new(&"1".repeat(64)).expect("a seed");
    let dag = store.held().expect("reads").dag;
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
        let Some(event) = object.event() else {
            continue;
        };
        let signed: Envelope<Letter> = Envelope::Signed(ada.sign(name));
        prints.insert(seal_hash(&signed), canonical_envelope(&signed).into_bytes());
        let Envelope::Change(change) = object else {
            continue;
        };
        if !chosen.contains(name) || !event.boxed() {
            continue;
        }
        let husk = HuskObject::of(change).expect("a box").print();
        let lie = husk.replacen(
            &format!("letter='{}'", event.letter.0),
            "letter='forged'",
            1,
        );
        prints.insert(name.clone(), lie.into_bytes());
        forged.insert(name.clone());
    }
    let root = Secret::new(&"9".repeat(64)).expect("a seed");
    let rooted: Envelope<Letter> = Envelope::Signed(root.sign(&seal_hash(&added)));
    prints.insert(seal_hash(&rooted), canonical_envelope(&rooted).into_bytes());
    let print_of = |name: &Hash| prints.get(name).cloned();

    for stranger in replicas::<Letter>(scratch, &Default::default()) {
        let stranger = stranger.replica();
        for name in prints.keys() {
            let _ = stranger.receive([name.clone()].into(), &print_of, Sender::Stranger);
        }
        let held = stranger.held().expect("reads");
        let proof = Proof::of(&held.dag, &Registrar::Anyone);
        for name in held.dag.objects().keys() {
            match proof.standing(name) {
                Proved::Proven | Proved::Vouched => {
                    prop_assert!(stranger.print(name).is_some(), "{} stands unhashed", name);
                }
                Proved::Unsigned => {}
            }
            prop_assert!(!forged.contains(name), "a stranger's husk was taken");
        }
    }

    let everything: BTreeSet<Hash> = prints.keys().cloned().collect();
    for keeper in replicas::<Letter>(scratch, &Default::default()) {
        let keeper = keeper.replica();
        keeper
            .receive(everything.clone(), &print_of, Sender::Keeper)
            .expect("takes the keeper's word");
        let held = keeper.held().expect("reads");
        let proof = Proof::of(&held.dag, &Registrar::Anyone);
        let policy = Proven::new(Everything, [root.public()], &held.dag);
        for name in &forged {
            prop_assert!(held.dag.husks().contains(name));
            prop_assert_eq!(proof.standing(name), Proved::Vouched);
            let event = held
                .dag
                .get(name)
                .and_then(Envelope::event)
                .expect("an event");
            prop_assert_eq!(policy.standing(name, event), Standing::Binds);
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// Law 72, over every store of record: `husk` changes only the prints
    /// of the named objects that hold a box, each to its husk's print;
    /// every other print is byte-identical, and the heads, the record of
    /// heads, the linear order and every object's parents do not change. A
    /// second `husk` of the same names changes nothing.
    #[test]
    fn a_husk_changes_only_the_named_prints(drafts in a_history(), choice in a_choice()) {
        let scratch = Scratch::new();
        for store in stores_of_record::<Letter>(&scratch, &Default::default()) {
            a_husk_changes_only_the_named(&*store, &drafts, &choice)?;
        }
    }

    /// Law 73, over every store of record and every replica: after
    /// `husk`, no sequence of operations makes a sealed part of a husked
    /// object readable. The store of record appends each whole event again,
    /// one by one and as a run, and syncs, receives and adopts every whole
    /// from a replica that holds them, as a stranger and as its keeper.
    /// Every replica that took the husks from the store does the same. An
    /// overlay whose own layer holds the wholes, over a base that then takes
    /// the husks, reads and answers the husks, and its flush keeps them.
    /// Each husked name keeps its husk's print, and its `print` answers
    /// nothing.
    #[test]
    fn a_dropped_part_is_never_taken_back(drafts in a_history(), choice in a_choice()) {
        let scratch = Scratch::new();
        for whole in stores_of_record::<Letter>(&scratch, &Default::default()) {
            never_taken_back_anywhere(&*whole, &scratch, &drafts, &choice)?;
        }
    }

    /// Law 74, over every store of record and every replica: a replica that
    /// pulls from a store holding husks, as from its keeper of record, holds
    /// the husks and reads as the store does, and `verify` is clean on both
    /// where each is a store of record. A pull from the same store as from
    /// a stranger refuses every husk and takes nothing.
    #[test]
    fn a_replica_of_a_husked_store_reads_as_it(drafts in a_history(), choice in a_choice()) {
        let scratch = Scratch::new();
        for store in stores_of_record::<Letter>(&scratch, &Default::default()) {
            a_replica_reads_as_its_keeper(&*store, &scratch, &drafts, &choice)?;
        }
    }

    /// Law 76, over every store of record: a run of appends writes the
    /// objects that one append after another writes, and answers the same
    /// names. A run with a refused event at any place stops there: the
    /// events before it are placed, as one append after another places
    /// them, and none after it is.
    #[test]
    fn a_run_writes_what_appends_one_by_one_write(drafts in a_history(), at in 0..13_usize) {
        let scratch = Scratch::new();
        for store in stores_of_record::<Letter>(&scratch, &Default::default()) {
            a_run_is_appends_one_by_one(&*store, &drafts, at)?;
        }
    }

    /// Law 77, over every replica: for any sequence of receipts from a
    /// party whose word is not taken, no object reads as proven, or vouched
    /// for, whose bytes the receiver did not hash to its name. The party
    /// holds a true signature of every change, by the key registered to its
    /// actor under a root's signature, and forged husks under some of their
    /// names. From the keeper of record the same husks are taken: the
    /// signature vouches for each and proves none, and the policy that
    /// requires signatures binds each, on the keeper's word.
    #[test]
    fn a_signature_proves_no_husk(drafts in a_history(), choice in a_choice()) {
        let scratch = Scratch::new();
        for store in stores_of_record::<Letter>(&scratch, &Default::default()) {
            no_husk_is_proven(&*store, &scratch, &drafts, &choice)?;
        }
    }
}
