//! Law 42 (SPEC §3, §5): who wrote an object, proven by the history.
//!
//! A generated history is a log of writes by two actors, and then keys
//! added and revoked (each by a root's signature or by nobody's), honest
//! signatures by registered keys, by unregistered ones and by a root, and
//! forged ones, over any object written so far, signatures included. The
//! proof it gives is a function of its objects: replicas that received
//! them one at a time in any order, or in two halves joined by sync, read
//! the same proof as the whole.
//!
//! And the policy that requires it, `Proven`: under a policy that does not,
//! signatures and keys change no reading; under one that does, an object
//! the history does not prove claims and every other is as the wrapped
//! policy says, whatever order the objects arrived in, and a history whose
//! every write is signed by its actor's registered key reads as the
//! wrapped policy reads it.

use std::collections::{BTreeMap, BTreeSet};

use prodrome::dag::Dag;
use prodrome::event::{canonical_envelope, seal_hash, Actor, Envelope, Hash, TodoEvent};
use prodrome::genesis::mk_genesis;
use prodrome::policy::{Everything, Policy, Proven, Standing, Untrusted};
use prodrome::reference::Todo;
use prodrome::sign::{mk_key_revoked, Checks, KeyAdded, Proof, Registrar, Secret, Signed};
use prodrome::store::{sync, MemoryStore, Replica};
use prodrome::todo::TodoVocabulary;
use prodrome::view::{entries, Entry};
use proptest::prelude::*;

use crate::common::{a_log, far};

type Event = TodoEvent<Todo>;
type Object = Envelope<Event>;

const ACTORS: [&str; 2] = ["ada", "triage"];

/// The devices' keys, and the root's, which registers them.
fn keys() -> [Secret; 3] {
    ['1', '2', '3'].map(|seed| Secret::new(&seed.to_string().repeat(64)).expect("a seed"))
}

fn root() -> Secret {
    Secret::new(&"9".repeat(64)).expect("a seed")
}

fn roots() -> Registrar {
    Registrar::Roots([root().public()].into())
}

/// One step after the writes.
#[derive(Debug, Clone)]
pub enum Step {
    Add {
        actor: usize,
        key: usize,
        rooted: bool,
    },
    Revoke {
        actor: usize,
        key: usize,
        keep: Vec<prop::sample::Index>,
        rooted: bool,
    },
    /// By a device's key, or by the root's (`key == 3`).
    Sign {
        key: usize,
        object: prop::sample::Index,
    },
    Forge {
        key: usize,
        object: prop::sample::Index,
    },
}

fn a_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        2 => (0..2usize, 0..3usize, prop::bool::weighted(0.8))
            .prop_map(|(actor, key, rooted)| Step::Add { actor, key, rooted }),
        1 => (
            0..2usize,
            0..3usize,
            prop::collection::vec(any::<prop::sample::Index>(), 0..4),
            prop::bool::weighted(0.8),
        )
            .prop_map(|(actor, key, keep, rooted)| Step::Revoke { actor, key, keep, rooted }),
        6 => (0..4usize, any::<prop::sample::Index>())
            .prop_map(|(key, object)| Step::Sign { key, object }),
        1 => (0..3usize, any::<prop::sample::Index>())
            .prop_map(|(key, object)| Step::Forge { key, object }),
    ]
}

/// A history's objects by name, each with its print.
#[derive(Debug, Clone)]
pub struct History {
    pub prints: BTreeMap<Hash, Vec<u8>>,
}

impl History {
    pub fn dag(&self) -> Dag<Event> {
        Dag::from_prints(
            &TodoVocabulary::default(),
            self.prints
                .iter()
                .map(|(name, print)| (name.clone(), Ok(print.clone()))),
        )
    }

    /// The same history without a signature or a key object.
    pub fn unsigned(&self) -> Dag<Event> {
        self.dag()
            .objects()
            .iter()
            .filter(|(_, object)| {
                !matches!(
                    object,
                    Envelope::Signed(_) | Envelope::KeyAdded(_) | Envelope::KeyRevoked(_)
                )
            })
            .map(|(name, object)| (name.clone(), object.clone()))
            .collect()
    }

