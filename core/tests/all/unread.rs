//! SPEC law 45: the store of record reads totally, as a replica does.
//!
//! A store on disk and a replica handed the same prints are one reading:
//! each is [`Dag::from_prints`] of what it holds, and its history is that
//! set's interior (law 40), so a file that is no object (it fails its hash,
//! it is not text, it does not parse at the schema the store is opened at)
//! is left out with everything resting on it, and [`Dag::excluded`] names
//! it with why, exactly as a replica's reading does (law 44). Only a write
//! refuses, naming the file: it would name the store's heads, and an unread
//! file may be one.
//!
//! The histories are the shared generators' (`common::draw`) over the
//! proposal schema as data, read at it or at a newer schema whose `Tagged`
//! has gained a field, so every `Tagged` written before fails to parse and
//! what rests on it waits, as in a store migrated away from. Random files
//! are then damaged in place, removed, or joined by prints that are no
//! object, and the store is read by a fresh handle and by one that held the
//! intact files first.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::sync::OnceLock;

use prodrome::dag::Dag;
use prodrome::declared::{Declaration, Declared};
use prodrome::event::{canonical_envelope, Actor, Hash};
use prodrome::policy::Everything;
use prodrome::registers::{fold, Node};
use prodrome::schema::Schema;
use prodrome::store::EventStore;
use prodrome::store::Sender;
use proptest::prelude::*;
use sha2::{Digest, Sha256};

use crate::common::draw::{a_draw, history, Draw};
use crate::declared::proposal::{a_proposal, Id, Proposal, Says, DECLARED};
use crate::declared::proposals;
use crate::memory::Scratch;

type Store = EventStore<Declared, Everything>;

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

/// What becomes of a file chosen to be damaged.
#[derive(Debug, Clone, Copy)]
enum Damage {
    /// Its bytes changed under its name: it fails its hash.
    Tampered,
    /// Removed: what rests on it names a parent no print is.
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

/// `prints` as `root`'s `objects/`, and nothing else.
fn lay_out(root: &std::path::Path, prints: &BTreeMap<Hash, Vec<u8>>) {
    let objects = root.join("objects");
    let _ = fs::remove_dir_all(&objects);
    fs::create_dir_all(&objects).expect("creates objects/");
    for (name, bytes) in prints {
        fs::write(objects.join(format!("{}.py", name.as_str())), bytes).expect("writes");
    }
}

/// The law at one set of prints: every read of a store holding them, by
/// `warm` (which held other files first) and by a fresh handle, is the
/// reading of a replica handed them, and a write refuses exactly when that
/// replica holds a print that is no object, with that print's refusal.
fn reads_as_its_replica(
    schema: &Declaration,
    warm: &Store,
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
    let fresh = Store::at(warm.root(), Everything, schema.clone());
    for store in [warm, &fresh] {
        let dag = store.dag().expect("a read is total");
        prop_assert_eq!(&*dag, &replica, "the store holds what the replica holds");
        prop_assert_eq!(
            dag.excluded(),
            replica.excluded(),
            "and leaves out the same"
        );
        prop_assert_eq!(&*store.folded().expect("folds"), &folded);
        prop_assert_eq!(store.tips().expect("derives"), replica.tips());
        prop_assert_eq!(store.events().expect("reads"), events.clone());
        for tip in replica.tips() {
            prop_assert_eq!(
                store.ancestors(&tip).expect("a head has ancestors"),
                replica.closure(
                    replica
                        .get(&tip)
                        .map(prodrome::event::parents_of)
                        .unwrap_or_default()
                )
            );
        }
    }
    // A write: refused, with the first unread print's own refusal, exactly
    // when something is unread. With nothing unread it is written over the
    // history's heads, and the store and a replica handed its files, the
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
        let print = fs::read(
            fresh
                .root()
                .join("objects")
                .join(format!("{}.py", name.as_str())),
        )
        .expect("the write is on disk");
        let mut after = prints.clone();
        after.insert(name.clone(), print);
        let replica = Dag::<Declared>::from_prints(
            schema,
            after
                .iter()
                .map(|(name, bytes)| (name.clone(), Ok(bytes.clone()))),
            Sender::Stranger,
        )
        .interior();
        prop_assert!(replica.get(&name).is_some(), "the write is in the history");
        for store in [warm, &fresh] {
            prop_assert_eq!(
                &*store.dag().expect("reads"),
                &replica,
                "and they still agree"
            );
            prop_assert_eq!(store.tips().expect("derives"), replica.tips());
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 45: over generated histories read at the schema they were
    /// written at or at a newer one, with random files damaged in place or
    /// removed and prints that are no object added, a store on disk, read
    /// cold and by a handle that held the intact files, holds what a
    /// replica handed the same prints holds: its history, its fold, its
    /// heads, its events and ancestry, and what it leaves out and why. A
    /// write refuses, with the first unread print's refusal, exactly when
    /// one is held; otherwise it is written, and the store and the replica
    /// still agree.
    #[test]
    fn a_store_reads_as_a_replica_holding_its_files(
        draw in a_draw(a_proposal()),
        newer in any::<bool>(),
        damage in prop::collection::vec((any::<prop::sample::Index>(), a_damage()), 0..4),
        junk in 0usize..3,
    ) {
        let schema = if newer { migrated() } else { proposals() };
        let mut prints = prints(&draw);
        let scratch = Scratch::new("unread");
        let warm = Store::at(scratch.0.join("store"), Everything, schema.clone());
        lay_out(warm.root(), &prints);
        let _ = warm.dag().expect("reads the intact files");
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
        lay_out(warm.root(), &prints);
        reads_as_its_replica(schema, &warm, &prints)?;
    }
}
