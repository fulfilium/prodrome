use std::collections::{BTreeMap, BTreeSet};

use crate::dag::Dag;
use crate::event::{Envelope, Hash};
use crate::fold::{read, Product, RegisterType};
use crate::laws::{history, Broken};
use crate::policy::Everything;
use crate::registers::{extend, fold, Folded, Genesis, Node};
use crate::schema::Schema;

/// How [`free`] reorders a history: a rank per object (the first `ranks.len()`
/// objects in their order; the rest rank 0), the positions of objects to fold
/// twice, and a bit per object, modulo 64, that puts it in the first of two
/// replicas. Every value is valid: positions are taken modulo the history's
/// length.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shuffle {
    /// The order to fold the objects in, lowest rank first among those
    /// whose parents are placed.
    pub ranks: Vec<u32>,
    /// The objects to fold a second time.
    pub twice: Vec<usize>,
    /// The bits that put an object, and what it rests on, in the first
    /// replica.
    pub replica: u64,
}

/// Every register's frontier and its reading by name, for every entity, as
/// object names: what law 33 compares.
type Reading<E> = BTreeMap<(Genesis, <E as Schema>::Key), Vec<(Vec<Hash>, Vec<Hash>)>>;

fn reading<E: Schema>(state: &Folded<E>) -> Reading<E> {
    let mut out = BTreeMap::new();
    for (genesis, prodrome) in state.prodromes() {
        for (key, stream) in prodrome {
            let registers = read(stream, None, &Everything);
            let frontiers = registers
                .registers()
                .into_iter()
                .map(|register| {
                    let names = registers
                        .reading(register)
                        .iter()
                        .map(|stamp| stamp.name.clone())
                        .collect();
                    (registers.frontier(register).names(), names)
                })
                .collect();
            out.insert((genesis.clone(), key.clone()), frontiers);
        }
    }
    out
}

/// `nodes` in the order `ranks` gives, parents first.
fn ordered<E: Schema>(nodes: &[Node<E>], ranks: &[u32]) -> Result<Vec<Node<E>>, Broken> {
    let mut placed: BTreeSet<&Hash> = BTreeSet::new();
    let mut left: Vec<(u32, &Node<E>)> = nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (ranks.get(i).copied().unwrap_or(0), node))
        .collect();
    let mut out = Vec::with_capacity(nodes.len());
    while !left.is_empty() {
        let ready = left
            .iter()
            .enumerate()
            .filter(|(_, (_, node))| {
                node.parents()
                    .iter()
                    .all(|p| placed.contains(p) || !nodes.iter().any(|n| n.name() == p))
            })
            .min_by_key(|(_, (rank, node))| (*rank, node.name()))
            .map(|(i, _)| i)
            .ok_or_else(|| Broken::of(33, "the objects have no parents-first order"))?;
        let (_, node) = left.remove(ready);
        placed.insert(node.name());
        out.push(node.clone());
    }
    Ok(out)
}

/// The first clause of law 33 over `objects`, a history closed under parents:
/// folded in the order `shuffle` gives, with some objects folded twice, and
/// split into two replicas folded apart and then merged, it reads as it reads
/// folded once in its own order, register by register.
///
/// # Errors
///
/// The first of the three foldings that reads otherwise, or objects that do
/// not form a DAG.
pub fn free<E: Schema>(objects: &[(Hash, Envelope<E>)], shuffle: &Shuffle) -> Result<(), Broken> {
    let dag: Dag<E> = objects.iter().cloned().collect();
    let nodes = dag
        .nodes()
        .map_err(|refused| Broken::of(33, format!("the objects are no DAG: {refused}")))?;
    let expected = reading(&fold(&nodes));
    let differs = |how: &str| Broken::of(33, format!("{how}, the history reads otherwise"));

    let permuted = ordered(&nodes, &shuffle.ranks)?;
    if reading(&fold(&permuted)) != expected {
        return Err(differs("permuted"));
    }

    if !permuted.is_empty() {
        let mut doubled = permuted.clone();
        for i in &shuffle.twice {
            let node = permuted[i % permuted.len()].clone();
            let after = doubled
                .iter()
                .position(|held| held.name() == node.name())
                .unwrap_or(0);
            let at = (after + 1 + i % 3).min(doubled.len());
            doubled.insert(at, node);
        }
        if reading(&fold(&doubled)) != expected {
            return Err(differs("with objects folded twice"));
        }
    }

    let seeds = |mine: bool| {
        nodes
            .iter()
            .enumerate()
            .filter(move |(i, _)| (shuffle.replica >> (i % 64) & 1 == 1) == mine)
            .map(|(_, node)| node.name().clone())
    };
    let (here, there) = (dag.closure(seeds(true)), dag.closure(seeds(false)));
    let within = |held: &BTreeSet<Hash>| -> Vec<Node<E>> {
        permuted
            .iter()
            .filter(|node| held.contains(node.name()))
            .cloned()
            .collect()
    };
    let merged = extend(
        &fold(&within(&here)),
        &ordered(&within(&there), &shuffle.ranks)?,
    );
    if reading(&merged) != expected {
        return Err(differs("split into two replicas and merged"));
    }
    Ok(())
}

/// The second clause of law 33 at `event`: folded alone, it joins exactly
/// the frontiers of the registers that [`Schema::writes`] names.
///
/// # Errors
///
/// A register whose frontier holds the event although the event does not
/// name it, or the other way round.
pub fn routed<E: Schema>(event: &E) -> Result<(), Broken> {
    let objects = history(&[(event.clone(), 0)])
        .map_err(|refused| Broken::of(33, format!("no history holds the event: {refused}")))?;
    let dag: Dag<E> = objects.into_iter().collect();
    let nodes = dag
        .nodes()
        .map_err(|refused| Broken::of(33, format!("the history is no DAG: {refused}")))?;
    let state = fold(&nodes);
    for (_, stream) in state.entities() {
        let registers = read(stream, None, &Everything);
        for register in registers.registers() {
            let joined = registers.frontier(register).writes().len();
            let named = usize::from(event.writes().any(|written| written == register));
            if joined != named {
                return Err(Broken::of(
                    33,
                    format!("{register:?} holds {joined} writes of an event that names it {named} times"),
                ));
            }
        }
    }
    Ok(())
}

/// The third clause of law 33 at `event`: the register type `R` of
/// `register` gives the event a value exactly when [`Schema::writes`] names
/// `register`.
///
/// # Errors
///
/// A value without the name, or the name without a value.
pub fn valued<E: Schema, R: RegisterType<E>>(
    event: &E,
    register: E::Register,
) -> Result<(), Broken> {
    let named = event.writes().any(|written| written == register);
    if R::value(event).is_some() == named {
        Ok(())
    } else {
        Err(Broken::of(
            33,
            format!("{register:?}: the type's value and the event's names disagree"),
        ))
    }
}
