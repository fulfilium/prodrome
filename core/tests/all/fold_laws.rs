//! SPEC §9.2, §9.3, §9.6, §9.9, §9.17 b and laws 24, 25 and 27, as
//! properties over random logs and random two-replica DAGs.
//!
//! The vectors say this side agrees with the reference on the cases the
//! reference happened to generate. These say the AGREEMENT IS STRUCTURAL:
//! nothing rewrites history, order is causal and the stamp is data, the
//! conflicts are named exactly and price as their most urgent world, no
//! linearisation and no claim decides a value, and the entry (§6.7) is the
//! composition of the registers and nothing else.
//!
//! The generators mirror the reference generator's `a_log`/`an_event` — the
//! same three todos, its seven kinds in its proportions with `Tended` beside
//! them, the same two actors with one of them on the reference policy's
//! roster — so a failure here is a failure the vector generator could have
//! produced, and a fix is checkable against it.
//!
//! And §9.18 at the store: over logs whose specs are `Absent`, references to
//! todos with no function, to todos never seen and to each other, a
//! reference links to `Absent` exactly when its todo is known and has no
//! function, and every row reads what that link reads.
//!
//! The DAG laws build REAL stores in temp directories and drive them the way a
//! second replica would: append, adopt, write concurrently, merge. Nothing
//! about a frontier may depend on this side having constructed the graph in
//! memory.

use crate::common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use prodrome::dag::Dag;
use prodrome::event::{
    event_id, mk_completed, mk_spec_revised, mk_tended, parents_of, seal_hash, Actor, Envelope,
    Hash, TodoEvent, TodoId,
};
use prodrome::fold::{Kind, Product, Write};
use prodrome::fpl::{self, print_term, Env};
use prodrome::literal::Datetime;
use prodrome::policy::{Everything, Policy, Untrusted};
use prodrome::reference::{mk_authored, mk_subtodo, Todo};
use prodrome::registers::{extend, fold, since, Folded, Node};
use prodrome::store::EventStore;
use prodrome::store::Sender;
use prodrome::term::Term;
use prodrome::view;
use proptest::prelude::*;

use common::{
    a_draft, a_log, a_log_with_absence, a_schedule, authored_at, chain_of, env_at, far, flatten,
    moment, realise, seal, specs_at, Draft, TODOS, WINDOW,
};

/// These laws are about the FOLDS, not about a record's fields, so the payload
/// they run under is the reference one — the shape the vector generator drew.
type Event = TodoEvent<Todo>;
type Chain = Node<TodoEvent<Todo>>;
type Store = EventStore<TodoEvent<Todo>, Untrusted>;
type State = Folded<TodoEvent<Todo>>;

/// The random log generator (`TODOS`, `ACTORS`, `WINDOW`, `origin`, `moment`,
/// `far`, `Draft`, `a_random_spec`, `a_draft`, `a_schedule`, `realise`,
/// `a_log`, `chain_of`) lives in `tests/common/mod.rs` now: it is also what
/// `examples/generate_view_vectors.rs` draws `conformance/view/*.py` from,
/// over a fixed seed, and a generator a vector file was taken from and a
/// property runs against had to be the same one.
fn roster() -> Untrusted {
    Untrusted::of([Actor::new("triage").expect("valid")])
}

/// The generated logs carry no `Ref`, so every function they flatten to is
/// closed as it stands.
fn closed(term: &Term) -> fpl::Closed {
    fpl::Closed::of(term.clone()).expect("the generators write no Ref")
}

/// A draft the reference policy can only let CLAIM: a lifecycle write or a
/// repricing, from the one actor on the roster. `Draft`'s rolls 3..8 are
/// `SpecRevised`, `Completed`, `Cancelled`, `Reopened` and `Tended` — the
/// kinds §5 makes provisional — and rolls 0..3 (`Created`, `Authored`) are the
/// ones it does not, which is why the range is exactly this one.
fn a_claim() -> impl Strategy<Value = Draft> {
    (a_draft(), 3u8..8).prop_map(|(draft, roll)| Draft {
        actor: "triage",
        roll,
        ..draft
    })
}

/// What every fold answers, as one comparable value. Terms and records go in
/// as their canonical PRINTS: a fold that agreed to nine decimals and printed
/// a different term would not be the same fold.
#[derive(Debug, PartialEq)]
struct Folds {
    env: Env,
    specs: BTreeMap<String, String>,
    content: BTreeMap<String, String>,
    flatten: BTreeMap<String, String>,
}

