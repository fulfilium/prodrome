//! Law 32 for any object set: an inflationary register reads the greatest
//! of all its writes, so its reading is a function of the set of objects,
//! and it never falls as objects arrive, whoever wrote them. An append
//! refuses a write below the reading, a receipt takes any valid object, and
//! `verify` finds a write below what it rests on.
//!
//! The histories here are drawn with any deps (`common::draw`), so a write
//! can rest on a higher one. The proposal schema (`../schemas/proposal.rs`)
//! has two inflationary registers, its state under a partial order and its
//! tags under inclusion, and two that may go back.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use prodrome::dag::{Dag, Finding};
use prodrome::event::{canonical_envelope, seal_hash, Envelope, Hash};
use prodrome::fold::sole::{One, Only, Sole};
use prodrome::fold::{maximal, read, Climb, Inflationary, Order, Product, RegisterType};
use prodrome::genesis::mk_genesis;
use prodrome::laws::script::{self, a_script, an_honest_script, Kind, Script, Step, Who, Writing};
use prodrome::laws::Broken;
use prodrome::literal::codec::Fields;
use prodrome::literal::{Datetime, ProdromeError, Value};
use prodrome::policy::{Everything, Policy, Standing, Untrusted};
use prodrome::registers::{fold, Folded, Stamp};
use prodrome::schema::headed::{Body, Event as HeadedEvent};
use prodrome::store::{sync, Held, MemoryStore, Replica};
use proptest::prelude::*;

use crate::common::draw::{a_draw, history};
use crate::declared::proposal::{
    a_proposal, Field, Id, Proposal, ProposalVocabulary, Says, State, StateRegister, TagsRegister,
};
use crate::schema_laws::failed;

/// A history's objects.
type Objects = BTreeMap<Hash, Envelope<Proposal>>;

/// What one proposal reads in its two inflationary registers, as values.
#[derive(Debug, Clone, PartialEq)]
struct Values {
    state: Vec<State>,
    tags: Vec<BTreeSet<String>>,
}

