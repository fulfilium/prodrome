//! What husks cost a store on disk (SPEC §3), on generated data. Run BY
//! HAND, in release (`cargo run --release --example husk_cost -p
//! prodrome-core`); nothing under `cargo test` runs it.
//!
//! The program writes two stores of `OBJECTS` objects each, from generated
//! events:
//!
//! - a todo store (the reference schema, which has no husks): one
//!   `Created` for each todo;
//! - a letter store (the letter schema of the tests, which has husks): each
//!   letter written in turn, every body with a part sealed under one of
//!   three keys, so that every change can be husked.
//!
//! For each store it prints the median of `ROUNDS` of a cold read (a fresh
//! handle's `dag`), a warm look (`tips` on a handle that has read
//! everything), and `verify`, and the bytes of `objects/`. For the letter
//! store it then husks a third of the changes and prints the time of that
//! `husk` and the same rows again. It also prints the two parts of a warm
//! look: the listing of `objects/`, and the listing with a `stat` of each
//! file. Last, it appends `RUN` new events to the todo store as one run
//! (`append_all`), and `ONE_BY_ONE` more with one `append` each.

#[path = "../tests/schemas/letter.rs"]
#[allow(dead_code)]
mod letter;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use prodrome::event::{
    canonical_envelope, mk_created, seal_hash, Actor, Envelope, Hash, TodoEvent,
};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::policy::{Policy, Untrusted};
use prodrome::reference::Todo;
use prodrome::schema::Schema;
use prodrome::store::{EventStore, MemoryStore, Replica};

use letter::{Id, Letter, Part, Says};

const OBJECTS: usize = 30_000;
const ROUNDS: usize = 5;
const RUN: usize = 10_000;
const ONE_BY_ONE: usize = 1_000;

fn at(n: usize) -> Datetime {
    let n = u32::try_from(n).expect("a small count");
    Datetime::new(2026, 1 + n % 12, 1 + n % 28, n % 24, n % 60, 0, 0).expect("an instant")
}

fn todos() -> Vec<TodoEvent<Todo>> {
    (1..OBJECTS)
        .map(|n| mk_created(&format!("todo-{n}"), at(n), "ada", "a todo", "").expect("an event"))
        .collect()
}

fn letters() -> Vec<Letter> {
    (1..OBJECTS)
        .map(|n| Letter {
            letter: Id(format!("letter-{}", n % 1_000)),
            at: at(n),
            actor: Actor::new("ada").expect("an actor"),
            says: Says::Wrote(vec![
                Part::Clear(format!("line {n}")),
                Part::Boxed {
                    key: format!("key-{}", n % 3),
                    sealed: format!("{n:064x}"),
                },
            ]),
        })
        .collect()
}

