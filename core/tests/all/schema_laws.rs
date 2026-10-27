//! Law 1 of `docs/design-register-types.md` §7, FREE: a reading is a
//! function of the object set. Every permutation of a history that keeps
//! parents first, every duplication, and every partition into two replicas
//! folded apart and then merged reads the same, register by register.
//!
//! Stated once, generic over a schema, and run over the todo schema, the
//! review schema of `review.rs` and the note schema of `note.rs`: the law
//! asks nothing of a schema, so none may break it. The note schema is an
//! application schema at the generic event (`schema::envelope`), so the law
//! covers the envelope and `fold::sole::One` too.
//!
//! Law 5, VALUATION: a reading's price is `Least` over the terms its
//! candidates price as, and `Least` is their meet in the fulfillment order.
//! Stated over [`Price`] and run over both schemas; the todo's price is also
//! a [`History`], whose moment with nothing first written is its reading.
//!
//! Beside them, the one thing a schema says twice: the route's NAMES
//! (`Schema::writes`) and the route itself (`Product::join`, and each
//! register type's `value`) agree, so a write joins exactly the frontiers its
//! event names.

use std::collections::{BTreeMap, BTreeSet};

use prodrome::dag::Dag;
use prodrome::event::{
    mk_cancelled, mk_completed, mk_created, mk_reopened, mk_spec_revised, mk_tended, Hash,
    TodoEvent,
};
use prodrome::fold::{price, read, Kind, Product, RegisterType};
use prodrome::fpl::{self, mk_flat, Closed, Env};
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::reference::{mk_authored, Todo};
use prodrome::registers::{extend, fold, Folded, Genesis, Node};
use prodrome::schema::{History, Price, Schema};
use prodrome::term::Term;
use proptest::prelude::*;

use crate::common::draw::{a_draw, history, Draw};

use prodrome::todo::{Content, Spec, State};

use crate::note::{self, Note};
use crate::review::{self, Field, Phase, PhaseRegister, Review, PHASES};

/// Every register's frontier and its reading by name ([`Product::reading`]),
/// for every entity: what the law is about, compared by object names because
/// a fold's positions are its own business.
type Reading<E> = BTreeMap<(Genesis, <E as Schema>::Key), Vec<(Vec<Hash>, Vec<Hash>)>>;

fn reading<E: Schema>(state: &Folded<E>) -> Reading<E> {
    let mut out = BTreeMap::new();
    for (genesis, prodrome) in state.prodromes() {
        for (key, stream) in prodrome {
            let registers = read(stream, None, &Everything);
            let frontiers = registers
                .registers()
                .into_iter()
                .map(|register| {
                    let read = registers.reading(register);
                    let names = read.iter().map(|stamp| stamp.name.clone()).collect();
                    (registers.frontier(register).names(), names)
                })
                .collect();
            out.insert((genesis.clone(), key.clone()), frontiers);
        }
    }
    out
}

/// `nodes` in the order their ranks give, parents first.
fn ordered<E: Schema>(nodes: &[Node<E>], ranks: &[u32]) -> Vec<Node<E>> {
    let mut placed: BTreeSet<&Hash> = BTreeSet::new();
    let mut left: Vec<(u32, &Node<E>)> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (ranks.get(i).copied().unwrap_or(0), node))
        .collect();
    let mut out = Vec::with_capacity(nodes.len());
    while !left.is_empty() {
        let ready = left
            .iter()
            .enumerate()
            .filter(|(_, (_, node))| {
                node.parents()
                    .iter()
                    .all(|p| placed.contains(p) || !nodes.iter().any(|n| n.name() == p))
            })
            .min_by_key(|(_, (rank, node))| (*rank, node.name()))
            .map(|(i, _)| i)
            .expect("a DAG always has a ready node");
        let (_, node) = left.remove(ready);
        placed.insert(node.name());
        out.push(node.clone());
    }
    out
}

