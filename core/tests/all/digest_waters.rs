//! SPEC laws 88 to 91: a digest's view under two waters.
//!
//! Over generated logs, summaries on a random share of the nodes, costs of
//! the lines that are a line's bytes or a short placeholder, and two
//! waters: equal waters give law 46's view under one budget; an arrival
//! that does not put the view over its high water adds one part at the end
//! and changes no other part; over a run of appended lines, the arrivals
//! that change a part before the last are bounded by the run's bytes over
//! the difference of the waters; and a summary recorded late changes no
//! view until the line after its node arrives, and then can change parts
//! on both sides of its node.

use std::collections::BTreeSet;

use crate::digest::{
    a_budget, a_log, line_costs, line_name, nodes, some_waters, summaries, tree_of, view, Ends,
    Line, LineCosts, Summaries,
};
use prodrome::digest::{self, Node, Part, Tree, View, Waters};
use prodrome::event::Hash;
use proptest::prelude::*;

/// A line that has seen every line of `log`, so that it comes last.
fn last_line(log: &[Line], bytes: u32) -> Line {
    let ids: BTreeSet<u16> = log.iter().map(|line| line.id).collect();
    let id = (0..=u16::MAX)
        .find(|id| !ids.contains(id))
        .expect("a free id");
    Line {
        id,
        bytes,
        parents: ids.into_iter().collect(),
    }
}

/// The view of `tree` under `lines`, `summaries` and `waters`.
fn view_under(tree: &Tree<Ends>, lines: LineCosts, summaries: Summaries, waters: Waters) -> View {
    digest::view(
        tree,
        |node: &Node<Ends>| lines.of(node),
        |name| summaries.of(name),
        waters,
    )
}

/// Whether `after` is `before` with one more part at the end.
fn one_more_at_the_end(before: &View, after: &View) -> bool {
    after.parts.len() == before.parts.len() + 1 && after.parts[..before.parts.len()] == before.parts
}

