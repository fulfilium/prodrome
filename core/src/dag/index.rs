//! The kept order as integers: each object's parents as positions in the
//! order, and a search from a name to its position.

use crate::event::Hash;

/// An index of a DAG's linear order. A reach is a set of positions in the
/// order ([`super::Reach`]), and this index lets a walk over a reach work
/// on positions alone: a parent is a position, and a name is looked up
/// once, by a binary search that copies no name.
///
/// It costs three arrays of integers: about 16 bytes for each object and 8
/// bytes for each parent edge.
#[derive(Debug, Clone)]
pub(crate) struct Index {
    /// For each position, the start of its parents in `parents`, and one
    /// more entry for the end.
    starts: Vec<usize>,
    /// The parents of each position, as positions, in turn.
    parents: Vec<usize>,
    /// The positions, in the order of the names at them.
    by_name: Vec<usize>,
}

impl Index {
    /// The index of an order given as ranks, where `parents[rank]` names the
    /// parents of the object of that rank, as ranks. Every rank is in
    /// `order` once.
    pub(super) fn of(order: &[usize], parents: &[Vec<usize>]) -> Index {
        let mut by_name = vec![0; order.len()];
        for (at, &rank) in order.iter().enumerate() {
            by_name[rank] = at;
        }
        let mut starts = Vec::with_capacity(order.len() + 1);
        let mut flat = Vec::new();
        for &rank in order {
            starts.push(flat.len());
            flat.extend(parents[rank].iter().map(|&parent| by_name[parent]));
        }
        starts.push(flat.len());
        Index {
            starts,
            parents: flat,
            by_name,
        }
    }

    /// The position of `name` in `order`, the order this index was made
    /// of, or `None` when the order does not hold it.
    pub(crate) fn position(&self, order: &[Hash], name: &Hash) -> Option<usize> {
        self.by_name
            .binary_search_by(|&at| order[at].cmp(name))
            .ok()
            .map(|found| self.by_name[found])
    }

    /// The parents of the object at `at`, as positions.
    pub(crate) fn parents(&self, at: usize) -> &[usize] {
        &self.parents[self.starts[at]..self.starts[at + 1]]
    }
}