fn tags_of(stamp: &Stamp<Proposal>) -> BTreeSet<String> {
    TagsRegister::value(&stamp.event)
        .expect("a write to the tags")
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn state_of(stamp: &Stamp<Proposal>) -> State {
    StateRegister::value(&stamp.event).expect("a write to the state")
}

/// The fold of `objects`.
fn folded(objects: &Objects) -> Folded<Proposal> {
    let dag: Dag<Proposal> = objects
        .iter()
        .map(|(name, object)| (name.clone(), object.clone()))
        .collect();
    fold(&dag.nodes().expect("a history"))
}

/// Each proposal's readings of its inflationary registers in `objects`.
fn readings(objects: &Objects) -> BTreeMap<Id, Values> {
    folded(objects)
        .entities()
        .map(|(key, stream)| {
            let registers = read(stream, None, &Everything);
            let values = Values {
                state: registers
                    .reading(Field::State)
                    .into_iter()
                    .map(state_of)
                    .collect(),
                tags: registers
                    .reading(Field::Tags)
                    .into_iter()
                    .map(tags_of)
                    .collect(),
            };
            (key.clone(), values)
        })
        .collect()
}

/// The maximal values among `values`, one for each value.
fn greatest<V: Order + Clone>(values: &[V]) -> Vec<V> {
    let at: Vec<usize> = (0..values.len()).collect();
    maximal(&at, |i| values[i].clone())
        .into_iter()
        .map(|i| values[i].clone())
        .collect()
}

/// Two sets of values given as lists, without order.
fn same<V: PartialEq>(a: &[V], b: &[V]) -> bool {
    a.len() == b.len() && a.iter().all(|v| b.contains(v))
}

/// Each proposal's greatest values over every write in `objects`, computed
/// from the writes alone.
fn expected(objects: &Objects) -> BTreeMap<Id, Values> {
    let mut all: BTreeMap<Id, (Vec<State>, Vec<BTreeSet<String>>)> = BTreeMap::new();
    for event in objects.values().filter_map(Envelope::event) {
        let entry = all.entry(event.proposal.clone()).or_default();
        match &event.says {
            Says::Moved(state) => entry.0.push(*state),
            Says::Tagged(tags) => entry.1.push(tags.iter().cloned().collect()),
            Says::Proposed(_) | Says::Attempted(_) => {}
        }
    }
    all.into_iter()
        .map(|(key, (state, tags))| {
            (
                key,
                Values {
                    state: greatest(&state),
                    tags: greatest(&tags),
                },
            )
        })
        .collect()
}

fn agree(got: &BTreeMap<Id, Values>, want: &BTreeMap<Id, Values>) -> bool {
    got.len() == want.len()
        && got.iter().all(|(key, got)| {
            want.get(key)
                .is_some_and(|want| same(&got.state, &want.state) && same(&got.tags, &want.tags))
        })
}

/// The objects of `history` that `seeds` (positions) rest on, the seeds
/// included.
fn down_set(history: &[(Hash, Envelope<Proposal>)], seeds: &[usize]) -> Objects {
    let dag: Dag<Proposal> = history.iter().cloned().collect();
    let held = dag.closure(seeds.iter().map(|at| history[at % history.len()].0.clone()));
    history
        .iter()
        .filter(|(name, _)| held.contains(name))
        .cloned()
        .collect()
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// Law 32: an inflationary register reads the maximal values of ALL
    /// its writes, in any history, written honestly or not.
    #[test]
    fn an_inflationary_register_reads_the_greatest_of_all_its_writes(
        draw in a_draw(a_proposal()),
    ) {
        let objects: Objects = history(&draw.events).into_iter().collect();
        prop_assert!(agree(&readings(&objects), &expected(&objects)));
    }

    /// Law 32: for any two down-sets `A` and `B` of a history, honest or
    /// not, `read(A ∪ B) = max(read(A) ∪ read(B))`, and the reading of
    /// `A ∪ B` stands at or above the reading of `A` ([`Product::climbed`]).
    #[test]
    fn an_inflationary_reading_is_a_homomorphism_of_any_object_sets(
        draw in a_draw(a_proposal()),
        a in prop::collection::vec(0..64usize, 0..4),
        b in prop::collection::vec(0..64usize, 0..4),
    ) {
        let history = history(&draw.events);
        let (a, b) = (down_set(&history, &a), down_set(&history, &b));
        let union: Objects = a.iter().chain(&b).map(|(n, o)| (n.clone(), o.clone())).collect();
        let (read_a, read_b, read_union) = (readings(&a), readings(&b), readings(&union));
        let mut joined = read_a.clone();
        for (key, values) in &read_b {
            let entry = joined.entry(key.clone()).or_insert_with(|| Values {
                state: Vec::new(),
                tags: Vec::new(),
            });
            entry.state = greatest(&[entry.state.clone(), values.state.clone()].concat());
            entry.tags = greatest(&[entry.tags.clone(), values.tags.clone()].concat());
        }
        prop_assert!(agree(&read_union, &joined));
        let (then, now) = (folded(&a), folded(&union));
        for (key, earlier) in then.entities() {
            let (_, later) = now.entities().find(|(other, _)| *other == key).expect("held");
            let climbed = read(later, None, &Everything).climbed(&read(earlier, None, &Everything));
            prop_assert!(climbed.is_ok(), "{climbed:?}");
        }
    }

    /// Law 32: `verify` finds exactly the writes to an inflationary
    /// register whose value is not at or above the value of every write to
    /// that register that they rest on, each once.
    #[test]
    fn verify_finds_each_write_below_what_it_rests_on(draw in a_draw(a_proposal())) {
        let objects: Objects = history(&draw.events).into_iter().collect();
        let dag: Dag<Proposal> = objects.clone().into_iter().collect();
        let mut below = BTreeSet::new();
        for (name, object) in &objects {
            let Some(event) = object.event() else { continue };
            let beneath = dag.closure([name.clone()]);
            let lower = beneath.iter().filter(|other| *other != name).filter_map(|other| {
                objects.get(other).and_then(Envelope::event)
            });
            let falls = lower.filter(|other| other.proposal == event.proposal).any(|other| {
                match (&other.says, &event.says) {
                    (Says::Moved(then), Says::Moved(now)) => !then.le(now),
                    (Says::Tagged(then), Says::Tagged(now)) => {
                        let then: BTreeSet<&String> = then.iter().collect();
                        let now: BTreeSet<&String> = now.iter().collect();
                        !then.is_subset(&now)
                    }
                    (Says::Moved(_) | Says::Tagged(_) | Says::Proposed(_) | Says::Attempted(_), _) => false,
                }
            });
            if falls {
                below.insert(name.clone());
            }
        }
        let found: Vec<Hash> = dag
            .verify(&Everything)
            .into_iter()
            .filter_map(|finding| {
                if let Finding::Below { write, .. } = finding {
                    Some(write)
                } else {
                    None
                }
            })
            .collect();
        prop_assert_eq!(found.len(), below.len(), "each once");
        prop_assert_eq!(found.into_iter().collect::<BTreeSet<_>>(), below);
    }
}

fn day(day: u32) -> Datetime {
    Datetime::new(2026, 10, day, 12, 0, 0, 0).expect("a real instant")
}

fn moved(by: Who, day_of: u32, state: State) -> Proposal {
    Proposal {
        proposal: Id("p-1".to_owned()),
        at: day(day_of),
        actor: by.actor(),
        says: Says::Moved(state),
    }
}

/// A store in memory that holds one genesis.
fn begun() -> MemoryStore<Proposal> {
    let genesis: Envelope<Proposal> =
        Envelope::Genesis(mk_genesis("proposals", &"ef".repeat(16)).expect("a genesis"));
    let print = canonical_envelope(&genesis).into_bytes();
    let store = MemoryStore::default();
    store
        .receive([seal_hash(&genesis)].into(), &|_| Some(print.clone()))
        .expect("begins");
    store
}

/// A proposal event that `who` writes, as `a_proposal` draws it.
fn a_proposal_write(who: Who) -> impl Strategy<Value = Proposal> {
    a_proposal().prop_map(move |proposal| Proposal {
        actor: who.actor(),
        ..proposal
    })
}

/// One step of two replicas: the replica `.0` appends `.1`, or, with
/// `None`, receives everything the other holds.
fn acts() -> impl Strategy<Value = Vec<(usize, Option<Proposal>)>> {
    prop::collection::vec(
        (
            0usize..2,
            prop::option::weighted(0.8, a_proposal_write(Who::Ada)),
        ),
        0..24,
    )
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// Law 32: no honest reading moves. In a history that only appends
    /// wrote, where each write stands at or above what it rests on, the
    /// reading of every inflationary register is the reading of its
    /// frontier, write for write: what the fold read before an inflationary
    /// register read all its writes.
    #[test]
    fn an_honest_reading_is_the_frontier_reading(acts in acts()) {
        let replicas = [begun(), MemoryStore::default()];
        sync(&replicas[0], &replicas[1]).expect("syncs");
        for (side, act) in acts {
            match act {
                Some(event) => {
                    let _ = replicas[side].append(event);
                }
                None => {
                    sync(&replicas[1 - side], &replicas[side]).expect("syncs");
                }
            }
        }
        for replica in &replicas {
            let held = replica.held().expect("reads");
            prop_assert!(held.dag.verify(&Everything).is_empty());
            for (_, stream) in held.folded.entities() {
                let registers = read(stream, None, &Everything);
                let names = |stamps: Vec<&Stamp<Proposal>>| -> Vec<Hash> {
                    stamps.into_iter().map(|stamp| stamp.name.clone()).collect()
                };
                prop_assert_eq!(
                    names(registers.reading(Field::State)),
                    names(registers.frontier(Field::State).reading::<StateRegister>())
                );
                prop_assert_eq!(
                    names(registers.reading(Field::Tags)),
                    names(registers.frontier(Field::Tags).reading::<TagsRegister>())
                );
            }
        }
    }
}

/// The longest script the normal run draws.
const STEPS: usize = 40;

fn runs(script: &Script<Proposal>) -> Result<script::Ran, TestCaseError> {
    script::run(&ProposalVocabulary, script).map_err(failed)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 68 over the proposal schema, whose state and tags are
    /// inflationary: every invariant holds after every step, with writes
    /// below their reading among the faults (law 32).
    #[test]
    fn every_step_of_a_proposal_script_keeps_every_invariant(
        script in a_script(a_proposal_write, STEPS),
    ) {
        runs(&script)?;
    }

    /// Law 68: after every step of an honest proposal script, `verify`
    /// finds nothing.
    #[test]
    fn an_honest_proposal_script_verifies_clean(
        script in an_honest_script(a_proposal_write, STEPS),
    ) {
        runs(&script)?;
    }
}

/// The shortest script for a write below its reading (law 32). Ada
/// dismisses a proposal, then writes "tried once, not sent" (`failed`) over
/// the dismissal, which `failed` does not stand above. The append refuses
/// it; the same event, received as a change whose deps name the dismissal,
/// is taken. `failed` and `dismissed` are incomparable, so the greatest
/// values of the two writes are both: the reading keeps `dismissed`, with
/// `failed` beside it as a conflict, and `verify` finds the write below.
/// A reading of the frontier alone would read `failed` and nothing else,
/// because the write below descends from the dismissal.
#[test]
fn a_write_below_its_reading_is_taken_and_does_not_lower_it() {
    let dismissed = moved(Who::Ada, 1, State::Dismissed);
    let failed = moved(Who::Ada, 2, State::Failed);
    let script = Script {
        kinds: [Kind::Memory, Kind::Memory, Kind::Memory],
        steps: vec![
            Step::Append {
                by: Who::Ada,
                of: Who::Ada,
                event: dismissed.clone(),
                writing: Writing::Signed,
            },
            Step::Append {
                by: Who::Ada,
                of: Who::Ada,
                event: failed.clone(),
                writing: Writing::Below,
            },
        ],
    };
    runs(&script).expect("the script keeps every invariant");

    let store = begun();
    let over = store.append(dismissed).expect("appends");
    assert!(store.append(failed.clone()).is_err(), "the append refuses");
    let genesis = store.held().expect("reads").dag.geneses();
    let genesis = genesis.first().expect("one genesis").clone();
    let change = Envelope::Change(
        prodrome::change::mk_change(genesis, vec![over], failed).expect("a change"),
    );
    let print = canonical_envelope(&change).into_bytes();
    let name = seal_hash(&change);
    assert_eq!(
        store
            .receive([name.clone()].into(), &|_| Some(print.clone()))
            .expect("the receipt takes a valid object"),
        vec![name.clone()]
    );
    let held = store.held().expect("reads");
    let objects: Objects = held
        .dag
        .objects()
        .iter()
        .map(|(n, o)| (n.clone(), o.clone()))
        .collect();
    assert_eq!(
        readings(&objects)[&Id("p-1".to_owned())].state,
        vec![State::Dismissed, State::Failed],
        "the reading keeps the dismissal"
    );
    assert!(matches!(
        held.dag.verify(&Everything).as_slice(),
        [Finding::Below { write, .. }] if *write == name
    ));
}

const RAISED: prodrome::literal::codec::Constructor =
    ("Raised", &["level", "at", "actor", "height"]);

/// A LEVEL schema: one register, a height that only rises, held through
/// `fold::sole::One` in a `Climb` ([`Sole::Held`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Height(u64);

impl Order for Height {
    fn le(&self, other: &Self) -> bool {
        self.0 <= other.0
    }
}

impl Inflationary for Height {}

type Level = HeadedEvent<Height>;

/// The register type of the height.
struct Rises;

impl RegisterType<Level> for Rises {
    type Value<'e> = Height;

    fn value(event: &Level) -> Option<Height> {
        Some(*event.body())
    }
}

impl Sole for Rises {
    type Schema = Level;
    type Held<'a> = Climb<'a, Level>;
}

impl Body for Height {
    type Key = String;
    type Register = Only;
    type Registers<'a> = One<'a, Rises>;
    type Refusal = std::convert::Infallible;

    const EVENTS: &'static [prodrome::literal::codec::Constructor] = &[RAISED];

    fn writes(&self) -> impl Iterator<Item = Only> {
        std::iter::once(Only)
    }

    fn fields(&self) -> (prodrome::literal::codec::Constructor, Vec<Value>) {
        (
            RAISED,
            vec![prodrome::literal::codec::Literal::to_value(&self.0)],
        )
    }

    fn parse(fields: &Fields<'_>) -> Result<Level, ProdromeError> {
        Ok(HeadedEvent::of(
            fields.get("level")?,
            fields.get("at")?,
            fields.get("actor")?,
            Height(fields.get("height")?),
        ))
    }
}

fn raised(day_of: u32, height: u64) -> Level {
    HeadedEvent::of(
        "l-1".to_owned(),
        day(day_of),
        Who::Ada.actor(),
        Height(height),
    )
}

/// Law 32 through `fold::sole::One`: a sole register whose type holds it
/// in a `Climb` reads the greatest of all its writes, and its append
/// refuses a write below that reading.
#[test]
fn a_sole_inflationary_register_reads_the_greatest_of_all_its_writes() {
    let objects =
        prodrome::laws::history(&[(raised(1, 3), 0), (raised(2, 1), 1)]).expect("a history");
    let dag: Dag<Level> = objects.into_iter().collect();
    let folded = fold(&dag.nodes().expect("a history"));
    let (_, stream) = folded.entities().next().expect("one level");
    let registers = read(stream, None, &Everything);
    let heights: Vec<Height> = registers
        .reading(Only)
        .into_iter()
        .map(|stamp| *stamp.event.body())
        .collect();
    assert_eq!(heights, [Height(3)], "the lower write does not lower it");
    assert_eq!(
        registers.frontier(Only).writes().len(),
        1,
        "the frontier is the lower write"
    );
    assert!(registers.grows(&raised(3, 2)).is_err());
    assert!(registers.grows(&raised(3, 3)).is_ok());
    assert!(matches!(
        dag.verify(&Everything).as_slice(),
        [Finding::Below { .. }]
    ));
}

/// A host's own invariant, checked by the script machine after every step
/// ([`script::run_with`]): in an honest script, each replica reads every
/// inflationary register as its frontier reads it (law 32).
fn reads_its_frontier(who: Who, held: &Held<Proposal>) -> Result<(), Broken> {
    for (key, stream) in held.folded.entities() {
        let registers = read(stream, None, &Everything);
        let names = |stamps: Vec<&Stamp<Proposal>>| -> Vec<Hash> {
            stamps.into_iter().map(|stamp| stamp.name.clone()).collect()
        };
        let state = names(registers.frontier(Field::State).reading::<StateRegister>());
        let tags = names(registers.frontier(Field::Tags).reading::<TagsRegister>());
        if names(registers.reading(Field::State)) != state
            || names(registers.reading(Field::Tags)) != tags
        {
            return Err(Broken {
                law: 32,
                what: format!("{}: {key:?} does not read its frontier", who.name()),
            });
        }
    }
    Ok(())
}

/// A policy that binds everything and counts the writes it was asked
/// about.
#[derive(Debug, Clone, Default)]
struct Counted(Rc<Cell<usize>>);

impl Policy<Proposal> for Counted {
    fn standing(&self, _: &Hash, _: &Proposal) -> Standing {
        self.0.set(self.0.get() + 1);
        Standing::Binds
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// Law 68 with a host's check after every step: no honest proposal
    /// script moves a reading from what its frontier reads (law 32).
    #[test]
    fn a_host_checks_its_own_invariant_after_every_step(
        script in an_honest_script(a_proposal_write, STEPS),
    ) {
        script::run_with(&ProposalVocabulary, &script, &Everything, reads_its_frontier)
            .map_err(failed)?;
    }

    /// Law 68 under a host's policy, where only `ada` and `cai` bind: every
    /// invariant holds, the claimed reading being the policy's.
    #[test]
    fn a_script_keeps_every_invariant_under_a_host_policy(
        script in a_script(a_proposal_write, STEPS),
    ) {
        let policy = Untrusted::of([Who::Ben.actor()]);
        script::run_with(&ProposalVocabulary, &script, &policy, |_, _| Ok(()))
            .map_err(failed)?;
    }
}

/// The host's check sees each actor's replica after every step, and its
/// refusal stops the script at that step; the claimed reading is read
/// under the host's policy.
#[test]
fn a_host_refusal_stops_the_script_at_its_step() {
    let script = Script {
        kinds: [Kind::Memory, Kind::Overlay, Kind::Disk],
        steps: vec![
            Step::Append {
                by: Who::Ben,
                of: Who::Ben,
                event: moved(Who::Ben, 1, State::Unknown),
                writing: Writing::Signed,
            },
            Step::SyncAll,
        ],
    };
    let mut seen = BTreeSet::new();
    let broken = script::run_with(&ProposalVocabulary, &script, &Everything, |who, held| {
        seen.insert(who);
        if who == Who::Cai && held.dag.objects().len() > 1 {
            Err(Broken {
                law: 0,
                what: "cai holds a write".to_owned(),
            })
        } else {
            Ok(())
        }
    })
    .expect_err("the host refuses");
    assert_eq!(seen, Who::ALL.into_iter().collect());
    assert_eq!(broken.law, 0);
    assert_eq!(broken.what, "at step 1: cai holds a write");

    let policy = Counted::default();
    script::run_with(&ProposalVocabulary, &script, &policy, |_, _| Ok(()))
        .expect("the script keeps every invariant");
    assert!(
        policy.0.get() > 0,
        "the machine reads under the host's policy"
    );
}
