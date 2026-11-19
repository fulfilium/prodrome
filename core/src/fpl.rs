//! §7 — FPL: the term language, one evaluator.
//!
//! See ../../SPEC.md. Implemented against ../../conformance/fpl.py, and
//! ported from the reference `fpl.py` — the arithmetic ORDER is part
//! of the port, because the vectors are the reference's floats.
//!
//! `Explanation` is the term's shape carrying an annotation — the Cofree the
//! spec calls "decoration, not evaluation". Invariants live in the `mk_*`
//! smart constructors; a `Term` that exists came through one of them (or
//! through `parse_term`, which calls them).
//!
//! ABSENT IS A VALUE. A term's value is `[0, 1] ∪ {∅}`, and `∅` is `None`:
//! [`fulfillment`] answers `Option<f64>`, so an object with no claim on
//! attention — the `Absent` leaf, or anything composed only of it — cannot be
//! read as a number. `∅` is the identity of composition: it neither raises
//! nor lowers anything it is composed with.
//!
//! OPEN AND CLOSED. `Ref(todo)` names another todo's fulfillment, so a term
//! holding one is open; [`link`] binds every reference and answers a
//! [`Closed`] term, and only a closed term is evaluated, explained or sampled.
//!
//! NO JSON HERE. A term has ONE serialization and it is §2's literal print;
//! the JSON shape a browser reads is `prodrome-wasm-exports`'s `json` module,
//! built on the types below, because JSON is JavaScript's literal grammar and
//! this crate has its own.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};
use thiserror::Error;

use crate::event::Hash;
use crate::literal::{self, print_literal, Call, Finite, ProdromeError};
use crate::schedule::Schedule;
use crate::term::schema::{self, signatures, Field, FieldSource, Fields, Slot};
use crate::term::{normalize, CurvePoint, Term, TermF};
use crate::topo::Topo;

/// Naive local time only (§2): a stored instant never carries a zone.
pub type Instant = NaiveDateTime;
/// A signed span, printed as §2's `timedelta(...)`.
pub type Delta = Duration;

/// The production conjunction exponent — a graded ∀, not a connective.
pub const PRIORITY_POWER: f64 = -4.0;
/// `Within` is 64 intervals, 65 evenly spaced points (§7, and the stated limit).
pub const WITHIN_SAMPLES: i64 = 64;

/// A refusal from a smart constructor or the literal parser. §2: malformed
/// input is a `ValueError`-shaped refusal, never a crash.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct FplError(pub String);

fn err<T>(msg: impl Into<String>) -> Result<T, FplError> {
    Err(FplError(msg.into()))
}

// --- §7 closed terms and the environment ------------------------------------

/// A term with no [`TermF::Ref`] anywhere in it: what evaluation takes (§7).
///
/// A newtype so that forgetting to [`link`] is a type error: a reference has
/// no value until it is bound.
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

    pub fn normalize(&self) -> Closed {
        Closed(normalize(&self.0))
    }
}

fn is_closed(term: &Term) -> bool {
    !term.any(|node| matches!(node, TermF::Ref { .. }))
}

/// Whether an `Absent` is anywhere in `term`, the only way a closed term reads `∅`.
pub fn holds_absent(term: &Term) -> bool {
    term.any(|node| matches!(node, TermF::Absent))
}

/// What history says about a named event. The two ways an event can be over
/// mean OPPOSITE things downstream, so they are an ADT and never a boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    Completed(Instant),
    Cancelled(Instant),
}

impl Outcome {
    pub fn at(&self) -> Instant {
        match self {
            Outcome::Completed(at) | Outcome::Cancelled(at) => *at,
        }
    }

    /// The constructor name the reference prints for this outcome.
    pub fn kind(&self) -> &'static str {
        match self {
            Outcome::Completed(_) => "Completed",
            Outcome::Cancelled(_) => "Cancelled",
        }
    }
}

/// A register's candidate bindings (§6.6), `None` where a candidate is open:
/// one member is a value, more are a conflict.
pub type Candidates = BTreeSet<Option<Outcome>>;

