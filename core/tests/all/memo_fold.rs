//! Law 8 (design §6.2, §7): the memoised fold is the fold.
//!
//! Over generated trees of three shapes (lists, binary trees, rose trees)
//! and generated edits (a node relabelled, a subtree grafted in place of
//! another): folding through a cache answers what the plain fold answers,
//! with entries dropped at random between folds or by a small bounded
//! policy; after an edit, the nodes computed are exactly the spine from the
//! edited subtrees to the root, and every other node is under one hit at the
//! top of its unchanged subtree; two caches joined fold as either does; and
//! a forged entry is reported and never answered.

use std::collections::{BTreeMap, BTreeSet};

use prodrome::event::Hash;
use prodrome::memo::{self, fold, Algebra, Cache, Evict, Lookup, Lru, Outcome, Tree};
use proptest::prelude::*;
use proptest::sample::Index;

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) name: Hash,
    label: u32,
    pub(crate) kids: Vec<Node>,
}

impl Node {
    fn new(label: u32, kids: Vec<Node>) -> Node {
        Node {
            name: memo::name(&label.to_be_bytes(), kids.iter().map(|kid| &kid.name)),
            label,
            kids,
        }
    }

    pub(crate) fn size(&self) -> usize {
        1 + self.kids.iter().map(Node::size).sum::<usize>()
    }

    pub(crate) fn names(&self, into: &mut BTreeSet<Hash>) {
        into.insert(self.name.clone());
        self.kids.iter().for_each(|kid| kid.names(into));
    }

    /// The `index`th node in preorder.
    fn at(&self, index: usize) -> &Node {
        fn walk<'n>(node: &'n Node, index: &mut usize) -> Option<&'n Node> {
            if *index == 0 {
                return Some(node);
            }
            *index -= 1;
            node.kids.iter().find_map(|kid| walk(kid, index))
        }
        walk(self, &mut { index }).expect("an index within the tree")
    }
}

impl Tree for Node {
    fn name(&self) -> &Hash {
        &self.name
    }

    fn children(&self) -> impl Iterator<Item = &Node> {
        self.kids.iter()
    }
}

/// Two algebras with one output type, so they share a cache and only their
/// names keep their entries apart.
pub(crate) struct Sum(pub(crate) Hash);
pub(crate) struct Weigh(pub(crate) Hash);

impl Algebra<Node> for Sum {
    type Out = u64;
    fn name(&self) -> &Hash {
        &self.0
    }
    fn apply(&self, node: &Node, children: Vec<u64>) -> u64 {
        u64::from(node.label) + children.into_iter().sum::<u64>()
    }
}

impl Algebra<Node> for Weigh {
    type Out = u64;
    fn name(&self) -> &Hash {
        &self.0
    }
    fn apply(&self, node: &Node, children: Vec<u64>) -> u64 {
        children
            .into_iter()
            .zip(1u64..)
            .fold(u64::from(node.label) + 7, |acc, (child, at)| {
                acc.wrapping_mul(31).wrapping_add(child.wrapping_mul(at))
            })
    }
}

pub(crate) fn sum() -> Sum {
    Sum(memo::name(b"sum", []))
}

pub(crate) fn weigh() -> Weigh {
    Weigh(memo::name(b"weigh", []))
}

/// The plain fold, the meaning a cache must not change.
pub(crate) fn cata<A: Algebra<Node>>(algebra: &A, node: &Node) -> A::Out {
    let children = node.kids.iter().map(|kid| cata(algebra, kid)).collect();
    algebra.apply(node, children)
}

// --- shapes and edits --------------------------------------------------------

/// A tree's shape, its labels drawn when it is built.
#[derive(Debug, Clone)]
pub(crate) struct Shape(Vec<Shape>);

fn list() -> impl Strategy<Value = Shape> {
    (0..40usize).prop_map(|length| (0..length).fold(Shape(vec![]), |tail, _| Shape(vec![tail])))
}

fn binary() -> impl Strategy<Value = Shape> {
    Just(Shape(vec![])).prop_recursive(6, 64, 2, |inner| {
        (inner.clone(), inner).prop_map(|(left, right)| Shape(vec![left, right]))
    })
}

fn rose() -> impl Strategy<Value = Shape> {
    Just(Shape(vec![])).prop_recursive(4, 64, 4, |inner| {
        prop::collection::vec(inner, 0..4).prop_map(Shape)
    })
}

pub(crate) fn shape() -> impl Strategy<Value = Shape> {
    prop_oneof![list(), binary(), rose()]
}

/// Labels for the nodes built: every one fresh, or drawn from three so that
/// subtrees repeat and share names.
pub(crate) struct Labels {
    next: u32,
    modulo: u32,
}

impl Labels {
    pub(crate) fn unique() -> Labels {
        Labels {
            next: 0,
            modulo: u32::MAX,
        }
    }

