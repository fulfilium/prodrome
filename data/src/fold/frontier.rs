use crate::event::Hash;
use crate::fold::order::{maximal, Discrete, Order};
use crate::fold::{Register, RegisterType};
use crate::schema::Schema;
use crate::stamp::Stamp;

/// The writes to one register that no other write descends from, sorted by
/// name. Empty is unwritten; more than one is a conflict.
#[derive(Debug, Clone, PartialEq)]
pub struct Frontier<'a, E: Schema>(Vec<&'a Stamp<E>>);

impl<E: Schema> Default for Frontier<'_, E> {
    fn default() -> Self {
        Frontier(Vec::new())
    }
}

impl<'a, E: Schema> Register for Frontier<'a, E> {
    type Write = &'a Stamp<E>;

    fn join(&mut self, write: &'a Stamp<E>) {
        if self
            .0
            .iter()
            .any(|held| held.name == write.name || held.descends(write))
        {
            return;
        }
        self.0.retain(|held| !write.descends(held));
        let at = self.0.partition_point(|held| held.name < write.name);
        self.0.insert(at, write);
    }
}

impl<'a, E: Schema> Frontier<'a, E> {
    pub fn writes(&self) -> &[&'a Stamp<E>] {
        &self.0
    }

    pub fn names(&self) -> Vec<Hash> {
        self.0.iter().map(|stamp| stamp.name.clone()).collect()
    }

    pub fn is_conflict(&self) -> bool {
        self.0.len() > 1
    }

    /// The writes whose values are maximal, each value named by the least
    /// write that carries it (law 31).
    pub fn read<V: Order>(&self, value: impl Fn(&'a Stamp<E>) -> V) -> Vec<&'a Stamp<E>> {
        maximal(&self.0, value)
    }

    #[must_use]
    pub fn reading<R: RegisterType<E>>(&self) -> Vec<&'a Stamp<E>> {
        self.read(|stamp| value::<E, R>(stamp))
    }

    /// The reading under the discrete order on whole events: twins are one
    /// candidate, and two events that agree only on the field that the
    /// register reads stay two candidates.
    pub fn candidates(&self) -> Vec<&'a Stamp<E>> {
        self.read(|stamp| Discrete::<&E>(&stamp.event))
    }
}

pub(super) fn value<E: Schema, R: RegisterType<E>>(stamp: &Stamp<E>) -> R::Value<'_> {
    R::value(&stamp.event).expect("a write joins only the registers it writes")
}