/// What history says, as a snapshot: `After`'s freeze variables, each the
/// candidate bindings of a todo that is not simply open, and the tendings
/// `Recur` re-anchors to — a grow-only set per todo, so two replicas' tendings
/// merge by union and never conflict.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Env {
    pub outcomes: BTreeMap<String, Candidates>,
    pub tended: BTreeMap<String, BTreeSet<Instant>>,
}

impl Env {
    /// The environment that binds nothing and records no tending.
    pub fn new() -> Env {
        Env::default()
    }

    /// `event` bound to exactly `outcome`.
    pub fn bind(&mut self, event: impl Into<String>, outcome: Outcome) {
        self.outcomes.insert(event.into(), [Some(outcome)].into());
    }
}

// --- Time arithmetic, in the reference's units ------------------------------

fn us_of(d: Delta) -> i64 {
    d.num_microseconds()
        .expect("timedelta out of microsecond range")
}

fn after(now: Instant, us: i64) -> Instant {
    now.checked_add_signed(Duration::microseconds(us))
        .expect("datetime overflow")
}

/// Python's `_divide_and_round`: floor division with a round-half-to-even fix-up.
fn divide_and_round(a: i64, b: i64) -> i64 {
    // Python's divmod floors toward −∞ for either sign of `b`.
    let mut q = a.div_euclid(b);
    let mut r = a.rem_euclid(b);
    if b < 0 && r != 0 {
        q += 1;
        r += b;
    }
    let r2 = r.checked_mul(2).expect("divide_and_round overflow");
    let greater_than_half = if b > 0 { r2 > b } else { r2 < b };
    if greater_than_half || (r2 == b && q.rem_euclid(2) == 1) {
        q += 1;
    }
    q
}

/// `timedelta / int` — exact microseconds, rounded half to even, as CPython.
pub fn div_delta(d: Delta, n: i64) -> Delta {
    Duration::microseconds(divide_and_round(us_of(d), n))
}

/// `timedelta / timedelta` — the float ratio CPython computes from microseconds.
fn ratio(a: Delta, b: Delta) -> f64 {
    us_of(a) as f64 / us_of(b) as f64
}

/// `timedelta.total_seconds()`: exact microseconds over 1e6.
pub fn total_seconds(d: Delta) -> f64 {
    us_of(d) as f64 / 1e6
}

/// `timedelta(hours=…)` for a float, with CPython's round-half-even microsecond.
pub fn delta_from_hours(hours: f64) -> Delta {
    let seconds = hours * 3600.0;
    let whole = seconds.trunc();
    let frac = seconds - whole;
    Duration::microseconds((whole as i64) * 1_000_000 + round_half_even(frac * 1e6))
}

fn round_half_even(x: f64) -> i64 {
    let away = x.round(); // half away from zero
    if (x - x.trunc()).abs() == 0.5 && away % 2.0 != 0.0 {
        (away - x.signum()) as i64
    } else {
        away as i64
    }
}

// --- §7 semantics -----------------------------------------------------------

/// utility.typ's `powerMean`, ported: a 0.001 floor CLAMP (not the slack
/// form), the geometric branch at `p == 0` taken in log space, and an empty
/// collection reading 0.5. The summation order is the reference's.
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

/// `[φ]_δ`, the CORRECTED form: `x·(1−|δ|) + max(0, δ)`.
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
    let window_start = end_date - lead_up;
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

/// What history says about `event` AS OF `now`, one reading per candidate. A
/// snapshot taken later may hold a binding dated after `now`; that binding is
/// not in force yet, and reading it would let a later completion rewrite an
/// earlier moment.
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

/// The latest tending of `todo` AS OF `now`, under `bound`'s guard: a pass
/// recorded later never rewrites an earlier moment.
pub fn last_tended(env: &Env, todo: &str, now: Instant) -> Option<Instant> {
    env.tended.get(todo)?.range(..=now).next_back().copied()
}

/// Where a `Periodic` reads its body: `now` folded into `[anchor, anchor +
/// period)` by a Euclidean remainder, so a moment before `anchor` folds too.
pub fn phase(period: Delta, anchor: Instant, now: Instant) -> Instant {
    after(anchor, us_of(now - anchor).rem_euclid(us_of(period)))
}

