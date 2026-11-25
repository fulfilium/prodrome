//! The chain compiler (SPEC §7.1): it rewrites a term against a snapshot of
//! the environment, so that no `After` or `Recur` is left.
//!
//! Law 13: the compiled term, read against the empty environment, has the
//! fulfillment of the original term read against the real environment, at
//! every instant. The readers of a compiled term call the evaluator of `fpl`,
//! so this module is not a second evaluator (SPEC §10). For the cost that the
//! compiler saves, see SPEC §7.1, "The cost."

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;

use thiserror::Error;

use crate::fpl::{
    self, piecewise, scalars, Closed, Delta, Env, Explanation, Instant, Note, Outcome, Scalar,
};
use crate::schedule::Schedule;
use crate::term::schema::{Fields, Slot};
use crate::term::{normalize, Term, TermF};
use prodrome_data::topo::Topo;

/// A refusal from [`compile_chain`] or [`chain_order`].
///
/// A chain that depends on itself gets this refusal, never a partial answer
/// (SPEC §7.1, "Cycles are a value").
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChainError {
    /// The links draw a dependency graph over the todos, and the graph has a
    /// cycle. The path holds the loop, with its first todo repeated at the end.
    #[error("the chain depends on itself: {}", .0.join(" → "))]
    Cycle(Vec<String>),
}

/// One `After` node, as the compiler resolved it against the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// The snapshot holds no binding of the upstream, so the link reads its
    /// pending branch at every instant. A later snapshot can compile the link
    /// differently.
    Pending { event: String },
    /// The upstream completed at `at`, `slip` after the link's `anchor`, so
    /// the body slides by `slip` from `at` on.
    ///
    /// `ready` is `at + needs` where the link declares a lead time. Only the
    /// compiler reads `needs`, never the evaluator (SPEC §7.1, "`needs` is the
    /// COMPILER's field, not the evaluator's").
    Completed {
        event: String,
        at: Instant,
        slip: Delta,
        needs: Option<Delta>,
        ready: Option<Instant>,
    },
    /// The upstream was cancelled at `at`, and from `at` the link reads 1.0.
    ///
    /// A reader must report this link, because a 1.0 from a cancellation looks
    /// the same as a 1.0 from a demand met (SPEC §7.1, "Cancellation is
    /// reported, never silent").
    Moot { event: String, at: Instant },
}

impl Link {
    /// The upstream this link names.
    pub fn event(&self) -> &str {
        match self {
            Link::Pending { event } | Link::Completed { event, .. } | Link::Moot { event, .. } => {
                event
            }
        }
    }

    /// The link's graded offset δ ∈ [0, 1]: 1 where the upstream was
    /// cancelled, and 0 elsewhere (SPEC §7.1, "Per link.").
    pub fn grade(&self) -> f64 {
        match self {
            Link::Moot { .. } => 1.0,
            Link::Pending { .. } | Link::Completed { .. } => 0.0,
        }
    }

    /// The instant at which the link binds, or `None` where the snapshot does
    /// not bind it.
    pub fn at(&self) -> Option<Instant> {
        match self {
            Link::Pending { .. } => None,
            Link::Completed { at, .. } | Link::Moot { at, .. } => Some(*at),
        }
    }

    /// The word `explain` writes for this reading, the same three the
    /// interpreted `After`'s `bound` note uses.
    fn bound(&self) -> &'static str {
        match self {
            Link::Pending { .. } => "pending",
            Link::Completed { .. } => "completed",
            Link::Moot { .. } => "cancelled",
        }
    }

    /// The link's fields, which the chain notes and the wasm wire render.
    pub fn fields(&self) -> Fields<Infallible> {
        let (slip, needs, ready) = match self {
            Link::Completed {
                slip, needs, ready, ..
            } => (
                Slot::Span(*slip),
                needs.map_or(Slot::Nothing, Slot::Span),
                ready.map_or(Slot::Nothing, Slot::At),
            ),
            Link::Pending { .. } | Link::Moot { .. } => {
                (Slot::Nothing, Slot::Nothing, Slot::Nothing)
            }
        };
        vec![
            ("event", Slot::Text(self.event().to_owned())),
            ("bound", Slot::Text(self.bound().to_owned())),
            ("grade", Slot::Real(self.grade())),
            ("at", self.at().map_or(Slot::Nothing, Slot::At)),
            ("slip", slip),
            ("needs", needs),
            ("ready_at", ready),
        ]
    }
}

/// A term with every `After` and `Recur` resolved, and the [`Link`]s that the
/// compiler resolved the `After` nodes from.
///
/// Its readers take no environment, because the compiled term holds no
/// constructor that reads one. A bare `Term` would let a caller pass an
/// environment that the term ignores. The links are kept because a reader
/// cannot otherwise tell a cancellation from a demand met.
#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    term: Closed,
    links: Vec<Link>,
}

