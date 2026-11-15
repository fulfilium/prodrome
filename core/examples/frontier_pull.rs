//! What one pull in pieces costs on a DAG of 30,000 objects, and where the
//! time of an answer goes. Run BY HAND, in release (`cargo run --release
//! --example frontier_pull -p prodrome-core`); nothing under `cargo test`
//! runs it.
//!
//! The DAG is the one of the `frontier_answer` example. The program prints:
//!
//! - the time of `Dag::tips`, first and once kept;
//! - the time of one pull from nothing in pieces of `PIECE` objects, on a
//!   DAG that does not change;
//! - for the last piece, the time of the closure of the token's tips, of
//!   the filter of the order by that closure, and of a clone of it;
//! - a prototype of the reading of a prefix off the kept order (design
//!   note `docs/design-frontier-reach.md`): the time to derive its facts
//!   once, and the time of one answer's check and its record's tips.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use prodrome::dag::Dag;
use prodrome::event::{
    mk_created, mk_sealed, mk_woven, parents_of, seal_hash, Envelope, Hash, TodoEvent,
};
use prodrome::literal::Datetime;
use prodrome::reference::Todo;
use prodrome::store::frontier::{Frontiers, Since, KEPT};

type Event = TodoEvent<Todo>;
type Objects = BTreeMap<Hash, Envelope<Event>>;

const OBJECTS: usize = 30_000;
const WRITERS: usize = 16;
const MERGE_EVERY: usize = 100;
const PIECE: usize = 1_000;
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

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn main() {
    let objects = objects();
    let dag: Dag<Event> = objects.into_iter().collect();
    let order = dag.linearise().expect("orders").to_vec();
    let frontiers = Frontiers::remembered(KEPT);
    let t = Instant::now();
    let _ = dag.tips();
    let first_tips = t.elapsed();
    let t = Instant::now();
    for _ in 0..1000 {
        let _ = dag.tips();
    }
    let tips_clone = t.elapsed() / 1000;
    println!(
        "dag.tips(): first {:.3} ms, kept clone {:.2} us",
        ms(first_tips),
        tips_clone.as_secs_f64() * 1e6
    );

    // One pull in pieces of PIECE, on a DAG that does not change.
    let mut since = Since::Nothing;
    let (mut answers, mut records) = (Vec::new(), Vec::new());
    loop {
        let t = Instant::now();
        let lacking = frontiers.answer(STORE, &since, &dag).expect("answers");
        answers.push(t.elapsed());
        if lacking.lacking().is_empty() {
            break;
        }
        let t = Instant::now();
        let recorded = lacking.record(PIECE).expect("records");
        records.push(t.elapsed());
        since = Since::Frontier(recorded.token);
    }
    let total: Duration = answers.iter().chain(&records).sum();
    println!("pull of {} pieces: total {:.1} ms; answer first {:.2} ms, last {:.2} ms; record first {:.2} ms, last {:.2} ms",
        records.len(), ms(total), ms(answers[0]), ms(answers[answers.len()-2]), ms(records[0]), ms(records[records.len()-1]));

    // Where the last piece's answer goes: the token's tips are those of
    // the prefix of OBJECTS - PIECE objects.
    let n = OBJECTS - PIECE;
    let named: BTreeSet<Hash> = order[..n]
        .iter()
        .flat_map(|x| parents_of(dag.get(x).expect("held")))
        .collect();
    let tips: BTreeSet<Hash> = order[..n]
        .iter()
        .filter(|x| !named.contains(*x))
        .cloned()
        .collect();
    let t = Instant::now();
    let reached = dag.closure(tips.clone());
    let closure = t.elapsed();
    assert_eq!(reached.len(), n);
    let t = Instant::now();
    let outside: Vec<&Hash> = order.iter().filter(|x| !reached.contains(*x)).collect();
    let filter = t.elapsed();
    assert_eq!(outside.len(), PIECE);
    let t = Instant::now();
    let held = reached.clone();
    let clone = t.elapsed();
    drop(held);
    println!(
        "last piece: closure {:.2} ms, filter of the order {:.2} ms, clone of the reach {:.2} ms",
        ms(closure),
        ms(filter),
        ms(clone)
    );

    // The prefix reading: kept facts of the order, built once.
    let t = Instant::now();
    let pos: BTreeMap<&Hash, usize> = order.iter().enumerate().map(|(i, h)| (h, i)).collect();
    let mut first_child = vec![usize::MAX; order.len()];
    for (i, name) in order.iter().enumerate() {
        for parent in parents_of(dag.get(name).expect("held")) {
            let p = pos[&parent];
            first_child[p] = first_child[p].min(i);
        }
    }
    let mut tip_count = vec![0usize; order.len() + 1];
    let mut count = 0usize;
    for (i, name) in order.iter().enumerate() {
        count += 1;
        let parents: BTreeSet<Hash> = parents_of(dag.get(name).expect("held"))
            .into_iter()
            .collect();
        for parent in parents {
            if first_child[pos[&parent]] == i {
                count -= 1;
            }
        }
        tip_count[i + 1] = count;
    }
    let build = t.elapsed();
    let t = Instant::now();
    let mut answered = 0;
    for _ in 0..1000 {
        let n = tips.iter().map(|t| pos[t]).max().map_or(0, |p| p + 1);
        let is_prefix = tips.len() == tip_count[n] && tips.iter().all(|t| first_child[pos[t]] >= n);
        assert!(is_prefix);
        let m = n + PIECE;
        let next: BTreeSet<&Hash> = tips
            .iter()
            .filter(|t| first_child[pos[*t]] >= m)
            .chain(
                order[n..m.min(order.len())]
                    .iter()
                    .filter(|x| first_child[pos[*x]] >= m),
            )
            .collect();
        answered += order[n..].len() + next.len();
    }
    let per = t.elapsed() / 1000;
    println!("prefix reading: built once in {:.2} ms; an answer and its record's tips {:.1} us ({answered})", ms(build), per.as_secs_f64()*1e6);
}
