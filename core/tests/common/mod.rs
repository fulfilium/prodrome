//! Shared plumbing for the conformance suites (SPEC §9): the vector grammar
//! (`vectors`), comparing two literal trees the way the spec compares them —
//! shape and strings exactly, floats to 1e-9 — the synthetic corpus of stored
//! objects the grammar and the event suites both read, and the RANDOM LOG
//! GENERATOR (`a_log` and what it is built from) that `tests/all/fold_laws.rs`
//! draws its properties from and `examples/generate_view_vectors.rs` draws
//! `conformance/view/*.py` from — one generator, read by a property and frozen
//! by a vector file, never two.
//!
//! Included by the test binary and by the examples, each of which uses a part
//! of it; the part one of them does not call is not dead code, it is another's.
#![allow(dead_code)]

#[path = "vectors.rs"]
pub mod vectors;

#[path = "terms.rs"]
pub mod terms;

#[path = "cases.rs"]
pub mod cases;

#[path = "draw.rs"]
pub mod draw;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Duration, NaiveDate};
use prodrome::literal::{print_literal, Value};
use proptest::prelude::*;

/// The largest float deviation any comparison has seen, so a suite can report
/// its margin rather than only its verdict.
pub static WORST: AtomicU64 = AtomicU64::new(0);

pub fn tolerance() -> f64 {
    1e-9
}

pub fn worst_seen() -> f64 {
    f64::from_bits(WORST.load(Ordering::Relaxed))
}

