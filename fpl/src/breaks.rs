use std::collections::BTreeSet;
use std::iter::Sum;
use std::ops::Add;

use chrono::Duration;

use crate::fpl::{
    advance, delta_of_us, div_delta, eval, fulfillment, retreat, us_of, Closed, Env, Instant,
};
use prodrome_term::term::{Term, TermF};

/// Where a term changes slope, where it jumps, and whether knots there are the
/// whole curve (SPEC §7, "Breakpoints"; law 7).
///
/// The type is a monoid: the instants concatenate, and `exact` is their
/// conjunction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breaks {
    pub slopes: Vec<Instant>,
    pub jumps: Vec<Instant>,
    pub exact: bool,
}

impl Breaks {
    fn sampled() -> Self {
        Breaks {
            exact: false,
            ..Breaks::default()
        }
    }

    fn constant(&self) -> bool {
        self.exact && self.slopes.is_empty() && self.jumps.is_empty()
    }
}

impl Default for Breaks {
    fn default() -> Self {
        Breaks {
            slopes: vec![],
            jumps: vec![],
            exact: true,
        }
    }
}

impl Add for Breaks {
    type Output = Breaks;

    fn add(mut self, other: Breaks) -> Breaks {
        self.slopes.extend(other.slopes);
        self.jumps.extend(other.jumps);
        self.exact = self.exact && other.exact;
        self
    }
}

impl Sum for Breaks {
    fn sum<I: Iterator<Item = Breaks>>(parts: I) -> Breaks {
        parts.fold(Breaks::default(), Add::add)
    }
}

/// The breakpoints of `term`.
///
/// A jump needs a knot a second before it too, or the line across it would be
/// a ramp that the evaluator never produced. A piecewise part keeps its
/// transitions as knots even when a composite part makes the whole sampled.
pub fn breakpoints(term: &Term) -> Breaks {
    term.para(|layer| match layer {
        TermF::Least { terms } => least(terms),
        layer => local(layer.map(|(_, breaks)| breaks.clone())),
    })
}

fn local(layer: TermF<Breaks>) -> Breaks {
    match layer {
        TermF::Flat { .. } | TermF::Absent => Breaks::default(),
        TermF::Decay {
            end_date,
            lead_up,
            start_date,
            ..
        } => Breaks {
            slopes: vec![end_date],
            jumps: BTreeSet::from_iter(
                [Some(retreat(end_date, lead_up)), start_date]
                    .into_iter()
                    .flatten(),
            )
            .into_iter()
            .collect(),
            exact: true,
        },
        TermF::Curve { points } => Breaks {
            slopes: points.iter().map(|point| point.at).collect(),
            ..Breaks::default()
        },
        TermF::Piecewise(schedule) => {
            let transitions = Breaks {
                jumps: schedule.knots().iter().map(|(at, _)| *at).collect(),
                ..Breaks::default()
            };
            let (head, pieces) = schedule.into_parts();
            std::iter::once(head)
                .chain(pieces.into_iter().map(|(_, piece)| piece))
                .sum::<Breaks>()
                + transitions
        }
        // Affine in the child.
        TermF::Offset { term, .. } => term,
        TermF::Shift { delta, term } => Breaks {
            slopes: term.slopes.iter().map(|at| retreat(*at, delta)).collect(),
            jumps: term.jumps.iter().map(|at| retreat(*at, delta)).collect(),
            exact: term.exact,
        },
        TermF::OffsetBy { delta, term } if term.constant() => delta,
        TermF::OffsetBy { delta, term } if delta.constant() => term,
        layer @ (TermF::Conj { .. } | TermF::Gate { .. } | TermF::Importance { .. })
            if layer.children().into_iter().all(Breaks::constant) =>
        {
            Breaks::default()
        }
        _ => Breaks::sampled(),
    }
}

