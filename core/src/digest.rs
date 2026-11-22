//! A digest: a summary tree over an append-only log of lines, read through
//! a view that fits a budget (design §6.5, law 46).
//!
//! The lines are the leaves of a tree ([`tree`]) whose shape is
//! [`fn@memo::balance`] over their names in the crate's one causal order.
//! So the shape is a function of the set of lines and not of when each line
//! arrived. A node is named by [`memo::name`] over its children and carries
//! the [`Measure`] of the lines that it spans.
//!
//! An inner node can have a summary: a short text that stands for its
//! lines. The caller writes a summary elsewhere and records it as an object
//! of its own. A summary is not a function of the lines, so this module
//! never makes or caches one. It reads only which nodes have a summary and
//! its length (`summarized(name)`), and [`pending`] tells which summaries
//! can be written next.
//!
//! A [`View`] is a cut through the tree that tiles the whole log under a
//! byte budget ([`view`]), and [`zoom`] opens a part into its children.
//! Everything here is a function of the leaves, the lengths of the
//! summaries and the budget: no clock and no model.

#![warn(clippy::pedantic)]

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::event::Hash;
use crate::memo::{self, Algebra};
use crate::topo;

mod cut;
mod prefixes;

use cut::{Arena, Cut};

/// The monoid that every node carries up the tree, such as the count of
/// its lines, their tokens or the instants they span. `join` must be
/// associative, with `empty` as its unit.
pub trait Measure: Clone + Eq {
    /// The measure of no lines.
    fn empty() -> Self;
    /// The measure of `self`'s lines followed by `other`'s.
    #[must_use]
    fn join(&self, other: &Self) -> Self;
}

/// The measure of lines given in order, the monoid's fold: the one fold
/// a chunk and [`Measured`] share.
fn joined<'m, M: Measure + 'm>(measures: impl IntoIterator<Item = &'m M>) -> M {
    measures
        .into_iter()
        .fold(M::empty(), |measure, next| measure.join(next))
}

/// No measure at all.
impl Measure for () {
    fn empty() {}
    fn join(&self, (): &()) {}
}

/// A line of the log.
///
/// The name must be the line's content name, and every other field must be
/// a function of it, because a tree, a [`read`] key and a [`Measured`] fold
/// are keyed by names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leaf<M> {
    /// The line's content name.
    pub name: Hash,
    /// The lines it rests on: what its writer had seen. A line given none,
    /// or only lines the log does not hold, rests on nothing here.
    pub parents: Vec<Hash>,
    /// What the line costs in a view, in bytes.
    pub bytes: u32,
    /// What the line carries up the tree.
    pub measure: M,
}

/// A node of the tree: a line, or a chunk of two to
/// [`memo::balance::WIDEST`] consecutive nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node<M> {
    name: Hash,
    span: Range<usize>,
    level: usize,
    bytes: u64,
    measure: M,
    children: Vec<Node<M>>,
}

impl<M> Node<M> {
    /// Its content name: a line's own, or [`memo::name`] over a chunk's
    /// children.
    #[must_use]
    pub fn name(&self) -> &Hash {
        &self.name
    }

    /// The lines it spans, as positions in the log.
    #[must_use]
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    /// Its height: 0 for a line, one more than its children's for a chunk.
    /// Every line is at one depth, so every child of a node is one level
    /// below it.
    #[must_use]
    pub fn level(&self) -> usize {
        self.level
    }

    /// The bytes of the lines that it spans, which it costs when it is
    /// opened all the way.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The fold of its lines' measures.
    #[must_use]
    pub fn measure(&self) -> &M {
        &self.measure
    }

    /// Its children, in the log's order; none for a line.
    #[must_use]
    pub fn children(&self) -> &[Node<M>] {
        &self.children
    }

    /// Whether it is a line.
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    fn part(&self) -> Part {
        Part {
            node: self.name.clone(),
            span: self.span(),
        }
    }

