use std::collections::BTreeSet;

use crate::fold::{Inflationary, Order};
use crate::schema::Schema;

/// A join-semilattice: joining the same writes in any order gives one value.
pub trait Register: Default {
    type Write;

    fn join(&mut self, write: Self::Write);
}

/// A grow-only set, joined by union: the tendings, which never conflict.
pub type GrowSet<T> = BTreeSet<T>;

impl<T: Ord> Inflationary for GrowSet<T> {}

impl<T: Ord> Register for GrowSet<T> {
    type Write = T;

    fn join(&mut self, write: T) {
        self.insert(write);
    }
}

/// One register's type in a schema: its values, and the value a write gives.
pub trait RegisterType<E: Schema> {
    type Value<'e>: Order;

    /// An implementation must return `Some` exactly for the events whose
    /// [`Schema::writes`] names the register.
    fn value(event: &E) -> Option<Self::Value<'_>>;
}
