//! How much of a digest's printed view stays a prefix of the next view, with
//! one water and with two.
//!
//! A model reading a view is prompted with it printed, one line a part, and
//! its prompt cache serves only the longest prefix that it saw before. So
//! what a line costs a reader is the view's printed bytes after the first
//! part that the line changed.
//!
//! The example generates a log of several writers that write in turn. Each
//! line rests on the line before it and on its writer's previous line, so
//! each new line comes last in the log. A line costs 40 to `LONGEST` bytes,
//! for each `LONGEST` in `LONGEST_LINES`. A compactor keeps up: every settled node has a summary of 200 to 800
//! bytes. For logs of `LINES` lines, under a high water of `HIGH` bytes and
//! a low water of `HIGH` or `HIGH / 2`, the example prints, over the last
//! `ARRIVALS` arrivals:
//!
//! - at how many arrivals a part before the last changed;
//! - how many bytes of the previous view stayed a prefix of the next, on
//!   average, and the previous view's average size;
//! - the time of one `view` call over the whole log: the median of `ROUNDS`
//!   medians of `RUNS` runs each.
//!
//!     cargo run --release -p prodrome-core --example digest_prefix

use std::time::{Duration, Instant};

use prodrome::digest::{tree, view, Leaf, Node, Part, Tree, View, Waters};
use prodrome::event::Hash;
use prodrome::memo;

const LINES: [usize; 2] = [2_000, 13_000];
const LONGEST_LINES: [u32; 2] = [2_000, 400];
const WRITERS: usize = 4;
const HIGH: u64 = 131_072;
const ARRIVALS: usize = 500;
const ROUNDS: usize = 5;
const RUNS: usize = 5;

/// A number in `low..=high` drawn from a name.
fn draw(name: &Hash, salt: &[u8], low: u32, high: u32) -> u32 {
    let hash = memo::name(salt, [name]);
    let value = u32::from_str_radix(&hash.as_str()[..8], 16).expect("hex");
    low + value % (high - low + 1)
}

fn summary(name: &Hash) -> Option<u32> {
    Some(draw(name, b"summary", 200, 800))
}

/// What a part costs, printed as one line, when a line costs at most
/// `longest` bytes.
fn printed(part: &Part, longest: u32) -> u64 {
    if part.span.len() == 1 {
        u64::from(draw(&part.node, b"line", 40, longest))
    } else {
        u64::from(summary(&part.node).expect("every node has a summary"))
    }
}

/// The bytes of `before` printed up to its first part that `after` does
/// not print in the same place.
fn kept(before: &View, after: &View, longest: u32) -> u64 {
    before
        .parts
        .iter()
        .zip(&after.parts)
        .take_while(|(old, new)| old == new)
        .map(|(part, _)| printed(part, longest))
        .sum()
}

/// A log of `lines` lines of at most `longest` bytes, over `WRITERS`
/// writers who write in turn.
fn log(lines: usize, longest: u32) -> Vec<Leaf<()>> {
    let mut log: Vec<Leaf<()>> = Vec::new();
    let mut last_of: Vec<Option<Hash>> = vec![None; WRITERS];
    for at in 0..lines {
        let name = memo::name(b"line", [&memo::name(&at.to_be_bytes(), [])]);
        let writer = draw(&name, b"writer", 0, WRITERS as u32 - 1) as usize;
        let mut parents: Vec<Hash> = log
            .last()
            .map(|line| line.name.clone())
            .into_iter()
            .collect();
        parents.extend(last_of[writer].replace(name.clone()));
        log.push(Leaf {
            bytes: draw(&name, b"line", 40, longest),
            parents,
            name,
            measure: (),
        });
    }
    log
}

fn the_tree(log: &[Leaf<()>]) -> Tree<()> {
    tree(log.to_vec()).expect("no cycle").expect("lines")
}

/// The median time of one view of `tree` under `waters`.
fn timed(tree: &Tree<()>, waters: Waters) -> Duration {
    let median = |mut times: Vec<Duration>| {
        times.sort();
        times[times.len() / 2]
    };
    median(
        (0..ROUNDS)
            .map(|_| {
                median(
                    (0..RUNS)
                        .map(|_| {
                            let started = Instant::now();
                            std::hint::black_box(view(tree, Node::bytes, summary, waters));
                            started.elapsed()
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

fn main() {
    for (lines, longest) in LINES
        .into_iter()
        .flat_map(|lines| LONGEST_LINES.map(|longest| (lines, longest)))
    {
        let log = log(lines, longest);
        let trees: Vec<Tree<()>> = (lines - ARRIVALS..=lines)
            .map(|length| the_tree(&log[..length]))
            .collect();
        for low in [HIGH, HIGH / 2] {
            let waters = Waters::new(HIGH, low).expect("the low water is under the high");
            let views: Vec<View> = trees
                .iter()
                .map(|tree| view(tree, Node::bytes, summary, waters))
                .collect();
            let (mut changed, mut prefix, mut size) = (0, 0, 0);
            for pair in views.windows(2) {
                let (before, after) = (&pair[0], &pair[1]);
                if after.parts.get(..before.parts.len()) != Some(&before.parts[..]) {
                    changed += 1;
                }
                prefix += kept(before, after, longest);
                size += before.bytes;
            }
            let time = timed(trees.last().expect("a tree"), waters);
            println!(
                "{lines:>6} lines of 40 to {longest:>4} bytes, low water {low:>6}: a part before the last changed at \
                 {changed} of {ARRIVALS} arrivals; prefix kept {} of {} bytes on average; \
                 one view in {time:?}",
                prefix / ARRIVALS as u64,
                size / ARRIVALS as u64,
            );
        }
    }
}