fn record(delta: f64) {
    let mut current = WORST.load(Ordering::Relaxed);
    while delta > f64::from_bits(current) {
        match WORST.compare_exchange_weak(
            current,
            delta.to_bits(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(actual) => current = actual,
        }
    }
}

/// `mine` against `theirs`, in the spec's terms: every shape, every string and
/// every instant EXACTLY, every float to 1e-9 (§9.8). Returns the first
/// disagreement as a path plus a message, because "something differs" in a
/// tree this size is not a report.
pub fn agrees(path: &str, mine: &Value, theirs: &Value) -> Result<(), String> {
    match (mine, theirs) {
        (Value::Float(a), Value::Float(b)) => {
            let (a, b) = (a.get(), b.get());
            let delta = (a - b).abs();
            record(delta);
            if delta <= tolerance() * a.abs().max(b.abs()).max(1.0) {
                Ok(())
            } else {
                Err(format!("{path}: {a} != {b} (off by {delta:e})"))
            }
        }
        (Value::Tuple(a), Value::Tuple(b)) => {
            if a.len() != b.len() {
                return Err(format!("{path}: {} items, expected {}", a.len(), b.len()));
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                agrees(&format!("{path}[{i}]"), x, y)?;
            }
            Ok(())
        }
        (Value::Call(a), Value::Call(b)) => {
            if a.name != b.name {
                return Err(format!("{path}: {} != {}", a.name, b.name));
            }
            let (mine, theirs): (Vec<&String>, Vec<&String>) = (
                a.fields.iter().map(|(k, _)| k).collect(),
                b.fields.iter().map(|(k, _)| k).collect(),
            );
            if mine != theirs {
                return Err(format!("{path}: fields {mine:?} != {theirs:?}"));
            }
            for ((key, x), (_, y)) in a.fields.iter().zip(&b.fields) {
                agrees(&format!("{path}.{key}"), x, y)?;
            }
            Ok(())
        }
        (x, y) if x == y => Ok(()),
        (x, y) => Err(format!(
            "{path}: {} != {}",
            print_literal(x),
            print_literal(y)
        )),
    }
}

/// One number, compared the way §9.8 compares one: to 1e-9, relative, and the
/// deviation recorded so a suite can report its margin.
pub fn close(path: &str, mine: f64, theirs: f64) -> Result<(), String> {
    let delta = (mine - theirs).abs();
    record(delta);
    if delta <= tolerance() * mine.abs().max(theirs.abs()).max(1.0) {
        Ok(())
    } else {
        Err(format!("{path}: {mine} != {theirs} (off by {delta:e})"))
    }
}

/// A READING compared the way §9.8 compares one: `None` — the vector's `∅` —
/// only against `None`, and a number to 1e-9.
pub fn reading(path: &str, mine: Option<f64>, theirs: &Value) -> Result<(), String> {
    match (mine, vectors::maybe_number(theirs)) {
        (Some(mine), Some(theirs)) => close(path, mine, theirs),
        (None, None) => Ok(()),
        (mine, theirs) => Err(format!("{path}: {mine:?} != {theirs:?}")),
    }
}

// --- the synthetic corpus ----------------------------------------------------
//
// The suites that used to read a vector file of stored objects read this
// instead: a chain of stored objects built here, through the crate's own
// smart constructors, covering every kind of §4 and every production of §2
// that a stored object can carry. It is authored rather than generated
// because the point is COVERAGE OF THE GRAMMAR — the quoting rules, the
// microsecond argument, the empty tuple, an absent optional field — and a
// random draw covers what it happens to draw.
//
// `tests/all/literals.rs` reads these prints through the OPEN vocabulary (§2 and
// nothing else); `tests/all/events.rs` reads the same prints through the CLOSED
// one as typed envelopes (§4). One corpus, two readings, exactly as the two
// files always stood to each other.

use prodrome::event::{
    mk_cancelled, mk_completed, mk_created, mk_reopened, mk_sealed, mk_spec_revised, mk_tended,
    mk_woven, seal_hash, Authored, Envelope, Hash, TodoEvent, TodoId,
};
use prodrome::fpl::{self, mk_conj, mk_decay, mk_flat, mk_piecewise, mk_within, Instant};
use prodrome::literal::Datetime;
pub use prodrome::reference::{mk_authored, mk_note, mk_source, mk_subtodo, Todo};
use prodrome::term::Term;

/// The corpus is written against the REFERENCE PAYLOAD (`prodrome::reference`)
/// — the record shape `conformance/*.py` was taken with, and therefore the
/// one every vector in this repository round-trips under.
pub type Event = TodoEvent<Todo>;
pub type Object = Envelope<TodoEvent<Todo>>;

/// A moment in the corpus's window, with microseconds where §2's seventh
/// argument has to be exercised.
pub fn at(day: u32, hour: u32, micro: u32) -> Datetime {
    Datetime::new(2026, 9, day, hour, 30, 15, micro).expect("a real instant")
}

/// A spec with the shapes that make the printer work: a schedule to splice, a
/// conjunction to weigh, a window to sample, floats that print in exponent
/// form and floats that do not.
pub fn a_spec() -> Term {
    let deadline = fpl::instant_of(at(20, 9, 0));
    let decay = mk_decay(
        0.55,
        0.05,
        deadline,
        fpl::delta_from_hours(72.0),
        Some(fpl::instant_of(at(10, 9, 0))),
    )
    .expect("the reference's decay defaults");
    let conj = mk_conj(
        vec![decay, mk_flat(0.25).expect("a fulfillment")],
        fpl::PRIORITY_POWER,
    )
    .expect("a conjunction");
    let within = mk_within(fpl::delta_from_hours(48.0), 2.0, conj).expect("a window");
    mk_piecewise(
        mk_flat(1e-05).expect("a fulfillment"),
        vec![(fpl::instant_of(at(12, 0, 0)), within)],
    )
    .expect("a schedule")
}

/// One event of every kind in §4, with the awkward values inside them: a
/// string that carries both quote characters, a newline and a tab, printable
/// non-ASCII and an unprintable code point, an empty optional field, an empty
/// tuple, and a record whose fields are all filled.
pub fn events() -> Vec<Event> {
    let ok = |result: Result<Event, prodrome::literal::ProdromeError>| {
        result.expect("the corpus only builds events the constructors admit")
    };
    vec![
        ok(mk_created(
            "todo-1",
            at(1, 9, 0),
            "ada",
            "write the README for a stranger",
            "",
        )),
        ok(mk_created(
            "todo-2",
            at(1, 10, 500_000),
            "triage",
            "it's \"quoted\", \ttabbed,\nnewlined, é ü ✓ 日本 🙂 and \u{200b}zero-width",
            "a note with a \\ backslash in it",
        )),
        ok(mk_spec_revised("todo-1", at(2, 9, 0), "ada", a_spec(), "")),
        ok(mk_authored(
            "todo-1",
            at(3, 9, 250),
            "ada",
            "todo",
            at(1, 9, 0),
            "= A heading\n\nBody with \\@escapes and #strong[markup].",
            Some(a_spec()),
            vec![
                "because it is the contract".to_owned(),
                "'quoted'".to_owned(),
            ],
            "writing",
            "review from Ada",
            "The detail, on its own line.",
            Some(mk_source(
                "someone@example.com",
                "Re: the README",
                at(1, 8, 0),
                "0".repeat(64).as_str(),
                "thread-7",
                "<message@example.com>",
            )),
            vec![
                mk_subtodo("draft it", true).expect("a subtodo"),
                mk_subtodo("read it back", false).expect("a subtodo"),
            ],
            vec![mk_note("block", vec!["a note line".to_owned()]).expect("a note")],
            "",
        )),
        // The same kind with every optional field absent: no spec, no source,
        // no category, empty tuples.
        ok(mk_authored(
            "todo-2",
            at(3, 10, 0),
            "triage",
            "mail",
            at(1, 10, 0),
            "A record with nothing optional in it.",
            None,
            vec![],
            "",
            "",
            "",
            None,
            vec![],
            vec![],
            "",
        )),
        ok(mk_completed("todo-1", at(4, 9, 0), "ada", "done")),
        ok(mk_tended("todo-2", at(4, 12, 0), "ada", "vinegared")),
        ok(mk_reopened("todo-1", at(5, 9, 0), "ada", "")),
        ok(mk_cancelled("todo-2", at(6, 9, 999_999), "triage", "")),
    ]
}

/// The events above sealed into a store's shape: a chain, then a FORK — two
/// objects on one parent — joined by a `Woven` that carries no event, so the
/// corpus holds every envelope kind §3 admits, genesis included.
pub fn corpus() -> Vec<(Hash, Object)> {
    let mut events = events();
    let last = events.pop().expect("the corpus is not empty");
    let fork = events.pop().expect("the corpus is not empty");
    let mut objects: Vec<(Hash, Object)> = Vec::new();
    let mut prev: Option<Hash> = None;
    for event in events {
        let envelope = mk_sealed(prev.clone(), event);
        prev = Some(seal_hash(&envelope));
        objects.push((seal_hash(&envelope), envelope));
    }
    let tip = prev.expect("the chain has a tip");
    let left = mk_sealed(Some(tip.clone()), fork);
    let right = mk_sealed(Some(tip), last);
    let (left_name, right_name) = (seal_hash(&left), seal_hash(&right));
    let merge = mk_woven(vec![left_name.clone(), right_name.clone()], None).expect("a merge");
    objects.push((left_name, left));
    objects.push((right_name, right));
    objects.push((seal_hash(&merge), merge));
    objects
}

// --- the random log generator -------------------------------------------
//
// Moved out of `tests/fold_laws.rs` (2026, view vectors) so that the SAME
// generator draws both a property's cases (`fold_laws.rs`'s `proptest!`
// block, unmoved) and `conformance/view/*.py`'s frozen ones
// (`examples/generate_view_vectors.rs`, over a fixed seed). A vector file
// generated by a different draw than the properties run against would be
// a second, undocumented generator; this is why there is exactly one.

/// The reference generator's three todos.
pub const TODOS: [&str; 3] = ["alpha", "beta", "gamma"];
/// The reference generator's actors, in its proportions: two writes off the
/// reference policy's roster for every one on it.
pub const ACTORS: [&str; 3] = ["ada", "ada", "triage"];
/// The generator's window: sixty days from the origin.
pub const WINDOW: i64 = 60 * 86_400;

pub fn origin() -> Instant {
    NaiveDate::from_ymd_opt(2026, 9, 1)
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .expect("a real date")
}

pub fn moment(seconds: i64) -> Datetime {
    fpl::datetime_of(origin() + Duration::seconds(seconds)).expect("inside the grammar's years")
}

/// Later than every event any generator here produces, so a dated fold sees
/// the whole log and can be compared with the undated register fold.
pub fn far() -> Datetime {
    moment(WINDOW * 20)
}

pub fn ok<T>(result: Result<T, prodrome::literal::ProdromeError>) -> T {
    result.expect("the generator only builds events the constructors admit")
}

/// An event with its instant left OPEN. The generators produce these, and a
/// caller realises them at whatever instant it is about: the prefix law
/// needs an event dated after the log, the shift law needs the same log
/// dated differently, and the view vectors need each log dated once and then
/// queried at several instants — none of which is expressible if the stamp
/// is baked in.
#[derive(Debug, Clone)]
pub struct Draft {
    pub todo: &'static str,
    pub actor: &'static str,
    pub roll: u8,
    pub spec: Option<Term>,
    pub items: usize,
    pub text: u32,
}

impl Draft {
    pub fn at(&self, at: Datetime) -> Event {
        self.at_with_note(at, "")
    }

    pub fn at_with_note(&self, at: Datetime, note: &str) -> Event {
        match self.roll {
            0 => ok(mk_created(
                self.todo,
                at,
                self.actor,
                &format!("t{}", self.text),
                note,
            )),
            1 | 2 => {
                let subtodos = (0..self.items)
                    .map(|i| ok(mk_subtodo(&format!("item {i}"), i % 3 == 0)))
                    .collect();
                ok(mk_authored(
                    self.todo,
                    at,
                    self.actor,
                    "todo",
                    at,
                    &format!("body {} \\@x", self.text),
                    self.spec.clone(),
                    vec![],
                    "",
                    "",
                    "",
                    None,
                    subtodos,
                    vec![],
                    note,
                ))
            }
            3 => ok(mk_spec_revised(
                self.todo,
                at,
                self.actor,
                self.spec
                    .clone()
                    .unwrap_or_else(|| fpl::mk_flat(0.5).expect("0.5 is a fulfillment")),
                note,
            )),
            4 => ok(mk_completed(self.todo, at, self.actor, note)),
            5 => ok(mk_cancelled(self.todo, at, self.actor, note)),
            6 => ok(mk_reopened(self.todo, at, self.actor, note)),
            _ => ok(mk_tended(self.todo, at, self.actor, note)),
        }
    }
}

/// A spec the way the reference's generator draws one, shallow: the shapes
/// that make `flatten`'s normal form do work — a schedule to splice, a
/// conjunction to weigh, a window to sample — without the depth `fpl`'s own
/// laws already cover.
///
/// Named `a_random_spec` and not `a_spec`: this file already has [`a_spec`],
/// the ONE fixed spec the synthetic corpus (`corpus`, above) is authored
/// with, and the two are different things — a strategy that draws many
/// shapes against a value that is one, chosen for what it exercises.
pub fn a_random_spec() -> impl Strategy<Value = Term> {
    let leaf = prop_oneof![
        (0.02f64..0.98).prop_map(|v| fpl::mk_flat(v).expect("in [0, 1]")),
        (0.3f64..0.95, 0.0f64..0.2, 0i64..WINDOW, 6i64..400).prop_map(|(start, end, at, lead)| {
            fpl::mk_decay(
                start,
                end,
                origin() + Duration::seconds(at),
                Duration::hours(lead),
                None,
            )
            .expect("a well-formed decay")
        }),
    ]
    .boxed();
    let plain = leaf.clone();
    leaf.prop_recursive(2, 8, 2, move |inner| {
        prop_oneof![
            (
                prop::collection::vec(inner.clone(), 1..3),
                prop::sample::select(vec![-4.0, -1.0, 0.0])
            )
                .prop_map(|(terms, p)| fpl::mk_conj(terms, p).expect("p is in range")),
            // A recurrence on one of the generator's todos, so its tendings
            // price what the folds hand the evaluator. Over plain leaves: a
            // pass before the anchor reads the body LATER than now, and a body
            // that read history there would see past the prefix law (§9.2).
            (
                prop::sample::select(TODOS.to_vec()),
                0i64..WINDOW,
                plain.clone(),
                plain.clone()
            )
                .prop_map(|(todo, anchor, term, pending)| fpl::mk_recur(
                    todo.to_owned(),
                    origin() + Duration::seconds(anchor),
                    term,
                    pending
                )
                .expect("a todo id")),
            (
                inner.clone(),
                prop::collection::btree_set(0i64..WINDOW, 1..3),
                prop::collection::vec(inner, 2)
            )
                .prop_map(|(head, ats, terms)| fpl::mk_piecewise(
                    head,
                    ats.iter()
                        .zip(terms)
                        .map(|(at, term)| (origin() + Duration::seconds(*at), term))
                        .collect()
                )
                .expect("the instants are a sorted set"))
        ]
    })
}

/// A spec as a log carries one now that `∅` and references exist:
/// [`a_random_spec`]'s shapes, `Absent`, a `Ref` onto one of the generator's
/// todos — which may have no function, or loop — or onto `delta`, which no
/// event names, and a conjunction mixing them.
pub fn a_spec_with_absence() -> impl Strategy<Value = Term> {
    let reference = prop::sample::select(vec!["alpha", "beta", "gamma", "delta"])
        .prop_map(|todo| fpl::mk_ref(todo.to_owned()).expect("a todo id"));
    let member = prop_oneof![
        2 => a_random_spec().boxed(),
        1 => Just(fpl::mk_absent()).boxed(),
        1 => reference.clone().boxed(),
    ];
    prop_oneof![
        3 => a_random_spec().boxed(),
        1 => Just(fpl::mk_absent()).boxed(),
        1 => reference.boxed(),
        1 => (
            prop::collection::vec(member, 1..4),
            prop::sample::select(vec![-4.0, -1.0, 0.0])
        )
            .prop_map(|(terms, p)| fpl::mk_conj(terms, p).expect("p is in range"))
            .boxed(),
    ]
}

pub fn a_draft() -> impl Strategy<Value = Draft> {
    a_draft_with(a_random_spec())
}

/// [`a_draft`] with its specs drawn from `specs`.
pub fn a_draft_with(specs: impl Strategy<Value = Term>) -> impl Strategy<Value = Draft> {
    (
        prop::sample::select(TODOS.to_vec()),
        prop::sample::select(ACTORS.to_vec()),
        0u8..8,
        // The generator carries a spec on most `Authored` records and not all.
        prop::option::weighted(0.85, specs),
        prop::sample::select(vec![0usize, 0, 2, 3]),
        0u32..999,
    )
        .prop_map(|(todo, actor, roll, spec, items, text)| Draft {
            todo,
            actor,
            roll,
            spec,
            items,
            text,
        })
}

/// Drafts with their instants, ready to realise. Kept apart from the events so
/// a caller can re-date the same log.
pub fn a_schedule(size: std::ops::Range<usize>) -> impl Strategy<Value = Vec<(Draft, i64)>> {
    prop::collection::vec((a_draft(), 0i64..WINDOW), size).prop_map(|mut drafts| {
        // `a_log` sorts by instant, and Rust's sort is stable, so equal stamps
        // keep the order they were drawn in — the reference's `sorted` too.
        drafts.sort_by_key(|(_, at)| *at);
        drafts
    })
}

pub fn realise(schedule: &[(Draft, i64)], shift: i64) -> Vec<Event> {
    schedule
        .iter()
        .map(|(draft, at)| draft.at(moment(at + shift)))
        .collect()
}

pub fn a_log() -> impl Strategy<Value = Vec<Event>> {
    a_schedule(0..25).prop_map(|schedule| realise(&schedule, 0))
}

/// Two writers over one history: the shared log, then each side's own, dated
/// after it and carrying the side's note, so that no event is both sides'.
pub fn two_writers() -> impl Strategy<Value = (Vec<Event>, Vec<Event>, Vec<Event>)> {
    (a_schedule(0..10), a_schedule(0..8), a_schedule(0..8)).prop_map(|(shared, mine, theirs)| {
        let noted = |drafts: &[(Draft, i64)], note: &str| -> Vec<Event> {
            drafts
                .iter()
                .map(|(draft, at)| draft.at_with_note(moment(at + WINDOW), note))
                .collect()
        };
        (
            realise(&shared, 0),
            noted(&mine, "mine"),
            noted(&theirs, "theirs"),
        )
    })
}

/// A log, and what a host that retried delivers again: some of its events,
/// in any order and any number of times.
pub fn a_replay() -> impl Strategy<Value = (Vec<Event>, Vec<Event>)> {
    a_log()
        .prop_flat_map(|log| {
            let picks = prop::collection::vec(any::<prop::sample::Index>(), 0..=log.len());
            (Just(log), picks)
        })
        .prop_map(|(log, picks)| {
            let replay = picks
                .iter()
                .map(|pick| log[pick.index(log.len())].clone())
                .collect();
            (log, replay)
        })
}

/// [`a_log`] whose specs are [`a_spec_with_absence`]'s: records and revisions
/// that price by `Absent`, reference todos with no function, never-seen ones
/// and each other.
pub fn a_log_with_absence() -> impl Strategy<Value = Vec<Event>> {
    prop::collection::vec((a_draft_with(a_spec_with_absence()), 0i64..WINDOW), 0..25).prop_map(
        |mut drafts| {
            drafts.sort_by_key(|(_, at)| *at);
            realise(&drafts, 0)
        },
    )
}

/// The legacy writer: `event` sealed on every tip of `store`, as every store
/// before changes were written.
pub fn seal<Pol: prodrome::policy::Policy<TodoEvent<Todo>>>(
    store: &prodrome::store::EventStore<TodoEvent<Todo>, Pol>,
    event: Event,
) -> Hash {
    let on: Vec<Hash> = store.tips().expect("the tips derive").into_iter().collect();
    let object = if on.len() > 1 {
        mk_woven(on, Some(event)).expect("distinct tips")
    } else {
        mk_sealed(on.into_iter().next(), event)
    };
    let name = seal_hash(&object);
    let print = prodrome::event::canonical_envelope(&object);
    store
        .adopt_objects(&[(name.clone(), print)].into(), &name)
        .expect("writes")
}

/// A log as the chain a single writer builds: each object sealed on the one
/// before, so the node's parents are real and its name is its own hash.
pub fn chain_of(log: &[Event]) -> Vec<prodrome::registers::Node<TodoEvent<Todo>>> {
    let mut prev: Option<Hash> = None;
    let mut objects = Vec::with_capacity(log.len());
    for event in log {
        let envelope = mk_sealed(prev.clone(), event.clone());
        let name = seal_hash(&envelope);
        prev = Some(name.clone());
        objects.push((name, envelope));
    }
    objects
        .into_iter()
        .collect::<prodrome::dag::Dag<TodoEvent<Todo>>>()
        .nodes()
        .expect("a chain")
}

/// A log's one prodrome, folded from the chain its writer built.
pub fn prodrome_of(log: &[Event]) -> prodrome::registers::Prodrome<TodoEvent<Todo>> {
    let state = prodrome::registers::fold(&chain_of(log));
    state
        .prodromes()
        .values()
        .next()
        .cloned()
        .unwrap_or_default()
}

/// §6.1 over a log, read as its chain.
pub fn env_at(
    log: &[Event],
    t: Datetime,
    policy: &impl prodrome::policy::Policy<TodoEvent<Todo>>,
) -> prodrome::fpl::Env {
    prodrome::fold::env(&prodrome_of(log), fpl::instant_of(t), policy)
}

/// A chain's one candidate per todo.
fn only<T>(map: BTreeMap<TodoId, Vec<T>>) -> BTreeMap<TodoId, T> {
    map.into_iter()
        .map(|(todo, mut candidates)| {
            assert_eq!(candidates.len(), 1, "a chain has one candidate");
            (todo, candidates.remove(0))
        })
        .collect()
}

/// §6.2 over a log, read as its chain.
pub fn specs_at(
    log: &[Event],
    t: Datetime,
    policy: &impl prodrome::policy::Policy<TodoEvent<Todo>>,
) -> BTreeMap<TodoId, Term> {
    only(prodrome::fold::specs(
        &prodrome_of(log),
        fpl::instant_of(t),
        policy,
    ))
}

/// §6.3 over a log, read as its chain.
pub fn authored_at(log: &[Event], t: Datetime) -> BTreeMap<TodoId, Authored<Todo>> {
    only(prodrome::fold::content(
        &prodrome_of(log),
        fpl::instant_of(t),
    ))
}

/// §6.4 over a log, read as its chain.
pub fn flatten(
    log: &[Event],
    t: Datetime,
    policy: &impl prodrome::policy::Policy<TodoEvent<Todo>>,
) -> Result<BTreeMap<TodoId, Term>, prodrome::fpl::FplError> {
    prodrome::fold::flatten(&prodrome_of(log), fpl::instant_of(t), policy)
}

/// One §6.7 entry as the vector grammar's `Row(...)`.
///
/// THE SAME TEN FIELDS `prodrome-wasm`'s `wire::json_entry` puts on the wire,
/// in the same forms — the lowercased state, `isoformat(" ")`, `""` for an
/// absent claim, `'absent'` for an absent value and `None` for one that does
/// not link, a term as its §2 PRINT. That is
/// deliberate and is what `conformance/view/*.py` is frozen against: the entry
/// as a CONSUMER reads it, not as the `Entry` struct happens to be shaped.
/// `wasm/exports/src/wire.rs`'s own test pins the JSON spelling of these same fields.
pub fn entry_value(
    entry: &prodrome::view::Entry<prodrome::event::TodoEvent<prodrome::reference::Todo>>,
) -> Value {
    let conflicts: Vec<Value> = entry
        .conflicts
        .iter()
        .map(|(kind, writes)| {
            Value::call(
                "RowConflict",
                vec![
                    ("kind".to_owned(), Value::Str(kind.as_str().to_owned())),
                    (
                        "writes".to_owned(),
                        Value::Tuple(
                            writes
                                .iter()
                                .map(|write| Value::Str(write.as_str().to_owned()))
                                .collect(),
                        ),
                    ),
                ],
            )
        })
        .collect();
    Value::call(
        "Row",
        vec![
            ("todo".to_owned(), Value::Str(entry.key.as_str().to_owned())),
            ("state".to_owned(), Value::Str(entry.state())),
            ("at".to_owned(), Value::Str(entry.at())),
            ("claimed".to_owned(), Value::Str(entry.claimed())),
            (
                "value".to_owned(),
                match entry.value() {
                    Ok(Some(v)) => Value::float(v).expect("a fulfillment is finite"),
                    Ok(None) => Value::Str("absent".to_owned()),
                    Err(_) => Value::None,
                },
            ),
            (
                "unconfirmed".to_owned(),
                Value::Bool(entry.confidence.is_provisional()),
            ),
            ("conflicts".to_owned(), Value::Tuple(conflicts)),
            (
                "content".to_owned(),
                match entry.content() {
                    [] => Value::None,
                    [one] => Value::Str(one.as_str().to_owned()),
                    many => Value::Tuple(
                        many.iter()
                            .map(|name| Value::Str(name.as_str().to_owned()))
                            .collect(),
                    ),
                },
            ),
            ("spec".to_owned(), Value::Str(fpl::print_term(entry.spec()))),
            (
                "stream".to_owned(),
                Value::Tuple(
                    entry
                        .stream
                        .iter()
                        .map(|name| Value::Str(name.as_str().to_owned()))
                        .collect(),
                ),
            ),
        ],
    )
}

/// One `explain` tree as the vector grammar's `Node(...)`: the kind tag, the
/// value (`None` for `∅`), the notes, and the children in order.
///
/// THE DECORATION AND NOT THE TERM. The term's own shape is the case's `term`
/// print, which the same vector holds; what explaining ADDS is a value and a
/// set of notes at each node, and that is all this carries. (The JSON shape
/// the browser reads is `prodrome-wasm`'s, pinned by its own vectors.)
pub fn explanation_value(node: &prodrome::fpl::Explanation) -> Value {
    let notes: Vec<Value> = node
        .notes
        .iter()
        .map(|(key, note)| {
            Value::call(
                "NoteEntry",
                vec![
                    ("key".to_owned(), Value::Str(key.clone())),
                    ("note".to_owned(), note_value(note)),
                ],
            )
        })
        .collect();
    let children: Vec<Value> = node
        .node
        .children()
        .into_iter()
        .map(explanation_value)
        .collect();
    Value::call(
        "Node",
        vec![
            ("kind".to_owned(), Value::Str(node.node.kind().to_owned())),
            (
                "value".to_owned(),
                node.value.map_or(Value::None, |v| {
                    Value::float(v).expect("a fulfillment is finite")
                }),
            ),
            ("notes".to_owned(), Value::Tuple(notes)),
            ("terms".to_owned(), Value::Tuple(children)),
        ],
    )
}

fn note_value(note: &prodrome::fpl::Note) -> Value {
    use prodrome::fpl::Note;
    match note {
        Note::One(s) => Value::call("One", vec![("value".to_owned(), scalar_value(s))]),
        Note::Many(xs) => Value::call(
            "Many",
            vec![(
                "values".to_owned(),
                Value::Tuple(xs.iter().map(scalar_value).collect()),
            )],
        ),
        Note::Maps(ms) => Value::call(
            "Maps",
            vec![(
                "maps".to_owned(),
                Value::Tuple(
                    ms.iter()
                        .map(|m| {
                            Value::call(
                                "Fields",
                                vec![(
                                    "pairs".to_owned(),
                                    Value::Tuple(
                                        m.iter()
                                            .map(|(k, v)| {
                                                Value::call(
                                                    "Pair",
                                                    vec![
                                                        ("key".to_owned(), Value::Str(k.clone())),
                                                        ("value".to_owned(), scalar_value(v)),
                                                    ],
                                                )
                                            })
                                            .collect(),
                                    ),
                                )],
                            )
                        })
                        .collect(),
                ),
            )],
        ),
    }
}

fn scalar_value(s: &prodrome::fpl::Scalar) -> Value {
    use prodrome::fpl::Scalar;
    match s {
        Scalar::Text(t) => Value::Str(t.clone()),
        Scalar::Float(f) => Value::float(*f).expect("a note's float is finite"),
        Scalar::Int(i) => Value::int(*i),
        Scalar::Bool(b) => Value::Bool(*b),
    }
}