impl Compiled {
    /// The compiled term, which holds no `After` or `Recur` and prints like
    /// any other term.
    pub fn term(&self) -> &Closed {
        &self.term
    }

    /// The compiled term, by value.
    pub fn into_term(self) -> Closed {
        self.term
    }

    /// Every link that the compiler resolved, in pre-order over the term.
    pub fn links(&self) -> &[Link] {
        &self.links
    }

    /// The links that read as moot, which a reader must report.
    pub fn moot(&self) -> impl Iterator<Item = &Link> {
        self.links
            .iter()
            .filter(|link| matches!(link, Link::Moot { .. }))
    }

    /// The fulfillment of the compiled term at `now`, from the one evaluator
    /// of SPEC §7, against the empty environment.
    pub fn fulfillment(&self, now: Instant) -> Option<f64> {
        fpl::fulfillment(&self.term, now, &Env::new())
    }

    /// The explanation of the compiled term (SPEC §7, "Explain"), with the
    /// chain notes of `notes` merged into the root.
    ///
    /// Only the `moot` note tells a piece that reads 1.0 because an upstream
    /// was cancelled from a piece that reads 1.0 because its demand was met.
    pub fn explain(&self, now: Instant) -> Explanation {
        let mut explanation = fpl::explained(&self.term, now, &Env::new());
        explanation.notes.extend(self.notes());
        explanation
    }

    /// The chain as notes: `links`, with one map for each link, and `moot`,
    /// with the names of the cancelled upstreams if there are any.
    ///
    /// The map is empty for a term that held no `After`.
    pub fn notes(&self) -> BTreeMap<String, Note> {
        let mut notes = BTreeMap::new();
        if self.links.is_empty() {
            return notes;
        }
        notes.insert(
            "links".to_owned(),
            Note::Maps(
                self.links
                    .iter()
                    .map(|link| scalars(&link.fields()))
                    .collect(),
            ),
        );
        let moot: Vec<Scalar> = self
            .moot()
            .map(|link| Scalar::Text(link.event().to_owned()))
            .collect();
        if !moot.is_empty() {
            notes.insert("moot".to_owned(), Note::Many(moot));
        }
        notes
    }
}

/// The term compiled against the environment as of one instant (SPEC §7.1),
/// and normalized, so that a chain of links is one schedule at the root.
pub fn compile(term: &Closed, env: &Env) -> Compiled {
    let mut links = Vec::new();
    let resolved = resolve(term.term(), env, &mut links);
    Compiled {
        term: Closed::of(normalize(&resolved)).expect("resolving introduces no Ref"),
        links,
    }
}

/// Every `After` and `Recur` read as the schedule the snapshot makes of it,
/// which is sound because a binding and a piece both take hold at `now >= τ`.
fn resolve(term: &Term, env: &Env, links: &mut Vec<Link>) -> Term {
    let (event, anchor, body, pending, needs) = match term.out() {
        TermF::After {
            event,
            anchor,
            term,
            pending,
            needs,
        } => (event, anchor, term, pending, needs),
        TermF::Recur {
            todo,
            anchor,
            term: body,
            pending,
        } => return recurrence(todo, *anchor, body, pending, env, links),
        _ => return Term::new(term.out().map(|child| resolve(child, env, links))),
    };
    // A conflict compiles to its most urgent candidate: `Least` of each.
    let candidates: Vec<Option<Outcome>> = env
        .outcomes
        .get(event)
        .map_or_else(|| vec![None], |held| held.iter().copied().collect());
    let terms = candidates
        .into_iter()
        .map(|binding| {
            let link = match binding {
                None => Link::Pending {
                    event: event.clone(),
                },
                Some(Outcome::Completed(at)) => Link::Completed {
                    event: event.clone(),
                    at,
                    slip: at - *anchor,
                    needs: *needs,
                    // §7.1: `needs` is CONSUMED HERE and nowhere else. The
                    // compiler is the first reader holding the declared lead
                    // time beside the upstream's actual instant; the evaluator
                    // still never reads it.
                    ready: needs.map(|needs| at + needs),
                },
                Some(Outcome::Cancelled(at)) => Link::Moot {
                    event: event.clone(),
                    at,
                },
            };
            links.push(link.clone());
            let pending = resolve(pending, env, links);
            match link {
                Link::Pending { .. } => pending,
                Link::Completed { at, slip, .. } => piecewise(Schedule::of(
                    pending,
                    BTreeMap::from([(at, slid(slip, resolve(body, env, links)))]),
                )),
                Link::Moot { at, .. } => {
                    let body = resolve(body, env, links);
                    piecewise(Schedule::of(pending, BTreeMap::from([(at, mk_moot(body))])))
                }
            }
        })
        .collect();
    fpl::least_of(terms).expect("an event has a candidate")
}

