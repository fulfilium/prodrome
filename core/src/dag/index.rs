//! The kept order as integers: each object's parents as positions in the
//! order, and a search from a name to its position.

use std::collections::BTreeMap;

use prodrome_data::bitset::BitSet;
use prodrome_data::event::Hash;

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

    /// Which of the positions `tops` lie beneath which. For each top, the
    /// answer is the set of indices into `tops` of the tops that rest on
    /// it, directly or through other objects.
    ///
    /// One walk goes down from every top at once. It carries a label for
    /// each position that it reaches: the set of tops above that position.
    /// It takes the greatest position first. Each object lies after its
    /// parents in the order, so every object above a position is taken
    /// before it, and a label is complete when the walk takes it. The walk
    /// takes each position once. It does not go below the least top,
    /// because no top lies there. So the walk reaches only the positions
    /// between the least top and the others, and from one top it reaches
    /// none.
    pub(crate) fn beneath(&self, tops: &[usize]) -> Vec<BitSet> {
        let mut found = vec![BitSet::default(); tops.len()];
        let Some(&floor) = tops.iter().min() else {
            return found;
        };
        let mut at_top: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        let mut labels: BTreeMap<usize, BitSet> = BTreeMap::new();
        for (top, &at) in tops.iter().enumerate() {
            at_top.entry(at).or_default().push(top);
            for &parent in self.parents(at).iter().filter(|&&parent| parent >= floor) {
                labels.entry(parent).or_default().insert(top);
            }
        }
        while let Some((at, above)) = labels.pop_last() {
            for &top in at_top.get(&at).into_iter().flatten() {
                found[top].union_with(&above);
            }
            for &parent in self.parents(at).iter().filter(|&&parent| parent >= floor) {
                labels.entry(parent).or_default().union_with(&above);
            }
        }
        found
    }
}
