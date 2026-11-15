//! What an answer to a pull by a frontier token costs on a DAG of 30,000
//! objects (SPEC law 47). Run BY HAND, in release (`cargo run --release
//! --example frontier_answer -p prodrome-core`); nothing under `cargo test`
//! runs it.
//!
//! The DAG is generated. Sixteen writers each append to a chain of their
//! own. Every hundredth object of a writer is a `Woven` object that joins
//! the writer's chain to the head of the next writer's chain, so the DAG is
//! long chains with occasional merges.
//!
//! The program prints the median of `ROUNDS` runs of four answers:
//!
//! - `cold, from nothing`: the first answer on a DAG that was just read;
//! - `again, from nothing`: an answer on the same DAG, which did not change;
//! - `again, last piece`: an answer from a token whose frontier lacks the
//!   last 1,000 objects of the linear order, on the same DAG;
//! - `again, quiet`: an answer from a token at the DAG's tips.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use prodrome::dag::Dag;
use prodrome::event::{mk_created, mk_sealed, mk_woven, seal_hash, Envelope, Hash, TodoEvent};
use prodrome::literal::Datetime;
use prodrome::reference::Todo;
use prodrome::store::frontier::{Frontiers, Since, KEPT};

type Event = TodoEvent<Todo>;
type Objects = BTreeMap<Hash, Envelope<Event>>;

const OBJECTS: usize = 30_000;
const WRITERS: usize = 16;
const MERGE_EVERY: usize = 100;
const LAST_PIECE: usize = 1_000;
const ROUNDS: usize = 15;
const STORE: &str = "notes";

/// The generated objects: each writer's chain, joined to the next writer's
/// chain at every hundredth object of the writer.
fn objects() -> Objects {
    let mut heads: Vec<Option<Hash>> = vec![None; WRITERS];
    let mut objects = Objects::new();
    for n in 0..OBJECTS {
        let writer = n % WRITERS;
        let step = n / WRITERS;
        let day = u32::try_from(1 + step % 28).expect("a day of the month");
        let at = Datetime::new(2026, 9, day, 12, 0, 0, 0).expect("a real instant");
        let event: Event =
            mk_created(&format!("todo-{n}"), at, "ada", "", "").expect("a valid event");
        let other = heads[(writer + 1) % WRITERS].clone();
        let object = match (heads[writer].clone(), other) {
            (Some(own), Some(other)) if step % MERGE_EVERY == MERGE_EVERY - 1 => {
                mk_woven(vec![own, other], Some(event)).expect("two distinct parents")
            }
            (own, _) => mk_sealed(own, event),
        };
        let name = seal_hash(&object);
        heads[writer] = Some(name.clone());
        objects.insert(name, object);
    }
    objects
}

fn median(mut runs: Vec<Duration>) -> Duration {
    runs.sort();
    runs[runs.len() / 2]
}

fn main() {
    let objects = objects();
    let frontiers = Frontiers::remembered(KEPT);
    let dag: Dag<Event> = objects.clone().into_iter().collect();
    let lacking = frontiers
        .answer(STORE, &Since::Nothing, &dag)
        .expect("the DAG has a linear order");
    let piece = Since::Frontier(
        lacking
            .record(OBJECTS - LAST_PIECE)
            .expect("memory records")
            .token,
    );
    let quiet = Since::Frontier(
        frontiers
            .answer(STORE, &Since::Nothing, &dag)
            .expect("the DAG has a linear order")
            .record(OBJECTS)
            .expect("memory records")
            .token,
    );

    let cold = median(
        (0..ROUNDS)
            .map(|_| {
                let fresh: Dag<Event> = objects.clone().into_iter().collect();
                let start = Instant::now();
                let lacking = frontiers
                    .answer(STORE, &Since::Nothing, &fresh)
                    .expect("the DAG has a linear order");
                let took = start.elapsed();
                assert_eq!(lacking.lacking().len(), OBJECTS);
                took
            })
            .collect(),
    );
    let again = |since: &Since, lacks: usize| {
        median(
            (0..ROUNDS)
                .map(|_| {
                    let start = Instant::now();
                    let lacking = frontiers
                        .answer(STORE, since, &dag)
                        .expect("the DAG has a linear order");
                    let took = start.elapsed();
                    assert_eq!(lacking.lacking().len(), lacks);
                    took
                })
                .collect(),
        )
    };
    println!("{OBJECTS} objects, {WRITERS} writers, a merge every {MERGE_EVERY} objects of each");
    println!("cold, from nothing:  {cold:?}");
    println!("again, from nothing: {:?}", again(&Since::Nothing, OBJECTS));
    println!("again, last piece:   {:?}", again(&piece, LAST_PIECE));
    println!("again, quiet:        {:?}", again(&quiet, 0));
}