    /// A replica in memory given `names`, one at a time in this order: each
    /// receipt takes in what its object rests on first.
    pub fn received(&self, names: impl IntoIterator<Item = Hash>) -> MemoryStore<Event> {
        let replica = MemoryStore::default();
        for name in names {
            replica
                .receive([name].into(), &|name| self.prints.get(name).cloned())
                .expect("every print is held");
        }
        replica
    }

    /// Two replicas, each given the names `halves` deals it (the second in
    /// reverse), the first then synced into the second.
    pub fn joined(&self, halves: &[bool]) -> MemoryStore<Event> {
        let (mut mine, mut theirs) = (Vec::new(), Vec::new());
        for (name, to_mine) in self.prints.keys().zip(halves.iter().cycle()) {
            if *to_mine {
                mine.push(name.clone());
            } else {
                theirs.push(name.clone());
            }
        }
        let mine = self.received(mine);
        let theirs = self.received(theirs.into_iter().rev());
        sync(&mine, &theirs).expect("syncs");
        theirs
    }
}

fn put(prints: &mut BTreeMap<Hash, Vec<u8>>, object: &Object) -> Hash {
    let name = seal_hash(object);
    prints.insert(name.clone(), canonical_envelope(object).into_bytes());
    name
}

/// The writes of `log` appended over one genesis, then `steps`.
pub fn build(log: &[Event], steps: &[Step]) -> History {
    let mut prints = BTreeMap::new();
    let genesis = put(
        &mut prints,
        &Envelope::Genesis(mk_genesis("signed", &"cd".repeat(16)).expect("a genesis")),
    );
    let store = MemoryStore::<Event>::default();
    store
        .receive([genesis.clone()].into(), &|name| prints.get(name).cloned())
        .expect("a genesis");
    let store = store.in_genesis(genesis.clone());
    for event in log {
        let name = store.append(event.clone()).expect("appends");
        prints.insert(name.clone(), store.print(&name).expect("held"));
    }
    let (keys, root) = (keys(), root());
    let mut signatures: Vec<Hash> = Vec::new();
    for step in steps {
        let objects: Vec<Hash> = prints.keys().cloned().collect();
        match step {
            Step::Add { actor, key, rooted } => {
                let added = put(
                    &mut prints,
                    &Envelope::KeyAdded(KeyAdded {
                        genesis: genesis.clone(),
                        actor: Actor::new(ACTORS[*actor]).expect("an actor"),
                        key: keys[*key].public(),
                    }),
                );
                if *rooted {
                    signatures.push(put(&mut prints, &Envelope::Signed(root.sign(&added))));
                }
            }
            Step::Revoke {
                actor,
                key,
                keep,
                rooted,
            } => {
                let deps: BTreeSet<Hash> = keep
                    .iter()
                    .filter(|_| !signatures.is_empty())
                    .map(|index| index.get(&signatures).clone())
                    .collect();
                let revoked = mk_key_revoked(
                    genesis.clone(),
                    deps.into_iter().collect(),
                    Actor::new(ACTORS[*actor]).expect("an actor"),
                    keys[*key].public(),
                )
                .expect("distinct");
                let revoked = put(&mut prints, &Envelope::KeyRevoked(revoked));
                if *rooted {
                    signatures.push(put(&mut prints, &Envelope::Signed(root.sign(&revoked))));
                }
            }
            Step::Sign { key, object } => {
                let by = keys.get(*key).unwrap_or(&root);
                let signed = by.sign(object.get(&objects));
                signatures.push(put(&mut prints, &Envelope::Signed(signed)));
            }
            Step::Forge { key, object } => {
                let forged = Signed {
                    object: object.get(&objects).clone(),
                    ..keys[*key].sign(&Hash::new("0".repeat(64)).expect("a name"))
                };
                signatures.push(put(&mut prints, &Envelope::Signed(forged)));
            }
        }
    }
    History { prints }
}

pub fn a_signed_history() -> impl Strategy<Value = History> {
    (a_log(), prop::collection::vec(a_step(), 0..24)).prop_map(|(log, steps)| build(&log, &steps))
}

/// A permutation of `names`, drawn by `draws`.
pub fn shuffled(names: &[Hash], draws: &[prop::sample::Index]) -> Vec<Hash> {
    let mut left = names.to_vec();
    let mut out = Vec::with_capacity(left.len());
    for draw in draws.iter().cycle().take(left.len()) {
        out.push(left.remove(draw.index(left.len())));
    }
    out
}

