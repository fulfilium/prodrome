//! What one pull in pieces costs on a DAG of 30,000 objects (SPEC laws 47
//! and 64). Run BY HAND, in release (`cargo run --release --example
//! frontier_pull -p prodrome-core`); nothing under `cargo test` runs it.
//!
//! The DAG is the one of the `frontier_answer` example: sixteen writers,
//! each with a chain of its own, and a merge every hundredth object of a
//! writer. The program pulls it twice, from nothing, in pieces of `PIECE`
//! objects, on a DAG that does not change:
//!
//! - `store order`: each piece is the next objects that `lacking()` lists,
//!   recorded with `record`;
//! - `host order`: each piece is the next objects of a linear extension of
//!   the host's own, which takes the chain of the writer with the latest
//!   object first, recorded with `record_with`. Each piece is closed under
//!   parents over what the replica holds, and almost none is a prefix of
//!   the store's order.
//!
//! For each pull it prints the total time of the answers and the records,
//! and the time of the last answer and the last record.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::time::{Duration, Instant};

use prodrome::dag::Dag;
use prodrome::event::{
    mk_created, mk_sealed, mk_woven, parents_of, seal_hash, Envelope, Hash, TodoEvent,
};
use prodrome::literal::Datetime;
use prodrome::reference::Todo;
use prodrome::store::frontier::{Frontiers, Since, KEPT};

type Event = TodoEvent<Todo>;

const OBJECTS: usize = 30_000;
const WRITERS: usize = 16;
const MERGE_EVERY: usize = 100;
const PIECE: usize = 1_000;
const STORE: &str = "notes";

/// The generated objects, each with the number of the step that wrote it:
/// each writer's chain, joined to the next writer's chain at every
/// hundredth object of the writer.
fn objects() -> Vec<(Hash, Envelope<Event>, usize)> {
    let mut heads: Vec<Option<Hash>> = vec![None; WRITERS];
    let mut objects = Vec::with_capacity(OBJECTS);
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
        objects.push((name, object, n));
    }
    objects
}

/// The host's linear extension: Kahn's walk that takes, among the objects
/// ready, the one of the writer whose latest object is the latest, and
/// within a writer the earliest.
fn host_order(dag: &Dag<Event>, steps: &BTreeMap<Hash, usize>) -> Vec<Hash> {
    let rank = |n: usize| (WRITERS - 1 - (OBJECTS - 1 - n % WRITERS) % WRITERS, n);
    let names: Vec<&Hash> = dag.objects().keys().collect();
    let place = |name: &Hash| names.binary_search(&name).expect("held");
    let mut waiting = vec![0_usize; names.len()];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); names.len()];
    for (at, object) in dag.objects().values().enumerate() {
        for parent in parents_of(object) {
            children[place(&parent)].push(at);
            waiting[at] += 1;
        }
    }
    let mut ready: BinaryHeap<Reverse<((usize, usize), usize)>> = (0..names.len())
        .filter(|&at| waiting[at] == 0)
        .map(|at| Reverse((rank(steps[names[at]]), at)))
        .collect();
    let mut order = Vec::with_capacity(names.len());
    while let Some(Reverse((_, at))) = ready.pop() {
        order.push(names[at].clone());
        for &child in &children[at] {
            waiting[child] -= 1;
            if waiting[child] == 0 {
                ready.push(Reverse((rank(steps[names[child]]), child)));
            }
        }
    }
    order
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// A pull's times: the answers and the records, in turn.
struct Pull {
    answers: Vec<Duration>,
    records: Vec<Duration>,
}

impl Pull {
    fn print(&self, what: &str) {
        let total: Duration = self.answers.iter().chain(&self.records).sum();
        println!(
            "{what}: {} pieces, total {:.1} ms; last answer {:.2} ms, last record {:.2} ms",
            self.records.len(),
            ms(total),
            ms(self.answers[self.records.len() - 1]),
            ms(self.records[self.records.len() - 1]),
        );
    }
}

/// One pull from nothing. `piece(k, lacking)` records the `k`th piece.
fn pull(
    dag: &Dag<Event>,
    mut piece: impl FnMut(usize, prodrome::store::frontier::Lacking<'_, Event>) -> Since,
) -> Pull {
    let frontiers = Frontiers::remembered(KEPT);
    let mut since = Since::Nothing;
    let (mut answers, mut records) = (Vec::new(), Vec::new());
    for k in 0..OBJECTS.div_ceil(PIECE) {
        let start = Instant::now();
        let lacking = frontiers
            .answer(STORE, &since, dag)
            .expect("the DAG has a linear order");
        answers.push(start.elapsed());
        let start = Instant::now();
        since = piece(k, lacking);
        records.push(start.elapsed());
    }
    let lacking = frontiers.answer(STORE, &since, dag).expect("answers");
    assert!(
        lacking.lacking().is_empty(),
        "the pull ends with every object"
    );
    Pull { answers, records }
}

fn main() {
    let generated = objects();
    let steps: BTreeMap<Hash, usize> = generated.iter().map(|(h, _, n)| (h.clone(), *n)).collect();
    let dag: Dag<Event> = generated.into_iter().map(|(h, o, _)| (h, o)).collect();
    dag.linearise().expect("orders");
    let host = host_order(&dag, &steps);

    let store_order = pull(&dag, |_, lacking| {
        Since::Frontier(lacking.record(PIECE).expect("records").token)
    });
    let host_order = pull(&dag, |k, lacking| {
        let piece = host[k * PIECE..((k + 1) * PIECE).min(OBJECTS)]
            .iter()
            .cloned();
        Since::Frontier(lacking.record_with(piece).expect("records").token)
    });
    println!("{OBJECTS} objects, {WRITERS} writers, pieces of {PIECE}");
    store_order.print("store order");
    host_order.print("host order ");
}
