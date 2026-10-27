//! SPEC laws 36 and 37: what a writer saw is what its change rests on, and
//! supersession stays per register.
//!
//! Two writers over one genesis, each on its own replica in memory, append
//! events dated anywhere in the window (time is data, never order) and sync
//! in between. Every append rests on every write to its entity its writer
//! held, whatever register each writes; so a writer's own writes to one
//! entity stream in the order it wrote them, and a write that arrived
//! before another was written streams before it. And a register's frontier
//! is still its writes that no other write TO IT descends from: descending
//! from a write in another register supersedes nothing.
//!
//! The laws run over the todo schema and over the note schema of `note.rs`,
//! an application schema at the generic event whose product is
//! `fold::sole::One`.

use crate::common;

use std::collections::BTreeSet;

use prodrome::event::{canonical_envelope, mk_completed, seal_hash, Envelope, Hash, TodoEvent};
use prodrome::fold::{self as folds, Kind, Product};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::reference::{mk_authored, Todo};
use prodrome::registers::{Folded, Stamp};
use prodrome::schema::Schema;
use prodrome::store::{sync, Held, MemoryStore, Replica};
use proptest::prelude::*;

use crate::note::Note;
use common::{a_draft, moment, Draft, WINDOW};

type Event = TodoEvent<Todo>;

/// Two replicas of `E` holding one genesis and nothing else.
fn two_replicas<E: Schema<Vocabulary: Default>>() -> [MemoryStore<E>; 2] {
    let genesis: Envelope<E> =
        Envelope::Genesis(mk_genesis("causal", &"0".repeat(32)).expect("a genesis"));
    let name = seal_hash(&genesis);
    let print = canonical_envelope(&genesis).into_bytes();
    let begun = || {
        let memory = MemoryStore::default();
        memory
            .receive([name.clone()].into(), &|held| {
                (*held == name).then(|| print.clone())
            })
            .expect("receives its genesis");
        memory
    };
    [begun(), begun()]
}

fn folded<E: Schema>(replica: &MemoryStore<E>) -> Folded<E> {
    let Held { folded, .. } = replica.held().expect("reads");
    (*folded).clone()
}

/// The stream of `event`'s entity, in the replica's one prodrome.
fn stream<'f, E: Schema>(state: &'f Folded<E>, event: &E) -> &'f [Stamp<E>] {
    state
        .entities()
        .find(|(key, _)| *key == event.key())
        .map_or(&[], |(_, stream)| stream)
}

/// What a writer appends: an event of `E`, given its instant and a note
/// that makes the event new.
trait Draws<E>: Clone + std::fmt::Debug {
    fn event(&self, at: Datetime, note: &str) -> E;
}

impl Draws<Event> for Draft {
    fn event(&self, at: Datetime, note: &str) -> Event {
        self.at_with_note(at, note)
    }
}

