//! A down-set of a DAG, as a set of positions in its linear order.

use std::collections::BTreeSet;
use std::fmt;

use super::index::Index;
use crate::event::Hash;

/// A set of objects of one DAG, held as one bit for each position of the
/// DAG's linear order ([`super::Dag::linearise`]). A frontier's reach is
/// one: the objects that its tips rest on, the tips included. The walks
/// over a reach work on positions, through the DAG's [`Index`], so a reach
/// of 30,000 objects is under 4 KB and its closure compares no names.
#[derive(Clone)]
pub struct Reach<'d> {
    order: &'d [Hash],
    index: &'d Index,
    bits: Vec<u64>,
    len: usize,
}

impl<'d> Reach<'d> {
    /// The empty set over `order`, which `index` indexes.
    pub(super) fn empty(order: &'d [Hash], index: &'d Index) -> Reach<'d> {
        Reach {
            order,
            index,
            bits: vec![0; order.len().div_ceil(64)],
            len: 0,
        }
    }

    /// The number of objects in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Does the set hold no object?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Does the set hold the object `name`?
    #[must_use]
    pub fn contains(&self, name: &Hash) -> bool {
        self.index
            .position(self.order, name)
            .is_some_and(|at| self.has(at))
    }

    /// The names in the set, in the DAG's linear order.
    pub fn iter(&self) -> impl Iterator<Item = &'d Hash> + '_ {
        self.positions(true).map(|at| &self.order[at])
    }

    /// The positions whose membership is `held`, in order.
    fn positions(&self, held: bool) -> impl Iterator<Item = usize> + '_ {
        (0..self.order.len()).filter(move |&at| self.has(at) == held)
    }

    /// The positions of the order that the set does not hold, in order,
    /// each with its name.
    pub(crate) fn outside(&self) -> impl Iterator<Item = (usize, &'d Hash)> + '_ {
        self.positions(false).map(|at| (at, &self.order[at]))
    }

    fn has(&self, at: usize) -> bool {
        self.bits[at / 64] & (1 << (at % 64)) != 0
    }

    /// Put the position `at` in the set.
    pub(crate) fn insert(&mut self, at: usize) {
        if !self.has(at) {
            self.bits[at / 64] |= 1 << (at % 64);
            self.len += 1;
        }
    }

    /// Put `seeds` in the set, and everything they rest on. A name that
    /// the DAG does not hold is passed over.
    pub(crate) fn close<'n>(&mut self, seeds: impl IntoIterator<Item = &'n Hash>) {
        let mut pending: Vec<usize> = seeds
            .into_iter()
            .filter_map(|name| self.index.position(self.order, name))
            .collect();
        while let Some(at) = pending.pop() {
            if !self.has(at) {
                self.insert(at);
                pending.extend(self.index.parents(at));
            }
        }
    }

    /// The members that no member names as a parent.
    pub(crate) fn tips(&self) -> BTreeSet<Hash> {
        let mut named = vec![0_u64; self.bits.len()];
        for at in self.positions(true) {
            for &parent in self.index.parents(at) {
                named[parent / 64] |= 1 << (parent % 64);
            }
        }
        self.positions(true)
            .filter(|&at| named[at / 64] & (1 << (at % 64)) == 0)
            .map(|at| self.order[at].clone())
            .collect()
    }
}

/// Two reaches are equal when they hold the same names.
impl PartialEq for Reach<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.iter().all(|name| other.contains(name))
    }
}

impl Eq for Reach<'_> {}

impl fmt::Debug for Reach<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}
