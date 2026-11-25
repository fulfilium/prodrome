//! FPL's one evaluator (SPEC §7): closed terms, evaluation, explanation
//! and linking. The terms, their smart constructors, their print and parse
//! and the environment that a term reads are in the module that this
//! module re-exports.
//!
//! The code is a port of the reference `fpl.py`, checked against
//! `conformance/fpl.py`. The port keeps the reference's order of arithmetic,
//! because the vectors hold the reference's floats.

use std::collections::BTreeMap;

use thiserror::Error;

use prodrome_data::event::Hash;
use prodrome_data::literal::ProdromeError;
use prodrome_data::topo::Topo;
pub use prodrome_term::fpl::*;
use prodrome_term::schedule::Schedule;
use prodrome_term::term::schema::{self, Fields, Slot};
use prodrome_term::term::{normalize, CurvePoint, Term, TermF};

use span::{ratio, rem_euclid, within_instants};

mod span;

/// The number of intervals of a `Within` window, which `Within` samples at 65
/// evenly spaced points (SPEC §7).
pub const WITHIN_SAMPLES: i64 = 64;

// --- §7 closed terms -------------------------------------------------------

/// A term with no [`TermF::Ref`] in it, the only kind of term that evaluation
/// takes (SPEC §7.2).
#[derive(Debug, Clone, PartialEq)]
pub struct Closed(Term);

impl Closed {
    /// `term`, where it holds no reference; `None` where it must be linked.
    pub fn of(term: Term) -> Option<Closed> {
        is_closed(&term).then_some(Closed(term))
    }

    pub fn term(&self) -> &Term {
        &self.0
    }

    pub fn into_term(self) -> Term {
        self.0
    }

    /// The term in normal form (SPEC §7), for evaluation and analysis. A shift
    /// can move a knot outside the years that the grammar prints, and then
    /// [`print_term`] cannot print the form.
    pub fn normalize(&self) -> Closed {
        Closed(normalize(&self.0))
    }
}

fn is_closed(term: &Term) -> bool {
    !term.any(|node| matches!(node, TermF::Ref { .. }))
}

/// Whether `term` holds an `Absent`, which is the only way that a closed term
/// reads `∅`.
pub fn holds_absent(term: &Term) -> bool {
    term.any(|node| matches!(node, TermF::Absent))
}

// --- §7 semantics -----------------------------------------------------------

/// The power mean of `values` with exponent `p`, as SPEC §7 defines it for
/// `Conj`, ported from `powerMean` in `utility.typ` with its summation order.
///
/// The clamp to 0.001 is a floor, not the slack form, and `p == 0` is
/// computed in log space.
pub fn power_mean(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.5;
    }
    let vs: Vec<f64> = values.iter().map(|v| v.max(0.001)).collect();
    let n = vs.len() as f64;
    if p == 0.0 {
        let mut s = 0.0;
        for v in &vs {
            s += v.ln();
        }
        return (s / n).exp();
    }
    let mut s = 0.0;
    for v in &vs {
        s += v.powf(p);
    }
    (s / n).powf(1.0 / p)
}

/// The offset `[φ]_δ` in its corrected form, `x·(1−|δ|) + max(0, δ)`.
pub fn offset(x: f64, delta: f64) -> f64 {
    x * (1.0 - delta.abs()) + 0.0f64.max(delta)
}

fn eval_decay(
    start: f64,
    end: f64,
    end_date: Instant,
    lead_up: Delta,
    start_date: Option<Instant>,
    now: Instant,
) -> f64 {
    if let Some(sd) = start_date {
        if sd > now {
            return 1.0;
        }
    }
    let window_start = retreat(end_date, lead_up);
    if window_start > now {
        return 0.98;
    }
    if now > end_date {
        return end;
    }
    let frac = ratio(now - window_start, lead_up);
    (start - end) * (1.0 - frac) + end
}

fn eval_curve(points: &[CurvePoint], now: Instant) -> f64 {
    let first = &points[0];
    if now <= first.at {
        return first.value;
    }
    for pair in points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if now <= b.at {
            let frac = ratio(now - a.at, b.at - a.at);
            return a.value + (b.value - a.value) * frac;
        }
    }
    points[points.len() - 1].value
}

