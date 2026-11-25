//! A write at its place in its entity's linear order.

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
    /// Whether this write descends from `earlier`, which must be a write to the
    /// same entity, because positions are compared within one scope.
    pub fn descends(&self, earlier: &Stamp<E>) -> bool {
        self.place.ancestry.contains(earlier.place.position)
    }

    /// For this library's own code; no promise. The position in the scope's linear order.
    #[doc(hidden)]
    pub fn position(&self) -> usize {
        self.place.position
    }

    /// For this library's own code; no promise. The positions in its scope that the write rests on.
    #[doc(hidden)]
    pub fn ancestry(&self) -> &BitSet {
        &self.place.ancestry
    }

    /// For this library's own code; no promise. Only the fold of the DAG places a write.
    #[doc(hidden)]
    pub fn placed(name: Hash, event: Arc<E>, place: Place<E::Key>) -> Stamp<E> {
        Stamp { name, event, place }
    }

    /// For this library's own code; no promise. Makes room at `slot` of the write's scope.
    #[doc(hidden)]
    pub fn open(&mut self, slot: usize) {
        self.place.open(slot);
    }
}

/// For this library's own code; no promise. A position in an index, and its ancestors' positions.
#[derive(Debug, Clone, PartialEq)]
#[doc(hidden)]
pub struct Place<K> {
    pub position: usize,
    pub ancestry: BitSet,
    pub scope: Scope<K>,
}

impl<K> Place<K> {
    /// Makes room at `slot`: this place, if at or after it, moves up one.
    pub fn open(&mut self, slot: usize) {
        if self.position >= slot {
            self.position += 1;
            self.ancestry.open(slot);
        }
    }
}

/// For this library's own code; no promise. The legacy scope (`None`), or one prodrome's entity.
#[doc(hidden)]
pub type Scope<K> = Option<(Hash, K)>;
