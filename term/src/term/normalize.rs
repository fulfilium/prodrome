use crate::fpl::piecewise;
use crate::schedule::Schedule;
use crate::term::{Term, TermF};

/// The term in Piecewise-outermost normal form (SPEC §7, "Normal form").
///
/// Law 5: `normalize` preserves every reading and is idempotent.
pub fn normalize(term: &Term) -> Term {
    term.cata(|layer| match layer {
        TermF::Piecewise(schedule) => piecewise(schedule),
        TermF::Shift { delta, term } => match term.into_out() {
            // ⟦Shift(δ, pw)⟧(now) = ⟦pw⟧(now + δ): the schedule read δ later.
            TermF::Piecewise(schedule) => piecewise(
                schedule
                    .shift(delta)
                    .map(|term| Term::new(TermF::Shift { delta, term })),
            ),
            term => Term::new(TermF::Shift {
                delta,
                term: Term::new(term),
            }),
        },
        layer @ (TermF::Conj { .. }
        | TermF::Least { .. }
        | TermF::Offset { .. }
        | TermF::Gate { .. }
        | TermF::Importance { .. }
        | TermF::OffsetBy { .. }) => pointwise(layer),
        layer => Term::new(layer),
    })
}

/// A pointwise layer over its children's schedules: the applicative's
/// `sequence` of them, the layer applied at every instant one changes.
fn pointwise(layer: TermF<Term>) -> Term {
    if !layer
        .children()
        .into_iter()
        .any(|child| matches!(child.out(), TermF::Piecewise(_)))
    {
        return Term::new(layer);
    }
    let children = layer.children().into_iter().map(|child| match child.out() {
        TermF::Piecewise(schedule) => schedule.clone(),
        _ => Schedule::constant(child.clone()),
    });
    piecewise(Schedule::sequence(children).map(|values| {
        // One value per child, in `children`'s order, which is `map`'s.
        let mut values = values.into_iter();
        Term::new(layer.map(|child| values.next().unwrap_or_else(|| child.clone())))
    }))
}