/// The bindings of `event` as of `now`, one reading for each candidate.
///
/// A binding dated after `now` is not in force yet: reading it would let a
/// later completion change an earlier moment.
pub fn bound<'e>(
    env: &'e Env,
    event: &str,
    now: Instant,
) -> impl Iterator<Item = Option<Outcome>> + 'e {
    let held = env.outcomes.get(event);
    held.is_none().then_some(None).into_iter().chain(
        held.into_iter()
            .flatten()
            .map(move |o| o.filter(|o| o.at() <= now)),
    )
}

/// The latest tending of `todo` at or before `now`.
pub fn last_tended(env: &Env, todo: &str, now: Instant) -> Option<Instant> {
    env.tended.get(todo)?.range(..=now).next_back().copied()
}

/// The instant at which a `Periodic` reads its body: `now` folded into
/// `[anchor, anchor + period)` by a Euclidean remainder, also before `anchor`.
pub fn phase(period: Delta, anchor: Instant, now: Instant) -> Instant {
    advance(anchor, rem_euclid(now - anchor, period))
}

/// A conjunction's reading: the power mean over the members that have a
/// value, `∅` when none do. `Conj([])` keeps its stored meaning of 0.5 — it
/// has no member to be absent — so only a conjunction whose members are all
/// absent is `∅`.
fn conj(values: &[Option<f64>], p: f64) -> Option<f64> {
    let present: Vec<f64> = values.iter().flatten().copied().collect();
    (values.is_empty() || !present.is_empty()).then(|| power_mean(&present, p))
}

/// The fulfillment of a closed term at `now` against `env`, with `None` for
/// `∅` (SPEC §7).
pub fn fulfillment(term: &Closed, now: Instant, env: &Env) -> Option<f64> {
    eval(term.term(), now, env)
}

/// The evaluator proper, over the subterms of a [`Closed`] — so a `Ref` is
/// unreachable here by the type's invariant.
pub(crate) fn eval(term: &Term, now: Instant, env: &Env) -> Option<f64> {
    match term.out() {
        TermF::Flat { value } => Some(*value),
        TermF::Decay {
            start,
            end,
            end_date,
            lead_up,
            start_date,
        } => Some(eval_decay(
            *start,
            *end,
            *end_date,
            *lead_up,
            *start_date,
            now,
        )),
        TermF::Curve { points } => Some(eval_curve(points, now)),
        TermF::Absent => None,
        TermF::Conj { terms, p } => {
            let vs: Vec<Option<f64>> = terms.iter().map(|t| eval(t, now, env)).collect();
            conj(&vs, *p)
        }
        TermF::Least { terms } => terms
            .iter()
            .filter_map(|t| eval(t, now, env))
            .reduce(f64::min),
        TermF::Offset { delta, term } => eval(term, now, env).map(|x| offset(x, *delta)),
        // An absent gate is no gate; an absent body is `∅`.
        TermF::Gate { gate, body } => {
            let body = eval(body, now, env)?;
            Some(eval(gate, now, env).map_or(body, |gate| (1.0 - gate).max(body)))
        }
        TermF::Shift { delta, term } => eval(term, advance(now, *delta), env),
        TermF::Within { window, p, term } => {
            let vs: Vec<Option<f64>> = within_instants(now, *window)
                .map(|at| eval(term, at, env))
                .collect();
            conj(&vs, *p)
        }
        TermF::Importance { w, term } => eval(term, now, env).map(|x| x.powf(*w)),
        TermF::After {
            event,
            anchor,
            term,
            pending,
            ..
        } => bound(env, event, now)
            .filter_map(|binding| match binding {
                None => eval(pending, now, env),
                // The body slides by the slippage: authored against `anchor`,
                // re-anchored to the actual completion.
                Some(Outcome::Completed(done)) => eval(term, retreat(now, done - *anchor), env),
                // Moot AS PRICING — reporting must surface it (§7, After).
                Some(Outcome::Cancelled(_)) => Some(1.0),
            })
            // A conflict reads as its most urgent candidate.
            .reduce(f64::min),
        TermF::Recur {
            todo,
            anchor,
            term,
            pending,
        } => match last_tended(env, todo, now) {
            None => eval(pending, now, env),
            // After's slide, from the last pass instead of the completion.
            Some(tended) => eval(term, retreat(now, tended - *anchor), env),
        },
        TermF::Periodic {
            period,
            anchor,
            term,
        } => eval(term, phase(*period, *anchor, now), env),
        TermF::Piecewise(schedule) => eval(schedule.at(now), now, env),
        // An absent offset is no offset; an absent term is `∅`.
        TermF::OffsetBy { delta, term } => {
            let x = eval(term, now, env)?;
            Some(eval(delta, now, env).map_or(x, |delta| offset(x, delta)))
        }
        TermF::Ref { entity, .. } => unreachable!("Ref({entity:?}) inside a Closed term"),
    }
}

