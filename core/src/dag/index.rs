//! The kept order as integers: each object's parents as positions in the
//! order, and a search from a name to its position.

use std::collections::BTreeMap;

use crate::event::{rests_on, Envelope, Hash};

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
    /// The index of `order`, a linear order of `objects`: every object of
    /// `objects` once, parents first.
    pub(super) fn of<E>(order: &[Hash], objects: &BTreeMap<Hash, Envelope<E>>) -> Index {
        let names: Vec<&Hash> = objects.keys().collect();
        let rank = |name: &Hash| names.binary_search(&name).ok();
        let mut by_name = vec![0; names.len()];
        let mut rank_at = vec![0; order.len()];
        for (at, name) in order.iter().enumerate() {
            if let Some(rank) = rank(name) {
                by_name[rank] = at;
                rank_at[at] = rank;
            }
        }
        let by_rank: Vec<Vec<usize>> = objects
            .values()
            .map(|object| {
                rests_on(object)
                    .into_iter()
                    .filter_map(|parent| rank(parent).map(|rank| by_name[rank]))
                    .collect()
            })
            .collect();
        let mut starts = Vec::with_capacity(order.len() + 1);
        let mut parents = Vec::new();
        for &rank in &rank_at {
            starts.push(parents.len());
            parents.extend(&by_rank[rank]);
        }
        starts.push(parents.len());
        Index {
            starts,
            parents,
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
