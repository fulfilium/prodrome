use std::convert::Infallible;

use crate::fpl::{Delta, Instant};
use crate::schedule::Schedule;

/// One layer of the term functor: every child position is an `A`.
#[derive(Debug, Clone, PartialEq)]
pub enum TermF<A> {
    Flat {
        value: f64,
    },
    Decay {
        start: f64,
        end: f64,
        end_date: Instant,
        lead_up: Delta,
        start_date: Option<Instant>,
    },
    Curve {
        points: Vec<CurvePoint>,
    },
    Conj {
        terms: Vec<A>,
        p: f64,
    },
    /// The least of the members that have a value; `∅` when none does.
    Least {
        terms: Vec<A>,
    },
    Offset {
        delta: f64,
        term: A,
    },
    Gate {
        gate: A,
        body: A,
    },
    Shift {
        delta: Delta,
        term: A,
    },
    Within {
        window: Delta,
        p: f64,
        term: A,
    },
    Importance {
        w: f64,
        term: A,
    },
    After {
        event: String,
        anchor: Instant,
        term: A,
        pending: A,
        needs: Option<Delta>,
    },
    /// `term`, authored against `anchor`, re-anchored to the last tending of
    /// `todo` as `After` is to a completion; `pending` before the first.
    Recur {
        todo: String,
        anchor: Instant,
        term: A,
        pending: A,
    },
    /// `term` on `[anchor, anchor + period)`, repeated on the calendar.
    Periodic {
        period: Delta,
        anchor: Instant,
        term: A,
    },
    /// A step function of terms: the term in force, read at `now`.
    Piecewise(Schedule<A>),
    OffsetBy {
        delta: A,
        term: A,
    },
    /// The fulfillment of the entity `entity`, a free variable that
    /// `fpl::link` binds (SPEC §7.2).
    ///
    /// `store` is `None` for an entity of the term's own prodrome, which
    /// prints as `Ref(todo)`. Otherwise it names the prodrome in the set that
    /// the host passes, and the term prints as `RefIn(store, entity)`.
    Ref {
        store: Option<String>,
        entity: String,
    },
    /// `∅` at every instant.
    Absent,
}

/// A named landmark on a `Curve`. Only `mk_curve` checks it.
#[derive(Debug, Clone, PartialEq)]
pub struct CurvePoint {
    pub at: Instant,
    pub value: f64,
    pub label: String,
}

impl<A> TermF<A> {
    /// The layer with `f` applied to every child in declaration order; it
    /// stops at the first error. It takes the children by reference, so that
    /// `map`, `as_ref` and `children` derive from it, and it clones the
    /// layer's own fields.
    pub fn traverse<'a, B, E>(
        &'a self,
        mut f: impl FnMut(&'a A) -> Result<B, E>,
    ) -> Result<TermF<B>, E> {
        Ok(match self {
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
            TermF::Conj { terms, p } => TermF::Conj {
                terms: terms.iter().map(f).collect::<Result<_, _>>()?,
                p: *p,
            },
            TermF::Least { terms } => TermF::Least {
                terms: terms.iter().map(f).collect::<Result<_, _>>()?,
            },
            TermF::Offset { delta, term } => TermF::Offset {
                delta: *delta,
                term: f(term)?,
            },
            TermF::Gate { gate, body } => TermF::Gate {
                gate: f(gate)?,
                body: f(body)?,
            },
            TermF::Shift { delta, term } => TermF::Shift {
                delta: *delta,
                term: f(term)?,
            },
            TermF::Within { window, p, term } => TermF::Within {
                window: *window,
                p: *p,
                term: f(term)?,
            },
            TermF::Importance { w, term } => TermF::Importance {
                w: *w,
                term: f(term)?,
            },
            TermF::After {
                event,
                anchor,
                term,
                pending,
                needs,
            } => TermF::After {
                event: event.clone(),
                anchor: *anchor,
                term: f(term)?,
                pending: f(pending)?,
                needs: *needs,
            },
            TermF::Recur {
                todo,
                anchor,
                term,
                pending,
            } => TermF::Recur {
                todo: todo.clone(),
                anchor: *anchor,
                term: f(term)?,
                pending: f(pending)?,
            },
            TermF::Periodic {
                period,
                anchor,
                term,
            } => TermF::Periodic {
                period: *period,
                anchor: *anchor,
                term: f(term)?,
            },
            TermF::Piecewise(schedule) => TermF::Piecewise(schedule.traverse(&mut f)?),
            TermF::OffsetBy { delta, term } => TermF::OffsetBy {
                delta: f(delta)?,
                term: f(term)?,
            },
            TermF::Ref { store, entity } => TermF::Ref {
                store: store.clone(),
                entity: entity.clone(),
            },
            TermF::Absent => TermF::Absent,
        })
    }