// --- The explanation: Cofree TermF Annotation -------------------------------

/// The Minimum Fulfillment Bound (Theorem IV.1): the floor that a composed
/// p-mean score proves about its worst direct member.
///
/// Apply it to one node at a time, because the flat bound is unsound across
/// the levels of a nested composition. The reference refuses `n < 1`; this
/// function is total and returns 1.0 for `n == 0`.
pub fn min_fulfillment(composed: f64, n: usize, p: f64) -> f64 {
    let nf = n as f64;
    if p == 0.0 {
        return composed.powf(nf);
    }
    let inner = nf * composed.powf(p) - (nf - 1.0);
    if inner > 0.0 {
        inner.powf(1.0 / p)
    } else {
        0.0
    }
}

/// Each member's share of responsibility for the p-mean it composes into,
/// `eᵢ = (1/n)·(xᵢ/M)ᵖ`; the shares sum to 1.
///
/// It applies the clamp of [`power_mean`] to the values, so that the shares
/// describe the number that `power_mean` returns.
pub fn member_shares(values: &[f64], p: f64) -> Vec<f64> {
    if values.is_empty() {
        return vec![];
    }
    let n = values.len();
    if p == 0.0 {
        return vec![1.0 / n as f64; n];
    }
    let m = power_mean(values, p);
    values
        .iter()
        .map(|v| (1.0 / n as f64) * (v.max(0.001) / m).powf(p))
        .collect()
}

/// A note's leaf: the scalars an explanation carries beside its value.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Text(String),
    Float(f64),
    Int(i64),
    Bool(bool),
}

/// The reference's `Note` as a closed type:
/// `Scalar | tuple[Scalar, …] | tuple[Mapping[str, Scalar], …]`.
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    One(Scalar),
    Many(Vec<Scalar>),
    Maps(Vec<BTreeMap<String, Scalar>>),
}

/// The term's own shape, with each node carrying its notes and its
/// fulfillment at the moment the parent used it (SPEC §7, "Explain").
///
/// A `Piecewise` node is the one exception to the shape: its `head` slot
/// holds the explanation of the piece in force, its `pieces` are empty, and
/// its notes hold the size and the start of the schedule.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub value: Option<f64>,
    pub notes: BTreeMap<String, Note>,
    pub node: Box<TermF<Explanation>>,
}

fn iso_note(t: Instant) -> Note {
    Note::One(Scalar::Text(iso(t)))
}

/// A field's key in the notes and on the wire: camelCase, with the suffix
/// `Hours` for a span.
pub fn wire_key(name: &str, span: bool) -> String {
    let mut words = name.split('_');
    let mut key = words.next().unwrap_or_default().to_owned();
    for word in words {
        let mut chars = word.chars();
        key.extend(chars.next().map(|c| c.to_ascii_uppercase()));
        key.push_str(chars.as_str());
    }
    if span {
        key.push_str("Hours");
    }
    key
}

/// A leaf field as a note's scalar, or `None` for an unset optional field or
/// an empty text.
pub fn scalar<A>(slot: &Slot<A>) -> Option<Scalar> {
    match slot {
        Slot::Real(value) => Some(Scalar::Float(*value)),
        Slot::At(t) => Some(Scalar::Text(iso(*t))),
        Slot::Span(d) => Some(Scalar::Float(hours(*d))),
        Slot::Text(text) if !text.is_empty() => Some(Scalar::Text(text.clone())),
        _ => None,
    }
}

/// Every leaf field, keyed by [`wire_key`].
pub fn scalars<A>(fields: &Fields<A>) -> BTreeMap<String, Scalar> {
    fields
        .iter()
        .filter_map(|(name, slot)| {
            Some((wire_key(name, matches!(slot, Slot::Span(_))), scalar(slot)?))
        })
        .collect()
}

/// The layer's own fields as notes; its subterms are explained, not noted.
fn scalar_notes(term: &Term) -> BTreeMap<String, Note> {
    let (_, fields) = schema::fields(term.out());
    fields
        .iter()
        .filter_map(|(name, slot)| {
            let note = match slot {
                Slot::Rows(_, rows) if !slot.holds_term() => {
                    Note::Maps(rows.iter().map(scalars).collect())
                }
                leaf => Note::One(scalar(leaf)?),
            };
            Some((wire_key(name, matches!(slot, Slot::Span(_))), note))
        })
        .collect()
}