    fn find(&self, name: &Hash) -> Option<&Node<M>> {
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            if &node.name == name {
                return Some(node);
            }
            stack.extend(&node.children);
        }
        None
    }
}

impl<M> memo::Tree for Node<M> {
    fn name(&self) -> &Hash {
        &self.name
    }

    fn children(&self) -> impl Iterator<Item = &Self> {
        self.children.iter()
    }
}

/// The summary tree over a nonempty log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree<M> {
    root: Node<M>,
}

impl<M> Tree<M> {
    /// The root, spanning the whole log.
    #[must_use]
    pub fn root(&self) -> &Node<M> {
        &self.root
    }

    /// How many lines the log holds: one or more.
    #[must_use]
    pub fn lines(&self) -> usize {
        self.root.span.end
    }

    /// Whether `node` is settled: it does not hold the last line, so no
    /// appended line renames it. The next line renames a node that holds the
    /// last line, so a summary of that node goes stale.
    #[must_use]
    pub fn settled(&self, node: &Node<M>) -> bool {
        node.span.end < self.lines()
    }

    /// The node named `name`, if the tree has one.
    #[must_use]
    pub fn node(&self, name: &Hash) -> Option<&Node<M>> {
        self.root.find(name)
    }

    /// The largest nodes that lie within the lines `span`, in order: the
    /// one node that spans it where there is one, and otherwise the nodes
    /// that tile it, each with no ancestor inside `span`. `None` for an
    /// empty span, or one that reaches past the last line.
    #[must_use]
    pub fn cover(&self, span: Range<usize>) -> Option<Vec<&Node<M>>> {
        fn within<'t, M>(node: &'t Node<M>, span: &Range<usize>, out: &mut Vec<&'t Node<M>>) {
            let lines = node.span();
            if span.start <= lines.start && lines.end <= span.end {
                out.push(node);
            } else if lines.start < span.end && span.start < lines.end {
                for child in node.children() {
                    within(child, span, out);
                }
            }
        }
        if span.is_empty() || span.end > self.lines() {
            return None;
        }
        let mut out = Vec::new();
        within(&self.root, &span, &mut out);
        Some(out)
    }
}

/// A log whose lines rest on one another in a cycle, with the least line
/// that no order places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle(pub Hash);

/// The tree over the set of `leaves`, given in any order and any number of
/// times, or `None` for no lines.
///
/// The lines are in the crate's one causal order ([`topo::linear`]): each
/// after the lines it rests on, and unrelated lines by name. So two
/// replicas that hold one set of lines build one tree.
///
/// # Errors
///
/// [`Cycle`] when the lines rest on one another in a cycle.
pub fn tree<M: Measure>(leaves: Vec<Leaf<M>>) -> Result<Option<Tree<M>>, Cycle> {
    let mut lines: BTreeMap<Hash, Leaf<M>> = BTreeMap::new();
    for leaf in leaves {
        lines.entry(leaf.name.clone()).or_insert(leaf);
    }
    let rests_on: BTreeMap<&Hash, Vec<&Hash>> = lines
        .iter()
        .map(|(name, leaf)| (name, leaf.parents.iter().collect()))
        .collect();
    let order: Vec<Hash> = topo::linear(&rests_on)
        .map_err(|stuck| Cycle(stuck.clone()))?
        .into_iter()
        .cloned()
        .collect();
    let nodes = order
        .into_iter()
        .filter_map(|name| lines.remove(&name))
        .enumerate()
        .map(|(at, leaf)| Node {
            name: leaf.name,
            span: at..at + 1,
            level: 0,
            bytes: u64::from(leaf.bytes),
            measure: leaf.measure,
            children: Vec::new(),
        })
        .collect();
    Ok(memo::balance(nodes, |children: Vec<Node<M>>| Node {
        name: memo::name(b"digest", children.iter().map(Node::name)),
        span: children[0].span.start..children[children.len() - 1].span.end,
        level: children[0].level + 1,
        bytes: children.iter().map(Node::bytes).sum(),
        measure: joined(children.iter().map(Node::measure)),
        children,
    })
    .map(|root| Tree { root }))
}

