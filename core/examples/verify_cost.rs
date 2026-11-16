//! What `Dag::verify` costs on long histories of one entity. Run BY HAND,
//! in release (`cargo run --release --example verify_cost -p prodrome-core`);
//! nothing under `cargo test` runs it.
//!
//! The program generates two shapes of DAG in memory, each of 10,000,
//! 20,000 and 40,000 changes to one todo after one genesis:
//!
//! - a CHAIN, where each change has one dep, the change before it;
//! - a chain of BRANCHES, where every tenth change has three deps. The
//!   three changes before it each rest on the change before them, and the
//!   tenth change rests on all three.
//!
//! For each DAG it prints the median time of `verify` over `ROUNDS` runs.
//! Each run verifies a fresh copy of the DAG, so the time includes the
//! derivation of the linear order.

use std::time::{Duration, Instant};

use prodrome::change::mk_change;
use prodrome::dag::Dag;
use prodrome::event::{mk_created, seal_hash, Envelope, Hash, TodoEvent};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::reference::Todo;

type Event = TodoEvent<Todo>;

const SIZES: [usize; 3] = [10_000, 20_000, 40_000];
const ROUNDS: usize = 5;

/// How the deps of each change are chosen.
#[derive(Debug, Clone, Copy)]
enum Shape {
    Chain,
    Branches,
}

/// The `n`th change's instant: one second after the change before it.
fn instant(n: usize) -> Datetime {
    let second = u32::try_from(n).expect("a size under a day of seconds");
    Datetime::new(2026, 9, 1, second / 3600, second / 60 % 60, second % 60, 0)
        .expect("a real instant")
}

/// One genesis and `size` changes to the todo `alpha`, in `shape`.
fn generate(shape: Shape, size: usize) -> Dag<Event> {
    let genesis = Envelope::Genesis(mk_genesis("home", &"0".repeat(32)).expect("a valid nonce"));
    let root = seal_hash(&genesis);
    let mut objects = vec![(root.clone(), genesis)];
    let mut names: Vec<Hash> = Vec::with_capacity(size);
    for n in 0..size {
        let deps: Vec<Hash> = match (shape, n % 10, n) {
            (_, _, 0) => Vec::new(),
            (Shape::Branches, 8 | 9, _) => vec![names[n - (n % 10) + 6].clone()],
            (Shape::Branches, 0, _) => names[n - 3..n].to_vec(),
            (Shape::Chain | Shape::Branches, _, _) => vec![names[n - 1].clone()],
        };
        let event =
            mk_created("alpha", instant(n), "ada", &format!("t{n}"), "").expect("a valid event");
        let change = Envelope::Change(mk_change(root.clone(), deps, event).expect("distinct deps"));
        let name = seal_hash(&change);
        names.push(name.clone());
        objects.push((name, change));
    }
    objects.into_iter().collect()
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

fn main() {
    for shape in [Shape::Chain, Shape::Branches] {
        for size in SIZES {
            let dag = generate(shape, size);
            let mut findings = 0;
            let times = (0..ROUNDS)
                .map(|_| {
                    let fresh: Dag<Event> = dag.objects().clone().into_iter().collect();
                    let start = Instant::now();
                    findings = fresh.verify(&Everything).len();
                    start.elapsed()
                })
                .collect();
            println!(
                "{shape:?} of {size}: verify {:.1} ms, {findings} findings",
                median(times).as_secs_f64() * 1000.0
            );
        }
    }
}