/// A conjunction's reading: the power mean over the members that have a
/// value, `∅` when none do. `Conj([])` keeps its stored meaning of 0.5 — it
/// has no member to be absent — so only a conjunction whose members are all
/// absent is `∅`.
fn conj(values: &[Option<f64>], p: f64) -> Option<f64> {
    let present: Vec<f64> = values.iter().flatten().copied().collect();
    (values.is_empty() || !present.is_empty()).then(|| power_mean(&present, p))
}

/// Evaluate a closed term at a moment against what history says. Total by
/// structure; every branch returns a value in [0, 1], or `∅` (`None`) where
/// the term has none.
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
        TermF::Shift { delta, term } => eval(term, after(now, us_of(*delta)), env),
        TermF::Within { window, p, term } => {
            let step = us_of(div_delta(*window, WITHIN_SAMPLES));
            let vs: Vec<Option<f64>> = (0..=WITHIN_SAMPLES)
                .map(|i| eval(term, after(now, step * i), env))
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
                Some(Outcome::Completed(done)) => {
                    eval(term, after(now, -us_of(done - *anchor)), env)
                }
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
            Some(tended) => eval(term, after(now, -us_of(tended - *anchor)), env),
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

/// The Minimum Fulfillment Bound (Theorem IV.1): the floor a composed p-mean
/// score PROVES about its worst DIRECT member. Applied per node — the flat
/// bound is unsound across the levels of a nested composition. The reference
/// refuses `n < 1`; here the formula is simply total (an empty `Conj` reads 1.0).
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

/// Each member's SHARE of responsibility for the p-mean it composes into:
/// `eᵢ = (1/n)·(xᵢ/M)ᵖ`, summing to exactly 1. Applies power_mean's clamp to
/// the same values, or the shares would not describe the number returned.
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

/// `Scalar | tuple[Scalar, …] | tuple[Mapping[str, Scalar], …]` — the
/// reference's `Note`, as a closed type.
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    One(Scalar),
    Many(Vec<Scalar>),
    Maps(Vec<BTreeMap<String, Scalar>>),
}

/// `Cofree TermF Annotation`: the term's OWN shape, each node carrying its
/// fulfillment at the moment the parent actually used it — `∅` where the node
/// has none — plus the notes.
///
/// One documented bend in the shape: a `Piecewise` node's decoration is the
/// PIECE IN FORCE, which rides in the `head` slot with `pieces` empty — head
/// and pieces are transitions, not subterms, and the schedule's size and start
/// are notes. Every other constructor's children line up one for one.
///
/// The layer is boxed for the same reason `Term`'s is: `TermF` holds its
/// children by value, so the fixed point needs exactly one indirection.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub value: Option<f64>,
    pub notes: BTreeMap<String, Note>,
    pub node: Box<TermF<Explanation>>,
}

fn iso_note(t: Instant) -> Note {
    Note::One(Scalar::Text(iso(t)))
}

/// A field's key in the notes and on the wire: camelCase, a span suffixed `Hours`.
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

pub fn hours(d: Delta) -> f64 {
    total_seconds(d) / 3600.0
}

/// A leaf field as a note's scalar; an unset optional or an empty text is none.
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