/// The measure as an algebra for [`fn@memo::fold`]: a line has its own, and
/// a chunk has the join of its children's. A fold can memoise it by node
/// name because a line's measure is a function of its name ([`Leaf`]).
#[derive(Debug, Clone)]
pub struct Measured(Hash);

impl Default for Measured {
    fn default() -> Self {
        Measured(memo::name(b"digest measure", []))
    }
}

impl<M: Measure> Algebra<Node<M>> for Measured {
    type Out = M;
    fn name(&self) -> &Hash {
        &self.0
    }
    fn apply(&self, node: &Node<M>, children: Vec<M>) -> M {
        if node.is_leaf() {
            node.measure.clone()
        } else {
            joined(&children)
        }
    }
}

/// The name of the tree as read under its summaries, over its root's name
/// and, in order, every line's cost and every inner node's summary length
/// or its absence.
///
/// [`view`] and [`pending`] read nothing else except the budget, so a cache
/// keyed by this name is never stale. A line's measure is not covered,
/// because it is a function of the line's name ([`Leaf`]).
#[must_use]
pub fn read<M>(tree: &Tree<M>, summarized: impl Fn(&Hash) -> Option<u32>) -> Hash {
    let mut own = Vec::new();
    let mut stack = vec![&tree.root];
    while let Some(node) = stack.pop() {
        if node.is_leaf() {
            own.extend(node.bytes.to_be_bytes());
        } else {
            match summarized(&node.name) {
                Some(bytes) => {
                    own.push(1);
                    own.extend(bytes.to_be_bytes());
                }
                None => own.push(0),
            }
        }
        stack.extend(node.children.iter().rev());
    }
    memo::name(&own, [&tree.root.name])
}

/// A part of a view: a node, standing for the lines it spans.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Part {
    /// The node's content name, its identity.
    pub node: Hash,
    /// The lines it spans.
    pub span: Range<usize>,
}

impl Part {
    /// A handle to show a reader, `first+lines`: its first line and how
    /// many lines it spans. A line that arrives before the part moves the
    /// handle, and only the node's name stays the same.
    #[must_use]
    pub fn handle(&self) -> String {
        format!("{}+{}", self.span.start, self.span.len())
    }
}

/// A cut through the tree under a budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// The parts, in the log's order, tiling it.
    pub parts: Vec<Part>,
    /// What the parts cost in bytes: each line's bytes and each summary's.
    pub bytes: u64,
    /// The nodes to summarise next for this view, in the log's order: the
    /// pending nodes at or under each node without a summary in a closing
    /// more due than the one made, or in any closing when none could be
    /// made. Writing them round after round ends with the view in its
    /// budget or with no closing that lowers its cost.
    pub waiting: Vec<Hash>,
}

/// The nodes whose summary can be written now, smallest first and then
/// leftmost: each is settled and has no summary, and each of its children
/// is a line or has a summary. The result depends only on which nodes have
/// a summary, so a caller can write summaries of different nodes in any
/// order, or at once.
#[must_use]
pub fn pending<M>(tree: &Tree<M>, summarized: impl Fn(&Hash) -> Option<u32>) -> Vec<Hash> {
    let mut ready = Vec::new();
    let mut stack = vec![&tree.root];
    while let Some(node) = stack.pop() {
        if node.is_leaf() {
            continue;
        }
        if tree.settled(node)
            && summarized(&node.name).is_none()
            && node
                .children
                .iter()
                .all(|child| child.is_leaf() || summarized(&child.name).is_some())
        {
            ready.push(node);
        }
        stack.extend(&node.children);
    }
    ready.sort_by_key(|node| (node.span.len(), node.span.start));
    ready.into_iter().map(|node| node.name.clone()).collect()
}

/// The children of `name` as parts; `None` when the tree has no inner node
/// of that name (a line does not open).
#[must_use]
pub fn zoom<M>(tree: &Tree<M>, name: &Hash) -> Option<Vec<Part>> {
    let node = tree.node(name).filter(|node| !node.is_leaf())?;
    Some(node.children.iter().map(Node::part).collect())
}