/// Exact members are lines between their instants, so their least bends only
/// there and where two of them cross.
fn least(members: Vec<(&Term, Breaks)>) -> Breaks {
    let (terms, breaks): (Vec<&Term>, Vec<Breaks>) = members.into_iter().unzip();
    let mut whole: Breaks = breaks.into_iter().sum();
    if whole.exact {
        let instants: BTreeSet<Instant> =
            whole.slopes.iter().chain(&whole.jumps).copied().collect();
        let crossings: Vec<Instant> = instants
            .iter()
            .zip(instants.iter().skip(1))
            .flat_map(|(from, to)| crossings(&terms, *from, *to))
            .collect();
        whole.slopes.extend(crossings);
    }
    whole
}

/// Where two members, each a line on `[from, to)`, meet strictly inside it.
/// An exact term reads no history, so the empty environment is its reading.
fn crossings(terms: &[&Term], from: Instant, to: Instant) -> Vec<Instant> {
    let span = (to - from).num_microseconds().unwrap_or(i64::MAX);
    let half = span / 2;
    let read = |at: Instant| -> Vec<Option<f64>> {
        terms
            .iter()
            .map(|term| eval(term, at, &Env::new()))
            .collect()
    };
    let lines: Vec<(f64, f64)> = read(from)
        .into_iter()
        .zip(read(advance(from, Duration::microseconds(half))))
        .filter_map(|(start, middle)| Some((start?, middle?)))
        .collect();
    let mut out = Vec::new();
    for (i, (a0, a1)) in lines.iter().enumerate() {
        for (b0, b1) in &lines[i + 1..] {
            let (d0, d1) = (a0 - b0, a1 - b1);
            let at = d0 / (d0 - d1) * half as f64;
            if d0 != d1 && 0.0 < at && at < span as f64 {
                out.push(advance(from, Duration::microseconds(at.round() as i64)));
            }
        }
    }
    out
}

/// The number of evaluations for a term that is not piecewise-linear: one
/// every ~1.9 h over a week, one every ~9 h over a month.
pub const SAMPLES: i64 = 96;

/// One point of a drawn curve. The value is `∅` where the term has no value,
/// and a reader must draw no line to or from that point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Knot {
    pub at: Instant,
    pub value: Option<f64>,
}

/// A term's fulfillment over a window, and whether the knots are the whole
/// curve.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub knots: Vec<Knot>,
    pub exact: bool,
}

/// The curve the graph draws for one term over `[from, to]`.
///
/// Exact where the term is piecewise-linear: knots at the window edges, at
/// every slope change inside it, and a second on either side of every jump.
/// Composite terms also get [`SAMPLES`] evenly spaced evaluations. Each knot
/// is evaluated against `env_at(t)`, the environment as of that instant, so a
/// dependency that an `After` reads is unbound before it completed and bound
/// from then on.
pub fn series_knots(
    term: &Closed,
    from: Instant,
    to: Instant,
    env_at: impl Fn(Instant) -> Env,
) -> Series {
    // The schedule at the root, where the breakpoints can read it.
    let spec = term.normalize();
    let breaks = breakpoints(spec.term());
    let mut instants: BTreeSet<Instant> = BTreeSet::from([from, to]);
    for b in breaks.slopes.iter().chain(&breaks.jumps) {
        if from < *b && *b < to {
            instants.insert(*b);
        }
    }
    for j in &breaks.jumps {
        if from < *j && *j <= to {
            instants.insert(retreat(*j, Duration::seconds(1)));
        }
    }
    if !breaks.exact {
        // The reference's `(end - start) / SAMPLES` is microsecond-exact and
        // rounded half to even; `i * step` is then exact. Anything coarser
        // would put the samples on instants the reference never evaluated.
        let step = us_of(div_delta(to - from, SAMPLES));
        for i in 0..=SAMPLES {
            instants.insert(advance(from, delta_of_us(step * i128::from(i))));
        }
    }
    Series {
        knots: instants
            .into_iter()
            .map(|at| Knot {
                at,
                value: fulfillment(&spec, at, &env_at(at)),
            })
            .collect(),
        exact: breaks.exact,
    }
}
