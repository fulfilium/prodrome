//! The smart constructors of a term: the only path to a term that keeps
//! the bounds of SPEC §7 (SPEC §1: records are dumb data).

use chrono::Duration;

use prodrome_data::event::TodoId;
use prodrome_data::segment::Segment;

use super::{err, iso, Delta, FplError, Instant, PRIORITY_POWER};
use crate::schedule::Schedule;
use crate::term::{CurvePoint, Term, TermF};

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

/// It refuses an empty `terms`, so that a term reads `∅` only where it holds
/// an `Absent`.
pub fn mk_least(terms: Vec<Term>) -> Result<Term, FplError> {
    if terms.is_empty() {
        return err("Least needs a member");
    }
    Ok(Term::new(TermF::Least { terms }))
}

/// `Least` of the distinct `terms`, or the one term where all are equal, so
/// that a reading without a conflict keeps its form.
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

/// `term` re-anchored to the last tending of `todo`. It refuses a `todo` that
/// breaks the rule of [`TodoId`].
pub fn mk_recur(
    todo: String,
    anchor: Instant,
    term: Term,
    pending: Term,
) -> Result<Term, FplError> {
    TodoId::new(todo.as_str()).map_err(|e| FplError(format!("Recur.todo: {e}")))?;
    Ok(Term::new(TermF::Recur {
        todo,
        anchor,
        term,
        pending,
    }))
}

/// `term` repeated every `period` from `anchor`, both ways in time. It refuses
/// a `period` that is not positive.
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

/// A reference to the fulfillment of todo `todo`. It refuses a `todo` that
/// breaks the rule of [`TodoId`].
pub fn mk_ref(todo: String) -> Result<Term, FplError> {
    TodoId::new(todo.as_str()).map_err(|e| FplError(format!("Ref.todo: {e}")))?;
    Ok(Term::new(TermF::Ref {
        store: None,
        entity: todo,
    }))
}

/// A qualified reference to `entity` in the prodrome that `store` names: a
/// genesis's name, or a name that the evaluator's `Stores::named` declares
/// (SPEC §7.2).
///
/// # Errors
///
/// It refuses a `store` that breaks the rule of [`TodoId`], or
/// an `entity` that breaks the rule of [`Segment`] (not empty, no `/`).
pub fn mk_ref_in(store: String, entity: String) -> Result<Term, FplError> {
    TodoId::new(store.as_str()).map_err(|e| FplError(format!("RefIn.store: {e}")))?;
    Segment::new(entity.as_str()).map_err(|e| FplError(format!("RefIn.entity: {e}")))?;
    Ok(Term::new(TermF::Ref {
        store: Some(store),
        entity,
    }))
}

/// The term with no value at any instant. It refuses nothing.
pub fn mk_absent() -> Term {
    Term::new(TermF::Absent)
}

/// A todo's fulfillment function from its own spec and the number of items
/// of its checklist (SPEC §6, "The todo schema's registers"). With neither a
/// spec nor an item, it returns `None`.
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

/// A schedule of terms in normal form (SPEC §7, "Normal form"), which reads
/// as the raw schedule at every instant.
///
/// Law 5: the normal form is a unit (no pieces is the head) and a join (no
/// `Piecewise` nests), it is idempotent, and it has no two adjacent equal
/// pieces.
///
/// # Errors
///
/// It refuses two pieces whose instants do not strictly increase.
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

/// For this library's own code; no promise. The normal-form term of a
/// schedule of terms.
#[doc(hidden)]
pub fn piecewise(schedule: Schedule<Term>) -> Term {
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
