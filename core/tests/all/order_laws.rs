//! The register types' laws (`docs/design-register-types.md` §7): each
//! order is a partial order, over generated values; a register reads the
//! maximal values of its frontier, over generated DAGs; and an inflationary
//! register refuses a write that would make its reading fall, so that its
//! reading is a homomorphism, over two generated replicas.
//!
//! Beside the todo vocabulary's orders, the review schema's machine
//! (`review.rs`), so the laws see an order that is neither discrete, total
//! nor inclusion, and that is inflationary. Here its values ride in the notes
//! of todo state writes, so they reach a frontier through the same fold as
//! the todo's, and its appends are refused by the same [`grows`] the store's
//! append path calls; `review.rs` drives the same refusal through the append
//! path itself.
//!
//! The note schema (`note.rs`) reads its one register through
//! `fold::sole::One` under the discrete order, and law 3 runs over its
//! generated histories too.
//!
//! Law 6, that the todo vocabulary reads byte for byte as before, is every
//! vector suite beside this one, unchanged.

use std::collections::{BTreeMap, BTreeSet};

use prodrome::dag::Dag;
use prodrome::event::{mk_completed, seal_hash, Envelope, Hash, TodoEvent, TodoId};
use prodrome::fold::sole::One;
use prodrome::fold::{grows, maximal, read, Discrete, Frontier, Order, Total};
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::reference::Todo;
use prodrome::registers::{fold, Folded, Node, Stamp};
use proptest::prelude::*;

use crate::common::draw::{a_draw, history};
use crate::note::{Note, Title};
use crate::review::{Phase, PHASES};
use crate::schema_laws::a_note_event;

fn a_phase() -> impl Strategy<Value = Phase> {
    prop::sample::select(PHASES.to_vec())
}

/// Reflexive, antisymmetric and transitive at `a`, `b`, `c`.
fn is_partial_order<V: Order + PartialEq + std::fmt::Debug>(a: &V, b: &V, c: &V) {
    assert!(a.le(a), "reflexive at {a:?}");
    if a.le(b) && b.le(a) {
        assert_eq!(a, b, "antisymmetric");
    }
    if a.le(b) && b.le(c) {
        assert!(a.le(c), "transitive at {a:?} ≤ {b:?} ≤ {c:?}");
    }
}

fn a_set() -> impl Strategy<Value = BTreeSet<u8>> {
    prop::collection::btree_set(0u8..4, 0..4)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// Law 2: every order is a partial order.
    #[test]
    fn every_order_is_a_partial_order(
        discrete in prop::array::uniform3(0u8..3),
        total in prop::array::uniform3(0u8..3),
        sets in prop::array::uniform3(a_set()),
        phases in prop::array::uniform3(a_phase()),
    ) {
        let [a, b, c] = discrete.map(Discrete);
        is_partial_order(&a, &b, &c);
        let [a, b, c] = total.map(Total);
        is_partial_order(&a, &b, &c);
        let [a, b, c] = sets;
        is_partial_order(&a, &b, &c);
        let [a, b, c] = phases;
        is_partial_order(&a, &b, &c);
    }
}

#[test]
fn the_orders_are_the_ones_named() {
    assert!(!Discrete(1).le(&Discrete(2)) && !Discrete(2).le(&Discrete(1)));
    assert!(Total(1).le(&Total(2)) && !Total(2).le(&Total(1)));
    let (one, two, both) = (
        BTreeSet::from([1]),
        BTreeSet::from([2]),
        BTreeSet::from([1, 2]),
    );
    assert!(Order::le(&one, &both) && !Order::le(&one, &two));
    assert!(Phase::Draft.le(&Phase::Merged) && Phase::Review.le(&Phase::Merged));
    assert!(!Phase::Merged.le(&Phase::Closed) && !Phase::Closed.le(&Phase::Merged));
}

/// One write of a generated DAG: the value it carries (an index into each
/// order's values), a day among two (so equal writes are twins), and the
/// earlier writes it rests on.
type Step = (u8, u32, u64);

fn a_dag() -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec((0u8..4, 0u32..2, any::<u64>()), 1..10)
}

/// A replica: legacy objects of one todo, by name.
type Objects = BTreeMap<Hash, Envelope<TodoEvent<Todo>>>;

