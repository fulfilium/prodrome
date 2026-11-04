//! A view being built: the state between two steps of [`super::view`], and
//! the steps.
//!
//! The view's parts always decay: their levels never rise toward the
//! present. So the parts below level `L` are the ones after one point of the
//! log, the end of the last part at level `L` or above, and the closing
//! started at a node of level `L` forces exactly the nodes of level `L`
//! from that point up to the start. [`Level`] keeps, for each level, that
//! point, the nodes without a summary, and each node's gain (the bytes of
//! the parts it spans, less its summary's) in a [`Prefixes`]. A closing
//! started at the `j`th node after the point is admissible when no node up
//! to it lacks a summary and the gains' prefix sum up to it exceeds every
//! earlier one: then each leftward prefix of the closing gains more than
//! nothing. Each of the two checks is one logarithmic query.

use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap};

use super::prefixes::Prefixes;
use super::{Node, Tree};

/// The tree's nodes numbered in pre-order, which is the log's order by
/// first line and, among nodes starting at one line, outermost first; so a
/// node's descendants are the ids after it, up to `after`.
pub(super) struct Arena<'t, M> {
    pub(super) nodes: Vec<&'t Node<M>>,
    parent: Vec<Option<usize>>,
    pub(super) after: Vec<usize>,
    pub(super) leaves: Vec<usize>,
}

impl<'t, M> Arena<'t, M> {
    pub(super) fn of(tree: &'t Tree<M>) -> Self {
        let mut arena = Arena {
            nodes: Vec::new(),
            parent: Vec::new(),
            after: Vec::new(),
            leaves: Vec::new(),
        };
        let mut stack = vec![(&tree.root, None)];
        while let Some((node, parent)) = stack.pop() {
            let id = arena.nodes.len();
            arena.nodes.push(node);
            arena.parent.push(parent);
            if node.is_leaf() {
                arena.leaves.push(id);
            }
            stack.extend(node.children.iter().rev().map(|child| (child, Some(id))));
        }
        // A node's subtree is itself and its children's, which come after it.
        let mut size = vec![1; arena.nodes.len()];
        for id in (1..arena.nodes.len()).rev() {
            if let Some(parent) = arena.parent[id] {
                size[parent] += size[id];
            }
        }
        arena.after = size
            .iter()
            .enumerate()
            .map(|(id, size)| id + size)
            .collect();
        arena
    }

    /// The node's ancestors, its parent first.
    fn ancestors(&self, id: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(self.parent[id], |&id| self.parent[id])
    }

    /// The node's children, in the log's order.
    pub(super) fn children(&self, id: usize) -> impl Iterator<Item = usize> + '_ {
        let end = self.after[id];
        std::iter::successors(Some(id + 1), |&child| Some(self.after[child]))
            .take_while(move |&child| child < end)
    }

    /// How due a node is at `now` lines: its age over its size.
    pub(super) fn due(&self, id: usize, now: usize) -> Due {
        let span = &self.nodes[id].span;
        Due {
            age: now - span.start,
            size: span.len(),
        }
    }
}

/// The nodes of one level of the tree, in the log's order, and what a
/// closing started at one of them reads.
struct Level {
    /// The nodes' ids.
    nodes: Vec<usize>,
    /// The positions, in `nodes`, of the nodes without a summary.
    unsummarised: Vec<usize>,
    /// Each node's gain: the bytes of the parts it spans less its summary's,
    /// or nothing for a node without one.
    gains: Prefixes,
    /// The position of the node that starts where the last part at this
    /// level or above ends, or the log's start: where every closing at this
    /// level begins.
    from: usize,
    /// The positions below this one have put their pending nodes in the
    /// view's waiting set.
    waited: usize,
    /// The positions of open nodes refused because a node from `from` up to
    /// them has no summary. `from` only moves right, so each stays refused,
    /// and what it puts in waiting stays a part of what it put there, until
    /// `from` passes the node without a summary.
    blocked: BTreeSet<usize>,
}

impl Level {
    /// The position of the first node without a summary at or after `from`.
    fn first_unsummarised(&self) -> Option<usize> {
        let at = self.unsummarised.partition_point(|&at| at < self.from);
        self.unsummarised.get(at).copied()
    }
}

/// What trying the closing started at a node found.
enum Verdict {
    /// It is admissible: these nodes, the start first and then leftward,
    /// with their summaries' bytes.
    Closes(Vec<(usize, u64)>),
    /// Some prefix of it does not lower the cost.
    Dearer,
    /// A node in it has no summary.
    Blocked,
}

/// Where a node stands in a view being built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// None of its lines has arrived.
    Unseen,
    /// It is above a part, and none of its ancestors is one: it may close.
    Open,
    /// It is shown as one piece.
    Part,
    /// It lies within a part.
    Covered,
}

