//! Law 1 of `docs/design-register-types.md` §7, FREE: a reading is a
//! function of the object set. Every permutation of a history that keeps
//! parents first, every duplication, and every partition into two replicas
//! folded apart and then merged reads the same, register by register.
//!
//! Stated once, in the law kit (`prodrome::laws::free`, `routed` and
//! `valued`, SPEC law 33), and run here over the todo schema, the
//! review schema of `review.rs` and the note schema of `note.rs`: the law
//! asks nothing of a schema, so none may break it. The note schema is an
//! application schema at the generic event (`schema::headed`), so the law
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

use prodrome::dag::Dag;
use prodrome::event::{
    mk_cancelled, mk_completed, mk_created, mk_reopened, mk_spec_revised, mk_tended, TodoEvent,
};
use prodrome::fold::sole::{Only, Whole};
use prodrome::fold::{price, read, Kind};
use prodrome::fpl::{self, mk_flat, Closed, Env};
use prodrome::laws;
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::reference::{mk_authored, Todo};
use prodrome::registers::{fold, Folded};
use prodrome::schema::{History, Price, Schema};
use prodrome::term::Term;
use proptest::prelude::*;

use crate::common::draw::{a_draw, history, Draw};

use prodrome::todo::{Content, Spec, State};

use crate::note::{self, Entry, Note};
use crate::review::{self, Field, Phase, PhaseRegister, Review, PHASES};

/// A broken law, as the property test's failure.
pub(crate) fn failed(broken: laws::Broken) -> TestCaseError {
    TestCaseError::fail(broken.to_string())
}

/// Law 33's first clause at one draw, by the kit.
pub(crate) fn free<E: Schema>(draw: &Draw<E>) -> Result<(), TestCaseError> {
    laws::free(&history(&draw.events), &draw.shuffle).map_err(failed)
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

/// Law 33's second clause at one event, by the kit.
pub(crate) fn routed<E: Schema>(event: &E) -> Result<(), TestCaseError> {
    laws::routed(event).map_err(failed)
}

fn at(seconds: u32) -> Datetime {
    Datetime::new(2026, 10, 1 + seconds / 86_400, 0, 0, 0, 0).expect("a real instant")
}

pub(crate) fn a_todo_event() -> impl Strategy<Value = TodoEvent<Todo>> {
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

pub(crate) fn a_review_event() -> impl Strategy<Value = Review> {
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

/// A note event by `ada`: one of two notes, at one of three instants, a
/// view or one of three titles, so that a draw holds equal events under
/// different deps, and events that write no register.
pub(crate) fn a_note_event() -> impl Strategy<Value = Note> {
    (0..2usize, 0..3u32, 0..4u8).prop_map(|(name, minute, title)| {
        let entry = match title {
            0 => Entry::Viewed,
            _ => note::titled(&format!("t{title}")),
        };
        note::by_ada(["n1", "n2"][name], note::at(minute * 60), entry)
    })
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// Law 33, over the todo schema, by the kit: a reading is a function of
    /// the object set.
    #[test]
    fn a_todo_reading_is_a_function_of_the_object_set(draw in a_draw(a_todo_event())) {
        free(&draw)?;
    }

    /// Law 33, over the review schema, by the kit: a reading is a function
    /// of the object set.
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

    /// Law 33, over the todo schema, by the kit: a write joins the registers
    /// it names, and each register type values exactly those writes.
    #[test]
    fn a_todo_write_joins_the_registers_it_names(event in a_todo_event()) {
        routed(&event)?;
        laws::valued::<_, State>(&event, Kind::State).map_err(failed)?;
        laws::valued::<_, Spec>(&event, Kind::Spec).map_err(failed)?;
        laws::valued::<_, Content>(&event, Kind::Content).map_err(failed)?;
    }

    /// Law 33, over the review schema, by the kit: a write joins the
    /// register it names, and the phase type values exactly those writes.
    #[test]
    fn a_review_write_joins_the_registers_it_names(event in a_review_event()) {
        routed(&event)?;
        laws::valued::<_, PhaseRegister>(&event, Field::Phase).map_err(failed)?;
    }

    /// Law 33, over the note schema: a title joins `Only`, a view joins no
    /// register, and the register type `Whole` values exactly the titles.
    #[test]
    fn a_note_write_joins_the_registers_it_names(event in a_note_event()) {
        routed(&event)?;
        laws::valued::<_, Whole<Note>>(&event, Only).map_err(failed)?;
    }
}