/// The view of the log under `budget` bytes (law 46).
///
/// The view is built line by line, each line appended as a part of its
/// own. While the parts cost more than the budget, the view makes a
/// closing: settled nodes ([`Tree::settled`], so that no later line renames
/// a closed part) replace the parts that they span and are shown as their
/// summaries. A closing starts at one node and also holds the nodes that
/// decay forces. Decay means that the levels of the parts never rise toward
/// the present: where the part before a node is lower than the node, the
/// node at its level over that part closes too, and so on leftward until a
/// part as high or the start of the log. A closing is admissible when each
/// of its nodes has a summary and every leftward prefix of it costs less
/// than the parts it replaces. So every closing lowers the cost of the
/// view, and the view never costs more than its lines. A start that would
/// not lower the cost itself never closes on the account of a forced node.
///
/// Of the admissible closings, the one started at the most due node
/// closes. The due of a node is its age over its size: the lines from its
/// first line to the present, over the lines that it spans. Of two nodes
/// that are as due, the older closes. A closing started at a more due node
/// that lacks a summary, or every closing when none is admissible, puts in
/// `waiting` the pending nodes at or under each of its nodes that lacks
/// one. The view stops over its budget only when no closing is admissible.
/// The view is greedy: decay forces nodes only at the level of the start,
/// so the view can stop over its budget although a fit with a coarser node
/// to the left exists. A closed part is never renamed and never opens as
/// lines arrive, so what a reader was shown of the past stays as it was
/// shown, or coarser.
///
/// A part costs its summary's bytes, or a line's own bytes.
///
/// From decay, a reader can infer that a part never sits at a lower level
/// of the tree than a part after it, and a settled node at level `h` spans
/// between `2^h` and `16^h` lines. A reader cannot infer that an earlier
/// part spans more lines than a later one: chunks hold two to sixteen
/// nodes, and no view can keep both decay in lines and its budget.
///
/// # Cost
///
/// Let the tree have `n` nodes and depth `d`. A line's arrival, and each
/// node's closing, updates the node's `d` ancestors in time `O(d log n)`;
/// each node closes, or comes to lie within a part, at most once. Trying a
/// closing takes two queries of time `O(log n)` and reads no part (see
/// `cut.rs`). A refused node is tried again only after a closing that
/// starts before its end changes the parts it reads, or after the first
/// node without a summary at its level moves past it, which happens once.
/// Each step that looks for a closing puts the nodes that may close and are
/// not yet refused in a heap, in time linear in their number. They are open
/// nodes, so there are at most `d` for each part of the view.
#[must_use]
pub fn view<M>(tree: &Tree<M>, summarized: impl Fn(&Hash) -> Option<u32>, budget: u64) -> View {
    let arena = Arena::of(tree);
    let cost: Vec<Option<u64>> = arena
        .nodes
        .iter()
        .map(|node| {
            if node.is_leaf() {
                Some(node.bytes)
            } else {
                summarized(&node.name).map(u64::from)
            }
        })
        .collect();
    // What `pending` offers, by id.
    let ready: BTreeSet<usize> = (0..arena.nodes.len())
        .filter(|&id| {
            let node = arena.nodes[id];
            !node.is_leaf()
                && tree.settled(node)
                && cost[id].is_none()
                && arena.children(id).all(|child| cost[child].is_some())
        })
        .collect();
    let mut cut = Cut::new(&arena, &cost, &ready);
    for (at, &line) in arena.leaves.iter().enumerate() {
        cut.arrive(at, line, arena.nodes[line].bytes);
        while cut.bytes() > budget && cut.close_most_due() {}
    }
    View {
        parts: cut
            .parts()
            .into_iter()
            .map(|id| arena.nodes[id].part())
            .collect(),
        bytes: cut.bytes(),
        waiting: cut
            .waiting()
            .map(|id| arena.nodes[id].name.clone())
            .collect(),
    }
}