/// A view being built: each node's [`Place`], and what the parts cost.
/// FRESH is every open node whose lines have all arrived that is not known
/// to be refused. DEARER holds, by its end and id, each open node refused
/// because a prefix of its closing does not lower the cost, until a closing
/// starts before its end.
pub(super) struct Cut<'a, 't, M> {
    arena: &'a Arena<'t, M>,
    cost: &'a [Option<u64>],
    ready: &'a BTreeSet<usize>,
    position: Vec<usize>,
    levels: Vec<Level>,
    now: usize,
    place: Vec<Place>,
    covered: Vec<u64>,
    fresh: BTreeSet<usize>,
    dearer: BTreeSet<(usize, usize)>,
    waiting: BTreeSet<usize>,
    bytes: u64,
}

impl<'a, 't, M> Cut<'a, 't, M> {
    /// An empty view of the tree in `arena`, with each node's cost, a
    /// line's bytes or a summary's, and the ids of the nodes that can be
    /// summarised now.
    pub(super) fn new(
        arena: &'a Arena<'t, M>,
        cost: &'a [Option<u64>],
        ready: &'a BTreeSet<usize>,
    ) -> Self {
        let mut position = vec![0; arena.nodes.len()];
        let mut nodes: Vec<Vec<usize>> = Vec::new();
        for (id, node) in arena.nodes.iter().enumerate() {
            if nodes.len() <= node.level {
                nodes.resize_with(node.level + 1, Vec::new);
            }
            position[id] = nodes[node.level].len();
            nodes[node.level].push(id);
        }
        let levels = nodes
            .into_iter()
            .map(|nodes| Level {
                unsummarised: (0..nodes.len())
                    .filter(|&at| cost[nodes[at]].is_none())
                    .collect(),
                gains: Prefixes::of(nodes.iter().map(|&id| gain(0, cost[id]))),
                nodes,
                from: 0,
                waited: 0,
                blocked: BTreeSet::new(),
            })
            .collect();
        Cut {
            arena,
            cost,
            ready,
            position,
            levels,
            now: 0,
            place: vec![Place::Unseen; arena.nodes.len()],
            covered: vec![0; arena.nodes.len()],
            fresh: BTreeSet::new(),
            dearer: BTreeSet::new(),
            waiting: BTreeSet::new(),
            bytes: 0,
        }
    }

    /// What the parts cost.
    pub(super) fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The `at`th line arrived, as a part of its own: `now` is `at + 1`
    /// lines, and the nodes that end at `at` hold no line still to come.
    pub(super) fn arrive(&mut self, at: usize, line: usize, bytes: u64) {
        self.now = at + 1;
        self.close(line, bytes);
        if let Some(&before) = at
            .checked_sub(1)
            .and_then(|before| self.arena.leaves.get(before))
        {
            let settled = std::iter::once(before)
                .chain(self.arena.ancestors(before))
                .take_while(|&id| self.arena.nodes[id].span.end == at);
            for id in settled {
                if self.place[id] == Place::Open {
                    self.fresh.insert(id);
                }
            }
        }
    }

    /// The most due closing that lowers the cost, made: of the fresh
    /// nodes, most due first and of two as due the older, each is tried and
    /// refused until one is admissible, and that one closes. `false` when
    /// none is admissible.
    pub(super) fn close_most_due(&mut self) -> bool {
        let now = self.now;
        let mut order: BinaryHeap<Candidate> = self
            .fresh
            .iter()
            .map(|&id| Candidate {
                due: self.arena.due(id, now),
                id,
            })
            .collect();
        while let Some(Candidate { id, .. }) = order.pop() {
            self.fresh.remove(&id);
            match self.verdict(id) {
                Verdict::Closes(closing) => {
                    self.make(id, closing);
                    return true;
                }
                Verdict::Dearer => {
                    self.dearer.insert((self.arena.nodes[id].span.end, id));
                }
                Verdict::Blocked => {
                    let level = &mut self.levels[self.arena.nodes[id].level];
                    let at = self.position[id];
                    level.blocked.insert(at);
                    // Every node without a summary from `from` up to the
                    // start waits, with the pending nodes under it.
                    let first = level.from.max(level.waited);
                    let unsummarised = level.unsummarised.partition_point(|&u| u < first);
                    for &u in level.unsummarised[unsummarised..]
                        .iter()
                        .take_while(|&&u| u <= at)
                    {
                        let node = level.nodes[u];
                        self.waiting
                            .extend(self.ready.range(node..self.arena.after[node]));
                    }
                    level.waited = level.waited.max(at + 1);
                }
            }
        }
        false
    }

    /// Whether the closing started at the open, settled node `id` is
    /// admissible.
    fn verdict(&self, id: usize) -> Verdict {
        let level = &self.levels[self.arena.nodes[id].level];
        let at = self.position[id];
        if level.first_unsummarised().is_some_and(|u| u <= at) {
            return Verdict::Blocked;
        }
        let before = level.gains.range(level.from, at);
        if before.total + level.gains.get(at) <= before.best {
            return Verdict::Dearer;
        }
        let closing: Option<Vec<(usize, u64)>> = (level.from..=at)
            .rev()
            .map(|at| {
                let node = level.nodes[at];
                self.cost[node].map(|cost| (node, cost))
            })
            .collect();
        closing.map_or(Verdict::Blocked, Verdict::Closes)
    }

