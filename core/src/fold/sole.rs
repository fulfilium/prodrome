//! The product of a schema whose events write one register.
//!
//! The register is named [`Only`]. Its value is one [`Frontier`]: the
//! writes that no later write descends from. The register's type is a
//! [`RegisterType`] that also implements [`Sole`], and [`One`] is then the
//! schema's product. [`Whole`] is the register type whose value is the whole
//! event, so its reading is the frontier's candidates.

#![warn(clippy::pedantic)]

use std::marker::PhantomData;

use crate::fold::{Discrete, Frontier, Product, Register as _, RegisterType};
use crate::literal::ProdromeError;
use crate::registers::Stamp;
use crate::schema::Schema;

/// The name of the one register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Only;

/// The type of the one register of [`Sole::Schema`].
///
/// The register reads its frontier under this [`RegisterType`], and a write
/// joins it exactly when the type gives the write a value.
pub trait Sole: RegisterType<Self::Schema> + 'static {
    /// The schema whose events write [`Only`].
    type Schema: Schema<Register = Only>;

    /// The append's refusal of a write whose value falls below the reading.
    /// A register type that is not inflationary refuses nothing, which is
    /// the default. An inflationary one answers
    /// `frontier.grows::<Self>(event)`.
    ///
    /// # Errors
    ///
    /// A write whose value falls below, or beside, the reading that it
    /// supersedes.
    fn grows(_: &Frontier<'_, Self::Schema>, _: &Self::Schema) -> Result<(), ProdromeError> {
        Ok(())
    }
}

/// The registers of one entity at the schema of the register type `R`: the
/// frontier of [`Only`].
pub struct One<'a, R: Sole>(Frontier<'a, R::Schema>);

impl<R: Sole> Default for One<'_, R> {
    fn default() -> Self {
        Self(Frontier::default())
    }
}

impl<R: Sole> Clone for One<'_, R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<R: Sole> PartialEq for One<'_, R> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<R: Sole> std::fmt::Debug for One<'_, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("One").field(&self.0).finish()
    }
}

impl<'a, R: Sole> Product<'a> for One<'a, R> {
    type Schema = R::Schema;

    fn registers(&self) -> Vec<Only> {
        vec![Only]
    }

    /// Joins `stamp` where its event writes the register, and nowhere else.
    fn join(&mut self, stamp: &'a Stamp<R::Schema>) {
        if R::value(&stamp.event).is_some() {
            self.0.join(stamp);
        }
    }

    fn frontier(&self, _: Only) -> &Frontier<'a, R::Schema> {
        &self.0
    }

    fn frontier_mut(&mut self, _: Only) -> &mut Frontier<'a, R::Schema> {
        &mut self.0
    }

    fn reading(&self, _: Only) -> Vec<&'a Stamp<R::Schema>> {
        self.0.reading::<R>()
    }

    fn grows(&self, event: &R::Schema) -> Result<(), ProdromeError> {
        R::grows(&self.0, event)
    }
}

/// The register type whose value is the whole event that writes [`Only`],
/// under the discrete order: the register reads the frontier's candidates.
/// An event that writes no register has no value, so it joins nothing.
pub struct Whole<E>(PhantomData<fn() -> E>);

impl<E: Schema<Register = Only>> RegisterType<E> for Whole<E> {
    type Value<'e> = Discrete<&'e E>;

    fn value(event: &E) -> Option<Discrete<&E>> {
        event.writes().next().map(|Only| Discrete(event))
    }
}

impl<E: Schema<Register = Only>> Sole for Whole<E> {
    type Schema = E;
}