/// The explanation of `term` at `now`: the term's shape, with each node
/// carrying its fulfillment at the instant at which its parent used it (SPEC
/// §7, "Explain").
pub fn explained(term: &Closed, now: Instant, env: &Env) -> Explanation {
    explain(term.term(), now, env)
}

fn explain(term: &Term, now: Instant, env: &Env) -> Explanation {
    let value = eval(term, now, env);
    let mut notes = scalar_notes(term);
    let node: TermF<Explanation> = match term.out() {
        TermF::Flat { value } => TermF::Flat { value: *value },
        TermF::Decay {
            start,
            end,
            end_date,
            lead_up,
            start_date,
        } => TermF::Decay {
            start: *start,
            end: *end,
            end_date: *end_date,
            lead_up: *lead_up,
            start_date: *start_date,
        },
        TermF::Curve { points } => TermF::Curve {
            points: points.clone(),
        },
        TermF::Conj { terms, p } => {
            let kids: Vec<Explanation> = terms.iter().map(|t| explain(t, now, env)).collect();
            // An absent member bears no share, and a conjunction with no value apportions nothing.
            if let Some(value) = value {
                let vs: Vec<f64> = kids.iter().filter_map(|k| k.value).collect();
                notes.insert(
                    "certifies".into(),
                    Note::One(Scalar::Float(min_fulfillment(value, vs.len(), *p))),
                );
                let mut shares = member_shares(&vs, *p).into_iter();
                notes.insert(
                    "shares".into(),
                    Note::Many(
                        kids.iter()
                            .map(|k| {
                                Scalar::Float(match k.value {
                                    Some(_) => shares.next().expect("one share per value"),
                                    None => 0.0,
                                })
                            })
                            .collect(),
                    ),
                );
            }
            TermF::Conj { terms: kids, p: *p }
        }
        TermF::Least { terms } => TermF::Least {
            terms: terms.iter().map(|t| explain(t, now, env)).collect(),
        },
        TermF::Offset { delta, term } => TermF::Offset {
            delta: *delta,
            term: explain(term, now, env),
        },
        TermF::Importance { w, term } => TermF::Importance {
            w: *w,
            term: explain(term, now, env),
        },
        TermF::Gate { gate, body } => TermF::Gate {
            gate: explain(gate, now, env),
            body: explain(body, now, env),
        },
        TermF::OffsetBy { delta, term } => TermF::OffsetBy {
            delta: explain(delta, now, env),
            term: explain(term, now, env),
        },
        TermF::Shift { delta, term } => TermF::Shift {
            delta: *delta,
            term: explain(term, advance(now, *delta), env),
        },
        TermF::Within { window, p, term } => {
            // The peak is over the samples that have a value, as the mean is; with none, `now`.
            let sampled: Vec<(Instant, f64)> = within_instants(now, *window)
                .filter_map(|t| eval(term, t, env).map(|v| (t, v)))
                .collect();
            let vs: Vec<f64> = sampled.iter().map(|(_, v)| *v).collect();
            let shares = member_shares(&vs, *p);
            // `max(range(n), key=…)` keeps the FIRST maximal index.
            let mut peak = 0usize;
            for (i, s) in shares.iter().enumerate() {
                if *s > shares[peak] {
                    peak = i;
                }
            }
            let at = match sampled.get(peak) {
                Some((at, _)) => {
                    notes.insert("peakAt".into(), iso_note(*at));
                    notes.insert("peakShare".into(), Note::One(Scalar::Float(shares[peak])));
                    *at
                }
                None => now,
            };
            TermF::Within {
                window: *window,
                p: *p,
                term: explain(term, at, env),
            }
        }
        TermF::After {
            event,
            anchor,
            term,
            pending,
            needs,
        } => {
            let reading = |binding: &Option<Outcome>| match binding {
                None => eval(pending, now, env),
                Some(Outcome::Completed(done)) => eval(term, retreat(now, *done - *anchor), env),
                Some(Outcome::Cancelled(_)) => Some(1.0),
            };
            let urgent = bound(env, event, now)
                .reduce(|a, b| match (reading(&a), reading(&b)) {
                    (Some(x), Some(y)) if y < x => b,
                    (None, Some(_)) => b,
                    _ => a,
                })
                .flatten();
            let (label, at) = match urgent {
                None => ("pending", now),
                // The slippage the parent applied.
                Some(Outcome::Completed(done)) => ("completed", retreat(now, done - *anchor)),
                Some(Outcome::Cancelled(_)) => ("cancelled", now),
            };
            notes.insert("bound".into(), Note::One(Scalar::Text(label.to_string())));
            TermF::After {
                event: event.clone(),
                anchor: *anchor,
                term: explain(term, at, env),
                pending: explain(pending, now, env),
                needs: *needs,
            }
        }
        TermF::Recur {
            todo,
            anchor,
            term,
            pending,
        } => {
            let at = match last_tended(env, todo, now) {
                None => {
                    notes.insert("bound".into(), Note::One(Scalar::Text("pending".into())));
                    now
                }
                Some(tended) => {
                    notes.insert("bound".into(), Note::One(Scalar::Text("tended".into())));
                    notes.insert("tended".into(), iso_note(tended));
                    notes.insert(
                        "agoHours".into(),
                        Note::One(Scalar::Float(hours(now - tended))),
                    );
                    retreat(now, tended - *anchor)
                }
            };
            TermF::Recur {
                todo: todo.clone(),
                anchor: *anchor,
                term: explain(term, at, env),
                pending: explain(pending, now, env),
            }
        }
        TermF::Periodic {
            period,
            anchor,
            term,
        } => {
            let at = phase(*period, *anchor, now);
            notes.insert("cycleStart".into(), iso_note(retreat(now, at - *anchor)));
            TermF::Periodic {
                period: *period,
                anchor: *anchor,
                term: explain(term, at, env),
            }
        }
        TermF::Piecewise(schedule) => {
            let (since, in_f) = schedule.since(now);
            let pieces = schedule.knots().len();
            notes.insert("pieces".into(), Note::One(Scalar::Int(pieces as i64)));
            notes.insert(
                "since".into(),
                Note::One(Scalar::Text(since.map(iso).unwrap_or_default())),
            );
            TermF::Piecewise(Schedule::constant(explain(in_f, now, env)))
        }
        TermF::Ref { store, entity } => TermF::Ref {
            store: store.clone(),
            entity: entity.clone(),
        },
        TermF::Absent => TermF::Absent,
    };
    Explanation {
        value,
        notes,
        node: Box::new(node),
    }
}