fn folds(log: &[Event], t: Datetime, policy: &impl Policy<TodoEvent<Todo>>) -> Folds {
    Folds {
        env: env_at(log, t, policy),
        specs: specs_at(log, t, policy)
            .iter()
            .map(|(todo, spec)| (todo.as_str().to_owned(), print_term(spec)))
            .collect(),
        content: authored_at(log, t)
            .iter()
            .map(|(todo, record)| {
                (
                    todo.as_str().to_owned(),
                    prodrome::literal::print_literal(&record.to_value()),
                )
            })
            .collect(),
        flatten: flatten(log, t, policy)
            .expect("the log folds")
            .iter()
            .map(|(todo, term)| (todo.as_str().to_owned(), print_term(term)))
            .collect(),
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// §9.2, THE PREFIX LAW — nothing rewrites history. Append an event later
    /// than the log's last, and at every earlier moment every todo's function
    /// reads exactly what it read before, against the environment of that
    /// moment.
    ///
    /// With ONE exception, which is `flatten`'s definition and not a bug: the
    /// HEAD is `checklist(first spec ever, first checklist ever)`, so an
    /// appended event that records EITHER half for the first time re-heads
    /// that todo's whole curve. It can only bite a todo that already had a
    /// function from the other half alone — a checklist with no spec, or a
    /// spec with no content record — and `a_late_first_half_of_the_head_re_heads_the_curve`
    /// below is the witness for both, read off the reference. Everything else
    /// is untouchable, which is what this asserts.
    #[test]
    fn appending_a_later_event_changes_no_earlier_moment(
        schedule in a_schedule(1..25),
        extra in a_draft(),
        gap in 1i64..3600,
        asked in prop::collection::vec(0i64..WINDOW, 1..6),
    ) {
        let policy = roster();
        let log = realise(&schedule, 0);
        let last = schedule.last().expect("a non-empty schedule").1;
        let tau = moment(last + gap);
        let mut longer = log.clone();
        longer.push(extra.at(tau));

        let before = flatten(&log, tau, &policy).expect("folds");
        let after = flatten(&longer, tau, &policy).expect("folds");
        // A later event can only ADD a function, never remove one.
        for todo in before.keys() {
            prop_assert!(after.contains_key(todo));
        }
        // A todo whose head the appended event DETERMINES: the first spec, or
        // the first content record (which fixes the first checklist length).
        // `specs_at`/`authored_at` hold a key exactly when the chain holds
        // such a write, which is the condition `flatten`'s head reads.
        let head_of = |log: &[Event]| -> (BTreeSet<TodoId>, BTreeSet<TodoId>) {
            (
                specs_at(log, tau, &policy).into_keys().collect(),
                authored_at(log, tau).into_keys().collect(),
            )
        };
        let (priced_before, written_before) = head_of(&log);
        let (priced_after, written_after) = head_of(&longer);
        for seconds in asked {
            // STRICTLY before tau: the law is about EARLIER moments, and at
            // tau itself the appended event is in force by design — that is
            // the fold working, not history being rewritten.
            let t = moment(seconds.min(last + gap - 1));
            let now = fpl::instant_of(t);
            for (todo, term) in &before {
                let re_headed = (!priced_before.contains(todo) && priced_after.contains(todo))
                    || (!written_before.contains(todo) && written_after.contains(todo));
                if re_headed {
                    continue;
                }
                prop_assert_eq!(
                    fpl::fulfillment(&closed(term), now, &env_at(&log, t, &policy)),
                    fpl::fulfillment(&closed(&after[todo]), now, &env_at(&longer, t, &policy)),
                    "{:?} at {:?}", todo, t
                );
            }
        }
    }

    /// §9.3, first half — INDEPENDENT EVENTS COMMUTE. Shuffle the log, then
    /// restore each todo's own order: that is a linearisation of the per-todo
    /// partial order, which is exactly what "independent" means. Every fold
    /// answers the same, so each is a function of the event SET and two
    /// replicas that merge in different orders converge.
    #[test]
    fn independent_events_commute(
        log in a_log(),
        keys in prop::collection::vec(0u32..1000, 0..25),
        at in 0i64..WINDOW,
    ) {
        let policy = roster();
        let t = moment(at);
        // A permutation of the positions, from keys the shrinker can shrink.
        let mut order: Vec<usize> = (0..log.len()).collect();
        order.sort_by_key(|i| (keys.get(*i).copied().unwrap_or(0), *i));

        let mut queues: BTreeMap<&TodoId, Vec<&Event>> = BTreeMap::new();
        for event in &log {
            queues.entry(event.todo()).or_default().push(event);
        }
        for queue in queues.values_mut() {
            queue.reverse(); // pop from the back keeps each todo's own order
        }
        let relinearised: Vec<Event> = order
            .iter()
            .map(|i| {
                queues
                    .get_mut(log[*i].todo())
                    .and_then(Vec::pop)
                    .expect("one event per position")
                    .clone()
            })
            .collect();
        prop_assert_eq!(relinearised.len(), log.len());
        prop_assert_eq!(folds(&relinearised, t, &policy), folds(&log, t, &policy));
    }

    /// §9.3, second half — THE STAMP IS DATA, NOT ORDER. Shift every `at` by
    /// the same amount and move the question by the same amount: the time
    /// machine's window moves, and no register's winner changes.
    #[test]
    fn a_uniform_shift_changes_no_winner(
        schedule in a_schedule(0..25),
        shift in -(WINDOW / 2)..(WINDOW / 2),
        at in 0i64..WINDOW,
    ) {
        let policy = roster();
        let here = realise(&schedule, 0);
        let there = realise(&schedule, shift);
        let t = moment(at);
        let moved = moment(at + shift);

        let kinds = |env: &Env| -> BTreeMap<String, Vec<&'static str>> {
            env.outcomes.iter()
                .map(|(todo, candidates)| {
                    (todo.clone(), candidates.iter().flatten().map(fpl::Outcome::kind).collect())
                })
                .collect()
        };
        prop_assert_eq!(
            kinds(&env_at(&here, t, &policy)),
            kinds(&env_at(&there, moved, &policy))
        );
        let printed = |specs: BTreeMap<TodoId, Term>| -> BTreeMap<String, String> {
            specs
                .iter()
                .map(|(todo, spec)| (todo.as_str().to_owned(), print_term(spec)))
                .collect()
        };
        prop_assert_eq!(
            printed(specs_at(&here, t, &policy)),
            printed(specs_at(&there, moved, &policy))
        );
    }

    /// §6.6's monoid action on a log's chain of nodes, with no store in the
    /// way: `extend(extend(s, xs), ys) == extend(s, xs ++ ys)`, and extending
    /// with what is already folded changes nothing.
    #[test]
    fn extending_is_a_monoid_action(log in a_log(), split in 0usize..25) {
        let nodes = chain_of(&log);
        let split = split.min(nodes.len());
        let whole = fold(&nodes);
        prop_assert_eq!(&extend(&fold(&nodes[..split]), &nodes[split..]), &whole);
        prop_assert_eq!(&extend(&whole, &nodes), &whole);
        prop_assert_eq!(
            since(&fold(&nodes[..split]), &nodes),
            nodes[split..].iter().collect::<Vec<_>>()
        );
    }

    /// ONE FOLD OVER TIME: an entity's registers at `t` are its readings,
    /// the step function, at `t`, and ever are their last value, under a
    /// policy and under everything.
    #[test]
    fn a_reading_at_a_moment_is_the_readings_at_it(log in a_log()) {
        let state = fold(&chain_of(&log));
        for (_, stream) in state.entities() {
            for policy in [roster(), Untrusted::none()] {
                let readings = prodrome::fold::readings(stream, None, &policy);
                prop_assert_eq!(&prodrome::fold::read(stream, None, &policy), readings.last());
                for stamp in stream {
                    let t = fpl::instant_of(stamp.event.at());
                    for t in [t - chrono::Duration::hours(1), t, t + chrono::Duration::hours(1)] {
                        prop_assert_eq!(&prodrome::fold::read(stream, Some(t), &policy), readings.at(t));
                        let until = prodrome::fold::readings(stream, Some(t), &policy);
                        prop_assert_eq!(until.last(), readings.at(t));
                    }
                }
            }
        }
    }

    /// §9.10 — UNDER A POLICY THAT BINDS EVERYTHING THE TWO READINGS ARE ONE.
    ///
    /// [`Everything`]'s whole content is that no event claims, so the CLAIMED
    /// reading (§6.7, taken under it) and the CONFIRMED one (taken under the
    /// policy) coincide: every fold answers the same, no entry carries a claim,
    /// and no entry is provisional. It is the law that says the two readings
    /// are ONE reading asked twice and not two different pieces of machinery —
    /// and `Untrusted::none()`, the empty roster, is the same policy said with
    /// a roster, which is checked here too.
    #[test]
    fn the_two_readings_agree_under_a_policy_that_binds_everything(
        log in a_log(),
        at in 0i64..WINDOW,
    ) {
        let t = moment(at);
        prop_assert_eq!(folds(&log, t, &Everything), folds(&log, t, &Untrusted::none()));

        let nodes = chain_of(&log);
        for row in view::entries(&nodes, t, &Everything).expect("the log folds") {
            prop_assert_eq!(row.claim, None, "nothing is refused, so nothing is claimed");
            prop_assert_eq!(row.confidence, view::Confidence::Confirmed, "confidence");
        }
    }

    /// §9.11 — A CLAIMING EVENT NEVER MOVES THE CONFIRMED READING.
    ///
    /// Append one event the policy only lets CLAIM — a lifecycle write or a
    /// repricing from an actor on the roster, at any instant, about any todo —
    /// and every confirmed answer is the answer it was: the environment, the
    /// specs, the content and the functions. That is what
    /// "stored and shown but not folded" MEANS, and it is the containment the
    /// roster is kept for.
    ///
    /// Asked at several instants, including instants before the claim, because
    /// a fold that let a claim through at one moment and not another would
    /// still pass at one moment.
    #[test]
    fn a_claiming_event_never_moves_the_confirmed_reading(
        log in a_log(),
        claim in a_claim(),
        when in 0i64..WINDOW,
        asked in prop::collection::vec(0i64..WINDOW, 1..5),
    ) {
        let policy = roster();
        let claim = claim.at(moment(when));
        prop_assert!(
            policy.standing(&event_id(&claim), &claim).claims(),
            "the generator draws a claim"
        );

        let mut extended = log.clone();
        extended.push(claim);
        for seconds in asked.into_iter().chain([when, WINDOW]) {
            let t = moment(seconds);
            prop_assert_eq!(folds(&extended, t, &policy), folds(&log, t, &policy));
        }
    }

    /// §9.12 — STANDING SELECTS EVENTS, NOT POSITIONS.
    ///
    /// [`Policy::standing`] is a function of the event alone: not of the log it
    /// sits in, not of where in the log it sits, not of the moment being asked
    /// about. So folding under a policy IS folding the sub-log of the events it
    /// binds — which is checked here against the ONE policy that binds
    /// everything — and that stays true under any permutation of the log,
    /// because filtering commutes with reordering. Which events count is a
    /// function of the event SET; only which of them WINS is a function of the
    /// order, and §9.3 is that half.
    #[test]
    fn standing_selects_events_and_not_positions(
        log in a_log(),
        keys in prop::collection::vec(0u32..1000, 0..25),
        at in 0i64..WINDOW,
    ) {
        let policy = roster();
        let t = moment(at);
        let mut order: Vec<usize> = (0..log.len()).collect();
        order.sort_by_key(|i| (keys.get(*i).copied().unwrap_or(0), *i));
        let shuffled: Vec<Event> = order.iter().map(|i| log[*i].clone()).collect();

        for log in [&log, &shuffled] {
            let kept: Vec<Event> = log
                .iter()
                .filter(|event| policy.standing(&event_id(*event), *event).binds())
                .cloned()
                .collect();
            // `authored_at` takes no policy and reads EVERY record, so the
            // sub-log has to keep them; the reference policy binds them, which
            // is why this comparison is about the other four folds and this
            // assertion is what says so.
            prop_assert_eq!(
                kept.iter().filter(|e| matches!(e, TodoEvent::Authored(_))).count(),
                log.iter().filter(|e| matches!(e, TodoEvent::Authored(_))).count()
            );
            prop_assert_eq!(folds(&kept, t, &Everything), folds(log, t, &policy));
        }
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// A TENDING LEAVES STATE ALONE. Append a binding `Tended` about any todo
    /// at any instant, and at every moment every outcome, spec, content record
    /// and function is what it was, no register is written, and every entry
    /// keeps its state, its function and its content — an open todo stays on
    /// the open list. Only the tendings grow, and only from the tending on.
    #[test]
    fn a_tending_changes_no_state_spec_or_content(
        log in a_log(),
        todo in prop::sample::select(common::TODOS.to_vec()),
        when in 0i64..WINDOW,
        asked in prop::collection::vec(0i64..WINDOW, 1..5),
    ) {
        let policy = roster();
        let mut tended = log.clone();
        tended.push(mk_tended(todo, moment(when), "bassel", "").expect("valid"));
        let (before, after) = (chain_of(&log), chain_of(&tended));
        let id = TodoId::new(todo).expect("valid");
        for seconds in asked.into_iter().chain([when, WINDOW]) {
            let t = moment(seconds);
            let (was, is) = (folds(&log, t, &policy), folds(&tended, t, &policy));
            prop_assert_eq!(&is.env.outcomes, &was.env.outcomes);
            prop_assert_eq!(&is.specs, &was.specs);
            prop_assert_eq!(&is.content, &was.content);
            prop_assert_eq!(&is.flatten, &was.flatten);
            let grown = is.env.tended.get(id.as_str()).is_some_and(|set| set.contains(&fpl::instant_of(moment(when))));
            prop_assert_eq!(grown, seconds >= when);

            let (unwritten, written) = (fold(&before), fold(&after));
            let frontiers = |state: &State| -> Vec<(TodoId, Vec<Vec<Hash>>)> {
                state.entities().map(|(todo, stream)| {
                    let registers = prodrome::fold::read(stream, Some(fpl::instant_of(t)), &policy);
                    let names = [Kind::State, Kind::Spec, Kind::Content]
                        .map(|kind| registers.frontier(kind).names());
                    (todo.clone(), names.to_vec())
                })
                .filter(|(_, names)| names.iter().any(|n| !n.is_empty()))
                .collect()
            };
            prop_assert_eq!(frontiers(&written), frontiers(&unwritten));

            let rows = view::entries(&before, t, &policy).expect("the log folds");
            for row in view::entries(&after, t, &policy).expect("the log folds") {
                match rows.iter().find(|was| was.key == row.key) {
                    Some(was) => {
                        prop_assert_eq!(row.outcome(), was.outcome(), "state");
                        prop_assert_eq!(&row.claim, &was.claim, "claim");
                        prop_assert_eq!(row.spec(), was.spec(), "spec");
                        prop_assert_eq!(row.content(), was.content(), "content");
                    }
                    // The tending is the todo's first mention: an open row,
                    // unpriced and with no content, like any mention.
                    None => {
                        prop_assert!(row.is_open());
                        prop_assert_eq!(row.spec(), &fpl::mk_absent());
                        prop_assert!(row.content().is_empty());
                    }
                }
            }
        }
    }
}

/// THE ONE PLACE A LATER EVENT REACHES BACK, named with its witnesses.
///
/// `flatten`'s head is `checklist(first spec ever, first checklist ever)`, and
/// a todo can have a function from EITHER half alone: a checklist with no spec
/// is `Conj(n × Flat(0.5))`, and a spec with no content record is the spec. So
/// the event that records the OTHER half for the first time re-heads the curve
/// at every earlier moment, once each way below.
///
/// The reference's generator draws exactly this — an `Authored`
/// carries a spec 85% of the time and subtodos some of the time, and a
/// `SpecRevised` prices a todo that has no content record at all — where
/// `tests/test_laws.py`'s generator never does (its authored records always
/// carry a spec and never a subtodo), which is why the reference's own prefix
/// law never meets it.
///
/// This is the REFERENCE's behaviour and not a porting error: every number and
/// print below was read off the reference's `events.py` on these exact
/// inputs. Pinning it keeps the exception a decision instead of a surprise.
#[test]
fn a_late_first_half_of_the_head_re_heads_the_curve() {
    let policy = roster();
    let start = moment(0);
    let tau = moment(1);
    let env = prodrome::fpl::Env::new();
    let now = fpl::instant_of(start);

    // A checklist with no spec, priced for the first time at tau.
    let checklist_only = mk_authored(
        "gamma",
        start,
        "bassel",
        "todo",
        start,
        "body",
        None,
        vec![],
        "",
        "",
        "",
        None,
        vec![
            mk_subtodo("item 0", true).expect("valid"),
            mk_subtodo("item 1", false).expect("valid"),
        ],
        vec![],
        "",
    )
    .expect("valid");
    let first_price = mk_authored(
        "gamma",
        tau,
        "bassel",
        "todo",
        tau,
        "body",
        Some(fpl::mk_flat(0.02).expect("in [0, 1]")),
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
    let gamma = TodoId::new("gamma").expect("valid");
    let before = flatten(std::slice::from_ref(&checklist_only), tau, &policy).expect("folds");
    let after = flatten(&[checklist_only, first_price], tau, &policy).expect("folds");
    assert_eq!(
        print_term(&before[&gamma]),
        "Conj(terms=(Flat(value=0.5), Flat(value=0.5)), p=-4.0)"
    );
    assert_eq!(
        print_term(&after[&gamma]),
        "Piecewise(head=OffsetBy(delta=Flat(value=0.02), \
         term=Conj(terms=(Flat(value=0.5), Flat(value=0.5)), p=-4.0)), \
         pieces=(Piece(at=datetime(2026, 9, 1, 0, 0, 1), term=Flat(value=0.02)),))"
    );
    assert_eq!(
        fpl::fulfillment(&closed(&before[&gamma]), now, &env),
        Some(0.5)
    );
    assert_eq!(
        fpl::fulfillment(&closed(&after[&gamma]), now, &env),
        Some(0.51)
    );

    // And the other way: a spec with no content record, given a checklist for
    // the first time at tau.
    let priced = mk_spec_revised(
        "beta",
        start,
        "bassel",
        fpl::mk_flat(0.5).expect("in [0, 1]"),
        "",
    )
    .expect("valid");
    let first_checklist = mk_authored(
        "beta",
        tau,
        "bassel",
        "todo",
        tau,
        "body",
        None,
        vec![],
        "",
        "",
        "",
        None,
        vec![
            mk_subtodo("item 0", true).expect("valid"),
            mk_subtodo("item 1", false).expect("valid"),
        ],
        vec![],
        "",
    )
    .expect("valid");
    let beta = TodoId::new("beta").expect("valid");
    let before = flatten(std::slice::from_ref(&priced), tau, &policy).expect("folds");
    let after = flatten(&[priced, first_checklist], tau, &policy).expect("folds");
    assert_eq!(print_term(&before[&beta]), "Flat(value=0.5)");
    assert_eq!(
        print_term(&after[&beta]),
        "OffsetBy(delta=Flat(value=0.5), \
         term=Conj(terms=(Flat(value=0.5), Flat(value=0.5)), p=-4.0))"
    );
    assert_eq!(
        fpl::fulfillment(&closed(&before[&beta]), now, &env),
        Some(0.5)
    );
    assert_eq!(
        fpl::fulfillment(&closed(&after[&beta]), now, &env),
        Some(0.75)
    );
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A scratch root that cleans itself up when the law is done with it.
struct Replicas(std::path::PathBuf);

impl Drop for Replicas {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Two replicas that diverged: a shared history, then concurrent writes on
/// each side, then an import. Built the only way a second head can appear.
fn diverged(shared: &[Event], mine: &[Event], theirs: &[Event]) -> (Replicas, Store) {
    let root = std::env::temp_dir().join(format!(
        "prodrome-laws-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    let guard = Replicas(root.clone());
    let here = Store::new(root.join("here"), roster());
    for event in shared {
        seal(&here, event.clone());
    }
    let there = Store::new(root.join("there"), roster());
    for tip in here.tips().expect("the tips derive") {
        there.adopt(&here, &tip, Sender::Stranger).expect("adopts");
    }
    for event in mine {
        seal(&here, event.clone());
    }
    for event in theirs {
        seal(&there, event.clone());
    }
    for tip in there.tips().expect("the tips derive") {
        here.adopt(&there, &tip, Sender::Stranger).expect("adopts");
    }
    (guard, here)
}

fn nodes_from(store: &Store) -> Vec<Chain> {
    store
        .dag()
        .and_then(|dag| dag.nodes())
        .expect("the DAG reads")
}

/// A WORLD of a legacy store: the same history with the `dropped` writes'
/// events taken out, as a history of its own. Each object is printed again,
/// parents first, over its parents' new names, and a dropped write becomes a
/// `Woven` carrying no event over the parents it had: the shape is the
/// store's, so every write kept descends from exactly what it descended from.
/// A node is only ever the DAG's, so a world is a DAG and never an edited node.
fn world(store: &Store, dropped: &BTreeSet<&Hash>) -> Vec<Chain> {
    let dag = store.dag().expect("the DAG reads");
    let mut renamed: BTreeMap<Hash, Hash> = BTreeMap::new();
    let mut objects: Vec<(Hash, Envelope<Event>)> = Vec::new();
    for name in dag.linearise().expect("the DAG linearises") {
        let held = dag.get(name).expect("a linearised object is held");
        let parents: Vec<Hash> = parents_of(held)
            .iter()
            .map(|parent| renamed[parent].clone())
            .collect();
        let object = match (held, dropped.contains(name)) {
            (_, true) => Envelope::Woven {
                parents,
                event: None,
            },
            (Envelope::Sealed { event, .. }, false) => Envelope::Sealed {
                prev: parents.into_iter().next(),
                event: event.clone(),
            },
            (Envelope::Woven { event, .. }, false) => Envelope::Woven {
                parents,
                event: event.clone(),
            },
            (other, false) => panic!("{} is not a legacy object", other.name()),
        };
        let print = seal_hash(&object);
        renamed.insert(name.clone(), print.clone());
        objects.push((print, object));
    }
    objects
        .into_iter()
        .collect::<Dag<Event>>()
        .nodes()
        .expect("a world is a DAG")
}

fn written(events: &[Event], policy: &Untrusted) -> BTreeSet<(Kind, TodoId)> {
    events
        .iter()
        .filter(|event| policy.standing(&event_id(*event), *event).binds())
        .flat_map(|event| {
            Write::of(event)
                .filter_map(|write| write.kind())
                .map(|kind| (kind, event.todo().clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The registers of `nodes` with more than one live write.
fn conflicted(nodes: &[Chain], policy: &Untrusted) -> BTreeSet<(Kind, TodoId)> {
    fold(nodes)
        .entities()
        .flat_map(|(todo, stream)| {
            prodrome::fold::read(stream, None, policy)
                .conflicts()
                .into_keys()
                .map(|kind| (kind, todo.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The environment `nodes` fold to at `t`.
fn env_of(nodes: &[Chain], t: Datetime, policy: &impl Policy<TodoEvent<Todo>>) -> Env {
    let state = fold(nodes);
    let mut env = Env::new();
    for (todo, stream) in state.entities() {
        prodrome::fold::read(stream, Some(fpl::instant_of(t)), policy).bind(todo, &mut env);
    }
    env
}

fn branch(drafts: &[(Draft, i64)], note: &str) -> Vec<Event> {
    drafts
        .iter()
        .map(|(d, at)| d.at_with_note(moment(*at), note))
        .collect()
}

/// A draft that writes exactly one register, from a writer the policy binds:
/// `SpecRevised`, `Completed`, `Cancelled` or `Reopened`.
fn a_single_write() -> impl Strategy<Value = (Draft, i64)> {
    ((a_draft(), 3u8..7), 0i64..WINDOW).prop_map(|((draft, roll), at)| {
        (
            Draft {
                actor: "bassel",
                roll,
                ..draft
            },
            at,
        )
    })
}

/// Law 25's other order: a topological order of `nodes` drawn by `keys`.
fn relinearised(nodes: &[Chain], keys: &[u32]) -> Vec<Chain> {
    let mut placed: BTreeSet<&Hash> = BTreeSet::new();
    let mut left: Vec<(u32, &Chain)> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (keys.get(i).copied().unwrap_or(0), node))
        .collect();
    let mut out = Vec::with_capacity(nodes.len());
    while !left.is_empty() {
        let ready = left
            .iter()
            .enumerate()
            .filter(|(_, (_, node))| node.parents().iter().all(|p| placed.contains(p)))
            .min_by_key(|(_, (key, node))| (*key, node.name()))
            .map(|(i, _)| i)
            .expect("a DAG always has a ready node");
        let (_, node) = left.remove(ready);
        placed.insert(node.name());
        out.push(node.clone());
    }
    out
}

proptest! {
    // Each case builds two stores on disk and imports one into the other, so
    // the budget buys graphs rather than repetitions.
    #![proptest_config(crate::common::cases::cases(24))]

    /// §6.6 — CONFLICTS ARE EXACTLY THE REGISTERS BOTH BRANCHES WROTE (the
    /// shared prefix's writes are ancestors of both and never conflict); A
    /// MERGE SETTLES NOTHING, because it carries no write; and a write that
    /// DESCENDS FROM BOTH settles every register it writes.
    #[test]
    fn conflicts_are_the_concurrent_writes_and_a_descending_write_settles(
        shared in a_schedule(1..8),
        mine in a_schedule(1..6),
        theirs in a_schedule(1..6),
    ) {
        let policy = roster();
        let (mine, theirs) = (branch(&mine, "mine"), branch(&theirs, "theirs"));
        let (_guard, store) = diverged(&realise(&shared, 0), &mine, &theirs);

        let expected: BTreeSet<(Kind, TodoId)> = written(&mine, &policy)
            .intersection(&written(&theirs, &policy))
            .cloned()
            .collect();
        prop_assert_eq!(conflicted(&nodes_from(&store), &policy), expected.clone());

        if store.tips().expect("the tips derive").len() > 1 {
            store.merge(None, None).expect("merges");
            prop_assert_eq!(
                conflicted(&nodes_from(&store), &policy),
                expected.clone(),
                "a merge carries no write and settles nothing"
            );
            let settle = mk_completed("alpha", far(), "bassel", "settled").expect("valid");
            seal(&store, settle);
            let alpha = TodoId::new("alpha").expect("valid");
            let mut left = expected;
            left.remove(&(Kind::State, alpha));
            prop_assert_eq!(conflicted(&nodes_from(&store), &policy), left);
        }
    }

    /// §6.7 — THE ENTRY IS THE COMPOSITION IT NAMES: each field the todo's
    /// registers, its prodrome's functions and §7 over them, one row per
    /// todo any event mentions whatever `t` is.
    #[test]
    fn the_entry_is_the_composition_of_the_folds_it_names(
        shared in a_schedule(1..8),
        mine in a_schedule(1..6),
        theirs in a_schedule(1..6),
        when in 0i64..WINDOW,
    ) {
        let policy = roster();
        let (_guard, store) =
            diverged(&realise(&shared, 0), &branch(&mine, "mine"), &branch(&theirs, "theirs"));
        let nodes = nodes_from(&store);
        let (t, now) = (moment(when), fpl::instant_of(moment(when)));
        let rows = view::entries(&nodes, t, &policy).expect("the DAG folds");
        let state = fold(&nodes);
        let prodrome = state.prodromes().get(&None).expect("a legacy prodrome");

        prop_assert_eq!(
            rows.iter().map(|row| &row.key).collect::<Vec<_>>(),
            prodrome.keys().collect::<Vec<_>>()
        );
        let functions = prodrome::fold::flatten(prodrome, now, &policy).expect("flattens");
        let specs = prodrome::fold::link_specs(&functions, prodrome.keys());
        let env = prodrome::fold::env(prodrome, now, &policy);
        for row in &rows {
            let stream = &prodrome[&row.key];
            let registers = prodrome::fold::read(stream, Some(now), &policy);
            prop_assert_eq!(row.outcome(), &registers.outcomes(), "outcome");
            let claimed = prodrome::fold::read(stream, Some(now), &Everything).outcomes();
            let kinds = |c: &fpl::Candidates| -> BTreeSet<&str> {
                c.iter().map(|b| b.map_or("open", |b| b.kind())).collect()
            };
            let disputed = kinds(&claimed) != kinds(&registers.outcomes());
            prop_assert_eq!(row.claim.as_ref().map(|claim| &claim.outcome), disputed.then_some(&claimed), "claim");
            let spec = functions.get(&row.key).cloned().unwrap_or_else(fpl::mk_absent);
            prop_assert_eq!(row.spec(), &spec, "spec");
            let linked = fpl::link(&spec, &specs);
            prop_assert_eq!(row.value(), linked.as_ref().map(|l| fpl::fulfillment(l, now, &env)));
            let names: Vec<Hash> = registers.content.candidates().iter().map(|s| s.name.clone()).collect();
            prop_assert_eq!(row.content(), &names, "content");
            prop_assert_eq!(&row.conflicts, &registers.conflicts(), "conflicts");
            let provisional = disputed
                || registers.content.candidates().iter().any(|s| !policy.confirms(&s.name, &*s.event));
            prop_assert_eq!(row.confidence.is_provisional(), provisional, "confidence");
            let names: Vec<&Hash> = stream.iter().map(|s| &s.name).collect();
            prop_assert_eq!(row.stream.iter().collect::<Vec<_>>(), names, "stream");
        }
    }

    /// Law 24, first clause — ONE CANDIDATE READS AS BEFORE. Where no two
    /// writes to one register are concurrent, the DAG reads exactly as the
    /// chain of its linearisation, the reading before candidates existed.
    #[test]
    fn one_candidate_reads_as_before(
        shared in a_schedule(1..8),
        mine in a_schedule(1..3),
        theirs in a_schedule(1..3),
        when in 0i64..WINDOW,
    ) {
        let policy = roster();
        let (_guard, store) =
            diverged(&realise(&shared, 0), &branch(&mine, "mine"), &branch(&theirs, "theirs"));
        let nodes = nodes_from(&store);
        let state = fold(&nodes);
        let chained = state.entities().all(|(_, stream)| {
            stream.iter().all(|a| stream.iter().all(|b| {
                a.name == b.name
                    || state.descends(&a.name, &b.name)
                    || state.descends(&b.name, &a.name)
                    || !Write::of(&a.event).any(|w| Write::of(&b.event).any(|v| w.kind().is_some() && w.kind() == v.kind()))
            }))
        });
        if chained {
            let t = moment(when);
            let linear: Vec<Event> = nodes.iter().filter_map(|node| node.event().cloned()).collect();
            let dag = view::entries(&nodes, t, &policy).expect("folds");
            let chain = view::entries(&chain_of(&linear), t, &policy).expect("folds");
            for (a, b) in dag.iter().zip(&chain) {
                let claimed = |row: &view::Entry<Event>| row.claim.as_ref().map(|claim| claim.outcome.clone());
                prop_assert_eq!((&a.key, a.outcome(), claimed(a)), (&b.key, b.outcome(), claimed(b)));
                prop_assert_eq!((a.spec(), a.value(), a.confidence), (b.spec(), b.value(), b.confidence));
            }
        }
    }

    /// Law 24 — A CONFLICT PRICES AS ITS MOST URGENT WORLD. Each world keeps
    /// one candidate of each register and drops the others' events; the row's
    /// value is the least of the worlds' values.
    #[test]
    fn conflict_is_its_most_urgent_world(
        shared in a_schedule(1..8),
        mine in prop::collection::vec(a_single_write(), 1..5),
        theirs in prop::collection::vec(a_single_write(), 1..5),
        when in 0i64..WINDOW,
    ) {
        let policy = roster();
        let (_guard, store) =
            diverged(&realise(&shared, 0), &branch(&mine, "mine"), &branch(&theirs, "theirs"));
        let nodes = nodes_from(&store);
        let state = &fold(&nodes);
        for t in [moment(when), far()] {
        let now = fpl::instant_of(t);
        for row in view::entries(&nodes, t, &policy).expect("folds") {
            let stream = &state.prodromes()[&None][&row.key];
            let registers = prodrome::fold::read(stream, Some(now), &policy);
            if registers.state.candidates().len() < 2 && registers.spec.candidates().len() < 2 {
                continue;
            }
            let choices = |kind: Kind| -> Vec<Option<Hash>> {
                let names: Vec<Option<Hash>> =
                    registers.frontier(kind).candidates().iter().map(|s| Some(s.name.clone())).collect();
                if names.is_empty() { vec![None] } else { names }
            };
            let mut least: Option<f64> = None;
            for state_pick in choices(Kind::State) {
                for spec_pick in choices(Kind::Spec) {
                    // The world keeps its pick of each register and what the
                    // pick descends from, and drops every other write to it.
                    let dropped: BTreeSet<&Hash> = [(Kind::State, &state_pick), (Kind::Spec, &spec_pick)]
                        .into_iter()
                        .filter_map(|(kind, pick)| pick.as_ref().map(|pick| (kind, pick)))
                        .flat_map(|(kind, pick)| {
                            stream
                                .iter()
                                .filter(move |s| Write::of(&s.event).any(|w| w.kind() == Some(kind)))
                                .map(|s| &s.name)
                                .filter(move |name| *name != pick && !state.descends(pick, name))
                        })
                        .collect();
                    let world = world(&store, &dropped);
                    let rows = view::entries(&world, t, &policy).expect("folds");
                    let value = rows.iter().find(|r| r.key == row.key).expect("a row").value();
                    if let Ok(Some(value)) = value {
                        least = Some(least.map_or(value, |l: f64| l.min(value)));
                    }
                }
            }
            prop_assert_eq!(row.value(), Ok(least), "{:?}", row.key);
        }
        }
    }

    /// Law 25 — THE LINEARISATION DECIDES NO VALUE. Any topological order of
    /// the DAG folds to the same entries; only the streams' order may differ.
    #[test]
    fn any_linear_extension_folds_alike(
        shared in a_schedule(1..8),
        mine in a_schedule(1..6),
        theirs in a_schedule(1..6),
        keys in prop::collection::vec(0u32..1000, 0..30),
        when in 0i64..WINDOW,
    ) {
        let policy = roster();
        let (_guard, store) =
            diverged(&realise(&shared, 0), &branch(&mine, "mine"), &branch(&theirs, "theirs"));
        let nodes = nodes_from(&store);
        let t = moment(when);
        let sorted = |nodes: &[Chain]| -> Vec<view::Entry<Event>> {
            let mut rows = view::entries(nodes, t, &policy).expect("folds");
            for row in &mut rows {
                row.stream.sort();
            }
            rows
        };
        prop_assert_eq!(sorted(&relinearised(&nodes, &keys)), sorted(&nodes));
    }

    /// Law 27 — A CLAIM SETTLES NOTHING IT IS NOT TRUSTED TO. A claiming write
    /// over every tip leaves every confirmed frontier, and every price, as it
    /// was.
    #[test]
    fn claim_never_settles(
        shared in a_schedule(1..8),
        mine in a_schedule(1..6),
        theirs in a_schedule(1..6),
        claim in a_claim(),
        when in 0i64..WINDOW,
    ) {
        let policy = roster();
        let (_guard, store) =
            diverged(&realise(&shared, 0), &branch(&mine, "mine"), &branch(&theirs, "theirs"));
        let t = moment(when);
        let frontiers = |nodes: &[Chain]| -> BTreeMap<TodoId, [Vec<Hash>; 3]> {
            fold(nodes).entities().map(|(todo, stream)| {
                let registers = prodrome::fold::read(stream, Some(fpl::instant_of(t)), &policy);
                (todo.clone(), [Kind::State, Kind::Spec, Kind::Content].map(|k| registers.frontier(k).names()))
            })
            .collect()
        };
        let prices = |nodes: &[Chain]| -> BTreeMap<TodoId, (fpl::Candidates, view::Price)> {
            view::entries(nodes, t, &policy)
                .expect("folds")
                .into_iter()
                .map(|row| (row.key.clone(), (row.reading.outcome, row.price)))
                .collect()
        };
        let before = nodes_from(&store);
        let claim = claim.at(moment(when));
        let todo = claim.todo().clone();
        seal(&store, claim);
        let after = nodes_from(&store);
        let (mut was, is) = (frontiers(&before), frontiers(&after));
        was.entry(todo.clone()).or_insert_with(|| [vec![], vec![], vec![]]);
        prop_assert_eq!(is, was);
        let (mut was, is) = (prices(&before), prices(&after));
        if let Some(fresh) = is.get(&todo).filter(|_| !was.contains_key(&todo)) {
            was.insert(todo, fresh.clone());
        }
        prop_assert_eq!(is, was);
    }
}

fn tendings(drawn: &[(&'static str, i64)], note: &str) -> Vec<Event> {
    drawn
        .iter()
        .map(|(todo, at)| mk_tended(todo, moment(*at), "bassel", note).expect("valid"))
        .collect()
}

fn a_tending() -> impl Strategy<Value = (&'static str, i64)> {
    (prop::sample::select(common::TODOS.to_vec()), 0i64..WINDOW)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(24))]

    /// TENDINGS MERGE BY UNION. Two replicas tend concurrently; the merged
    /// store's tendings are the union of what each side folds to, before the
    /// merge object and after it, and not one register — so not one conflict
    /// — is written by a tending.
    #[test]
    fn tendings_merge_by_union(
        shared in a_schedule(1..6),
        mine in prop::collection::vec(a_tending(), 1..5),
        theirs in prop::collection::vec(a_tending(), 1..5),
    ) {
        let policy = roster();
        let shared = realise(&shared, 0);
        let (mine, theirs) = (tendings(&mine, "mine"), tendings(&theirs, "theirs"));
        let side = |branch: &[Event]| {
            env_at(&[shared.clone(), branch.to_vec()].concat(), far(), &policy).tended
        };
        let mut union = side(&mine);
        for (todo, set) in side(&theirs) {
            union.entry(todo).or_default().extend(set);
        }

        let (_guard, store) = diverged(&shared, &mine, &theirs);
        let nodes = nodes_from(&store);
        prop_assert_eq!(&env_of(&nodes, far(), &policy).tended, &union);
        prop_assert!(conflicted(&nodes, &policy).is_empty(), "a tending wrote a register");
        if store.tips().expect("the tips derive").len() > 1 {
            store.merge(None, None).expect("merges");
            prop_assert_eq!(&env_of(&nodes_from(&store), far(), &policy).tended, &union);
        }
    }
}

/// Every object file of `from`, copied into `to/objects/` — what `git merge`
/// of two clones does to a store's directory, since every name is new or the
/// same bytes.
fn copy_files(from: &Store, to: &std::path::Path) {
    let objects = to.join("objects");
    fs::create_dir_all(&objects).expect("creates objects/");
    for entry in fs::read_dir(from.root().join("objects")).expect("lists objects/") {
        let path = entry.expect("an entry").path();
        fs::copy(&path, objects.join(path.file_name().expect("a name"))).expect("copies");
    }
}

/// Two CLONES of one store: a shared history, copied as files, then each
/// writer appending to its own copy with nothing named as a parent — exactly
/// what two people with a git checkout of the same store each do.
fn clones(shared: &[Event], mine: &[Event], theirs: &[Event]) -> (Replicas, Store, Store) {
    let root = std::env::temp_dir().join(format!(
        "prodrome-clones-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    let guard = Replicas(root.clone());
    let here = Store::new(root.join("here"), roster());
    for event in shared {
        seal(&here, event.clone());
    }
    let there = Store::new(root.join("there"), roster());
    copy_files(&here, there.root());
    for event in mine {
        seal(&here, event.clone());
    }
    for event in theirs {
        seal(&there, event.clone());
    }
    (guard, here, there)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(24))]

    /// §9.17 b — A GIT MERGE IS THE PRODROME'S MERGE. Two writers append to
    /// clones of one store; their object directories, unioned as files, read
    /// EXACTLY as the Prodrome's own replica merge of the two stores does
    /// (`adopt` of every tip the other side has): the same tips, the same
    /// linearisation, the same registers and the same entries at every moment
    /// asked. Settling it is the same object on both routes — whether by a bare
    /// `merge` or by the next `append`, which weaves every tip.
    #[test]
    fn a_union_of_two_clones_folds_as_the_prodrome_merge(
        shared in a_schedule(1..8),
        mine in a_schedule(0..6),
        theirs in a_schedule(0..6),
        when in 0i64..WINDOW,
    ) {
        let policy = roster();
        let (guard, here, there) =
            clones(&realise(&shared, 0), &branch(&mine, "mine"), &branch(&theirs, "theirs"));

        let unioned = Store::new(guard.0.join("unioned"), roster());
        copy_files(&here, unioned.root());
        copy_files(&there, unioned.root());
        let adopted = Store::new(guard.0.join("adopted"), roster());
        copy_files(&here, adopted.root());
        for tip in there.tips().expect("the tips derive") {
            adopted.adopt(&there, &tip, Sender::Stranger).expect("adopts");
        }

        prop_assert_eq!(unioned.tips().expect("derives"), adopted.tips().expect("derives"));
        let (by_files, by_adoption) = (nodes_from(&unioned), nodes_from(&adopted));
        prop_assert_eq!(&by_files, &by_adoption);
        prop_assert_eq!(fold(&by_files), fold(&by_adoption));
        let t = moment(when);
        prop_assert_eq!(
            view::entries(&by_files, t, &policy).expect("folds"),
            view::entries(&by_adoption, t, &policy).expect("folds")
        );
        prop_assert_eq!(unioned.verify(), adopted.verify());

        if unioned.tips().expect("derives").len() > 1 {
            let settle = mk_completed("alpha", far(), "bassel", "settled").expect("valid");
            let woven = seal(&unioned, settle.clone());
            prop_assert_eq!(unioned.tips().expect("derives"), [woven.clone()].into_iter().collect());
            let named: Vec<prodrome::event::Hash> =
                adopted.tips().expect("derives").into_iter().collect();
            prop_assert_eq!(adopted.merge(Some(&named), Some(settle)).expect("merges"), woven);
        }
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// §9.18: `link(Ref(x))` against a store's specs is `Absent` exactly when
    /// the store knows `x` and it has no function, `x`'s own function linked
    /// when it has one, and `Unknown(x)` for an id no event names.
    #[test]
    fn a_reference_links_to_absent_exactly_for_a_known_todo_with_no_function(
        log in a_log_with_absence(),
        asked in 0i64..WINDOW,
    ) {
        let (t, policy) = (moment(asked), roster());
        let functions = flatten(&log, t, &policy).expect("the log folds");
        let known: BTreeSet<&TodoId> = log.iter().map(TodoEvent::todo).collect();
        let specs = prodrome::fold::link_specs(&functions, known.iter().copied());
        for todo in TODOS.iter().chain(&["delta"]) {
            let id = TodoId::new(*todo).expect("a todo id");
            let linked = fpl::link(&fpl::mk_ref((*todo).to_owned()).expect("a todo id"), &specs);
            match (known.contains(&id), functions.get(&id)) {
                (false, _) => prop_assert_eq!(
                    linked,
                    Err(fpl::LinkError::Unknown((*todo).to_owned())),
                    "Ref({}) never seen",
                    todo
                ),
                (true, None) => prop_assert_eq!(
                    linked,
                    Ok(fpl::Closed::of(fpl::mk_absent()).expect("closed")),
                    "Ref({}) known, no function",
                    todo
                ),
                // A loop is named from where it was entered, so only what it
                // links to is compared.
                (true, Some(function)) => prop_assert_eq!(
                    linked.ok(),
                    fpl::link(function, &specs).ok(),
                    "Ref({}) with a function",
                    todo
                ),
            }
        }
    }

    /// §9.9 over the same logs: every row's function is `flatten`'s, `Absent`
    /// where there is none, and its value is that function linked against
    /// every known todo's, read under the confirmed environment — a number,
    /// `∅`, or the `LinkError`.
    #[test]
    fn every_row_reads_its_function_linked_over_what_the_store_knows(
        log in a_log_with_absence(),
        asked in 0i64..WINDOW,
    ) {
        let (t, policy) = (moment(asked), roster());
        let rows = view::entries(&chain_of(&log), t, &policy).expect("the log folds");
        let functions = flatten(&log, t, &policy).expect("the log folds");
        let env = env_at(&log, t, &policy);
        let specs = prodrome::fold::link_specs(&functions, rows.iter().map(|row| &row.key));
        for row in &rows {
            let spec = functions.get(&row.key).cloned().unwrap_or_else(fpl::mk_absent);
            prop_assert_eq!(row.spec(), &spec);
            let linked = fpl::link(&spec, &specs);
            prop_assert_eq!(
                row.value(),
                linked.as_ref().map(|linked| fpl::fulfillment(linked, fpl::instant_of(t), &env))
            );
        }
    }
}