/// A state write of `value`, carried in its note, over `parents`: a legacy
/// object, `Sealed` on none or one and `Woven` on more, under its own name.
fn object(value: u8, day: u32, mut parents: Vec<Hash>) -> (Hash, Envelope<TodoEvent<Todo>>) {
    let at = Datetime::new(2026, 9, 1 + day, 12, 0, 0, 0).expect("a real instant");
    let event = mk_completed("alpha", at, "writer", &value.to_string()).expect("valid");
    parents.sort();
    parents.dedup();
    let object = if parents.len() < 2 {
        Envelope::Sealed {
            prev: parents.pop(),
            event,
        }
    } else {
        Envelope::Woven {
            parents,
            event: Some(event),
        }
    };
    (seal_hash(&object), object)
}

/// A replica's objects as the fold reads them: through its DAG, the one
/// thing that makes a node.
fn placed(objects: &Objects) -> Vec<Node<TodoEvent<Todo>>> {
    objects
        .iter()
        .map(|(name, object)| (name.clone(), object.clone()))
        .collect::<Dag<TodoEvent<Todo>>>()
        .nodes()
        .expect("a replica is closed under parents")
}

/// The steps as legacy objects of one todo: the `i`th rests on the earlier
/// steps its bits name.
fn nodes(dag: &[Step]) -> Vec<Node<TodoEvent<Todo>>> {
    let mut names: Vec<Hash> = Vec::new();
    let mut objects = Objects::new();
    for (i, (value, day, parents)) in dag.iter().enumerate() {
        let parents = (0..i.min(64)).filter(|j| parents >> j & 1 == 1);
        let (name, object) = object(*value, *day, parents.map(|j| names[j].clone()).collect());
        names.push(name.clone());
        objects.insert(name, object);
    }
    placed(&objects)
}

fn value(stamp: &Stamp<TodoEvent<Todo>>) -> u8 {
    match &*stamp.event {
        TodoEvent::Completed(e) => e.note.parse().expect("a value"),
        _ => unreachable!("every write is a completion"),
    }
}

/// The todo's state frontier, read structurally.
fn frontier(folded: &Folded<TodoEvent<Todo>>) -> Frontier<'_, TodoEvent<Todo>> {
    let todo = TodoId::new("alpha").expect("valid");
    folded
        .prodromes()
        .get(&None)
        .and_then(|todos| todos.get(&todo))
        .map(|stream| prodrome::fold::read(stream, None, &Everything).state)
        .unwrap_or_default()
}