    /// The admissible closing started at `id` made: its nodes close, the
    /// refusals its parts were read for end, and every level at or below it
    /// begins its closings after it.
    fn make(&mut self, id: usize, closing: Vec<(usize, u64)>) {
        let level = self.arena.nodes[id].level;
        let end = self.arena.nodes[id].span.end;
        let from = self.levels[level].nodes[self.levels[level].from];
        let start = self.arena.nodes[from].span.start;
        for (node, size) in closing {
            self.close(node, size);
        }
        // A closing reads the parts before its node's end, so one refused
        // for a dearer prefix stays refused while the parts before its end
        // stay as they are.
        for (_, again) in self.dearer.split_off(&(start + 1, 0)) {
            self.fresh.insert(again);
        }
        // The node at each level below that starts at `end`: the line there,
        // and its ancestors. `end` is before the last line, as `id` is
        // settled.
        let mut after = self.arena.leaves[end];
        for below in 1..=level {
            if let Some(parent) = self.arena.parent[after] {
                after = parent;
            }
            let lower = &mut self.levels[below];
            lower.from = lower.from.max(self.position[after]);
            let unsummarised = lower.first_unsummarised().unwrap_or(lower.nodes.len());
            let still = lower.blocked.split_off(&unsummarised);
            for freed in std::mem::replace(&mut lower.blocked, still) {
                self.fresh.insert(lower.nodes[freed]);
            }
        }
    }

    /// `id` made a part costing `size` in place of the parts it spans (none,
    /// for a line just arrived).
    fn close(&mut self, id: usize, size: u64) {
        let was = self.covered[id];
        self.bytes = self.bytes - was + size;
        for above in self.arena.ancestors(id) {
            self.covered[above] = self.covered[above] - was + size;
            let level = self.arena.nodes[above].level;
            self.levels[level].gains.set(
                self.position[above],
                gain(self.covered[above], self.cost[above]),
            );
            if self.place[above] == Place::Unseen {
                self.place[above] = Place::Open;
                if self.arena.nodes[above].span.end < self.now {
                    self.fresh.insert(above);
                }
            }
        }
        self.shut(id);
        self.place[id] = Place::Part;
        // Within `id`, every open node and every part is covered. A part's
        // own nodes are covered already, so each node is visited once in
        // all.
        let mut below = id + 1;
        while below < self.arena.after[id] {
            match self.place[below] {
                Place::Open => {
                    self.shut(below);
                    self.place[below] = Place::Covered;
                    below += 1;
                }
                Place::Part => {
                    self.place[below] = Place::Covered;
                    below = self.arena.after[below];
                }
                Place::Unseen | Place::Covered => below = self.arena.after[below],
            }
        }
    }

    /// `id` no longer open: no longer a candidate, fresh or refused.
    fn shut(&mut self, id: usize) {
        self.fresh.remove(&id);
        self.dearer.remove(&(self.arena.nodes[id].span.end, id));
        self.levels[self.arena.nodes[id].level]
            .blocked
            .remove(&self.position[id]);
    }

    /// The parts, in the log's order.
    pub(super) fn parts(&self) -> Vec<usize> {
        let mut parts = Vec::new();
        let mut id = 0;
        while id < self.place.len() {
            if self.place[id] == Place::Part {
                parts.push(id);
                id = self.arena.after[id];
            } else {
                id += 1;
            }
        }
        parts
    }

    /// The nodes to summarise next: every pending node that a refused
    /// closing needed and that is still open.
    pub(super) fn waiting(&self) -> impl Iterator<Item = usize> + '_ {
        self.waiting
            .iter()
            .copied()
            .filter(|&id| self.place[id] == Place::Open)
    }
}

/// What closing a node whose parts cost `covered` gains: nothing for a
/// node without a summary, which never closes.
fn gain(covered: u64, cost: Option<u64>) -> i128 {
    cost.map_or(0, |cost| i128::from(covered) - i128::from(cost))
}

/// A fresh node in the order its closing is tried: the most due first,
/// and of two as due the one with the smaller id, which is the older.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Candidate {
    due: Due,
    id: usize,
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.due.cmp(&other.due).then(other.id.cmp(&self.id))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A ratio, compared exactly.
#[derive(Clone, Copy)]
pub(super) struct Due {
    age: usize,
    size: usize,
}

impl Ord for Due {
    fn cmp(&self, other: &Self) -> Ordering {
        let wide = |n: usize| n as u128;
        (wide(self.age) * wide(other.size)).cmp(&(wide(other.age) * wide(self.size)))
    }
}

impl PartialEq for Due {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Due {}

impl PartialOrd for Due {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