// --- Linking: every Ref bound to the todo it names --------------------------

/// The reason that a term does not link (SPEC §7.2, "Refusals are values.").
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LinkError {
    /// `specs` holds no function for the todo of a `Ref`. A todo of another
    /// prodrome appears as `genesis/todo`.
    #[error("Ref({0:?}) names no known todo")]
    Unknown(String),
    /// The references loop: the path holds the cycle, with its first todo
    /// repeated at the end. An entity of another prodrome appears as
    /// `genesis/entity`.
    #[error("the references loop: {}", .0.join(" → "))]
    Cycle(Vec<String>),
}

/// One prodrome as a qualified reference reads it (SPEC §7.2): the function
/// of each entity, under the key that an unqualified `Ref` inside the
/// prodrome uses, and the environment that its `After` and `Recur` nodes read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Member {
    pub specs: BTreeMap<String, Term>,
    pub env: Env,
}

/// The set of prodromes that the qualified references of a term read (SPEC
/// §7.2), keyed by genesis, with the names that the host declares.
///
/// Law 41: a reference into one member reads that member and nothing else.
/// A member is one reading of one prodrome: to join two replicas, sync their
/// objects and read the union.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stores {
    members: BTreeMap<Hash, Member>,
    names: BTreeMap<String, Hash>,
}

impl Stores {
    /// The empty set: every qualified reference reads `∅`.
    #[must_use]
    pub fn new() -> Stores {
        Stores::default()
    }

    /// These stores with `member` as the reading of the prodrome that
    /// `genesis` begins. It replaces an earlier reading of `genesis`.
    #[must_use]
    pub fn with(mut self, genesis: Hash, member: Member) -> Stores {
        self.members.insert(genesis, member);
        self
    }

