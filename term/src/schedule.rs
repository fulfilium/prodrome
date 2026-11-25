//! A piecewise constant function of time, `Instant → A` (design §6.2).
//! `tests/all/schedule.rs` checks each of these laws against
//! [`Schedule::at`]:
//!
//! - `map f s` at `t` is `f` of `s` at `t` (a functor);
//! - `zip a b` at `t` is `(a at t, b at t)`, and `constant` is its unit (an
//!   applicative);
//! - `a + b` at `t` is `a at t + b at t` where `A` is a monoid (`Default` and
//!   `Add`), so `at` is a monoid homomorphism;
//! - `join ss` at `t` is `ss at t` at `t` (the diagonal of the reader monad);
//! - `shift δ s` at `t` is `s` at `t + δ`;
//! - `normal s` reads as `s` everywhere, and two schedules read alike
//!   everywhere exactly when their normal forms are equal;
//! - `next_change s t` is the first knot after `t` whose value differs from
//!   `s` at `t`, so `s` is constant on `[t, next)` and differs at `next`.
//!
//! Knots strictly increase, and a knot may repeat the value before it.
//! [`Schedule::normal`] removes such repeats.

use std::collections::BTreeMap;
use std::iter::Sum;
use std::ops::Add;

use thiserror::Error;

use crate::fpl::{Delta, Instant};

/// A step function of time: `head` until the first knot, then each knot's
/// value from its instant until the next. Knots strictly increase.
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule<A> {
    head: A,
    knots: Vec<(Instant, A)>,
}

/// The refusal of [`Schedule::new`]: two knots whose instants do not strictly
/// increase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("a schedule's instants must strictly increase: {before} then {after}")]
pub struct Unordered {
    pub before: Instant,
    pub after: Instant,
}

impl<A> Schedule<A> {
    /// `value` at every instant.
    pub fn constant(value: A) -> Schedule<A> {
        Schedule {
            head: value,
            knots: Vec::new(),
        }
    }

    /// A schedule of `head` and `knots`.
    ///
    /// # Errors
    ///
    /// Returns [`Unordered`] where two knots' instants do not strictly
    /// increase.
    pub fn new(head: A, knots: Vec<(Instant, A)>) -> Result<Schedule<A>, Unordered> {
        if let Some(pair) = knots.windows(2).find(|pair| pair[0].0 >= pair[1].0) {
            return Err(Unordered {
                before: pair[0].0,
                after: pair[1].0,
            });
        }
        Ok(Schedule { head, knots })
    }

    /// `head`, then each value of `knots` from its instant.
    pub fn of(head: A, knots: BTreeMap<Instant, A>) -> Schedule<A> {
        Schedule {
            head,
            knots: knots.into_iter().collect(),
        }
    }

    /// The value before the first knot.
    pub fn head(&self) -> &A {
        &self.head
    }

    /// Every knot, its instants strictly increasing.
    pub fn knots(&self) -> &[(Instant, A)] {
        &self.knots
    }

    pub fn into_parts(self) -> (A, Vec<(Instant, A)>) {
        (self.head, self.knots)
    }

    /// The value in force at `t`.
    pub fn at(&self, t: Instant) -> &A {
        self.since(t).1
    }

    /// The value in force at `t` and the knot it has held since, `None` for
    /// the head.
    pub fn since(&self, t: Instant) -> (Option<Instant>, &A) {
        match self.knots.partition_point(|(at, _)| *at <= t) {
            0 => (None, &self.head),
            n => {
                let (at, value) = &self.knots[n - 1];
                (Some(*at), value)
            }
        }
    }

    /// The value from the last knot on, for ever after.
    pub fn last(&self) -> &A {
        self.knots.last().map_or(&self.head, |(_, value)| value)
    }

    /// `f` of the value at every instant.
    #[must_use]
    pub fn map<B>(self, mut f: impl FnMut(A) -> B) -> Schedule<B> {
        Schedule {
            head: f(self.head),
            knots: self
                .knots
                .into_iter()
                .map(|(at, value)| (at, f(value)))
                .collect(),
        }
    }

    /// `map` by reference, head first, stopping at the first error.
    ///
    /// # Errors
    ///
    /// Returns the first error of `f`.
    pub fn traverse<'a, B, E>(
        &'a self,
        mut f: impl FnMut(&'a A) -> Result<B, E>,
    ) -> Result<Schedule<B>, E> {
        Ok(Schedule {
            head: f(&self.head)?,
            knots: self
                .knots
                .iter()
                .map(|(at, value)| Ok((*at, f(value)?)))
                .collect::<Result<_, _>>()?,
        })
    }

