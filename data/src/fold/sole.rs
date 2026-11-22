//! The product [`One`] of a schema whose events write one register, [`Only`].
//! The register's type implements [`Sole`], and through [`Sole::Held`] it
//! chooses a [`Frontier`] or, for an inflationary register, a [`Climb`].

#![warn(clippy::pedantic)]

use std::fmt::Debug;
use std::marker::PhantomData;

use crate::fold::{Climb, Discrete, Frontier, Inflationary, Order, Product, RegisterType};
use crate::literal::ProdromeError;
use crate::schema::headed::{Body, Event};
use crate::schema::Schema;
use crate::stamp::Stamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Only;

/// The type of the one register of [`Sole::Schema`]: a write joins the
/// register exactly when the type gives the write a value.
pub trait Sole: RegisterType<Self::Schema> + Sized + 'static {
    type Schema: Schema<Register = Only>;

    /// `Frontier<'a, Self::Schema>` for a register that may go back, and
    /// `Climb<'a, Self::Schema>` for an inflationary one.
    type Held<'a>: Hold<'a, Self>;
}

/// How [`One`] holds the register of the type `R`.
pub trait Hold<'a, R: Sole>: Default + Clone + Debug + PartialEq {
    /// The register's type must give `stamp` a value.
    fn join(&mut self, stamp: &'a Stamp<R::Schema>);

    fn frontier(&self) -> &Frontier<'a, R::Schema>;

    fn reading(&self) -> Vec<&'a Stamp<R::Schema>>;

    /// [`Product::grows`].
    ///
    /// # Errors
    ///
    /// Returns an error where the value of a write falls below the reading
    /// or is incomparable with it.
    fn grows(&self, event: &R::Schema) -> Result<(), ProdromeError>;

    /// [`Product::climbed`].
    ///
    /// # Errors
    ///
    /// Returns an error where a reading fell.
    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError>;

    /// [`Only`] where the register is inflationary ([`Product::climbing`]).
    fn climbing(&self) -> Vec<Only>;
}

/// A register that may go back, read by its frontier, refuses no write.
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

    fn climbing(&self) -> Vec<Only> {
        Vec::new()
    }
}

/// An inflationary register, read by the greatest of all its writes, refuses
/// a write below its reading.
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

    fn climbing(&self) -> Vec<Only> {
        vec![Only]
    }
}

/// The registers of one entity at the schema of `R`: [`Only`], held as `R`
/// chooses.
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

    fn climbing(&self) -> Vec<Only> {
        self.0.climbing()
    }
}

/// The register type whose value is the whole event under the discrete
/// order, so the register reads the frontier's candidates.
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

/// The register type whose value is the body of an [`Event`], under the
/// body's own [`Order`]. Two writes whose bodies are equal under the order are
/// one value, named by the least write, whoever wrote them and whenever.
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