    /// These stores with `name` declared for the prodrome `genesis`.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>, genesis: Hash) -> Stores {
        self.names.insert(name.into(), genesis);
        self
    }

    /// The member of the prodrome that `genesis` begins, if the set holds it.
    #[must_use]
    pub fn member(&self, genesis: &Hash) -> Option<&Member> {
        self.members.get(genesis)
    }

    /// The member a qualifier names, by its genesis: a declared name's,
    /// else the qualifier read as a genesis's name.
    fn named_by(&self, store: &str) -> Option<(&Hash, &Member)> {
        match self.names.get(store) {
            Some(genesis) => self.members.get_key_value(genesis),
            None => Hash::new(store)
                .ok()
                .and_then(|genesis| self.members.get_key_value(&genesis)),
        }
    }
}

impl FromIterator<(Hash, Member)> for Stores {
    fn from_iter<I: IntoIterator<Item = (Hash, Member)>>(members: I) -> Stores {
        Stores {
            members: members.into_iter().collect(),
            names: BTreeMap::new(),
        }
    }
}

/// The term with every `Ref(x)` bound to `specs[x]`, linked in turn (SPEC
/// §7.2).
///
/// Law 14: linking is bind, and a closed term links to itself. A qualified
/// reference reads `∅`, because `link` is [`link_in`] with no stores. It
/// refuses a `Ref` to a todo that `specs` does not hold, and references that
/// loop.
pub fn link(term: &Term, specs: &BTreeMap<String, Term>) -> Result<Closed, LinkError> {
    link_in(term, specs, &Stores::new())
}

/// [`link`], with `stores` as the set that the qualified references read
/// (SPEC §7.2, "A qualified reference.").
///
/// A store that the set does not hold, or an entity that its member does
/// not hold, reads `∅`.
///
/// # Errors
///
/// It refuses an unqualified `Ref` that its prodrome does not hold, and
/// references that loop, within one member or across members.
pub fn link_in(
    term: &Term,
    specs: &BTreeMap<String, Term>,
    stores: &Stores,
) -> Result<Closed, LinkError> {
    if let Some(closed) = Closed::of(term.clone()) {
        return Ok(closed);
    }
    linked(term, None, specs, stores, &mut Topo::default()).map(Closed)
}

/// A variable as the linker keys it: the member it lives in, by genesis,
/// none for the term's own prodrome, and its entity.
type Variable = (Option<Hash>, String);

/// A variable as a refusal names it: `entity` at home, `genesis/entity`
/// elsewhere.
fn shown((genesis, entity): &Variable) -> String {
    genesis
        .as_ref()
        .map_or_else(|| entity.clone(), |genesis| format!("{genesis}/{entity}"))
}

/// `term`, read in the member `home` (none for the term's own prodrome,
/// whose functions are `specs`), every reference bound.
fn linked(
    term: &Term,
    home: Option<&Hash>,
    specs: &BTreeMap<String, Term>,
    stores: &Stores,
    topo: &mut Topo<Variable, Term>,
) -> Result<Term, LinkError> {
    term.try_cata(|layer| match layer {
        TermF::Ref {
            store: None,
            entity,
        } => resolved(home, specs, entity, stores, topo),
        TermF::Ref {
            store: Some(store),
            entity,
        } => match stores.named_by(&store) {
            Some((genesis, member)) if member.specs.contains_key(&entity) => {
                resolved(Some(genesis), &member.specs, entity, stores, topo)
            }
            _ => Ok(mk_absent()),
        },
        TermF::Piecewise(schedule) => Ok(piecewise(schedule)),
        layer => Ok(Term::new(layer)),
    })
}

/// The variable `entity` of the member `home`, whose functions are `specs`:
/// its function linked there, and closed against that member's history.
fn resolved(
    home: Option<&Hash>,
    specs: &BTreeMap<String, Term>,
    entity: String,
    stores: &Stores,
    topo: &mut Topo<Variable, Term>,
) -> Result<Term, LinkError> {
    let variable = (home.cloned(), entity);
    topo.settle(
        &variable,
        |path| LinkError::Cycle(path.iter().map(shown).collect()),
        |topo| {
            let spec = specs
                .get(&variable.1)
                .ok_or_else(|| LinkError::Unknown(shown(&variable)))?;
            let term = linked(spec, home, specs, stores, topo)?;
            Ok(match home.and_then(|genesis| stores.members.get(genesis)) {
                Some(member) => crate::chain::compile(&Closed(term), &member.env)
                    .into_term()
                    .into_term(),
                None => term,
            })
        },
    )
}

/// A store whose terms do not link is invalid content, like a term refused.
impl From<LinkError> for ProdromeError {
    fn from(error: LinkError) -> ProdromeError {
        ProdromeError::Invalid(error.to_string())
    }
}
