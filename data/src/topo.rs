use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

/// The crate's one causal order: each key comes after the keys of `rests_on`
/// that it rests on, and unrelated keys come in key order.
///
/// The order depends only on the relation, not on the order of the input.
/// The walk ignores an edge to a key that `rests_on` does not hold. `Err`
/// holds the least key that a cycle leaves unplaced.
///
/// The heap holds each key's index among the keys of the map, which are in
/// key order, so an index orders as its key does.
pub fn linear<'k, K: Ord + ?Sized>(
    rests_on: &BTreeMap<&'k K, Vec<&'k K>>,
) -> Result<Vec<&'k K>, &'k K> {
    let keys: Vec<&'k K> = rests_on.keys().copied().collect();
    let mut above: Vec<Vec<usize>> = vec![Vec::new(); keys.len()];
    let mut waiting = vec![0_usize; keys.len()];
    for (at, below) in rests_on.values().enumerate() {
        for under in below {
            if let Ok(under) = keys.binary_search(under) {
                above[under].push(at);
                waiting[at] += 1;
            }
        }
    }
    let mut ready: BinaryHeap<Reverse<usize>> = (0..keys.len())
        .filter(|&at| waiting[at] == 0)
        .map(Reverse)
        .collect();
    let mut order = Vec::with_capacity(keys.len());
    while let Some(Reverse(at)) = ready.pop() {
        order.push(keys[at]);
        for &over in &above[at] {
            waiting[over] -= 1;
            if waiting[over] == 0 {
                ready.push(Reverse(over));
            }
        }
    }
    match (0..keys.len()).find(|&at| waiting[at] > 0) {
        Some(stuck) => Err(keys[stuck]),
        None => Ok(order),
    }
}

/// A depth-first walk that settles each key after the keys it expands into.
#[doc(hidden)]
pub struct Topo<K, V> {
    path: Vec<K>,
    settled: BTreeMap<K, V>,
    order: Vec<K>,
}

impl<K, V> Default for Topo<K, V> {
    fn default() -> Self {
        Topo {
            path: Vec::new(),
            settled: BTreeMap::new(),
            order: Vec::new(),
        }
    }
}

impl<K: Ord + Clone, V: Clone> Topo<K, V> {
    /// The value of `key`; the walk keeps each value that `expand` returns,
    /// and calls `expand` only for a key without a kept value.
    ///
    /// If the walk meets `key` again on its own path, `cycle` receives the
    /// path from `key`, with `key` repeated at the end.
    pub fn settle<E>(
        &mut self,
        key: &K,
        cycle: impl FnOnce(Vec<K>) -> E,
        expand: impl FnOnce(&mut Self) -> Result<V, E>,
    ) -> Result<V, E> {
        if let Some(value) = self.settled.get(key) {
            return Ok(value.clone());
        }
        if let Some(from) = self.path.iter().position(|on| on == key) {
            let mut path = self.path[from..].to_vec();
            path.push(key.clone());
            return Err(cycle(path));
        }
        self.path.push(key.clone());
        let value = expand(self);
        self.path.pop();
        let value = value?;
        self.settled.insert(key.clone(), value.clone());
        self.order.push(key.clone());
        Ok(value)
    }

    /// Every settled key, each after the keys it expanded into.
    pub fn into_order(self) -> Vec<K> {
        self.order
    }
}

#[cfg(test)]
mod tests {
    use super::linear;
    use std::collections::BTreeMap;

    #[test]
    fn parents_first_and_ties_by_key() {
        // c rests on d; a and b on nothing: a, b, d, then c.
        let rests_on: BTreeMap<&str, Vec<&str>> = BTreeMap::from([
            ("c", vec!["d"]),
            ("a", vec![]),
            ("d", vec!["x"]),
            ("b", vec![]),
        ]);
        assert_eq!(linear(&rests_on), Ok(vec!["a", "b", "d", "c"]));
    }

    #[test]
    fn a_cycle_names_its_least_key() {
        let rests_on: BTreeMap<&str, Vec<&str>> =
            BTreeMap::from([("a", vec![]), ("b", vec!["c"]), ("c", vec!["b"])]);
        assert_eq!(linear(&rests_on), Err("b"));
    }
}