    pub(crate) fn shared() -> Labels {
        Labels { next: 0, modulo: 3 }
    }

    fn draw(&mut self) -> u32 {
        self.next += 1;
        self.next % self.modulo
    }
}

pub(crate) fn build(shape: &Shape, labels: &mut Labels) -> Node {
    let label = labels.draw();
    let kids = shape.0.iter().map(|kid| build(kid, labels)).collect();
    Node::new(label, kids)
}

/// An edit at a node: relabelled, or replaced by a subtree of a new shape.
#[derive(Debug, Clone)]
pub(crate) enum Edit {
    Relabel,
    Graft(Shape),
}

pub(crate) fn edits() -> impl Strategy<Value = Vec<(Index, Edit)>> {
    let edit = prop_oneof![Just(Edit::Relabel), rose().prop_map(Edit::Graft)];
    prop::collection::vec((any::<Index>(), edit), 0..4)
}

/// `node` with `edits` applied at their preorder positions, and the names
/// of every node of the result the edits changed: an edited node, every node
/// of a grafted subtree, and every ancestor of one. Edits under a graft are
/// grafted over.
pub(crate) fn apply(
    node: &Node,
    edits: &BTreeMap<usize, Edit>,
    labels: &mut Labels,
) -> (Node, BTreeSet<Hash>) {
    fn walk(
        node: &Node,
        position: &mut usize,
        edits: &BTreeMap<usize, Edit>,
        labels: &mut Labels,
        changed: &mut BTreeSet<Hash>,
    ) -> Node {
        let here = *position;
        *position += 1;
        if let Some(Edit::Graft(shape)) = edits.get(&here) {
            *position += node.size() - 1;
            let grafted = build(shape, labels);
            grafted.names(changed);
            return grafted;
        }
        let before = changed.len();
        let kids: Vec<Node> = node
            .kids
            .iter()
            .map(|kid| walk(kid, position, edits, labels, changed))
            .collect();
        let relabelled = matches!(edits.get(&here), Some(Edit::Relabel));
        let label = if relabelled {
            labels.draw()
        } else {
            node.label
        };
        let rebuilt = Node::new(label, kids);
        if relabelled || changed.len() > before {
            changed.insert(rebuilt.name.clone());
        }
        rebuilt
    }
    let mut changed = BTreeSet::new();
    let edited = walk(node, &mut 0, edits, labels, &mut changed);
    (edited, changed)
}

pub(crate) fn positions(tree: &Node, edits: &[(Index, Edit)]) -> BTreeMap<usize, Edit> {
    edits
        .iter()
        .map(|(index, edit)| (index.index(tree.size()), edit.clone()))
        .collect()
}

/// Drops the cache's keys the draw picks, in key order.
fn evict<E: Evict>(cache: &mut Cache<u64, E>, picks: &[bool]) {
    let mut keys: Vec<_> = cache.keys().collect();
    keys.sort();
    for (key, drop) in keys.iter().zip(picks.iter().cycle()) {
        if *drop {
            cache.remove(key);
        }
    }
}