/// The objects of a genesis and `events`, appended in memory, written as
/// files into `root/objects/`.
fn write<E: Schema<Vocabulary: Default>>(root: &Path, events: Vec<E>) {
    let memory = MemoryStore::<E>::default();
    let genesis: Envelope<E> =
        Envelope::Genesis(mk_genesis("cost", &"0".repeat(32)).expect("a genesis"));
    let name = seal_hash(&genesis);
    let print = canonical_envelope(&genesis).into_bytes();
    memory
        .receive([name].into(), &|_| Some(print.clone()))
        .expect("begins");
    for event in events {
        memory.append(event).expect("appends");
    }
    let objects = root.join("objects");
    fs::create_dir_all(&objects).expect("creates objects/");
    for name in memory.held().expect("reads").dag.objects().keys() {
        let print = memory.print(name).expect("a whole");
        fs::write(objects.join(format!("{}.py", name.as_str())), print).expect("writes");
    }
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

fn timed(run: impl Fn()) -> Duration {
    median(
        (0..ROUNDS)
            .map(|_| {
                let start = Instant::now();
                run();
                start.elapsed()
            })
            .collect(),
    )
}

fn bytes(root: &Path) -> u64 {
    fs::read_dir(root.join("objects"))
        .expect("lists")
        .map(|entry| entry.expect("an entry").metadata().expect("a stat").len())
        .sum()
}

/// The rows of one store: a cold read, a warm look, `verify`, and its bytes.
fn rows<E: Schema<Vocabulary: Default>>(label: &str, root: &Path)
where
    Untrusted: Policy<E>,
{
    let cold = timed(|| {
        let store = EventStore::<E>::new(root, Untrusted::none());
        store.dag().expect("reads");
    });
    let warm_store = EventStore::<E>::new(root, Untrusted::none());
    warm_store.dag().expect("reads");
    let warm = timed(|| {
        warm_store.tips().expect("reads");
    });
    let verify = timed(|| {
        let findings = EventStore::<E>::new(root, Untrusted::none()).verify();
        assert!(findings.is_empty(), "{findings:?}");
    });
    println!(
        "{label:<28} cold read {:>8.1} ms   warm look {:>7.1} ms   verify {:>8.1} ms   objects/ {:>6.2} MB",
        cold.as_secs_f64() * 1e3,
        warm.as_secs_f64() * 1e3,
        verify.as_secs_f64() * 1e3,
        bytes(root) as f64 / 1e6,
    );
}

/// A warm look's two parts: the listing alone, and the listing with a
/// `stat` of each file.
fn look_parts(root: &Path) {
    let dir = root.join("objects");
    let list = timed(|| {
        let names = fs::read_dir(&dir).expect("lists").count();
        assert!(names > 0);
    });
    let stat = timed(|| {
        let stats = fs::read_dir(&dir)
            .expect("lists")
            .filter_map(|entry| entry.ok()?.metadata().ok())
            .count();
        assert!(stats > 0);
    });
    println!(
        "{:<28} listing {:>8.1} ms   listing and a stat of each file {:>7.1} ms",
        "a warm look's parts",
        list.as_secs_f64() * 1e3,
        stat.as_secs_f64() * 1e3,
    );
}

fn main() {
    let root: PathBuf =
        std::env::temp_dir().join(format!("prodrome-husk-cost-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let todo = root.join("todo");
    let letter = root.join("letter");
    write(&todo, todos());
    write(&letter, letters());

    rows::<TodoEvent<Todo>>("todo store, no husks", &todo);
    look_parts(&todo);
    rows::<Letter>("letter store, all whole", &letter);

    let store = EventStore::<Letter>::new(&letter, Untrusted::none());
    let names: Vec<Hash> = store
        .dag()
        .expect("reads")
        .objects()
        .iter()
        .filter(|(_, object)| matches!(object, Envelope::Change(_)))
        .map(|(name, _)| name.clone())
        .step_by(3)
        .collect();
    let before = bytes(&letter);
    let start = Instant::now();
    let husked = store.husk(names).expect("husks");
    let took = start.elapsed();
    println!(
        "{:<28} {} names in {:>8.1} ms, objects/ {:.2} MB to {:.2} MB",
        "husk",
        husked.len(),
        took.as_secs_f64() * 1e3,
        before as f64 / 1e6,
        bytes(&letter) as f64 / 1e6,
    );
    rows::<Letter>("letter store, a third husks", &letter);

    let store = EventStore::<TodoEvent<Todo>>::new(&todo, Untrusted::none());
    let fresh = |from: usize, count: usize| {
        (from..from + count)
            .map(|n| mk_created(&format!("more-{n}"), at(n), "ada", "more", "").expect("an event"))
            .collect::<Vec<_>>()
    };
    let start = Instant::now();
    let names = store.append_all(fresh(0, RUN)).expect("appends");
    let run = start.elapsed();
    let start = Instant::now();
    for event in fresh(RUN, ONE_BY_ONE) {
        store.append(event).expect("appends");
    }
    let one_by_one = start.elapsed();
    println!(
        "{:<28} a run of {} in {:>8.1} ms   {} one by one in {:>8.1} ms",
        "appends",
        names.len(),
        run.as_secs_f64() * 1e3,
        ONE_BY_ONE,
        one_by_one.as_secs_f64() * 1e3,
    );

    let _ = fs::remove_dir_all(&root);
}