/// Law 1 at one draw.
pub(crate) fn free<E: Schema>(draw: &Draw<E>) -> Result<(), TestCaseError> {
    let dag: Dag<E> = history(&draw.events).into_iter().collect();
    let nodes = dag.nodes().expect("a DAG");
    let expected = reading(&fold(&nodes));

    let permuted = ordered(&nodes, &draw.ranks);
    prop_assert_eq!(reading(&fold(&permuted)), expected.clone(), "permuted");

    let mut doubled = permuted.clone();
    for i in &draw.twice {
        let node = permuted[i % permuted.len()].clone();
        let after = doubled
            .iter()
            .position(|held| held.name() == node.name())
            .expect("held");
        let at = (after + 1 + i % 3).min(doubled.len());
        doubled.insert(at, node);
    }
    prop_assert_eq!(reading(&fold(&doubled)), expected.clone(), "duplicated");

    let seeds = |mine: bool| {
        nodes
            .iter()
            .enumerate()
            .filter(move |(i, _)| (draw.replica >> (i % 64) & 1 == 1) == mine)
            .map(|(_, node)| node.name().clone())
    };
    let (here, there) = (dag.closure(seeds(true)), dag.closure(seeds(false)));
    let within = |held: &BTreeSet<Hash>| -> Vec<Node<E>> {
        permuted
            .iter()
            .filter(|node| held.contains(node.name()))
            .cloned()
            .collect()
    };
    let merged = extend(
        &fold(&within(&here)),
        &ordered(&within(&there), &draw.ranks),
    );
    prop_assert_eq!(reading(&merged), expected, "partitioned, then merged");
    Ok(())
}

/// Every entity's registers in `draw`'s history, read whole.
fn entities<E: Schema>(draw: &Draw<E>) -> Folded<E> {
    let dag: Dag<E> = history(&draw.events).into_iter().collect();
    fold(&dag.nodes().expect("a DAG"))
}

/// A term's value at `t`, `∅` (`None`) where it has none.
fn value(term: &Term, t: Datetime) -> Option<f64> {
    let closed = Closed::of(term.clone()).expect("no Ref");
    fpl::fulfillment(&closed, fpl::instant_of(t), &Env::new())
}

/// Law 5 at one draw. A reading with no priced candidate has no price; one
/// with one is priced as it, written as it; and a conflict's price is, at
/// every instant, the least of its candidates' values, `∅` the top: below
/// each, and attained.
fn least<E: Price>(draw: &Draw<E>) -> Result<(), TestCaseError> {
    let state = entities(draw);
    for (key, stream) in state.entities() {
        let registers = read(stream, None, &Everything);
        let terms = E::terms(&registers).expect("valid terms");
        let priced = price::<E>(&registers).expect("a price");
        match (priced, terms.as_slice()) {
            (None, []) => {}
            (Some(priced), [one]) => prop_assert_eq!(&priced, one, "{:?}", key),
            (Some(priced), many) if many.len() > 1 => {
                for t in (0..6).map(|day| at(day * 86_400)) {
                    let least = many
                        .iter()
                        .filter_map(|term| value(term, t))
                        .reduce(f64::min);
                    prop_assert_eq!(value(&priced, t), least, "{:?}", key);
                }
            }
            (priced, terms) => prop_assert!(false, "{:?}: {:?} over {:?}", key, priced, terms),
        }
    }
    Ok(())
}

/// A moment of a history with nothing first written and no head is the
/// reading itself.
fn historied<E: History>(draw: &Draw<E>) -> Result<(), TestCaseError> {
    let state = entities(draw);
    for (key, stream) in state.entities() {
        let registers = read(stream, None, &Everything);
        prop_assert_eq!(
            E::moment(&registers, &E::Registers::default(), None).expect("valid terms"),
            E::terms(&registers).expect("valid terms"),
            "{:?}",
            key
        );
    }
    Ok(())
}

/// One write, folded alone, joins exactly the frontiers its event names.
pub(crate) fn routed<E: Schema>(event: &E) -> Result<(), TestCaseError> {
    let dag: Dag<E> = history(&[(event.clone(), 0)]).into_iter().collect();
    let state = fold(&dag.nodes().expect("a DAG"));
    let (_, stream) = state.entities().next().expect("one entity");
    let registers = read(stream, None, &Everything);
    for register in registers.registers() {
        prop_assert_eq!(
            registers.frontier(register).writes().len(),
            usize::from(event.writes().any(|written| written == register)),
            "{:?}",
            register
        );
    }
    Ok(())
}

