//! Law 70, the append and `verify` ask one question of every write, over
//! the floored review schema (`../schemas/floored.rs`), whose refusal holds
//! a rule that its order does not state, and over the proposal schema,
//! whose refusal is its order. The first write to an entity is asked
//! against the empty reading. Law 68 runs the floored schema through the
//! script machine with faults and without.

use prodrome::change::mk_change;
use prodrome::dag::Finding;
use prodrome::event::{canonical_envelope, seal_hash, Envelope, Hash};
use prodrome::laws::script::{self, a_script, an_honest_script, Kind, Script, Step, Who, Writing};
use prodrome::policy::Everything;
use prodrome::registers::heads;
use prodrome::schema::headed::Event;
use prodrome::schema::Schema;
use prodrome::store::{sync, MemoryStore, Replica};
use proptest::prelude::*;

use crate::declared::proposal::a_proposal;
use crate::review::{day, Phase, PHASES};
use crate::schema_laws::failed;
use crate::tags::{begun, genesis};

#[path = "../schemas/floored.rs"]
mod schema;

pub use schema::*;

/// The longest script the normal run draws.
const STEPS: usize = 40;

/// A move of one of two reviews to any phase, by `who`, on one of four
/// days. A quarter of the first writes to a review are merges, which the
/// append refuses.
fn a_floored_write(who: Who) -> impl Strategy<Value = Floored> {
    (0..2usize, prop::sample::select(PHASES.to_vec()), 1..5u32).prop_map(
        move |(review, phase, day_of)| {
            Event::of(
                ["r-1", "r-2"][review].to_owned(),
                day(day_of),
                who.actor(),
                Move(phase),
            )
        },
    )
}

fn runs(script: &Script<Floored>) -> Result<script::Ran, TestCaseError> {
    script::run(&Default::default(), script).map_err(failed)
}

/// Law 70 over `events`, appended in order to one store. After each append
/// that the store takes, `verify` finds nothing in the store. Each append
/// that the store refuses writes nothing. The refused write, built as a
/// change on its entity's heads and received by a copy of the store, is
/// the one write that `verify` finds in the copy.
fn the_append_refuses_what_verify_finds<E: Schema<Vocabulary: Default>>(
    events: Vec<E>,
) -> Result<(), TestCaseError> {
    let store = begun::<E>();
    let root = seal_hash(&genesis::<E>());
    for event in events {
        let held = store.held().expect("memory reads");
        let deps = heads(held.folded.stream(&held.dag.key(&root), event.key()));
        let Err(refused) = store.append(event.clone()) else {
            let found = store.held().expect("memory reads").dag.verify(&Everything);
            prop_assert!(found.is_empty(), "an append-only store finds {found:?}");
            continue;
        };
        let after = store.held().expect("memory reads");
        prop_assert_eq!(after.dag.objects().len(), held.dag.objects().len());
        let copy = MemoryStore::default();
        sync(&store, &copy).expect("syncs");
        let change = Envelope::Change(mk_change(root.clone(), deps, event).expect("a change"));
        let (name, print) = (seal_hash(&change), canonical_envelope(&change).into_bytes());
        copy.receive([name.clone()].into(), &|_| Some(print.clone()))
            .expect("a receipt takes a valid object");
        let found = copy.held().expect("memory reads").dag.verify(&Everything);
        prop_assert!(
            matches!(found.as_slice(), [Finding::Below { write, .. }] if *write == name),
            "the append refused {name} ({refused}), and verify finds {found:?}"
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// Law 70 over the floored review schema: the append refuses exactly
    /// the writes that `verify` finds, a merge as the first write to a
    /// review included.
    #[test]
    fn the_append_refuses_what_verify_finds_in_a_floored_review(
        events in prop::collection::vec(a_floored_write(Who::Ada), 0..16),
    ) {
        the_append_refuses_what_verify_finds(events)?;
    }

    /// Law 70 over the proposal schema, whose refusal is its order.
    #[test]
    fn the_append_refuses_what_verify_finds_in_a_proposal(
        events in prop::collection::vec(a_proposal(), 0..16),
    ) {
        the_append_refuses_what_verify_finds(events)?;
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Laws 68 and 70 over the floored review schema: every invariant holds
    /// after every step, with writes that do not stand on their reading
    /// among the faults, first writes included.
    #[test]
    fn every_step_of_a_floored_script_keeps_every_invariant(
        script in a_script(a_floored_write, STEPS),
    ) {
        runs(&script)?;
    }

    /// Laws 68 and 70: after every step of an honest floored script,
    /// `verify` finds nothing.
    #[test]
    fn an_honest_floored_script_verifies_clean(
        script in an_honest_script(a_floored_write, STEPS),
    ) {
        runs(&script)?;
    }
}

/// `phase`, written to the review `r-1` by ada on `day_of`.
fn moved(day_of: u32, phase: Phase) -> Floored {
    Event::of("r-1".to_owned(), day(day_of), Who::Ada.actor(), Move(phase))
}

/// The shortest script that shows a first write (law 70): ada merges a
/// review that nothing is written to. The empty reading holds no review, so
/// every kind of replica refuses the append, and `verify` finds nothing.
/// Written as a change and received (`Writing::Below`), the merge is the
/// one finding. After a review, the append takes the merge.
#[test]
fn a_merge_as_the_first_write_is_refused() {
    for writing in [Writing::Signed, Writing::Below] {
        for by in [Who::Ada, Who::Ben, Who::Cai] {
            let script = Script {
                kinds: [Kind::Memory, Kind::Disk, Kind::Overlay],
                steps: vec![Step::Append {
                    by,
                    of: Who::Ada,
                    event: moved(1, Phase::Merged),
                    writing,
                }],
            };
            runs(&script).expect("the script keeps every invariant");
        }
    }

    let store = begun::<Floored>();
    assert!(
        store.append(moved(1, Phase::Merged)).is_err(),
        "the append refuses a merge on the empty reading"
    );
    assert_eq!(store.held().expect("memory reads").dag.objects().len(), 1);
    let review: Hash = store.append(moved(1, Phase::Review)).expect("appends");
    let merge = store.append(moved(2, Phase::Merged)).expect("appends");
    assert_ne!(review, merge);
    assert!(store
        .held()
        .expect("memory reads")
        .dag
        .verify(&Everything)
        .is_empty());
}