fn proofs(dag: &Dag<Event>) -> (Proof, Proof) {
    (Proof::of(dag, &Registrar::Anyone), Proof::of(dag, &roots()))
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// A signature's validity is a function of the object set: a replica
    /// that received the objects one at a time in any order, and one that
    /// received two halves apart and joined them by sync, prove what the
    /// whole set proves, under either reading of the keys.
    #[test]
    fn a_proof_is_a_function_of_the_object_set(
        history in a_signed_history(),
        draws in prop::collection::vec(any::<prop::sample::Index>(), 1..16),
        halves in prop::collection::vec(any::<bool>(), 1..16),
    ) {
        let whole = proofs(&history.dag());
        let names: Vec<Hash> = history.prints.keys().cloned().collect();
        let one_at_a_time = history.received(shuffled(&names, &draws));
        prop_assert_eq!(&proofs(&one_at_a_time.held().expect("reads").dag), &whole);

        let joined = history.joined(&halves);
        prop_assert_eq!(&proofs(&joined.held().expect("reads").dag), &whole);
    }

    /// Signatures and keys change no reading under a policy that does not
    /// ask for them: every entry, claim, price and confidence of the
    /// history reads as the history without them.
    #[test]
    fn signatures_change_no_reading_under_a_policy_that_does_not_require_them(
        history in a_signed_history(),
    ) {
        let (signed, unsigned) = (history.dag(), history.unsigned());
        prop_assert_eq!(read(&signed, &Everything), read(&unsigned, &Everything));
        prop_assert_eq!(read(&signed, &roster()), read(&unsigned, &roster()));
    }

    /// Under `Proven`, an object the history does not prove claims, and
    /// every other is the wrapped policy's: the reading is the wrapped
    /// policy's with exactly the unproven objects' events claims. An object
    /// no signature that verifies covers is among them, a forged one
    /// included. And the reading is a function of the object set.
    #[test]
    fn under_proven_an_unproven_object_claims_in_any_arrival_order(
        history in a_signed_history(),
        draws in prop::collection::vec(any::<prop::sample::Index>(), 1..16),
        halves in prop::collection::vec(any::<bool>(), 1..16),
    ) {
        let dag = history.dag();
        let proven = required(&dag);
        let mut unproven = BTreeSet::new();
        for (name, object) in dag.objects() {
            let Some(event) = object.event() else {
                continue;
            };
            let signed = dag.objects().values().any(|object| {
                matches!(object, Envelope::Signed(s) if s.object == *name && s.verifies())
            });
            if proven.proof().proves(name) {
                prop_assert!(signed, "a proven object carries a signature that verifies");
                prop_assert_eq!(proven.standing(name, event), roster().standing(name, event));
                prop_assert_eq!(proven.confirms(name, event), roster().confirms(name, event));
            } else {
                prop_assert_eq!(proven.standing(name, event), Standing::Claims);
                prop_assert!(!proven.confirms(name, event));
                unproven.insert(name.clone());
            }
        }
        let whole = read(&dag, &proven);
        prop_assert_eq!(&whole, &read(&dag, &Except { claimed: unproven }));

        let names: Vec<Hash> = history.prints.keys().cloned().collect();
        for replica in [history.received(shuffled(&names, &draws)), history.joined(&halves)] {
            let held = replica.held().expect("reads").dag;
            prop_assert_eq!(&read(&held, &required(&held)), &whole);
        }
    }

    /// A history whose every write its actor's registered key signed reads
    /// under `Proven` as under the policy it wraps.
    #[test]
    fn a_history_signed_throughout_reads_as_the_wrapped_policy(log in a_log()) {
        let mut steps = vec![];
        for (actor, _) in ACTORS.iter().enumerate() {
            steps.push(Step::Add { actor, key: actor, rooted: true });
        }
        let history = build(&log, &steps);
        let mut prints = history.prints.clone();
        let (keys, dag) = (keys(), history.dag());
        for (name, object) in dag.objects() {
            if let Some(event) = object.event() {
                let actor = ACTORS.iter().position(|a| *a == event.actor().as_str());
                let key = &keys[actor.expect("one of the two")];
                put(&mut prints, &Envelope::Signed(key.sign(name)));
            }
        }
        let signed = History { prints }.dag();
        let proven = required(&signed);
        prop_assert_eq!(read(&signed, &proven), read(&signed, &roster()));
        prop_assert!(signed.verify(&proven).is_empty());
    }
}

