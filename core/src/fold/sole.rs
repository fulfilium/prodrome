//! The product of a schema whose events write one register.
//!
//! The register is named [`Only`]. Its value is one [`Frontier`]: the
//! writes that no later write descends from. A type that implements
//! [`Sole`] says how the frontier reads and whether an append must grow it,
//! and [`One`] is then the schema's product.

#![warn(clippy::pedantic)]

use std::collections::BTreeMap;

use crate::event::Hash;
use crate::fold::{Frontier, Product, Register as _};
use crate::literal::ProdromeError;
use crate::registers::Stamp;
use crate::schema::Schema;

/// The name of the one register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Only;

/// The rules of the one register of [`Sole::Schema`].
///
/// The implementing type is the schema itself, or, for a schema at
/// [`crate::schema::envelope::Event`], the body type. The body type is local
/// to the application's crate, so the application can implement this trait
/// for it, and `Event<Body>` is the schema.
pub trait Sole: 'static {
    /// The schema whose events write [`Only`].
    type Schema: Schema<Register = Only>;

    /// The register's reading: one write, or the candidates of a conflict.
    fn reading<'a>(frontier: &Frontier<'a, Self::Schema>) -> Vec<&'a Stamp<Self::Schema>>;

    /// The append's refusal of a write whose value falls below the reading.
    /// A register that is not inflationary refuses nothing, which is the
    /// default.
    ///
    /// # Errors
    ///
    /// A write whose value falls below, or beside, the reading that it
    /// supersedes.
    fn grows(_: &Frontier<'_, Self::Schema>, _: &Self::Schema) -> Result<(), ProdromeError> {
        Ok(())
    }

    /// Is the frontier a conflict that a writer must settle? By default it
    /// is when it holds more than one write. A register whose join is union
    /// has no conflicts.
    #[must_use]
    fn conflict(frontier: &Frontier<'_, Self::Schema>) -> bool {
        frontier.is_conflict()
    }
}

/// The registers of one entity at the schema of `R`: the frontier of
/// [`Only`].
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

impl<'a, R: Sole> One<'a, R> {
    /// The reading of the register.
    #[must_use]
    pub fn reading(&self) -> Vec<&'a Stamp<R::Schema>> {
        R::reading(&self.0)
    }
}

impl<'a, R: Sole> Product<'a> for One<'a, R> {
    type Schema = R::Schema;

    fn registers(&self) -> Vec<Only> {
        vec![Only]
    }

    fn join(&mut self, stamp: &'a Stamp<R::Schema>) {
        self.0.join(stamp);
    }

    fn frontier(&self, _: Only) -> &Frontier<'a, R::Schema> {
        &self.0
    }

    fn frontier_mut(&mut self, _: Only) -> &mut Frontier<'a, R::Schema> {
        &mut self.0
    }

    fn reading(&self, _: Only) -> Vec<&'a Stamp<R::Schema>> {
        R::reading(&self.0)
    }

    fn grows(&self, event: &R::Schema) -> Result<(), ProdromeError> {
        R::grows(&self.0, event)
    }

    fn conflicts(&self) -> BTreeMap<Only, Vec<Hash>> {
        if R::conflict(&self.0) {
            BTreeMap::from([(Only, self.0.names())])
        } else {
            BTreeMap::new()
        }
    }
}