/// The term's own shape, with every node carrying its own fulfillment.
///
/// ⚠️ CHILDREN ARE ANNOTATED AT THE TIME THE PARENT ACTUALLY USED: Shift,
/// Within, After, Recur and Periodic re-anchor time for their subterm, and a
/// child evaluated at `now` would report a number the parent never consumed.
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
            term: explain(term, after(now, us_of(*delta)), env),
        },
        TermF::Within { window, p, term } => {
            let step = us_of(div_delta(*window, WITHIN_SAMPLES));
            // The peak is over the samples that have a value, as the mean is; with none, `now`.
            let sampled: Vec<(Instant, f64)> = (0..=WITHIN_SAMPLES)
                .map(|i| after(now, step * i))
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
                Some(Outcome::Completed(done)) => {
                    eval(term, after(now, -us_of(*done - *anchor)), env)
                }
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
                Some(Outcome::Completed(done)) => ("completed", after(now, -us_of(done - *anchor))),
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
                    after(now, -us_of(tended - *anchor))
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
            notes.insert(
                "cycleStart".into(),
                iso_note(after(now, -us_of(at - *anchor))),
            );
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

// --- Smart constructors (§1: records are dumb data; these are the only path) -

fn unit(name: &str, v: f64) -> Result<f64, FplError> {
    if !(0.0..=1.0).contains(&v) {
        return err(format!("{name} must be in [0, 1], got {v}"));
    }
    Ok(v)
}

pub fn mk_flat(value: f64) -> Result<Term, FplError> {
    Ok(Term::new(TermF::Flat {
        value: unit("Flat.value", value)?,
    }))
}

pub fn mk_decay(
    start: f64,
    end: f64,
    end_date: Instant,
    lead_up: Delta,
    start_date: Option<Instant>,
) -> Result<Term, FplError> {
    if lead_up <= Duration::zero() {
        return err("Decay.lead_up must be positive");
    }
    if start > 0.98 {
        // The pre-window value IS 0.98; a higher start would make urgency DROP
        // on window entry, breaking slippage-monotonicity.
        return err(format!(
            "Decay.start must be ≤ 0.98 (pre-window value), got {start}"
        ));
    }
    Ok(Term::new(TermF::Decay {
        start: unit("Decay.start", start)?,
        end: unit("Decay.end", end)?,
        end_date,
        lead_up,
        start_date,
    }))
}

pub fn mk_curve(points: Vec<CurvePoint>) -> Result<Term, FplError> {
    if points.is_empty() {
        return err("Curve needs at least one point");
    }
    for pt in &points {
        let name = if pt.label.is_empty() {
            iso(pt.at)
        } else {
            pt.label.clone()
        };
        unit(&format!("Curve point {name}"), pt.value)?;
    }
    for pair in points.windows(2) {
        if pair[1].at <= pair[0].at {
            return err("Curve points must be strictly increasing in time");
        }
    }
    Ok(Term::new(TermF::Curve { points }))
}

pub fn mk_conj(terms: Vec<Term>, p: f64) -> Result<Term, FplError> {
    // ≤ 0: FPL conjunctions only. ≥ −32: overflow guard under the 0.001 clamp.
    if !(-32.0..=0.0).contains(&p) {
        return err(format!("Conj.p must be in [-32, 0], got {p}"));
    }
    Ok(Term::new(TermF::Conj { terms, p }))
}

/// Never empty, so a term reads `∅` only where it holds an `Absent`.
pub fn mk_least(terms: Vec<Term>) -> Result<Term, FplError> {
    if terms.is_empty() {
        return err("Least needs a member");
    }
    Ok(Term::new(TermF::Least { terms }))
}

/// `Least` of the distinct `terms`: one is itself, so a reading with no
/// conflict is written exactly as it was.
pub fn least_of(terms: Vec<Term>) -> Result<Term, FplError> {
    let mut distinct: Vec<Term> = Vec::with_capacity(terms.len());
    for term in terms {
        if !distinct.contains(&term) {
            distinct.push(term);
        }
    }
    match <[Term; 1]>::try_from(distinct) {
        Ok([one]) => Ok(one),
        Err(many) => mk_least(many),
    }
}

pub fn mk_offset(delta: f64, term: Term) -> Result<Term, FplError> {
    if !(-1.0..=1.0).contains(&delta) {
        return err(format!("Offset.delta must be in [-1, 1], got {delta}"));
    }
    Ok(Term::new(TermF::Offset { delta, term }))
}

pub fn mk_gate(gate: Term, body: Term) -> Result<Term, FplError> {
    Ok(Term::new(TermF::Gate { gate, body }))
}

pub fn mk_shift(delta: Delta, term: Term) -> Result<Term, FplError> {
    Ok(Term::new(TermF::Shift { delta, term }))
}

pub fn mk_within(window: Delta, p: f64, term: Term) -> Result<Term, FplError> {
    if window <= Duration::zero() {
        return err("Within.window must be positive");
    }
    if !(-32.0..=32.0).contains(&p) {
        return err(format!(
            "Within.p must be in [-32, 32] (overflow guard), got {p}"
        ));
    }
    Ok(Term::new(TermF::Within { window, p, term }))
}

pub fn mk_importance(w: f64, term: Term) -> Result<Term, FplError> {
    // Stated as what `w` MUST be rather than what it must not: `w <= 0.0` is
    // false for a NaN, which would have admitted the one unprintable float a
    // term can hold (§2: `inf`/`nan` are not storable).
    if !(w.is_finite() && w > 0.0) {
        return err(format!("Importance.w must be positive, got {w}"));
    }
    Ok(Term::new(TermF::Importance { w, term }))
}

pub fn mk_offset_by(delta: Term, term: Term) -> Result<Term, FplError> {
    Ok(Term::new(TermF::OffsetBy { delta, term }))
}

pub fn mk_after(
    event: String,
    anchor: Instant,
    term: Term,
    pending: Term,
    needs: Option<Delta>,
) -> Result<Term, FplError> {
    if event.is_empty() {
        return err("After.event must name a completion event");
    }
    if let Some(n) = needs {
        if n <= Duration::zero() {
            return err("After.needs must be positive");
        }
    }
    Ok(Term::new(TermF::After {
        event,
        anchor,
        term,
        pending,
        needs,
    }))
}

/// `term` re-anchored to the last tending of `todo`, whose id obeys the rule
/// a [`crate::event::TodoId`] does.
pub fn mk_recur(
    todo: String,
    anchor: Instant,
    term: Term,
    pending: Term,
) -> Result<Term, FplError> {
    crate::event::TodoId::new(todo.as_str()).map_err(|e| FplError(format!("Recur.todo: {e}")))?;
    Ok(Term::new(TermF::Recur {
        todo,
        anchor,
        term,
        pending,
    }))
}

/// `term` repeated every `period` from `anchor`, both ways in time.
pub fn mk_periodic(period: Delta, anchor: Instant, term: Term) -> Result<Term, FplError> {
    if period <= Duration::zero() {
        return err("Periodic.period must be positive");
    }
    Ok(Term::new(TermF::Periodic {
        period,
        anchor,
        term,
    }))
}

/// A reference to todo `todo`'s fulfillment. The id obeys the rule a
/// [`crate::event::TodoId`] does, checked by that type's own constructor.
pub fn mk_ref(todo: String) -> Result<Term, FplError> {
    crate::event::TodoId::new(todo.as_str()).map_err(|e| FplError(format!("Ref.todo: {e}")))?;
    Ok(Term::new(TermF::Ref {
        store: None,
        entity: todo,
    }))
}

/// A QUALIFIED reference: the fulfillment of `entity` in the prodrome
/// `store` names, a genesis's name or a name the host declares for one
/// ([`Stores::named`]); both obey [`crate::event::TodoId`]'s rule, which a
/// genesis's 64 lowercase hex does. `entity` is a key as a nest's path
/// holds it ([`crate::nest::Segment`]): not empty, no `/`.
///
/// # Errors
///
/// A store or an entity that breaks its rule.
pub fn mk_ref_in(store: String, entity: String) -> Result<Term, FplError> {
    crate::event::TodoId::new(store.as_str()).map_err(|e| FplError(format!("RefIn.store: {e}")))?;
    crate::nest::Segment::new(entity.as_str())
        .map_err(|e| FplError(format!("RefIn.entity: {e}")))?;
    Ok(Term::new(TermF::Ref {
        store: Some(store),
        entity,
    }))
}

/// No temporal value. It has no field, so there is nothing to refuse.
pub fn mk_absent() -> Term {
    Term::new(TermF::Absent)
}

/// A todo's fulfillment function given its own spec and its checklist: each
/// item is a leaf worth 0.5, the checklist is their conjunction, and the
/// parent's own value is an OFFSET on that aggregate. No items: the spec
/// itself. No spec: the aggregate alone. Neither: `None`.
pub fn checklist(own: Option<Term>, items: usize) -> Result<Option<Term>, FplError> {
    if items == 0 {
        return Ok(own);
    }
    let mut leaves = Vec::with_capacity(items);
    for _ in 0..items {
        leaves.push(mk_flat(0.5)?);
    }
    let aggregate = mk_conj(leaves, PRIORITY_POWER)?;
    Ok(Some(match own {
        None => aggregate,
        Some(o) => mk_offset_by(o, aggregate)?,
    }))
}

/// The NORMAL FORM of a schedule of functions, and the laws it satisfies:
/// unit (no pieces IS the head), join (a piece whose term is a Piecewise is
/// spliced in, so no Piecewise ever nests), two adjacent pieces with the same
/// term are one piece, and instants strictly increase or this refuses.
/// `fulfillment` gives the raw record and its normal form the same reading at
/// every instant.
///
/// # Errors
///
/// Two pieces whose instants do not strictly increase.
pub fn mk_piecewise(head: Term, pieces: Vec<(Instant, Term)>) -> Result<Term, FplError> {
    Schedule::new(head, pieces)
        .map(piecewise)
        .map_err(|unordered| {
            FplError(format!(
                "piecewise instants must strictly increase: {} then {}",
                iso(unordered.before),
                iso(unordered.after)
            ))
        })
}

/// A schedule of terms in normal form: each term opened into its own
/// schedule, the schedule of schedules joined (its diagonal, which is
/// splicing), and a piece that repeats the one before it dropped. A schedule
/// with no piece left is its head.
pub(crate) fn piecewise(schedule: Schedule<Term>) -> Term {
    let normal = schedule.map(opened).join().normal();
    if normal.knots().is_empty() {
        normal.into_parts().0
    } else {
        Term::new(TermF::Piecewise(normal))
    }
}

/// A term as the schedule it is: a `Piecewise`'s own, in normal form, and
/// any other term constant.
fn opened(term: Term) -> Schedule<Term> {
    match term.into_out() {
        TermF::Piecewise(schedule) => match piecewise(schedule).into_out() {
            TermF::Piecewise(normal) => normal,
            term => Schedule::constant(Term::new(term)),
        },
        term => Schedule::constant(Term::new(term)),
    }
}

// --- Linking: every Ref bound to the todo it names --------------------------

/// Why a term does not link. A value like every refusal here: a reference is
/// stored data, and stored data can name a todo that is not there, or loop.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LinkError {
    /// A `Ref` names a todo `specs` does not hold — for a store, one it has
    /// never seen, since a known todo with no function is `Absent` there.
    /// Inside another prodrome, reached through a qualified reference, the
    /// todo is named `genesis/todo`.
    #[error("Ref({0:?}) names no known todo")]
    Unknown(String),
    /// The references loop. The path names the cycle, its first todo repeated
    /// at the end; an entity of another prodrome as `genesis/entity`.
    #[error("the references loop: {}", .0.join(" → "))]
    Cycle(Vec<String>),
}

