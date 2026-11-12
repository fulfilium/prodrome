//! The memoised fold (design §6.2):
//!
//! ```text
//! memoCata alg (Fix f) = memo (name (Fix f)) (alg (fmap (memoCata alg) f))
//! ```
//!
//! one combinator over any tree whose nodes carry a content name. The
//! algebra is memoised at EVERY node, keyed by its own name and the node's,
//! so the cache takes the data's own shape: after a change, the nodes whose
//! names moved (the spine from each changed subtree to the root) are
//! computed, and every other subtree is one hit at its top, never entered.
//! Nothing names a level.

use crate::event::Hash;

use super::cache::{Key, Lookup};
use super::holder::Holder;

/// A node of a content-addressed tree: a fixed point of some functor, seen
/// through its name and its recursive positions.
///
/// THE NAME MUST COVER THE NODE: its own content and its children's names,
/// in order, as [`super::name`] makes one. Then a changed node renames
/// exactly itself and its ancestors, and two nodes with one name fold to one
/// value. A name that covers too little makes a hit wrong; one that covers
/// too much makes too much miss.
pub trait Tree {
    /// The node's content name.
    fn name(&self) -> &Hash;
    /// The node's children, in the order an algebra receives their results.
    fn children(&self) -> impl Iterator<Item = &Self>;
}

/// An `f`-algebra over a tree: what a node is, given what its children are.
///
/// `apply` is `alg` applied to one layer of the functor: the node stands
/// for its own content, and the children's results fill its recursive
/// positions. It must be PURE: a function of that layer and of nothing the
/// node's name does not cover. Its own name covers its code and everything
/// that code reads, so two algebras share a cache without sharing an entry.
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
    /// The cache held its value: the node and its whole subtree are this one
    /// lookup.
    Hit,
    /// Its value was computed, and is held from now on.
    Computed,
    /// Its key held two values. Its value was computed, and neither held
    /// value was used.
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
/// computed. A fold's cost is the computed nodes' work, so a cost that does
/// not follow from these counts is itself a finding.
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
/// node by (the algebra's name, the node's name), with what it looked at.
/// The cache is any [`Holder`]: a [`super::Cache`] in memory or in a file,
/// or a [`super::Front`] of memory before a file.
///
/// Its value is the plain fold's (`memo f = f`) whatever entries were
/// dropped, wherever the cache's entries were made by a pure algebra's
/// folds, here or in a cache joined into this one: a held value is the one
/// the algebra computes, an entry dropped is computed again, and a key
/// holding two values is computed, never chosen, and reported. A single
/// value an untrusted writer put at a key nothing else filled is a claim,
/// which only recomputing it checks. Iterative, so a tree's depth costs
/// heap and not stack.
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
