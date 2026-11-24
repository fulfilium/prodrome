//! A declared register's order, interpreted as an order on values
//! ([`Order`]), so that a declared register is read by the completion that
//! every register is read by.

use std::collections::BTreeSet;

use prodrome_data::fold::{Discrete, Inflationary, Order, Total};

use super::Datum;

/// A register's order, admitted: the machine's closure computed once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Interpreted {
    Discrete,
    Total,
    Machine(Closure),
    Inclusion,
}

/// A finite partial order given by its covering relation: `below[i][j]`
/// iff state `i ≤` state `j`, the reflexive and transitive closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Closure {
    states: Vec<String>,
    below: Vec<Vec<bool>>,
}

impl Closure {
    /// The closure of `covers` over `states`, or a state that lies strictly
    /// above itself: a cycle, under which `≤` would not be antisymmetric.
    pub(crate) fn of(states: &[String], covers: &[(String, String)]) -> Result<Closure, String> {
        let index = |state: &str| states.iter().position(|s| s == state);
        let n = states.len();
        // `strict[i][j]`: a nonempty chain of covers leads from `i` to `j`.
        let mut strict = vec![vec![false; n]; n];
        for (lower, upper) in covers {
            if let (Some(i), Some(j)) = (index(lower), index(upper)) {
                strict[i][j] = true;
            }
        }
        for k in 0..n {
            let through = strict[k].clone();
            for row in &mut strict {
                if row[k] {
                    for (reach, &onward) in row.iter_mut().zip(&through) {
                        *reach |= onward;
                    }
                }
            }
        }
        if let Some(i) = (0..n).find(|&i| strict[i][i]) {
            return Err(states[i].clone());
        }
        for (i, row) in strict.iter_mut().enumerate() {
            row[i] = true;
        }
        Ok(Closure {
            states: states.to_vec(),
            below: strict,
        })
    }

    pub(crate) fn le(&self, lower: &str, upper: &str) -> bool {
        let index = |state: &str| self.states.iter().position(|s| s == state);
        match (index(lower), index(upper)) {
            (Some(i), Some(j)) => self.below[i][j],
            _ => false,
        }
    }

    /// Is some state below every state?
    pub(crate) fn has_bottom(&self) -> bool {
        self.below.iter().any(|row| row.iter().all(|&le| le))
    }
}

/// A value of a declared register under that register's order. Only
/// [`super::Declaration::value`] builds one.
#[derive(Debug, Clone, Copy)]
pub struct Valued<'d> {
    pub(crate) order: &'d Interpreted,
    pub(crate) datum: &'d Datum,
}

impl Valued<'_> {
    /// The value that the order compares: the datum itself, but under
    /// inclusion the list's items sorted and each once.
    ///
    /// So two writes that the order cannot tell apart give one datum, and a
    /// print of a reading depends only on the reading.
    #[must_use]
    pub fn value(&self) -> Datum {
        match (self.order, self.datum) {
            (Interpreted::Inclusion, Datum::List(items)) => {
                let set: BTreeSet<&Datum> = items.iter().collect();
                Datum::List(set.into_iter().cloned().collect())
            }
            (_, datum) => datum.clone(),
        }
    }
}

/// Each order of the vocabulary is one of the data crate's: discrete is [`Discrete`],
/// total is [`Total`] on the integers, inclusion is the order on
/// `BTreeSet`s, and a machine is its closure. Values of different shapes
/// (which admission's typing law rules out) are incomparable.
impl Order for Valued<'_> {
    fn le(&self, other: &Self) -> bool {
        match (self.order, self.datum, other.datum) {
            (Interpreted::Discrete, a, b) => Discrete(a).le(&Discrete(b)),
            (Interpreted::Total, Datum::Integer(a), Datum::Integer(b)) => Total(a).le(&Total(b)),
            (Interpreted::Machine(closure), Datum::Alternative(a), Datum::Alternative(b)) => {
                closure.le(a, b)
            }
            (Interpreted::Inclusion, Datum::List(a), Datum::List(b)) => {
                let a: BTreeSet<&Datum> = a.iter().collect();
                let b: BTreeSet<&Datum> = b.iter().collect();
                Order::le(&a, &b)
            }
            _ => false,
        }
    }
}

/// A value of a register declared inflationary, and only of one. The
/// declaration is not trusted, so the type that lets the append call
/// [`grows`](prodrome_data::fold::grows) is made only where admission let the
/// register climb.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Climbing<'d>(pub(crate) Valued<'d>);

impl Order for Climbing<'_> {
    fn le(&self, other: &Self) -> bool {
        self.0.le(&other.0)
    }
}

impl Inflationary for Climbing<'_> {}
