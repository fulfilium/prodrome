use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

/// The crate's one causal order: Kahn's walk with a min-heap on the key,
/// so every key comes after the keys it rests on among `rests_on`'s keys,
/// and keys no order relates come in key order. It is a function of the
/// relation alone, not of the order it was given in. A key resting on one
/// that is not among them is passed over; `Err` names the least key a
/// cycle leaves unplaced.
///
/// The walk runs over each key's place among the keys, which a map holds
/// in key order: a place orders as its key does, so the heap compares
/// numbers, and an edge costs one binary search among the keys.
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

/// A depth-first walk that settles each key after what it expands into, with
/// the keys being expanded as the cycle witness.
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
    /// `key`'s value, expanded once. A key met again on its own path is a
    /// cycle, named from that key with it repeated at the end.
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

    /// Every settled key, each after what it expanded into.
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