    /// At `t`, the result is this schedule at `t + by`, so a knot at `τ`
    /// holds from `τ − by`.
    #[must_use]
    pub fn shift(self, by: Delta) -> Schedule<A> {
        Schedule {
            head: self.head,
            knots: self
                .knots
                .into_iter()
                .map(|(at, value)| (at - by, value))
                .collect(),
        }
    }

    /// Both values at every instant, with a knot wherever either schedule has
    /// one.
    #[must_use]
    pub fn zip<B>(self, other: Schedule<B>) -> Schedule<(A, B)>
    where
        A: Clone,
        B: Clone,
    {
        let head = (self.head, other.head);
        let (mut a, mut b) = head.clone();
        let mut knots = Vec::with_capacity(self.knots.len() + other.knots.len());
        let mut left = self.knots.into_iter().peekable();
        let mut right = other.knots.into_iter().peekable();
        loop {
            let at = match (left.peek(), right.peek()) {
                (Some((l, _)), Some((r, _))) => *l.min(r),
                (Some((l, _)), None) => *l,
                (None, Some((r, _))) => *r,
                (None, None) => break,
            };
            if let Some((_, value)) = left.next_if(|(l, _)| *l == at) {
                a = value;
            }
            if let Some((_, value)) = right.next_if(|(r, _)| *r == at) {
                b = value;
            }
            knots.push((at, (a.clone(), b.clone())));
        }
        Schedule { head, knots }
    }

    /// Every schedule's value at every instant, as `zip` over a sequence.
    pub fn sequence(schedules: impl IntoIterator<Item = Schedule<A>>) -> Schedule<Vec<A>>
    where
        A: Clone,
    {
        schedules
            .into_iter()
            .fold(Schedule::constant(Vec::new()), |all, one| {
                all.zip(one).map(|(mut all, one)| {
                    all.push(one);
                    all
                })
            })
    }

    /// Drops each knot that repeats the value before it, so two schedules that
    /// read alike everywhere become equal.
    #[must_use]
    pub fn normal(self) -> Schedule<A>
    where
        A: PartialEq,
    {
        let mut knots: Vec<(Instant, A)> = Vec::with_capacity(self.knots.len());
        for (at, value) in self.knots {
            let before = knots.last().map_or(&self.head, |(_, value)| value);
            if *before != value {
                knots.push((at, value));
            }
        }
        Schedule {
            head: self.head,
            knots,
        }
    }

    /// The first knot after `t` whose value differs from the value at `t`, or
    /// `None`. The schedule is constant on `[t, next)`.
    pub fn next_change(&self, t: Instant) -> Option<Instant>
    where
        A: PartialEq,
    {
        let now = self.at(t);
        let after = self.knots.partition_point(|(at, _)| *at <= t);
        self.knots[after..]
            .iter()
            .find(|(_, value)| value != now)
            .map(|(at, _)| *at)
    }
}

impl<A> Schedule<Schedule<A>> {
    /// The diagonal: at `t`, the inner schedule in force at `t`, read at `t`.
    /// The knots of an inner schedule outside the stretch of its outer knot
    /// are not read.
    #[must_use]
    pub fn join(self) -> Schedule<A> {
        let first = self.knots.first().map(|(at, _)| *at);
        let (head, before) = self.head.into_parts();
        let mut knots: Vec<(Instant, A)> = before
            .into_iter()
            .filter(|(at, _)| first.is_none_or(|first| *at < first))
            .collect();
        let mut outer = self.knots.into_iter().peekable();
        while let Some((from, inner)) = outer.next() {
            let until = outer.peek().map(|(at, _)| *at);
            let (mut value, inner) = inner.into_parts();
            let mut later = Vec::new();
            for (at, next) in inner {
                if at <= from {
                    value = next;
                } else if until.is_none_or(|until| at < until) {
                    later.push((at, next));
                }
            }
            knots.push((from, value));
            knots.extend(later);
        }
        Schedule { head, knots }
    }
}

/// The constant default: the monoid's unit where `A` has one.
impl<A: Default> Default for Schedule<A> {
    fn default() -> Schedule<A> {
        Schedule::constant(A::default())
    }
}

/// Pointwise: `(a + b)` at `t` is `a` at `t` plus `b` at `t`.
impl<A: Add<Output = A> + Clone> Add for Schedule<A> {
    type Output = Schedule<A>;

    fn add(self, other: Schedule<A>) -> Schedule<A> {
        self.zip(other).map(|(a, b)| a + b)
    }
}

impl<A: Add<Output = A> + Clone + Default> Sum for Schedule<A> {
    fn sum<I: Iterator<Item = Schedule<A>>>(schedules: I) -> Schedule<A> {
        schedules.fold(Schedule::default(), Add::add)
    }
}
