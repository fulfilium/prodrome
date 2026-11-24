//! A digest: a summary tree over an append-only log of lines, read through
//! a view that fits a budget (design §6.5, law 46).
//!
//! The caller writes and records each summary; this module reads only which
//! nodes have a summary and its length in bytes (`summarized(name)`).
//!
//! The crate depends only on `prodrome-data` and `prodrome-memo`.
//! `prodrome-core` re-exports it as `prodrome::digest`.

#![warn(clippy::pedantic)]

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use prodrome_data::event::Hash;
use prodrome_data::topo;
use prodrome_memo::{self as memo, Algebra};

mod cut;
mod prefixes;

use cut::{Arena, Cut};

/// The monoid that every node carries up the tree. `join` must be
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
    pub name: Hash,
    /// The lines that its writer had seen. A line that names only lines
    /// that the log does not hold rests on nothing.
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

    #[must_use]
    pub fn node(&self, name: &Hash) -> Option<&Node<M>> {
        self.root.find(name)
    }

    /// The largest nodes that lie within the lines `span`, in order (law 50).
    /// `None` for an empty span, or one that reaches past the last line.
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
/// times, or `None` for no lines (law 46).
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

/// The measure as an algebra for [`fn@memo::fold`]. A fold can memoise it
/// by node name because a line's measure is a function of its name
/// ([`Leaf`]).
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
/// A cache of [`view`] or [`pending`] keyed by this name is never stale
/// (law 46). A line's measure is not covered, because it is a function of
/// the line's name ([`Leaf`]).
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
    /// The node's content name.
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
    /// The nodes to summarise next for this view, in the log's order
    /// (design §6.5, "What waits is what to summarise").
    pub waiting: Vec<Hash>,
}

/// The nodes whose summary can be written now, smallest first and then
/// leftmost (law 46). A caller can write summaries of different nodes in
/// any order, or at once.
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

/// The view of the log under `budget` bytes (design §6.5, law 46).
///
/// Of the admissible closings, the one started at the most due node closes.
/// The due of a node is its age over its size, both in lines, and of two
/// nodes that are as due, the older closes. The view is greedy, so it can
/// stop over its budget although a fit with a coarser node to the left
/// exists.
///
/// # Cost
///
/// For a tree of `n` nodes and depth `d`, a line's arrival and each closing
/// update `d` ancestors in time `O(d log n)`, and each node closes, or comes
/// to lie within a part, at most once. Trying a closing takes time
/// `O(log n)`, and a search for a closing keeps at most `d` candidate nodes
/// for each part of the view.
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