/// The reference policy, with one actor on the roster.
fn roster() -> Untrusted {
    Untrusted::of([Actor::new("triage").expect("an actor")])
}

/// The roster, held to the actors' keys under the root, at `history`.
fn required(history: &Dag<Event>) -> Proven<Untrusted> {
    Proven::new(roster(), [root().public()], history)
}

/// The roster, with exactly `claimed` claims besides.
#[derive(Debug, Clone)]
struct Except {
    claimed: BTreeSet<Hash>,
}

impl Policy<Event> for Except {
    fn standing(&self, object: &Hash, event: &Event) -> Standing {
        if self.claimed.contains(object) {
            Standing::Claims
        } else {
            roster().standing(object, event)
        }
    }

    fn confirms(&self, object: &Hash, event: &Event) -> bool {
        !self.claimed.contains(object) && roster().confirms(object, event)
    }
}

/// Every entry of `dag`, as the view composes it, long after everything.
fn read(dag: &Dag<Event>, policy: &impl Policy<Event>) -> Vec<Entry<Event>> {
    entries(&dag.nodes().expect("closed"), far(), policy).expect("reads")
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 49: a table of signature checks means `Signed::verifies`. Over
    /// any signed history, forged signatures among them, a proof read
    /// through a table answers what the plain proof answers, under either
    /// reading of the keys: through a table that keeps few checks and drops
    /// the rest, through the same table read again, and through a cold
    /// one. Each check through a table is the signature's own check.
    #[test]
    fn a_proof_through_a_table_of_checks_is_the_plain_proof(
        history in a_signed_history(),
        capacity in 1..8usize,
    ) {
        let dag = history.dag();
        let small = Checks::new(capacity);
        for registrar in [Registrar::Anyone, roots()] {
            let plain = Proof::of(&dag, &registrar);
            prop_assert_eq!(&Proof::through(&dag, &registrar, &small), &plain, "a small table");
            prop_assert_eq!(&Proof::through(&dag, &registrar, &small), &plain, "read again");
            prop_assert_eq!(
                &Proof::through(&dag, &registrar, &Checks::new(1 << 16)),
                &plain,
                "a cold table"
            );
        }
        for object in dag.objects().values() {
            if let Envelope::Signed(signed) = object {
                prop_assert_eq!(small.verifies(signed), signed.verifies());
            }
        }
        let warm = Checks::new(1 << 16);
        for object in dag.objects().values() {
            if let Envelope::Signed(signed) = object {
                if signed.verifies() {
                    prop_assert!(warm.verifies(signed));
                }
            }
        }
        for registrar in [Registrar::Anyone, roots()] {
            prop_assert_eq!(
                &Proof::through(&dag, &registrar, &warm),
                &Proof::of(&dag, &registrar),
                "a table that holds every true check"
            );
        }
    }
}

/// Law 49: a table that holds a true check for one `Signed` object answers
/// false for a different one that does not verify: the same signature
/// carried to another object, the same signature claimed for another key,
/// and a signature of another key's over another object. The table names
/// each check by the object it is given, so no check of one object answers
/// for another.
#[test]
fn a_table_holding_a_true_check_answers_false_for_a_forged_signature() {
    let [key, other, _] = keys();
    let (object, another) = (
        Hash::new("1".repeat(64)).expect("a name"),
        Hash::new("2".repeat(64)).expect("a name"),
    );
    let honest = key.sign(&object);
    let carried = Signed {
        object: another.clone(),
        ..honest.clone()
    };
    let claimed = Signed {
        key: other.public(),
        ..honest.clone()
    };
    let swapped = Signed {
        object: object.clone(),
        ..other.sign(&another)
    };
    let table = Checks::new(16);
    assert!(table.verifies(&honest));
    for forged in [&carried, &claimed, &swapped] {
        assert!(!forged.verifies());
        assert!(!table.verifies(forged), "{forged:?}");
        assert!(!table.verifies(forged), "asked again: {forged:?}");
    }
    assert!(table.verifies(&honest));
}
