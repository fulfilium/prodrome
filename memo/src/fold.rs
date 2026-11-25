//! The memoised fold (design §6.2):
//!
//! ```text
//! memoCata alg (Fix f) = memo (name (Fix f)) (alg (fmap (memoCata alg) f))
//! ```
//!
//! The fold memoises the algebra at every node, keyed by the algebra's name
//! and the node's name. After a change, the fold computes the nodes whose
//! names changed, and every other subtree is one hit at its top.

use prodrome_data::event::Hash;

use super::cache::{Key, Lookup};
use super::holder::Holder;

/// A node of a content-addressed tree, seen through its name and its
/// children.
///
/// The name must cover the node: its own content and its children's names,
/// in order, as [`super::name`] makes one. A name that covers too little
/// makes a hit wrong, and a name that covers too much makes too many
/// lookups miss.
pub trait Tree {
    /// The node's content name.
    fn name(&self) -> &Hash;
    /// The node's children, in the order an algebra receives their results.
    fn children(&self) -> impl Iterator<Item = &Self>;
}

/// An algebra over a tree: the result at a node, given its children's
/// results.
///
/// `apply` must be pure, and a function of nothing that the node's name does
/// not cover. The algebra's name must cover its code and everything that the
/// code reads, so that two algebras never share an entry.
pub trait Algebra<T: ?Sized> {
    /// What the fold answers at each node.
    type Out;
    /// The algebra's name.
    fn name(&self) -> &Hash;
    /// The node's result, from the node and its children's results in the
    /// order [`Tree::children`] gives them.
    fn apply(&self, node: &T, children: Vec<Self::Out>) -> Self::Out;
}

/// What became of one node a fold looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    /// The cache held its value, so the fold did not enter its subtree.
    Hit,
    /// The fold computed its value and added it to the cache.
    Computed,
    /// Its key held two values. The fold computed its value and used
    /// neither held value.
    Finding,
}

/// One node a fold looked at.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Visit {
    /// The node's name.
    pub name: Hash,
    /// Its distance from the root, the root at 0.
    pub depth: usize,
    /// What became of it.
    pub outcome: Outcome,
}

/// How many nodes at one depth had each outcome.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Held values used.
    pub hits: usize,
    /// Values computed.
    pub computed: usize,
    /// Keys holding two values, computed instead.
    pub findings: usize,
}

/// What a fold looked at: per node and by depth, what hit and what was
/// computed. The cost of a fold is the work of its computed nodes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    visits: Vec<Visit>,
}

impl Report {
    /// Every node looked at, a node's children before it unless it hit.
    #[must_use]
    pub fn visits(&self) -> &[Visit] {
        &self.visits
    }

    /// The names of the nodes with `outcome`.
    pub fn names(&self, outcome: Outcome) -> impl Iterator<Item = &Hash> {
        self.visits
            .iter()
            .filter(move |visit| visit.outcome == outcome)
            .map(|visit| &visit.name)
    }

    /// The counts at each depth, the root's first.
    #[must_use]
    pub fn by_depth(&self) -> Vec<Counts> {
        let mut depths: Vec<Counts> = Vec::new();
        for visit in &self.visits {
            if depths.len() <= visit.depth {
                depths.resize(visit.depth + 1, Counts::default());
            }
            let counts = &mut depths[visit.depth];
            match visit.outcome {
                Outcome::Hit => counts.hits += 1,
                Outcome::Computed => counts.computed += 1,
                Outcome::Finding => counts.findings += 1,
            }
        }
        depths
    }
}

enum Frame<'t, T> {
    Enter(&'t T, usize),
    Exit {
        node: &'t T,
        depth: usize,
        arity: usize,
        finding: bool,
    },
}

/// The algebra folded over `tree` bottom-up, memoised in `cache` at every
/// node by the algebra's name and the node's name, with what it looked at.
///
/// The value equals the plain fold's (law 51) whatever entries were
/// dropped, provided that the folds of a pure algebra made every entry of
/// the cache. A single value that an untrusted writer put at a key that
/// nothing else filled is a claim, and only a recomputation checks it. A key
/// that holds two values is computed and reported, and the fold uses neither
/// value. The fold is iterative, so the depth of a tree costs heap and not
/// stack.
// Its one `expect` cannot fire: every node entered leaves exactly one value,
// so the root's is the one left when the stack empties.
#[allow(clippy::missing_panics_doc)]
pub fn fold<T, A, H>(algebra: &A, tree: &T, cache: &mut H) -> (A::Out, Report)
where
    T: Tree,
    A: Algebra<T>,
    A::Out: Clone,
    H: Holder<A::Out>,
{
    let key = |node: &T| Key {
        function: algebra.name().clone(),
        argument: node.name().clone(),
    };
    let mut report = Report::default();
    let mut results: Vec<A::Out> = Vec::new();
    let mut stack = vec![Frame::Enter(tree, 0)];
    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Enter(node, depth) => {
                let finding = match cache.get(&key(node)) {
                    Lookup::Hit(value) => {
                        results.push(value);
                        report.visits.push(Visit {
                            name: node.name().clone(),
                            depth,
                            outcome: Outcome::Hit,
                        });
                        continue;
                    }
                    Lookup::Miss => false,
                    Lookup::Finding(_) => true,
                };
                let children: Vec<&T> = node.children().collect();
                stack.push(Frame::Exit {
                    node,
                    depth,
                    arity: children.len(),
                    finding,
                });
                stack.extend(
                    children
                        .into_iter()
                        .rev()
                        .map(|child| Frame::Enter(child, depth + 1)),
                );
            }
            Frame::Exit {
                node,
                depth,
                arity,
                finding,
            } => {
                let children = results.split_off(results.len() - arity);
                let value = algebra.apply(node, children);
                let outcome = if finding {
                    Outcome::Finding
                } else {
                    // The key missed when this node was entered, so a value
                    // there now came from this fold: its own, for a pure
                    // algebra, and a second one is reported where it lands.
                    match cache.add(key(node), value.clone()) {
                        Ok(()) => Outcome::Computed,
                        Err(_) => Outcome::Finding,
                    }
                };
                report.visits.push(Visit {
                    name: node.name().clone(),
                    depth,
                    outcome,
                });
                results.push(value);
            }
        }
    }
    let value = results.pop().expect("the root's value is the one left");
    (value, report)
}
