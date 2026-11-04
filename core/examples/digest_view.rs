//! How long one view takes over a partly summarised tree.
//!
//! A log of `LINES` lines, each resting on the one before it, alternates a
//! long line (an answer, `LONG` bytes) with a short one (a question), and
//! summaries are written as a compactor writes them: [`pending`]'s nodes,
//! round after round, until `n` are written. Each summary is 90 bytes. This
//! prints, for each `n`, how many parts and bytes the view under `BUDGET`
//! holds and how long [`view`] took, the best of `RUNS`.
//!
//!     cargo run --release -p prodrome-core --example digest_view

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use prodrome::digest::{pending, tree, view, Leaf};
use prodrome::event::Hash;
use prodrome::memo;

const LINES: usize = 15_900;
const LONG: u32 = 16_000;
const SHORT: u32 = 45;
const SUMMARY: u32 = 90;
const BUDGET: u64 = 16 * 1024;
const RUNS: usize = 3;

fn main() {
    let mut log: Vec<Leaf<()>> = (0..LINES)
        .map(|at| Leaf {
            name: memo::name(b"line", [&memo::name(&at.to_be_bytes(), [])]),
            parents: Vec::new(),
            bytes: if at % 2 == 1 { LONG } else { SHORT },
            measure: (),
        })
        .collect();
    for at in 1..LINES {
        log[at].parents = vec![log[at - 1].name.clone()];
    }
    let tree = tree(log).expect("a chain has no cycle").expect("lines");
    let mut summarised: BTreeSet<Hash> = BTreeSet::new();
    for most in [1_000, 2_000, 3_000, 3_500, usize::MAX] {
        while summarised.len() < most {
            let next = pending(&tree, |name| summarised.contains(name).then_some(SUMMARY));
            if next.is_empty() {
                break;
            }
            let room = most - summarised.len();
            summarised.extend(next.into_iter().take(room));
        }
        let summary = |name: &Hash| summarised.contains(name).then_some(SUMMARY);
        let mut best = Duration::MAX;
        let mut shown = None;
        for _ in 0..RUNS {
            let started = Instant::now();
            let now = view(&tree, summary, BUDGET);
            best = best.min(started.elapsed());
            shown = Some(now);
        }
        let shown = shown.expect("at least one run");
        println!(
            "{:>5} summaries: {:>5} parts, {:>8} bytes, {} waiting, in {best:?}",
            summarised.len(),
            shown.parts.len(),
            shown.bytes,
            shown.waiting.len(),
        );
    }
}