/// A register type's value is there exactly for the events whose names
/// name its register.
fn valued<E: Schema, R: RegisterType<E>>(event: &E, register: E::Register) -> bool {
    R::value(event).is_some() == event.writes().any(|written| written == register)
}

fn at(seconds: u32) -> Datetime {
    Datetime::new(2026, 10, 1 + seconds / 86_400, 0, 0, 0, 0).expect("a real instant")
}

fn a_todo_event() -> impl Strategy<Value = TodoEvent<Todo>> {
    (0..2usize, 0..8u8, 0u32..5 * 86_400, 0..3u8).prop_map(|(todo, kind, seconds, note)| {
        let flat = f64::from(note) / 4.0;
        let (todo, at, note) = (["alpha", "beta"][todo], at(seconds), format!("n{note}"));
        let spec = || mk_flat(flat).expect("valid");
        let record = |spec| {
            mk_authored(
                todo,
                at,
                "ana",
                "todo",
                at,
                &note,
                spec,
                vec![],
                "",
                "",
                "",
                None,
                vec![],
                vec![],
                "",
            )
        };
        match kind {
            0 => mk_completed(todo, at, "ana", &note),
            1 => mk_cancelled(todo, at, "ana", &note),
            2 => mk_reopened(todo, at, "ana", &note),
            3 => mk_tended(todo, at, "ana", &note),
            4 => mk_created(todo, at, "ana", &note, ""),
            5 => record(None),
            6 => record(Some(spec())),
            _ => mk_spec_revised(todo, at, "ana", spec(), &note),
        }
        .expect("valid")
    })
}

fn a_review_event() -> impl Strategy<Value = Review> {
    (
        0..2usize,
        prop::option::of(prop::sample::select(PHASES.to_vec())),
        0u32..5,
    )
        .prop_map(|(pr, phase, day): (usize, Option<Phase>, u32)| {
            let pr = ["pr-1", "pr-2"][pr];
            match phase {
                Some(phase) => review::moved(pr, review::day(1 + day), "ana", phase),
                None => review::opened(pr, review::day(1 + day), "ana", "a change"),
            }
        })
}

/// A note event by `ada`: one of two notes, one of three titles, at one of
/// three instants, so that a draw holds equal events under different deps.
pub(crate) fn a_note_event() -> impl Strategy<Value = Note> {
    (0..2usize, 0..3u32, 0..3u8).prop_map(|(name, minute, title)| {
        note::by_ada(
            ["n1", "n2"][name],
            note::at(minute * 60),
            &format!("t{title}"),
        )
    })
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    #[test]
    fn a_todo_reading_is_a_function_of_the_object_set(draw in a_draw(a_todo_event())) {
        free(&draw)?;
    }

    #[test]
    fn a_review_reading_is_a_function_of_the_object_set(draw in a_draw(a_review_event())) {
        free(&draw)?;
    }

    /// Law 33, over the note schema: an application schema at the generic
    /// event, whose product is `fold::sole::One`.
    #[test]
    fn a_note_reading_is_a_function_of_the_object_set(draw in a_draw(a_note_event())) {
        free(&draw)?;
    }

    #[test]
    fn a_todo_conflict_prices_as_the_least_of_its_candidates(draw in a_draw(a_todo_event())) {
        least(&draw)?;
        historied(&draw)?;
    }

    #[test]
    fn a_review_conflict_prices_as_the_least_of_its_candidates(draw in a_draw(a_review_event())) {
        least(&draw)?;
    }

    #[test]
    fn a_todo_write_joins_the_registers_it_names(event in a_todo_event()) {
        routed(&event)?;
        prop_assert!(valued::<_, State>(&event, Kind::State));
        prop_assert!(valued::<_, Spec>(&event, Kind::Spec));
        prop_assert!(valued::<_, Content>(&event, Kind::Content));
    }

    #[test]
    fn a_review_write_joins_the_registers_it_names(event in a_review_event()) {
        routed(&event)?;
        prop_assert!(valued::<_, PhaseRegister>(&event, Field::Phase));
    }

    /// Law 33, over the note schema: a write joins `Only`, the one register
    /// it names.
    #[test]
    fn a_note_write_joins_the_registers_it_names(event in a_note_event()) {
        routed(&event)?;
    }
}
