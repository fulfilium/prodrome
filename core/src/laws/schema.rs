use std::collections::{BTreeMap, BTreeSet};

use crate::dag::Dag;
use crate::event::{Envelope, Hash};
use crate::fold::{read, Product, RegisterType};
use crate::laws::reading::Registers;
use crate::laws::{history, Broken, Shuffle};
use crate::policy::Everything;
use crate::registers::{extend, fold, Folded, Node};
use crate::schema::Schema;
use crate::topo;

/// `nodes` folded parents first, each placed by its rank and then its name
/// among the nodes whose parents are placed: the crate's one causal order,
/// keyed by `(rank, name)`.
fn ordered<E: Schema>(nodes: &[(u32, &Node<E>)]) -> Result<Vec<(u32, Node<E>)>, Broken> {
    let keys: Vec<(u32, &Hash)> = nodes
        .iter()
        .map(|(rank, node)| (*rank, node.name()))
        .collect();
    let by_name: BTreeMap<&Hash, &(u32, &Hash)> = keys.iter().map(|key| (key.1, key)).collect();
    let rests_on: BTreeMap<&(u32, &Hash), Vec<&(u32, &Hash)>> = keys
        .iter()
        .zip(nodes)
        .map(|(key, (_, node))| {
            let parents = node
                .parents()
                .iter()
                .filter_map(|parent| by_name.get(parent).copied())
                .collect();
            (key, parents)
        })
        .collect();
    let order = topo::linear(&rests_on)
        .map_err(|stuck| Broken::of(33, format!("{} rests on itself", stuck.1)))?;
    let node_of: BTreeMap<&Hash, &Node<E>> =
        nodes.iter().map(|(_, node)| (node.name(), *node)).collect();
    Ok(order
        .into_iter()
        .filter_map(|(rank, name)| node_of.get(name).map(|node| (*rank, (*node).clone())))
        .collect())
}

/// The nodes of `ranked`, in its order.
fn plain<E: Schema>(ranked: &[(u32, Node<E>)]) -> Vec<Node<E>> {
    ranked.iter().map(|(_, node)| node.clone()).collect()
}

/// The first clause of law 33 over `objects`, a history closed under
/// parents: folded in the order `shuffle` gives, with the objects it names
/// folded twice, and split into its two replicas folded apart and then
/// merged, it reads as it reads folded once in its own order, register by
/// register.
///
/// # Errors
///
/// The first of the three foldings that reads otherwise, naming the first
/// entity and register that differ with both sides; objects that are no
/// DAG; or a shuffle with another number of records than objects.
pub fn free<E: Schema>(objects: &[(Hash, Envelope<E>)], shuffle: &Shuffle) -> Result<(), Broken> {
    let dag: Dag<E> = objects.iter().cloned().collect();
    let nodes = dag
        .nodes()
        .map_err(|refused| Broken::of(33, format!("the objects are no DAG: {refused}")))?;
    let expected = Registers::of(&fold(&nodes));
    let ranks: Vec<(u32, &Node<E>)> = shuffle
        .of(&nodes)?
        .into_iter()
        .map(|(rank, _, node)| (rank, node))
        .collect();
    let compare = |how: &str, folded: &Folded<E>| match Registers::of(folded).differs(&expected) {
        Some(difference) => Err(Broken::of(33, format!("{how}: {difference}"))),
        None => Ok(()),
    };

    let ranked = ordered(&ranks)?;
    let permuted = plain(&ranked);
    compare("permuted", &fold(&permuted))?;

    if !permuted.is_empty() {
        let mut doubled = permuted.clone();
        for i in &shuffle.twice {
            let node = permuted[i % permuted.len()].clone();
            let after = doubled
                .iter()
                .position(|held| held.name() == node.name())
                .ok_or_else(|| Broken::of(33, "a node of the history is not in its own fold"))?;
            let at = (after + 1 + i % 3).min(doubled.len());
            doubled.insert(at, node);
        }
        compare("with objects folded twice", &fold(&doubled))?;
    }

    let (here, there) = shuffle.halves(objects)?;
    let within = |half: &[(Hash, Envelope<E>)]| -> Vec<(u32, &Node<E>)> {
        let held: BTreeSet<&Hash> = half.iter().map(|(name, _)| name).collect();
        ranked
            .iter()
            .filter(|(_, node)| held.contains(node.name()))
            .map(|(rank, node)| (*rank, node))
            .collect()
    };
    let merged = extend(
        &fold(&plain(&ordered(&within(&here))?)),
        &plain(&ordered(&within(&there))?),
    );
    compare("split into two replicas, then merged", &merged)
}

/// The second clause of law 33 at `event`: folded alone, it folds to its
/// entity, every register it names is one of the entity's, and it joins
/// exactly the frontiers of the registers that [`Schema::writes`] names.
///
/// # Errors
///
/// An event that folds to no entity, that names a register its entity does
/// not hold, or whose frontiers disagree with its names.
pub fn routed<E: Schema>(event: &E) -> Result<(), Broken> {
    let objects = history(&[(event.clone(), 0)])
        .map_err(|refused| Broken::of(33, format!("no history holds the event: {refused}")))?;
    let dag: Dag<E> = objects.into_iter().collect();
    let nodes = dag
        .nodes()
        .map_err(|refused| Broken::of(33, format!("the history is no DAG: {refused}")))?;
    let state = fold(&nodes);
    let stream = state
        .entities()
        .find(|(key, _)| *key == event.key())
        .map(|(_, stream)| stream)
        .ok_or_else(|| Broken::of(33, "the event folds to no entity"))?;
    let registers = read(stream, None, &Everything);
    let held: BTreeSet<E::Register> = registers.registers().into_iter().collect();
    if let Some(unheld) = event.writes().find(|written| !held.contains(written)) {
        return Err(Broken::of(
            33,
            format!("the event names {unheld:?}, which its entity does not hold"),
        ));
    }
    for register in held {
        let joined = registers.frontier(register).writes().len();
        let named = usize::from(event.writes().any(|written| written == register));
        if joined != named {
            return Err(Broken::of(
                33,
                format!(
                    "{register:?} holds {joined} writes of an event that names it {named} times"
                ),
            ));
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
    let has_value = R::value(event).is_some();
    if has_value == named {
        Ok(())
    } else {
        Err(Broken::of(
            33,
            format!(
                "{register:?}: the type gives a value {has_value}, and the event names it {named}"
            ),
        ))
    }
}
