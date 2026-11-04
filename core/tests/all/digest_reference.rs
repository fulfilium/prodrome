//! The reference digest view: `digest::view` as it was first written, line by
//! line and closing by closing, through the public API alone. It sorts every
//! open node by due after each line while the view is over its budget, and it
//! walks each closing leftward part by part, so it costs time quadratic in
//! the log's length. The law
//! `digest.rs::the_view_is_the_reference_view` checks that the crate's view
//! answers exactly what this one answers.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use prodrome::digest::{Node, Part, Tree, View};
use prodrome::event::Hash;

/// The view of the log under `budget` bytes, as `digest::view` documents it.
pub fn view<M>(tree: &Tree<M>, summarized: impl Fn(&Hash) -> Option<u32>, budget: u64) -> View {
    let arena = Arena::of(tree);
    let cost = |id: usize| -> Option<u64> {
        let node = arena.nodes[id];
        if node.is_leaf() {
            Some(node.bytes())
        } else {
            summarized(node.name()).map(u64::from)
        }
    };
    let ready: BTreeSet<usize> = (0..arena.nodes.len())
        .filter(|&id| {
            let node = arena.nodes[id];
            !node.is_leaf()
                && tree.settled(node)
                && cost(id).is_none()
                && node
                    .children()
                    .iter()
                    .all(|child| child.is_leaf() || summarized(child.name()).is_some())
        })
        .collect();
    let mut cut = Cut {
        arena: &arena,
        parts: Vec::new(),
        covered: vec![0; arena.nodes.len()],
        open: BTreeSet::new(),
        bytes: 0,
    };
    let mut waiting: BTreeSet<usize> = BTreeSet::new();
    let mut refused: BTreeSet<usize> = BTreeSet::new();
    for (now, &leaf) in arena.leaves.iter().enumerate() {
        let now = now + 1;
        cut.close(leaf, arena.nodes[leaf].bytes());
        while cut.bytes > budget {
            let mut order: Vec<usize> = cut
                .open
                .iter()
                .copied()
                .filter(|&id| arena.nodes[id].span().end < now && !refused.contains(&id))
                .collect();
            order.sort_by(|&a, &b| arena.due(b, now).cmp(&arena.due(a, now)).then(a.cmp(&b)));
            let mut chosen = None;
            let mut unsummarised = Vec::new();
            for id in order {
                refused.insert(id);
                let Some(closing) = cut.closing(id) else {
                    continue;
                };
                let costs: Option<Vec<u64>> = closing.iter().map(|&id| cost(id)).collect();
                match costs {
                    Some(costs)
                        if (1..=closing.len()).all(|upto| {
                            costs[..upto].iter().sum::<u64>()
                                < closing[..upto]
                                    .iter()
                                    .map(|&id| cut.covered[id])
                                    .sum::<u64>()
                        }) =>
                    {
                        chosen = Some(closing.into_iter().zip(costs).collect::<Vec<_>>());
                        break;
                    }
                    Some(_) => {}
                    None => {
                        unsummarised.extend(closing.into_iter().filter(|&id| cost(id).is_none()));
                    }
                }
            }
            for id in unsummarised {
                waiting.extend(ready.range(id..arena.after[id]));
            }
            let Some(chosen) = chosen else { break };
            for (closing, size) in chosen {
                let start = arena.nodes[closing].span().start;
                refused.retain(|&id| arena.nodes[id].span().end <= start);
                cut.close(closing, size);
            }
        }
    }
    View {
        parts: cut
            .parts
            .iter()
            .map(|&id| Part {
                node: arena.nodes[id].name().clone(),
                span: arena.nodes[id].span(),
            })
            .collect(),
        bytes: cut.bytes,
        waiting: waiting
            .iter()
            .filter(|id| cut.open.contains(id))
            .map(|&id| arena.nodes[id].name().clone())
            .collect(),
    }
}

struct Cut<'a, 't, M> {
    arena: &'a Arena<'t, M>,
    parts: Vec<usize>,
    covered: Vec<u64>,
    open: BTreeSet<usize>,
    bytes: u64,
}

impl<M> Cut<'_, '_, M> {
    fn first(&self, id: usize) -> usize {
        let start = self.arena.nodes[id].span().start;
        self.parts
            .partition_point(|&part| self.arena.nodes[part].span().start < start)
    }

    fn closing(&self, id: usize) -> Option<Vec<usize>> {
        let level = self.arena.nodes[id].level();
        let mut closing = vec![id];
        let mut at = self.first(id);
        while at > 0 && self.arena.nodes[self.parts[at - 1]].level() < level {
            let forced = self
                .arena
                .ancestors(self.parts[at - 1])
                .find(|&above| self.arena.nodes[above].level() == level)?;
            closing.push(forced);
            at = self.first(forced);
        }
        Some(closing)
    }

    fn close(&mut self, id: usize, size: u64) {
        let span = self.arena.nodes[id].span();
        let from = self.first(id);
        let to = from
            + self.parts[from..]
                .partition_point(|&part| self.arena.nodes[part].span().start < span.end);
        self.parts.splice(from..to, [id]);
        let was = self.covered[id];
        self.bytes = self.bytes - was + size;
        for above in self.arena.ancestors(id) {
            self.covered[above] = self.covered[above] - was + size;
            self.open.insert(above);
        }
        let inside: Vec<usize> = self.open.range(id..self.arena.after[id]).copied().collect();
        for below in inside {
            self.open.remove(&below);
        }
    }
}

struct Arena<'t, M> {
    nodes: Vec<&'t Node<M>>,
    parent: Vec<Option<usize>>,
    after: Vec<usize>,
    leaves: Vec<usize>,
}

impl<'t, M> Arena<'t, M> {
    fn of(tree: &'t Tree<M>) -> Self {
        let mut arena = Arena {
            nodes: Vec::new(),
            parent: Vec::new(),
            after: Vec::new(),
            leaves: Vec::new(),
        };
        let mut stack = vec![(tree.root(), None)];
        while let Some((node, parent)) = stack.pop() {
            let id = arena.nodes.len();
            arena.nodes.push(node);
            arena.parent.push(parent);
            if node.is_leaf() {
                arena.leaves.push(id);
            }
            stack.extend(node.children().iter().rev().map(|child| (child, Some(id))));
        }
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

    fn ancestors(&self, id: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(self.parent[id], |&id| self.parent[id])
    }

    fn due(&self, id: usize, now: usize) -> Due {
        let span = self.nodes[id].span();
        Due {
            age: now - span.start,
            size: span.len(),
        }
    }
}

#[derive(Clone, Copy)]
struct Due {
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
