//! The product of a schema whose events write one register.
//!
//! The register is named [`Only`]. The register's type is a
//! [`RegisterType`] that also implements [`Sole`], and [`One`] is then the
//! schema's product. The type chooses how the register is held
//! ([`Sole::Held`]): a [`Frontier`], the writes that no later write
//! descends from, for a register that may go back, or a [`Climb`], for an
//! inflationary register, whose reading is the greatest of all its writes.
//! [`Whole`] is the register type whose value is the whole event, so its
//! reading is the frontier's candidates. [`Bodies`] is the register type
//! whose value is the body of an [`Event`](crate::schema::headed::Event),
//! under the body's own order.

#![warn(clippy::pedantic)]

use std::fmt::Debug;
use std::marker::PhantomData;

use crate::fold::{Climb, Discrete, Frontier, Inflationary, Order, Product, RegisterType};
use crate::literal::ProdromeError;
use crate::registers::Stamp;
use crate::schema::headed::{Body, Event};
use crate::schema::Schema;

/// The name of the one register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Only;

/// The type of the one register of [`Sole::Schema`].
///
/// The register reads under this [`RegisterType`], and a write joins it
/// exactly when the type gives the write a value.
pub trait Sole: RegisterType<Self::Schema> + Sized + 'static {
    /// The schema whose events write [`Only`].
    type Schema: Schema<Register = Only>;

    /// How [`One`] holds the register: `Frontier<'a, Self::Schema>` for a
    /// register that may go back, and `Climb<'a, Self::Schema>` for an
    /// inflationary one, which the append holds to [`Climb::grows`].
    type Held<'a>: Hold<'a, Self>;
}

/// How [`One`] holds the register of the type `R`: what a write joins, and
/// how the register reads.
pub trait Hold<'a, R: Sole>: Default + Clone + Debug + PartialEq {
    /// Join `stamp`, a write that the register's type gives a value.
    fn join(&mut self, stamp: &'a Stamp<R::Schema>);

    /// The register's frontier.
    fn frontier(&self) -> &Frontier<'a, R::Schema>;

    /// The register's reading under `R`.
    fn reading(&self) -> Vec<&'a Stamp<R::Schema>>;

    /// The append's refusal ([`Product::grows`]).
    ///
    /// # Errors
    ///
    /// A write whose value falls below, or beside, the reading that it
    /// supersedes.
    fn grows(&self, event: &R::Schema) -> Result<(), ProdromeError>;

    /// Law 32 between two readings ([`Product::climbed`]).
    ///
    /// # Errors
    ///
    /// A reading that fell.
    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError>;
}

/// A register that may go back: read by the maximal values of its frontier,
/// and refusing no write.
impl<'a, R: Sole> Hold<'a, R> for Frontier<'a, R::Schema> {
    fn join(&mut self, stamp: &'a Stamp<R::Schema>) {
        crate::fold::Register::join(self, stamp);
    }

    fn frontier(&self) -> &Frontier<'a, R::Schema> {
        self
    }

    fn reading(&self) -> Vec<&'a Stamp<R::Schema>> {
        Frontier::reading::<R>(self)
    }

    fn grows(&self, _: &R::Schema) -> Result<(), ProdromeError> {
        Ok(())
    }

    fn climbed(&self, _: &Self) -> Result<(), ProdromeError> {
        Ok(())
    }
}

/// An inflationary register: read by the greatest of all its writes, and
/// refusing a write below its reading.
impl<'a, R> Hold<'a, R> for Climb<'a, R::Schema>
where
    R: Sole,
    for<'v> R::Value<'v>: Inflationary,
{
    fn join(&mut self, stamp: &'a Stamp<R::Schema>) {
        Climb::join::<R>(self, stamp);
    }

    fn frontier(&self) -> &Frontier<'a, R::Schema> {
        Climb::frontier(self)
    }

    fn reading(&self) -> Vec<&'a Stamp<R::Schema>> {
        Climb::reading::<R>(self)
    }

    fn grows(&self, event: &R::Schema) -> Result<(), ProdromeError> {
        Climb::grows::<R>(self, event)
    }

    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError> {
        Climb::climbed::<R>(self, earlier)
    }
}

/// The registers of one entity at the schema of the register type `R`: the
/// register [`Only`], held as `R` chooses.
pub struct One<'a, R: Sole>(R::Held<'a>);

impl<R: Sole> Default for One<'_, R> {
    fn default() -> Self {
        Self(Default::default())
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
        self.0.frontier()
    }

    fn join_to(&mut self, _: Only, stamp: &'a Stamp<R::Schema>) {
        self.0.join(stamp);
    }

    fn reading(&self, _: Only) -> Vec<&'a Stamp<R::Schema>> {
        self.0.reading()
    }

    fn grows(&self, event: &R::Schema) -> Result<(), ProdromeError> {
        self.0.grows(event)
    }

    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError> {
        self.0.climbed(&earlier.0)
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
    type Held<'a> = Frontier<'a, E>;
}

/// The register type whose value is the body of an `Event<B>` that writes
/// [`Only`], under the body's own [`Order`]. Two writes whose bodies are
/// equal under it are one value, named by the least write, whoever wrote
/// them and whenever. An event whose body writes no register has no value,
/// so it joins nothing.
pub struct Bodies<B>(PhantomData<fn() -> B>);

impl<B: Body<Register = Only> + Order> RegisterType<Event<B>> for Bodies<B> {
    type Value<'e> = &'e B;

    fn value(event: &Event<B>) -> Option<&B> {
        let body = event.body();
        body.writes().next().map(|Only| body)
    }
}

impl<B: Body<Register = Only> + Order> Sole for Bodies<B> {
    type Schema = Event<B>;
    type Held<'a> = Frontier<'a, Event<B>>;
}