/// What a fold over `tree` should look at, as (name, depth, outcome), when
/// the cache holds every node but those named in `changed`.
fn expected(tree: &Node, changed: &BTreeSet<Hash>) -> BTreeSet<(Hash, usize, Outcome)> {
    fn walk(
        node: &Node,
        depth: usize,
        changed: &BTreeSet<Hash>,
        into: &mut BTreeSet<(Hash, usize, Outcome)>,
    ) {
        if changed.contains(&node.name) {
            into.insert((node.name.clone(), depth, Outcome::Computed));
            node.kids
                .iter()
                .for_each(|kid| walk(kid, depth + 1, changed, into));
        } else {
            into.insert((node.name.clone(), depth, Outcome::Hit));
        }
    }
    let mut into = BTreeSet::new();
    walk(tree, 0, changed, &mut into);
    into
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// `memoCata alg = cata alg`: over a run of edits, two algebras sharing
    /// one cache, entries dropped at random between folds, and again under
    /// a small bounded policy.
    #[test]
    fn the_memoised_fold_is_the_fold(
        shape in shape(),
        rounds in prop::collection::vec(edits(), 1..4),
        picks in prop::collection::vec(any::<bool>(), 1..16),
        capacity in 0..8usize,
    ) {
        let mut labels = Labels::shared();
        let mut tree = build(&shape, &mut labels);
        let mut dropped = Cache::default();
        let mut bounded = Cache::new(Lru::new(capacity));
        for edits in rounds {
            for value in [
                fold(&sum(), &tree, &mut dropped).0,
                fold(&sum(), &tree, &mut bounded).0,
            ] {
                prop_assert_eq!(value, cata(&sum(), &tree));
            }
            for value in [
                fold(&weigh(), &tree, &mut dropped).0,
                fold(&weigh(), &tree, &mut bounded).0,
            ] {
                prop_assert_eq!(value, cata(&weigh(), &tree));
            }
            prop_assert_eq!(dropped.findings().len(), 0);
            evict(&mut dropped, &picks);
            tree = apply(&tree, &positions(&tree, &edits), &mut labels).0;
        }
    }

    /// After an edit, the fold computes exactly the spine from the edited
    /// subtrees to the root, each node once, and hits exactly at the top of
    /// every unchanged subtree beside it, entering none.
    #[test]
    fn an_edit_recomputes_its_spine(shape in shape(), edits in edits()) {
        let mut labels = Labels::unique();
        let tree = build(&shape, &mut labels);
        let mut cache = Cache::default();
        let _ = fold(&sum(), &tree, &mut cache);
        let (edited, changed) = apply(&tree, &positions(&tree, &edits), &mut labels);
        let (value, report) = fold(&sum(), &edited, &mut cache);
        prop_assert_eq!(value, cata(&sum(), &edited));
        let seen: BTreeSet<_> = report
            .visits()
            .iter()
            .map(|visit| (visit.name.clone(), visit.depth, visit.outcome))
            .collect();
        prop_assert_eq!(seen.len(), report.visits().len(), "a node looked at twice");
        prop_assert_eq!(&seen, &expected(&edited, &changed));
        let by_depth = report.by_depth();
        prop_assert_eq!(by_depth.iter().map(|counts| counts.computed).sum::<usize>(), changed.len());
        prop_assert_eq!(by_depth.first().map(|root| root.hits + root.computed), Some(1));
    }

    /// With subtrees shared, what a fold computes is by name: exactly the
    /// names the cache had not seen, each once.
    #[test]
    fn a_fold_computes_the_names_it_has_not_seen(shape in shape(), edits in edits()) {
        let mut labels = Labels::shared();
        let tree = build(&shape, &mut labels);
        let mut cache = Cache::default();
        let _ = fold(&weigh(), &tree, &mut cache);
        let edited = apply(&tree, &positions(&tree, &edits), &mut labels).0;
        let (value, report) = fold(&weigh(), &edited, &mut cache);
        prop_assert_eq!(value, cata(&weigh(), &edited));
        let (mut before, mut after) = (BTreeSet::new(), BTreeSet::new());
        tree.names(&mut before);
        edited.names(&mut after);
        let computed: Vec<&Hash> = report.names(Outcome::Computed).collect();
        let unique: BTreeSet<Hash> = computed.iter().copied().cloned().collect();
        prop_assert_eq!(unique.len(), computed.len(), "a name computed twice");
        prop_assert_eq!(unique, &after - &before);
    }

    /// Two caches, each filled by its own folds and thinned at random,
    /// joined by union fold every tree as either alone does, and as the
    /// plain fold does; a pure algebra's caches never disagree.
    #[test]
    fn joined_caches_fold_as_either(
        one in shape(),
        two in shape(),
        edits in edits(),
        picks in prop::collection::vec(any::<bool>(), 1..16),
    ) {
        let mut labels = Labels::shared();
        let (one, two) = (build(&one, &mut labels), build(&two, &mut labels));
        let (mut left, mut right) = (Cache::default(), Cache::default());
        let _ = fold(&sum(), &one, &mut left);
        let _ = fold(&sum(), &two, &mut right);
        evict(&mut left, &picks);
        evict(&mut right, &picks[1..]);
        let mut joined = left.clone();
        prop_assert!(joined.join(&mut right.clone()).is_empty());
        let tree = apply(&one, &positions(&one, &edits), &mut labels).0;
        for cache in [&mut left, &mut right, &mut joined] {
            prop_assert_eq!(fold(&sum(), &tree, cache).0, cata(&sum(), &tree));
        }
    }

    /// A second value forged at a node's key is reported where the fold
    /// reads it, the node computed instead, and the forged value answered
    /// nowhere.
    #[test]
    fn a_forged_entry_is_reported(shape in shape(), at in any::<Index>()) {
        let mut labels = Labels::shared();
        let tree = build(&shape, &mut labels);
        let mut cache = Cache::default();
        let _ = fold(&sum(), &tree, &mut cache);
        let forged = tree.at(at.index(tree.size())).name.clone();
        let key = memo::Key { function: sum().0, argument: forged.clone() };
        let held = match cache.get(&key) {
            Lookup::Hit(value) => value,
            other => panic!("a folded node is held, got {other:?}"),
        };
        prop_assert!(cache.insert(key.clone(), held + 1).is_err());
        let others: Vec<_> = cache.keys().filter(|other| *other != key).collect();
        for other in &others {
            cache.remove(other);
        }
        let (value, report) = fold(&sum(), &tree, &mut cache);
        prop_assert_eq!(value, cata(&sum(), &tree));
        prop_assert!(report.names(Outcome::Finding).any(|name| *name == forged));
        prop_assert_eq!(cache.get(&key), Lookup::Finding(vec![held, held + 1]));
    }
}