/// One prodrome as a qualified reference reads it (§7.2): each entity's
/// own function, under the key an unqualified `Ref` inside the prodrome
/// gives it, and the environment its `After`s and `Recur`s read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Member {
    pub specs: BTreeMap<String, Term>,
    pub env: Env,
}

/// THE SET OF PRODROMES a term's qualified references read (§7.2), each
/// member by its genesis's name, its content name, and the names a host
/// declares for some of them. A host's environment, passed in: a schema
/// never reads another store (design §4, `Bind`). Its reading is the
/// product of its members' readings, so a reference into one member reads
/// that member and nothing else (the disjointness law).
///
/// A member is ONE READING of one prodrome: two replicas of a prodrome are
/// joined by syncing their objects and reading the union, never by setting
/// two readings side by side, so `with` a genesis the set holds replaces
/// its reading.
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

    /// These stores with `member`, the reading of the prodrome `genesis`
    /// begins.
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

    /// The member the prodrome `genesis` begins, where the set holds it.
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

/// §7.2: every `Ref(x)` bound to `specs[x]`, linked in turn, and refused as a
/// cycle where it is reached again on its own expansion. A closed term is its
/// own link, untouched. A qualified reference reads `∅`: no other prodrome
/// is given ([`link_in`] with no stores).
pub fn link(term: &Term, specs: &BTreeMap<String, Term>) -> Result<Closed, LinkError> {
    link_in(term, specs, &Stores::new())
}

