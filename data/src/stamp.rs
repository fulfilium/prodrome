//! One object that carries an event, at its place in its entity's linear
//! order, where the entity's registers can see it.

use std::sync::Arc;

use crate::bitset::BitSet;
use crate::event::Hash;
use crate::schema::Schema;

/// One object that carries an event, where its entity's registers can see it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp<E: Schema> {
    pub name: Hash,
    pub event: Arc<E>,
    place: Place<E::Key>,
}

impl<E: Schema> Stamp<E> {
    /// Whether this write descends from `earlier`.
    ///
    /// The caller must pass a write to the same entity, because positions are
    /// compared within one scope.
    pub fn descends(&self, earlier: &Stamp<E>) -> bool {
        self.place.ancestry.contains(earlier.place.position)
    }

    /// The write's position in its scope's linear order.
    #[doc(hidden)]
    pub fn position(&self) -> usize {
        self.place.position
    }

    /// The positions of every object of its scope that the write rests on.
    #[doc(hidden)]
    pub fn ancestry(&self) -> &BitSet {
        &self.place.ancestry
    }

    /// The write `event`, named `name`, at `place`. Only the fold of the
    /// DAG places a write.
    #[doc(hidden)]
    pub fn placed(name: Hash, event: Arc<E>, place: Place<E::Key>) -> Stamp<E> {
        Stamp { name, event, place }
    }

    /// Make room at `slot` of the write's scope.
    #[doc(hidden)]
    pub fn open(&mut self, slot: usize) {
        self.place.open(slot);
    }
}

/// A position in an index, and the positions of every ancestor in it.
#[derive(Debug, Clone, PartialEq)]
#[doc(hidden)]
pub struct Place<K> {
    pub position: usize,
    pub ancestry: BitSet,
    pub scope: Scope<K>,
}

impl<K> Place<K> {
    /// Make room at `slot`: this place, if at or after it, moves up one.
    pub fn open(&mut self, slot: usize) {
        if self.position >= slot {
            self.position += 1;
            self.ancestry.open(slot);
        }
    }
}

/// Where an object is placed: the legacy scope (`None`), or one entity of
/// one prodrome.
#[doc(hidden)]
pub type Scope<K> = Option<(Hash, K)>;
