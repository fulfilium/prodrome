//! The facts that a DAG derives from its objects and keeps: its tips, its
//! linear order and the order's index. Each is a function of the names and
//! their parents alone, so none of this code is generic over a schema.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use super::index::Index;
use crate::event::Hash;
use crate::literal::ProdromeError;
use crate::topo;

/// The names of a DAG's objects, in name order, each with the names it
/// rests on: what every kept fact is derived from.
pub(super) type Graph<'a> = Vec<(&'a Hash, Vec<&'a Hash>)>;

/// The kept facts of one set of objects, each derived on its first ask.
/// [`super::history::History`] holds one and replaces it whole when its
/// objects change, so no fact can outlive the objects it describes.
#[derive(Debug, Clone, Default)]
pub(super) struct Facts {
    tips: OnceLock<BTreeSet<Hash>>,
    ordered: OnceLock<Result<Ordered, ProdromeError>>,
}

/// The linear order, and its index.
#[derive(Debug, Clone)]
struct Ordered {
    order: Vec<Hash>,
    index: Index,
}

impl Facts {
    /// Every object no object rests on, of the objects that `graph` reads.
    pub(super) fn tips<'g>(&self, graph: &dyn Fn() -> Graph<'g>) -> &BTreeSet<Hash> {
        self.tips.get_or_init(|| super::tips_among(graph()))
    }

    /// Every object, parents first, by [`topo::linear`], and the order's
    /// [`Index`]. A parent that is not held is a refusal, and so is a cycle.
    pub(super) fn ordered<'g>(
        &self,
        graph: &dyn Fn() -> Graph<'g>,
    ) -> Result<(&[Hash], &Index), ProdromeError> {
        match self.ordered.get_or_init(|| ordered(&graph())) {
            Ok(Ordered { order, index }) => Ok((order, index)),
            Err(error) => Err(error.clone()),
        }
    }
}

/// The order of `graph` and its index, by Kahn's walk over the ranks of the
/// names: a rank orders as its name does, so the walk is
/// [`topo::linear`]'s over the names, and the index needs no search.
fn ordered(graph: &Graph<'_>) -> Result<Ordered, ProdromeError> {
    let names: Vec<&Hash> = graph.iter().map(|(name, _)| *name).collect();
    let parents: Vec<Vec<usize>> = graph
        .iter()
        .map(|(name, named)| {
            named
                .iter()
                .map(|parent| {
                    names.binary_search(parent).map_err(|_| {
                        ProdromeError::Store(format!(
                            "missing object {}, named as a parent by {}",
                            parent.as_str(),
                            name.as_str()
                        ))
                    })
                })
                .collect()
        })
        .collect::<Result<_, _>>()?;
    let ranks: Vec<usize> = (0..names.len()).collect();
    let rests_on: BTreeMap<&usize, Vec<&usize>> = ranks
        .iter()
        .zip(&parents)
        .map(|(rank, below)| (rank, below.iter().collect()))
        .collect();
    let order: Vec<usize> = topo::linear(&rests_on)
        .map_err(|stuck| cycle(names[*stuck]))?
        .into_iter()
        .copied()
        .collect();
    Ok(Ordered {
        index: Index::of(&order, &parents),
        order: order.into_iter().map(|rank| names[rank].clone()).collect(),
    })
}

/// `rests_on`'s names, parents first, by [`topo::linear`]. A cycle is a
/// refusal.
pub(super) fn linear(rests_on: &BTreeMap<&Hash, Vec<&Hash>>) -> Result<Vec<Hash>, ProdromeError> {
    match topo::linear(rests_on) {
        Ok(order) => Ok(order.into_iter().cloned().collect()),
        Err(stuck) => Err(cycle(stuck)),
    }
}

/// The refusal of a cycle, named by the least name it leaves unplaced.
fn cycle(stuck: &Hash) -> ProdromeError {
    ProdromeError::Store(format!("cycle in the object graph at {}", stuck.as_str()))
}