/// Law 3 at one frontier under `order`: the reading is exactly the maximal
/// values, each once, and one value exactly when one is greatest.
fn is_completion<V: Order + std::fmt::Debug>(
    frontier: &Frontier<TodoEvent<Todo>>,
    order: impl Fn(u8) -> V,
) {
    let of = |stamp: &Stamp<TodoEvent<Todo>>| order(value(stamp));
    let all: Vec<V> = frontier.writes().iter().map(|s| of(s)).collect();
    let read: Vec<V> = frontier.read(of).into_iter().map(of).collect();
    let maximal = |v: &V| !all.iter().any(|w| v.le(w) && !w.le(v));
    let equal = |v: &V, w: &V| v.le(w) && w.le(v);
    assert!(
        read.iter().all(maximal),
        "only maximal values: {read:?} of {all:?}"
    );
    for v in all.iter().filter(|v| maximal(v)) {
        assert_eq!(read.iter().filter(|w| equal(v, w)).count(), 1, "{v:?} once");
    }
    let greatest = all.iter().any(|g| all.iter().all(|v| v.le(g)));
    assert_eq!(read.len() == 1, greatest, "one value iff one is greatest");
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// Law 3: a reading is the set of maximal values of its frontier's
    /// values, under every order.
    #[test]
    fn a_reading_is_the_maximal_values(dag in a_dag()) {
        let folded = fold(&nodes(&dag));
        let frontier = frontier(&folded);
        is_completion(&frontier, Discrete);
        is_completion(&frontier, Total);
        is_completion(&frontier, |i| PHASES[usize::from(i)]);
    }

    /// Under the discrete order on events the reading is the candidates as
    /// they always were: the distinct events, in name order, each named by
    /// the least write that carries it.
    #[test]
    fn the_discrete_reading_is_the_candidates(dag in a_dag()) {
        let folded = fold(&nodes(&dag));
        let frontier = frontier(&folded);
        let mut twins_once: Vec<&Stamp<TodoEvent<Todo>>> = Vec::new();
        for stamp in frontier.writes() {
            if !twins_once.iter().any(|held| held.event == stamp.event) {
                twins_once.push(stamp);
            }
        }
        prop_assert_eq!(frontier.candidates(), twins_once);
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// Law 31, over the note schema: the reading of `fold::sole::One` is
    /// the discrete reading of its writes that no other write to the note
    /// descends from. The test states that frontier without the fold: the
    /// distinct events in name order, each named by the least write that
    /// carries it.
    #[test]
    fn a_note_reading_is_the_candidates(draw in a_draw(a_note_event())) {
        let objects: Dag<Note> = history(&draw.events).into_iter().collect();
        let folded = fold(&objects.nodes().expect("a DAG"));
        for (key, stream) in folded.entities() {
            let mut frontier: Vec<&Stamp<Note>> = stream
                .iter()
                .filter(|write| !stream.iter().any(|later| later.descends(write)))
                .collect();
            frontier.sort_by(|a, b| a.name.cmp(&b.name));
            let mut twins_once: Vec<&Stamp<Note>> = Vec::new();
            for stamp in frontier {
                if !twins_once.iter().any(|held| held.event == stamp.event) {
                    twins_once.push(stamp);
                }
            }
            let registers: One<'_, Title> = read(stream, None, &Everything);
            prop_assert_eq!(registers.reading(), twins_once, "{:?}", key);
        }
    }
}

fn phase(stamp: &Stamp<TodoEvent<Todo>>) -> Phase {
    PHASES[usize::from(value(stamp))]
}

/// The machine's reading of a replica's objects.
fn reading(objects: &Objects) -> Vec<Phase> {
    let folded = fold(&placed(objects));
    let frontier = frontier(&folded);
    frontier.read(phase).into_iter().map(phase).collect()
}

/// Two replicas' objects together.
fn union(a: &Objects, b: &Objects) -> Objects {
    a.iter()
        .chain(b)
        .map(|(name, object)| (name.clone(), object.clone()))
        .collect()
}

/// Two antichains as sets.
fn same(a: &[Phase], b: &[Phase]) -> bool {
    a.len() == b.len() && a.iter().all(|v| b.contains(v))
}

/// One step of two replicas: replica `.0` appends the phase `.1` indexes,
/// or with `None` adopts everything the other holds.
type Act = (usize, Option<u8>);

fn acts() -> impl Strategy<Value = Vec<Act>> {
    prop::collection::vec((0usize..2, prop::option::weighted(0.8, 0u8..4)), 0..16)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// Law 4: an append whose value is not `≥` the reading it supersedes is
    /// refused before any object carries it; every other is written over
    /// the frontier it supersedes. Over every history so written, the
    /// reading of a union is the join of the readings:
    /// `read(h₁ ∪ h₂) = max(read(h₁) ∪ read(h₂))`.
    #[test]
    fn an_inflationary_reading_is_a_homomorphism(acts in acts()) {
        let mut replicas: [Objects; 2] = [Objects::new(), Objects::new()];
        for (side, act) in acts {
            match act {
                Some(index) => {
                    let value = PHASES[usize::from(index)];
                    let held = reading(&replicas[side]);
                    let refused = grows(&held, &value).is_err();
                    prop_assert_eq!(refused, !held.iter().all(|v| v.le(&value)));
                    if !refused {
                        let folded = fold(&placed(&replicas[side]));
                        let deps = frontier(&folded).names();
                        let (name, object) = object(index, 0, deps);
                        replicas[side].insert(name, object);
                    }
                }
                None => replicas[side] = union(&replicas[0], &replicas[1]),
            }
        }
        let [h1, h2] = &replicas;
        let joined: Vec<Phase> = [reading(h1), reading(h2)].concat();
        prop_assert!(same(&reading(&union(h1, h2)), &maximal(&joined, |p| p)));
    }
}

#[test]
fn a_write_that_goes_back_is_refused() {
    use Phase::*;
    assert!(grows(&[], &Draft).is_ok());
    assert!(grows(&[Draft, Review], &Merged).is_ok());
    assert!(grows(&[Merged], &Merged).is_ok());
    assert!(grows(&[Merged], &Closed).is_err());
    assert!(grows(&[Review], &Draft).is_err());
    assert!(grows(&[Merged, Closed], &Closed).is_err());
}

#[test]
fn the_completion_keeps_the_first_of_equal_values_in_order() {
    use Phase::*;
    let phases = [Review, Closed, Draft, Merged, Closed];
    assert_eq!(maximal(&phases, |p| p), [Closed, Merged]);
    assert_eq!(maximal(&[3, 1, 3], Total), [3]);
    assert_eq!(maximal(&[3, 1, 3], Discrete), [3, 1]);
    assert!(maximal(&[] as &[u8], Discrete).is_empty());
}