/// A title for one of two notes, by `ada`. The note of the act makes the
/// title new.
#[derive(Debug, Clone)]
struct NoteDraft(&'static str);

impl Draws<Note> for NoteDraft {
    fn event(&self, at: Datetime, note: &str) -> Note {
        crate::note::by_ada(self.0, at, note)
    }
}

fn a_note_draft() -> impl Strategy<Value = NoteDraft> {
    prop::sample::select(vec!["n1", "n2"]).prop_map(NoteDraft)
}

/// One thing that happens to the two writers.
#[derive(Debug, Clone)]
enum Act<D> {
    /// A writer appends a draft, dated anywhere in the window.
    Append(usize, D, i64),
    /// Everything one writer holds, received by the other.
    Sync(usize),
}

fn an_act<D: std::fmt::Debug>(draft: impl Strategy<Value = D>) -> impl Strategy<Value = Act<D>> {
    prop_oneof![
        4 => (0usize..2, draft, 0i64..WINDOW)
            .prop_map(|(side, draft, at)| Act::Append(side, draft, at)),
        1 => (0usize..2).prop_map(Act::Sync),
    ]
}

/// The writes in `stream` to `register`, each kept unless another of them
/// descends from it: supersession per register, stated without the fold.
fn superseded_per_register<E: Schema>(stream: &[Stamp<E>], register: E::Register) -> Vec<Hash> {
    let writes: Vec<&Stamp<E>> = stream
        .iter()
        .filter(|stamp| stamp.event.writes().any(|r| r == register))
        .collect();
    let mut kept: Vec<Hash> = writes
        .iter()
        .filter(|w| !writes.iter().any(|later| later.descends(w)))
        .map(|w| w.name.clone())
        .collect();
    kept.sort();
    kept
}

/// Laws 36 and 37 over `acts`, at any schema.
fn rests_on_what_was_seen<E, D>(acts: &[Act<D>]) -> Result<(), TestCaseError>
where
    E: Schema<Vocabulary: Default>,
    D: Draws<E>,
{
    let replicas = two_replicas::<E>();
    // Each side's writes, in the order it wrote them, with what of the
    // entity it held when it wrote.
    let mut written: [Vec<(E, Hash)>; 2] = [Vec::new(), Vec::new()];
    let mut seen: Vec<(E, Hash, BTreeSet<Hash>)> = Vec::new();
    for (index, act) in acts.iter().enumerate() {
        match act {
            Act::Append(side, draft, at) => {
                let note = format!("{side}.{index}");
                let event = draft.event(moment(*at), &note);
                let held: BTreeSet<Hash> = stream(&folded(&replicas[*side]), &event)
                    .iter()
                    .map(|stamp| stamp.name.clone())
                    .collect();
                let name = replicas[*side].append(event.clone()).expect("appends");
                prop_assert!(!held.contains(&name), "every event here is new");
                let state = folded(&replicas[*side]);
                for earlier in &held {
                    prop_assert!(
                        state.descends(&name, earlier),
                        "{:?} rests on {:?}, which its writer held",
                        name,
                        earlier
                    );
                }
                written[*side].push((event.clone(), name.clone()));
                seen.push((event, name, held));
            }
            Act::Sync(from) => {
                sync(&replicas[*from], &replicas[1 - from]).expect("syncs");
            }
        }
    }
    sync(&replicas[0], &replicas[1]).expect("syncs");
    sync(&replicas[1], &replicas[0]).expect("syncs");
    let state = folded(&replicas[0]);
    prop_assert_eq!(&state, &folded(&replicas[1]));

    let place = |event: &E, name: &Hash| -> usize {
        stream(&state, event)
            .iter()
            .position(|stamp| stamp.name == *name)
            .expect("a write is in its entity's stream")
    };
    for side in &written {
        for (i, (event, name)) in side.iter().enumerate() {
            for (later, later_name) in &side[i + 1..] {
                if later.key() == event.key() {
                    prop_assert!(
                        place(event, name) < place(later, later_name),
                        "a writer's own writes stream in the order it wrote them"
                    );
                }
            }
        }
    }
    for (event, name, held) in &seen {
        for other in stream(&state, event) {
            prop_assert_eq!(
                state.descends(name, &other.name),
                held.contains(&other.name),
                "{:?} descends from {:?} exactly when its writer held it",
                name,
                other.name
            );
        }
    }
    for (_, stream) in state.entities() {
        let registers = folds::read(stream, None, &Everything);
        for register in &E::registers(&Default::default()) {
            prop_assert_eq!(
                registers.frontier(*register).names(),
                superseded_per_register(stream, *register),
                "{:?}",
                register
            );
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(48))]

    /// Law 36 — A CHANGE RESTS ON WHAT ITS WRITER SAW. Each append descends
    /// from every write to its entity the writer held, in any register, and
    /// from no other: happens-before is what the deps say, for one writer
    /// and across two.
    /// So in the stream, a writer's own writes to one entity come in the
    /// order it wrote them, whatever their instants and registers, and a
    /// write that reached a writer before it wrote comes before its write.
    ///
    /// Law 37 — SUPERSESSION IS PER REGISTER. In every stream, each
    /// register's frontier is its writes that no other write TO IT descends
    /// from; a write descending from a write in another register supersedes
    /// nothing there.
    #[test]
    fn a_change_rests_on_what_its_writer_saw(
        acts in prop::collection::vec(an_act(a_draft()), 1..40),
    ) {
        rests_on_what_was_seen::<Event, _>(&acts)?;
    }

    /// Laws 36 and 37 over the note schema: an application schema at the
    /// generic event, whose product is `fold::sole::One`.
    #[test]
    fn a_note_change_rests_on_what_its_writer_saw(
        acts in prop::collection::vec(an_act(a_note_draft()), 1..40),
    ) {
        rests_on_what_was_seen::<Note, _>(&acts)?;
    }
}

/// The case that asked for laws 36 and 37: one writer adds an entity and
/// then completes it, the completion stamped BEFORE the record. The
/// completion rests on the record, so it streams after it whatever the
/// instants say; and the record stays the content register's value, since
/// a state write supersedes nothing in the content register.
#[test]
fn an_add_then_a_complete_stream_in_the_order_written() {
    let [replica, _] = two_replicas::<Event>();
    let added = mk_authored(
        "alpha",
        moment(10),
        "bassel",
        "todo",
        moment(10),
        "write it",
        None,
        vec![],
        "",
        "",
        "",
        None,
        vec![],
        vec![],
        "",
    )
    .expect("valid");
    let completed = mk_completed("alpha", moment(9), "bassel", "").expect("valid");
    let add = replica.append(added.clone()).expect("appends");
    let complete = replica.append(completed).expect("appends");

    let state = folded(&replica);
    assert!(state.descends(&complete, &add));
    let stream = stream(&state, &added);
    let names: Vec<&Hash> = stream.iter().map(|stamp| &stamp.name).collect();
    assert_eq!(names, [&add, &complete]);
    let registers = folds::read(stream, None, &Everything);
    assert_eq!(registers.frontier(Kind::Content).names(), [add]);
    assert_eq!(registers.frontier(Kind::State).names(), [complete]);
}