/// [`link`], with `stores` the set its qualified references read. A
/// `RefIn(s, x)` is `x`'s function in the member `s` names, linked there
/// (an unqualified `Ref` inside it names that member's own entity) and
/// compiled against that member's environment (§7.1), so the term it lands
/// in reads it as the member does and no `After` or `Recur` of it reads the
/// wrong history. A store the set does not hold, or an entity its member
/// does not, reads `∅`: the documented absence, no claim on attention.
/// Cycles are refused across members as within one.
///
/// # Errors
///
/// An unqualified `Ref` its prodrome does not hold, or references that
/// loop.
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

// --- ISO instants: the one instant SPELLING every boundary shares ------------

/// Python's `datetime.isoformat()`: microseconds only when non-zero.
pub fn iso(t: Instant) -> String {
    if t.and_utc().timestamp_subsec_micros() == 0 {
        t.format("%Y-%m-%dT%H:%M:%S").to_string()
    } else {
        t.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
    }
}

/// `iso`'s inverse, for anything reading an instant off a wire.
pub fn parse_iso(s: &str) -> Result<Instant, FplError> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(0, 0, 0).expect("midnight is a time"))
        })
        .map_err(|e| FplError(format!("not an ISO instant: {s:?} ({e})")))
}

// --- §2 the literal bridge: the schema as `literal::Value` ----------------
// The grammar's time records carry no arithmetic and `chrono`'s do; they meet here.

