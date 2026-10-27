use std::collections::BTreeSet;

use proptest::prelude::*;

use crate::dag::Dag;
use crate::event::{Envelope, Hash};
use crate::laws::Broken;
use crate::registers::Node;
use crate::schema::Schema;

/// A history's objects, each with its name.
type Objects<E> = Vec<(Hash, Envelope<E>)>;

/// One record per node: its rank, whether it seeds the first replica, and
/// the node.
type Records<'n, E> = Vec<(u32, bool, &'n Node<E>)>;

/// How [`super::free`] reorders and splits a history.
///
/// `objects` holds one record per object of the history, in the order of
/// [`Dag::nodes`]: the object's rank, which orders it among the objects
/// whose parents are placed (lowest first, then by name), and whether it
/// seeds the first of two replicas. `twice` holds positions, in the
/// reordered history and taken modulo its length, of objects to fold a
/// second time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shuffle {
    /// One `(rank, first replica)` record per object, in [`Dag::nodes`]'s
    /// order.
    pub objects: Vec<(u32, bool)>,
    /// The objects to fold a second time.
    pub twice: Vec<usize>,
}

/// A [`Shuffle`] of a history of `objects` objects.
pub fn a_shuffle(objects: usize) -> impl Strategy<Value = Shuffle> {
    (
        prop::collection::vec((any::<u32>(), any::<bool>()), objects),
        prop::collection::vec(any::<usize>(), 0..4),
    )
        .prop_map(|(objects, twice)| Shuffle { objects, twice })
}

impl Shuffle {
    /// The records of `nodes`, each with its node.
    ///
    /// # Errors
    ///
    /// A shuffle with another number of records than `nodes` has.
    pub(super) fn of<'n, E: Schema>(&self, nodes: &'n [Node<E>]) -> Result<Records<'n, E>, Broken> {
        if self.objects.len() != nodes.len() {
            return Err(Broken::of(
                33,
                format!(
                    "the shuffle holds {} records for {} objects",
                    self.objects.len(),
                    nodes.len()
                ),
            ));
        }
        Ok(self
            .objects
            .iter()
            .zip(nodes)
            .map(|((rank, first), node)| (*rank, *first, node))
            .collect())
    }

    /// `objects` split into two replicas: what the first replica's seeds
    /// rest on, and what the others rest on. Each half is closed under
    /// parents, and together they hold every object.
    ///
    /// # Errors
    ///
    /// Objects that are no DAG, or a shuffle with another number of records.
    pub fn halves<E: Schema>(
        &self,
        objects: &[(Hash, Envelope<E>)],
    ) -> Result<(Objects<E>, Objects<E>), Broken> {
        let dag: Dag<E> = objects.iter().cloned().collect();
        let nodes = dag
            .nodes()
            .map_err(|refused| Broken::of(33, format!("the objects are no DAG: {refused}")))?;
        let records = self.of(&nodes)?;
        let seeds = |first: bool| {
            records
                .iter()
                .filter(move |(_, mine, _)| *mine == first)
                .map(|(_, _, node)| node.name().clone())
        };
        let half = |held: BTreeSet<Hash>| -> Objects<E> {
            objects
                .iter()
                .filter(|(name, _)| held.contains(name))
                .cloned()
                .collect()
        };
        Ok((
            half(dag.closure(seeds(true))),
            half(dag.closure(seeds(false))),
        ))
    }
}