/// Two waters that leave room between them, so that a merge closes many
/// nodes at once: a high water under the cost of most generated logs, and
/// a low water at or under it.
fn wide_waters() -> impl Strategy<Value = Waters> {
    (1..3000u64, 0..=100u64).prop_map(|(high, percent)| {
        Waters::new(high, high * percent / 100).expect("the low water is under the high")
    })
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// Law 88: under equal waters, the view is law 46's view under one
    /// budget, which after each arrival closes while it is over the budget.
    #[test]
    fn equal_waters_are_the_view_under_one_budget(
        log in a_log(300),
        summaries in summaries(),
        lines in line_costs(),
        budget in a_budget(),
    ) {
        let tree = tree_of(&log);
        let line = |node: &Node<Ends>| lines.of(node);
        let drawn = |name: &Hash| summaries.of(name);
        prop_assert_eq!(
            digest::view(&tree, line, drawn, Waters::at(budget)),
            crate::digest_reference::one_budget(&tree, line, drawn, budget)
        );
    }

    /// Law 89: when an appended line does not put the view over its high
    /// water, the new view is the old view and one more part at the end,
    /// the line itself, and it costs the old view's cost and the line's.
    /// No part grows, because a part is a node, and a closing makes a part
    /// only at a settled node, under a merge.
    #[test]
    fn between_merges_an_arrival_adds_one_part_at_the_end(
        log in a_log(300),
        summaries in summaries(),
        lines in line_costs(),
        waters in some_waters(),
        bytes in 1..=64u32,
    ) {
        let last = last_line(&log, bytes);
        let (name, at) = (line_name(last.id), log.len());
        let before = tree_of(&log);
        let after = tree_of(&[log, vec![last]].concat());
        let (old, new) = (
            view_under(&before, lines, summaries, waters),
            view_under(&after, lines, summaries, waters),
        );
        let line = after.node(&name).expect("the new line is in the tree");
        let cost = lines.of(line);
        if old.bytes + cost <= waters.high() {
            prop_assert!(one_more_at_the_end(&old, &new), "a part before the last changed");
            prop_assert_eq!(new.parts.last(), Some(&Part { node: name, span: at..at + 1 }));
            prop_assert_eq!(new.bytes, old.bytes + cost);
        }
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 90: over a run of appended lines, let a MERGE be an arrival at
    /// which a part before the last changes, and a merge that STOPS HIGH
    /// one after which the view costs more than the low water. Then the
    /// merges are at most `⌊R / (high − low)⌋ + 1 + S`, for `R` the bytes
    /// of the run's lines and `S` the merges that stop high. Proof: after
    /// a merge that ends at or under the low water, the next merge comes
    /// only when the arrivals after it, the next merge's included, add
    /// more than `high − low` bytes, and these stretches of arrivals do not
    /// overlap. This is a law over sequences: run it once at 20,000 cases
    /// before a change to the view.
    #[test]
    fn merges_are_rare(
        log in a_log(150),
        run in prop::collection::vec(1..=64u32, 1..80),
        summaries in summaries(),
        lines in line_costs(),
        waters in wide_waters(),
    ) {
        let mut log = log;
        let mut previous = view_under(&tree_of(&log), lines, summaries, waters);
        let (mut merges, mut high, mut bytes) = (0u64, 0u64, 0u64);
        for line in run {
            let last = last_line(&log, line);
            let name = line_name(last.id);
            log.push(last);
            let tree = tree_of(&log);
            let next = view_under(&tree, lines, summaries, waters);
            bytes += lines.of(tree.node(&name).expect("the new line is in the tree"));
            if !one_more_at_the_end(&previous, &next) {
                merges += 1;
                if next.bytes > waters.low() {
                    high += 1;
                }
            }
            previous = next;
        }
        // Equal waters leave no room, and the law bounds nothing.
        let room = waters.high() - waters.low();
        if let Some(rare) = bytes.checked_div(room) {
            prop_assert!(
                merges <= rare + 1 + high,
                "{} merges, {} stopped high, {} bytes, room {}", merges, high, bytes, room
            );
        }
    }

    /// Law 91: a summary recorded for a settled node `x` changes no view of
    /// a prefix of the log that ends at or before `x`'s last line: the
    /// replay reads `x`'s summary only once the line after `x` arrives.
    #[test]
    fn a_late_summary_changes_no_view_before_its_node_settles(
        log in a_log(120),
        summaries in summaries(),
        lines in line_costs(),
        waters in some_waters(),
        pick in any::<prop::sample::Index>(),
    ) {
        let tree = tree_of(&log);
        let settled: Vec<&Node<Ends>> = nodes(&tree)
            .into_iter()
            .filter(|node| !node.is_leaf() && tree.settled(node))
            .collect();
        // A log of a few lines has no settled inner node to summarise.
        let Some(&x) = settled.get(pick.index(settled.len().max(1))) else {
            return Ok(());
        };
        let without = |name: &Hash| if name == x.name() { None } else { summaries.of(name) };
        let with = |name: &Hash| if name == x.name() { Some(1) } else { summaries.of(name) };
        let line = |node: &Node<Ends>| lines.of(node);
        // The leaves in the log's order: a prefix of them is a prefix of
        // the log, and the tree over it is the tree over those lines.
        let order: Vec<Hash> = nodes(&tree)
            .into_iter()
            .filter(|node| node.is_leaf())
            .map(|node| node.name().clone())
            .collect();
        for end in 1..=x.span().end {
            let kept: BTreeSet<&Hash> = order[..end].iter().collect();
            let prefix: Vec<Line> = log
                .iter()
                .filter(|line| kept.contains(&line_name(line.id)))
                .cloned()
                .collect();
            let tree = tree_of(&prefix);
            prop_assert_eq!(
                digest::view(&tree, line, without, waters),
                digest::view(&tree, line, with, waters),
                "the view of the first {} lines", end
            );
        }
    }
}

/// Law 91, the smallest example of a late summary that changes a part
/// before its node: twelve lines in a chain, whose tree has two settled
/// chunks, `a` over lines 0 to 5 and `x` over lines 6 to 10, and the last
/// line. The summary of `a` costs more than `a`'s lines, so `a` alone never
/// closes, and `x` has no summary. Under a budget of nothing, nothing
/// closes. When `x`'s summary arrives, the closing at `x` forces `a` beside
/// it (decay), and every prefix of that closing lowers the cost: so the
/// lines 0 to 5, before `x`, become one part.
#[test]
fn a_late_summary_closes_a_part_before_its_node() {
    let costs = [1, 10, 10, 10, 1, 1, 28, 28, 10, 19, 19, 1];
    let log: Vec<Line> = (0..12u16)
        .map(|id| Line {
            id,
            bytes: costs[usize::from(id)],
            parents: id.checked_sub(1).into_iter().collect(),
        })
        .collect();
    let tree = tree_of(&log);
    let level_one: Vec<&Node<Ends>> = nodes(&tree)
        .into_iter()
        .filter(|node| node.level() == 1)
        .collect();
    let spans: Vec<std::ops::Range<usize>> = level_one.iter().map(|node| node.span()).collect();
    assert_eq!(spans, vec![0..6, 6..11, 11..12], "the tree's chunks");
    let (a, x) = (level_one[0].name().clone(), level_one[1].name().clone());
    assert_eq!(level_one[0].bytes(), 33, "a's lines");
    let without = |name: &Hash| (*name == a).then_some(40);
    let with = |name: &Hash| if *name == x { Some(1) } else { without(name) };
    let spans_of = |view: View| -> Vec<std::ops::Range<usize>> {
        view.parts.into_iter().map(|part| part.span).collect()
    };
    let old = spans_of(view(&tree, without, Waters::at(0)));
    let new = spans_of(view(&tree, with, Waters::at(0)));
    assert_eq!(old, (0..12).map(|at| at..at + 1).collect::<Vec<_>>());
    assert_eq!(new, vec![0..6, 6..11, 11..12]);
}

/// Law 91, the other way: a late summary can also leave a part before its
/// node finer. Once `x` closes, the view is lower, so a later merge that
/// closed an older, coarser node is no longer needed, and that node's
/// lines show in smaller parts. Some log of a few hundred lines shows it.
#[test]
fn a_late_summary_can_open_a_part_before_its_node() {
    let mut state = 12_345u64;
    let mut next = |modulo: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % modulo
    };
    for _ in 0..3000 {
        let n = 20 + u16::try_from(next(200)).expect("small");
        let log: Vec<Line> = (0..n)
            .map(|id| Line {
                id,
                bytes: 1 + u32::try_from(next(64)).expect("small"),
                parents: id.checked_sub(1).into_iter().collect(),
            })
            .collect();
        let tree = tree_of(&log);
        let summaries = Summaries {
            seed: next(1 << 40),
            percent: next(101),
            longest: 1 + next(96),
        };
        let unsummarised: Vec<&Node<Ends>> = nodes(&tree)
            .into_iter()
            .filter(|node| {
                !node.is_leaf() && tree.settled(node) && summaries.of(node.name()).is_none()
            })
            .collect();
        if unsummarised.is_empty() {
            continue;
        }
        let x = unsummarised[usize::try_from(next(unsummarised.len() as u64)).expect("small")];
        let with = |name: &Hash| {
            if name == x.name() {
                Some(1)
            } else {
                summaries.of(name)
            }
        };
        let waters = Waters::at(next(3000));
        let old = view(&tree, |name| summaries.of(name), waters);
        let new = view(&tree, with, waters);
        let opened = old
            .parts
            .iter()
            .filter(|part| part.span.end <= x.span().start)
            .any(|part| {
                !new.parts.iter().any(|wider| {
                    wider.span.start <= part.span.start && part.span.end <= wider.span.end
                })
            });
        if opened {
            return;
        }
    }
    panic!("no log showed a part before a late summary's node that opened");
}
