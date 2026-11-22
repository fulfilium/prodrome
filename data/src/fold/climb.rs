//! The register of an inflationary type: its reading is the greatest of all
//! its writes, so the reading is a function of the set of writes.

#![warn(clippy::pedantic)]

use crate::fold::frontier::value;
use crate::fold::order::{grows, maximal, Inflationary, Order};
use crate::fold::{Frontier, Register as _, RegisterType};
use crate::literal::ProdromeError;
use crate::schema::Schema;
use crate::stamp::Stamp;

/// One inflationary register: its [`Frontier`] and its peak, the writes that
/// no other write outranks (`docs/design-inflationary-reading.md`).
///
/// A write `u` outranks a write `w` when the value of `u` is strictly greater,
/// or when the values are equal and `u` descends from `w`. The reading is the
/// values of the peak ([`maximal`]). A write below what it rests on cannot
/// lower the reading, because a write above it is already in the peak.
#[derive(Debug, Clone, PartialEq)]
pub struct Climb<'a, E: Schema> {
    frontier: Frontier<'a, E>,
    peak: Vec<&'a Stamp<E>>,
}

impl<E: Schema> Default for Climb<'_, E> {
    fn default() -> Self {
        Climb {
            frontier: Frontier::default(),
            peak: Vec::new(),
        }
    }
}

/// Whether `upper` outranks `lower` (see [`Climb`]).
fn outranks<E: Schema, V: Order>(
    upper: &Stamp<E>,
    upper_value: &V,
    lower: &Stamp<E>,
    lower_value: &V,
) -> bool {
    lower_value.le(upper_value) && (!upper_value.le(lower_value) || upper.descends(lower))
}

impl<'a, E: Schema> Climb<'a, E> {
    #[must_use]
    pub fn frontier(&self) -> &Frontier<'a, E> {
        &self.frontier
    }

    /// Joins `stamp`, whose value under the register's order is `value(stamp)`.
    pub fn join_by<V: Inflationary>(
        &mut self,
        stamp: &'a Stamp<E>,
        value: impl Fn(&'a Stamp<E>) -> V,
    ) {
        self.frontier.join(stamp);
        if self.peak.iter().any(|held| held.name == stamp.name) {
            return;
        }
        let written = value(stamp);
        if self
            .peak
            .iter()
            .any(|held| outranks(held, &value(held), stamp, &written))
        {
            return;
        }
        self.peak
            .retain(|held| !outranks(stamp, &written, held, &value(held)));
        let at = self.peak.partition_point(|held| held.name < stamp.name);
        self.peak.insert(at, stamp);
    }

    pub fn join<R>(&mut self, stamp: &'a Stamp<E>)
    where
        R: RegisterType<E>,
        R::Value<'a>: Inflationary,
    {
        self.join_by(stamp, |stamp| value::<E, R>(stamp));
    }

    pub fn read<V: Order>(&self, value: impl Fn(&'a Stamp<E>) -> V) -> Vec<&'a Stamp<E>> {
        maximal(&self.peak, value)
    }

    #[must_use]
    pub fn reading<R: RegisterType<E>>(&self) -> Vec<&'a Stamp<E>> {
        self.read(|stamp| value::<E, R>(stamp))
    }

    /// The refusal of an append: the value of `event` must stand at or above
    /// every value of the reading.
    ///
    /// # Errors
    ///
    /// Returns an error where the value is below or incomparable with one.
    pub fn grows<'s, R>(&'s self, event: &'s E) -> Result<(), ProdromeError>
    where
        R: RegisterType<E>,
        R::Value<'s>: Inflationary,
    {
        let Some(written) = R::value(event) else {
            return Ok(());
        };
        let held: Vec<R::Value<'s>> = self
            .reading::<R>()
            .into_iter()
            .map(|stamp: &'s Stamp<E>| value::<E, R>(stamp))
            .collect();
        grows(&held, &written)
    }

    /// Law 32 between two readings of one register: every value that
    /// `earlier` reads stands at or below a value that this register reads.
    ///
    /// # Errors
    ///
    /// Returns an error that names a write of `earlier` whose value fell.
    pub fn climbed_by<V: Inflationary>(
        &self,
        earlier: &Self,
        value: impl Fn(&'a Stamp<E>) -> V,
    ) -> Result<(), ProdromeError> {
        let now: Vec<V> = self.read(&value).into_iter().map(&value).collect();
        match earlier
            .read(&value)
            .into_iter()
            .find(|then| !now.iter().any(|now| value(then).le(now)))
        {
            None => Ok(()),
            Some(then) => Err(ProdromeError::invalid(format!(
                "the reading of an inflationary register fell from {}",
                then.name
            ))),
        }
    }

    /// [`Climb::climbed_by`] under the register's type `R`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Climb::climbed_by`].
    pub fn climbed<R>(&self, earlier: &Self) -> Result<(), ProdromeError>
    where
        R: RegisterType<E>,
        R::Value<'a>: Inflationary,
    {
        self.climbed_by(earlier, |stamp| value::<E, R>(stamp))
    }
}