/// A term's refusal, as the store's refusal. The two layers keep their own
/// error types because their vocabularies of failure differ — a store reports
/// a missing parent, a term an out-of-range exponent — and this is the one
/// direction the boundary needs.
impl From<FplError> for ProdromeError {
    fn from(error: FplError) -> ProdromeError {
        ProdromeError::Invalid(error.0)
    }
}

/// A store whose terms do not link is invalid content, like a term refused.
impl From<LinkError> for ProdromeError {
    fn from(error: LinkError) -> ProdromeError {
        ProdromeError::Invalid(error.to_string())
    }
}

/// And back: a grammar refusal reaching a caller who asked for a term.
impl From<ProdromeError> for FplError {
    fn from(error: ProdromeError) -> FplError {
        FplError(error.to_string())
    }
}

fn float_value(value: f64) -> literal::Value {
    literal::Value::Float(
        Finite::new(value).expect("a Term's floats are finite: every mk_* refuses the rest"),
    )
}

/// The grammar's `datetime` as an evaluable instant; `fold` reads an event's
/// `at` through it.
pub fn instant_of(at: literal::Datetime) -> Instant {
    NaiveDate::from_ymd_opt(at.year(), at.month(), at.day())
        .and_then(|day| {
            day.and_hms_micro_opt(at.hour(), at.minute(), at.second(), at.microsecond())
        })
        .expect("literal::Datetime::new admits only representable instants")
}

/// And back; fallible, since the grammar's years are 1..=9999.
pub fn datetime_of(t: Instant) -> Result<literal::Datetime, ProdromeError> {
    literal::Datetime::new(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        t.and_utc().timestamp_subsec_micros(),
    )
}

fn instant_value(t: Instant) -> literal::Value {
    literal::Value::Datetime(
        datetime_of(t)
            .expect("a Term's instants came through the grammar, whose years are 1..=9999"),
    )
}

fn delta_value(d: Delta) -> literal::Value {
    literal::Value::Timedelta(
        literal::Timedelta::from_micros(i128::from(us_of(d)))
            .expect("a Delta of i64 microseconds is inside timedelta's range"),
    )
}

fn call_value(name: &str, fields: Fields<&Term>) -> literal::Value {
    literal::Value::call(
        name,
        fields
            .into_iter()
            .map(|(field, slot)| (field.to_owned(), slot_value(slot)))
            .collect(),
    )
}