    pub fn map<'a, B>(&'a self, mut f: impl FnMut(&'a A) -> B) -> TermF<B> {
        infallible(self.traverse(|child| Ok(f(child))))
    }

    pub fn as_ref(&self) -> TermF<&A> {
        self.map(|child| child)
    }

    /// The child positions, in declaration order.
    pub fn children(&self) -> Vec<&A> {
        let mut children = Vec::new();
        self.map(|child| children.push(child));
        children
    }

    /// The kind tag the wasm JSON puts on this layer.
    pub fn kind(&self) -> &'static str {
        match self {
            TermF::Flat { .. } => "flat",
            TermF::Decay { .. } => "decay",
            TermF::Curve { .. } => "curve",
            TermF::Conj { .. } => "conj",
            TermF::Least { .. } => "least",
            TermF::Offset { .. } => "offset",
            TermF::Gate { .. } => "gate",
            TermF::Shift { .. } => "shift",
            TermF::Within { .. } => "within",
            TermF::Importance { .. } => "importance",
            TermF::After { .. } => "after",
            TermF::Recur { .. } => "recur",
            TermF::Periodic { .. } => "periodic",
            TermF::Piecewise(_) => "piecewise",
            TermF::OffsetBy { .. } => "offsetBy",
            TermF::Ref { store: None, .. } => "ref",
            TermF::Ref { store: Some(_), .. } => "refIn",
            TermF::Absent => "absent",
        }
    }
}

fn infallible<T>(result: Result<T, Infallible>) -> T {
    match result {
        Ok(value) => value,
        Err(never) => match never {},
    }
}

/// The fixed point `Term ≅ TermF Term`, a deep embedding and never a closure.
#[derive(Debug, Clone, PartialEq)]
pub struct Term(Box<TermF<Term>>);

impl Term {
    /// A term of one layer, built without checks; use the `mk_*`
    /// constructors of `fpl` to build a checked term.
    #[inline]
    pub fn new(layer: TermF<Term>) -> Self {
        Term(Box::new(layer))
    }

    #[inline]
    pub fn out(&self) -> &TermF<Term> {
        &self.0
    }

    #[inline]
    pub fn into_out(self) -> TermF<Term> {
        *self.0
    }

    /// The fold of `alg` over each layer, children first.
    pub fn cata<R>(&self, mut alg: impl FnMut(TermF<R>) -> R) -> R {
        infallible(self.try_cata(|layer| Ok(alg(layer))))
    }

    /// The fold in `Result`, stopping at the first error in declaration order.
    pub fn try_cata<R, E>(&self, mut alg: impl FnMut(TermF<R>) -> Result<R, E>) -> Result<R, E> {
        fn go<R, E>(term: &Term, alg: &mut impl FnMut(TermF<R>) -> Result<R, E>) -> Result<R, E> {
            let layer = term.out().traverse(|child| go(child, alg))?;
            alg(layer)
        }
        go(self, &mut alg)
    }

    /// The fold that also sees each subterm it folded.
    pub fn para<R>(&self, mut alg: impl FnMut(TermF<(&Term, R)>) -> R) -> R {
        fn go<'a, R>(term: &'a Term, alg: &mut impl FnMut(TermF<(&'a Term, R)>) -> R) -> R {
            let layer = term.out().map(|child| (child, go(child, alg)));
            alg(layer)
        }
        go(self, &mut alg)
    }

    /// Whether `node` holds of this term or of any term beneath it.
    pub fn any(&self, node: impl Fn(&TermF<bool>) -> bool) -> bool {
        self.cata(|layer| node(&layer) || layer.children().into_iter().any(|below| *below))
    }
}