/// A `Recur` as the schedule its tendings make: pending before the first, and
/// from each tending `s` the body slid by `s − anchor` — `After`'s completed
/// case once per pass, which is exact because `last_tended` picks the latest
/// tending at or before `now` and a piece at `s` is in force from `s` until
/// the next.
fn recurrence(
    todo: &str,
    anchor: Instant,
    body: &Term,
    pending: &Term,
    env: &Env,
    links: &mut Vec<Link>,
) -> Term {
    let pending = resolve(pending, env, links);
    let body = resolve(body, env, links);
    piecewise(Schedule::of(
        pending,
        env.tended
            .get(todo)
            .into_iter()
            .flatten()
            .map(|tended| (*tended, slid(*tended - anchor, body.clone())))
            .collect(),
    ))
}

/// The body read `slip` later than authored. `Shift(0, x)` is `x`: the
/// identity, so it is not written.
fn slid(slip: Delta, body: Term) -> Term {
    if slip.is_zero() {
        body
    } else {
        mk_shift(-slip, body)
    }
}

/// `Shift`, where the argument is one the compiler computed. `mk_shift` refuses
/// nothing, so the `expect` is a proof and not a hope.
fn mk_shift(delta: Delta, term: Term) -> Term {
    fpl::mk_shift(delta, term).expect("Shift admits every span")
}

/// The MOOT CONSTANT as the graded offset it is: `offset(x, 1) = x·0 + 1 = 1`
/// for every `x` in [0, 1], so the value is exactly 1.0 and the demand that
/// was dropped is still in the tree for a reader to see.
///
/// An offset of `∅` is `∅`, and a cancelled upstream is moot whatever the
/// body reads, so a body holding an `Absent` is gated instead: `max(1 − x, 1)
/// = 1`, and an absent gate is no gate — exactly 1.0 either way, the demand
/// still in the tree.
fn mk_moot(term: Term) -> Term {
    if fpl::holds_absent(&term) {
        fpl::mk_gate(term, fpl::mk_flat(1.0).expect("1.0 is in [0, 1]"))
            .expect("Gate admits every pair")
    } else {
        fpl::mk_offset(1.0, term).expect("1.0 is in [-1, 1]")
    }
}

/// One [`Compiled`] for each todo of the chain, or the cycle (SPEC §7.1).
///
/// `functions` maps each todo, by the name that the links use, to its
/// function from [`crate::fold::flatten`], linked by [`fpl::link`]. The
/// function checks the whole graph of the links first, because one link alone
/// cannot show a loop.
pub fn compile_chain(
    functions: &BTreeMap<String, Closed>,
    env: &Env,
) -> Result<BTreeMap<String, Compiled>, ChainError> {
    let order = chain_order(functions)?;
    Ok(order
        .into_iter()
        .map(|todo| {
            let compiled = compile(&functions[&todo], env);
            (todo, compiled)
        })
        .collect())
}

/// The todos of the chain in dependency order, with ties broken by name, or
/// the cycle.
pub fn chain_order(functions: &BTreeMap<String, Closed>) -> Result<Vec<String>, ChainError> {
    let edges = upstreams(functions);
    let mut topo = Topo::default();
    for todo in functions.keys() {
        visit(todo, &edges, &mut topo)?;
    }
    Ok(topo.into_order())
}

/// Per todo, the todos of this chain its links name; a link onto anything else
/// is the snapshot's to answer and constrains no order.
fn upstreams(functions: &BTreeMap<String, Closed>) -> BTreeMap<String, BTreeSet<String>> {
    functions
        .iter()
        .map(|(todo, term)| {
            let mut events = events_of(term.term());
            events.retain(|event| functions.contains_key(event));
            (todo.clone(), events)
        })
        .collect()
}

/// Every event name an `After` in this term binds.
fn events_of(term: &Term) -> BTreeSet<String> {
    term.cata(|layer: TermF<BTreeSet<String>>| {
        let own = match &layer {
            TermF::After { event, .. } => Some(event.clone()),
            _ => None,
        };
        layer
            .children()
            .into_iter()
            .flatten()
            .cloned()
            .chain(own)
            .collect()
    })
}

fn visit(
    todo: &String,
    edges: &BTreeMap<String, BTreeSet<String>>,
    topo: &mut Topo<String, ()>,
) -> Result<(), ChainError> {
    topo.settle(todo, ChainError::Cycle, |topo| {
        edges
            .get(todo)
            .into_iter()
            .flatten()
            .try_for_each(|upstream| visit(upstream, edges, topo))
    })
}