fn slot_value(slot: Slot<&Term>) -> literal::Value {
    match slot {
        Slot::Real(value) => float_value(value),
        Slot::At(t) => instant_value(t),
        Slot::Span(d) => delta_value(d),
        Slot::Text(text) => literal::Value::Str(text),
        Slot::Nothing => literal::Value::None,
        Slot::Child(term) => term.to_value(),
        Slot::Children(terms) => {
            literal::Value::Tuple(terms.into_iter().map(Term::to_value).collect())
        }
        Slot::Rows(row, rows) => literal::Value::Tuple(
            rows.into_iter()
                .map(|fields| call_value(row, fields))
                .collect(),
        ),
    }
}

/// A call's fields, `None` or left out being absent.
struct CallSource<'a>(&'a Call);

impl<'a> CallSource<'a> {
    fn read<T>(
        &self,
        name: &'static str,
        decode: impl FnOnce(&'a literal::Value) -> Option<T>,
    ) -> Field<T> {
        Field::new(
            name,
            match self.0.field(name) {
                None | Some(literal::Value::None) => Ok(None),
                Some(value) => decode(value).map(Some).ok_or_else(|| {
                    FplError(format!("{}.{name} is malformed: {value:?}", self.0.name))
                }),
            },
        )
    }
}

impl FieldSource for CallSource<'_> {
    fn real(&mut self, name: &'static str) -> Field<f64> {
        self.read(name, |value| match value {
            literal::Value::Float(value) => Some(value.get()),
            literal::Value::Int(int) => int.as_i64().map(|value| value as f64),
            _ => None,
        })
    }

    fn at(&mut self, name: &'static str) -> Field<Instant> {
        self.read(name, |value| match value {
            literal::Value::Datetime(at) => Some(instant_of(*at)),
            _ => None,
        })
    }

    fn span(&mut self, name: &'static str) -> Field<Delta> {
        self.read(name, |value| match value {
            literal::Value::Timedelta(d) => i64::try_from(d.total_micros())
                .ok()
                .map(Duration::microseconds),
            _ => None,
        })
    }

    fn text(&mut self, name: &'static str) -> Field<String> {
        self.read(name, |value| value.as_str().map(str::to_owned))
    }

    fn child(&mut self, name: &'static str) -> Field<Term> {
        self.read(name, Some).and_then(Term::from_value)
    }

    fn children(&mut self, name: &'static str) -> Field<Vec<Term>> {
        self.read(name, literal::Value::as_tuple)
            .and_then(|items| items.iter().map(Term::from_value).collect())
    }

    fn rows<T>(
        &mut self,
        name: &'static str,
        row: &'static str,
        mut read: impl FnMut(&mut Self) -> Result<T, FplError>,
    ) -> Field<Vec<T>> {
        self.read(name, literal::Value::as_tuple).and_then(|items| {
            items
                .iter()
                .map(|item| match item.as_call() {
                    Some(call) if call.name == row => read(&mut CallSource(call)),
                    _ => err(format!("expected a {row}(...), got {item:?}")),
                })
                .collect()
        })
    }
}

impl Term {
    /// The term as one expression of §2's grammar.
    pub fn to_value(&self) -> literal::Value {
        let (kind, fields) = schema::fields(self.out());
        call_value(kind, fields)
    }

    /// One expression of §2's grammar, through the smart constructors.
    pub fn from_value(value: &literal::Value) -> Result<Term, FplError> {
        let call = value
            .as_call()
            .ok_or_else(|| FplError(format!("expected a term constructor, got {value:?}")))?;
        schema::build(&call.name, &mut CallSource(call))
    }
}

/// A term in a field of a stored type: an optional spec, for one. The FPL
/// signature table ([`schema::fields`] and [`schema::build`]) gives the
/// print and the read, so the impl is written here and not derived.
impl crate::literal::codec::Literal for Term {
    fn to_value(&self) -> literal::Value {
        Term::to_value(self)
    }

    fn from_value(value: &literal::Value) -> Result<Term, ProdromeError> {
        Ok(Term::from_value(value)?)
    }
}

/// The canonical §2 print of a term.
pub fn print_term(term: &Term) -> String {
    print_literal(&term.to_value())
}

/// `print_term`'s inverse.
pub fn parse_term(text: &str) -> Result<Term, FplError> {
    Term::from_value(&literal::parse_literal(text, signatures())?)
}
