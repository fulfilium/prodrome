//! What `EventStore::tips` and `EventStore::heads` cost on a store of 30,000
//! objects on disk, and what an append costs. Run BY HAND, in release
//! (`cargo run --release --example store_heads -p prodrome-core`); nothing
//! under `cargo test` runs it.
//!
//! The program lays out a generated store: a genesis and 29,999 changes to
//! 100 entities, each change over the entity's last. Each entity's changes
//! are one long chain, so the store has 100 tips.
//!
//! The program prints the median of `ROUNDS` runs of each figure:
//!
//! - `cold tips` and `cold heads`: one call on a new handle in a new
//!   process, which holds nothing in memory;
//! - `warm tips` and `warm heads`: one call on a handle that has read the
//!   store;
//! - `append`: one append through a handle that has read the store.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use prodrome::change::mk_change;
use prodrome::event::{canonical_envelope, seal_hash, Envelope, Hash, TodoEvent};
use prodrome::literal::Datetime;
use prodrome::policy::Untrusted;
use prodrome::reference::{mk_authored, Todo};
use prodrome::store::EventStore;

type Store = EventStore<TodoEvent<Todo>>;

const OBJECTS: usize = 30_000;
const ENTITIES: usize = 100;
const ROUNDS: usize = 15;

fn record(entity: usize, n: usize) -> TodoEvent<Todo> {
    let day = u32::try_from(1 + n % 28).expect("a day of the month");
    let at = Datetime::new(2026, 9, day, 12, 0, 0, 0).expect("a real instant");
    mk_authored(
        &format!("todo-{entity}"),
        at,
        "ada",
        "todo",
        at,
        &format!("change {n}"),
        None,
        vec![],
        "",
        "",
        "",
        None,
        vec![],
        vec![],
        "",
    )
    .expect("a valid event")
}

/// The store, laid out as appends would have left it, by writing each
/// object's file directly.
fn lay_out(root: &Path) {
    let store = Store::new(root, Untrusted::none());
    let genesis = store.init("heads").expect("begins");
    let objects = root.join("objects");
    let mut last: Vec<Option<Hash>> = vec![None; ENTITIES];
    for n in 1..OBJECTS {
        let entity = n % ENTITIES;
        let deps = last[entity].iter().cloned().collect();
        let object =
            Envelope::Change(mk_change(genesis.clone(), deps, record(entity, n)).expect("valid"));
        let name = seal_hash(&object);
        fs::write(
            objects.join(format!("{}.py", name.as_str())),
            canonical_envelope(&object),
        )
        .expect("writes");
        last[entity] = Some(name);
    }
}

fn median(mut runs: Vec<Duration>) -> Duration {
    runs.sort();
    runs[runs.len() / 2]
}

fn timed<T>(work: impl FnOnce() -> T) -> Duration {
    let start = Instant::now();
    work();
    start.elapsed()
}

/// One call of `phase` on a new handle in a new process, in nanoseconds.
fn cold(root: &Path, phase: &str) -> Duration {
    let exe = std::env::current_exe().expect("the program's own path");
    let output = Command::new(exe)
        .arg(phase)
        .arg(root)
        .output()
        .expect("the program runs itself");
    let nanos: u64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("the phase prints its time");
    Duration::from_nanos(nanos)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let [_, phase, root] = args.as_slice() {
        let store = Store::new(root, Untrusted::none());
        let took = match phase.as_str() {
            "tips" => timed(|| store.tips().expect("lists")),
            other => panic!("no phase {other}"),
        };
        println!("{}", took.as_nanos());
        return;
    }

    let root = std::env::temp_dir().join(format!("prodrome-store-heads-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    lay_out(&root);
    let writer = Store::new(&root, Untrusted::none());
    let mut n = OBJECTS;
    let mut next = || {
        n += 1;
        record(n % ENTITIES, n)
    };
    writer.append(next()).expect("appends");
    let append = median(
        (0..ROUNDS)
            .map(|_| {
                let event = next();
                timed(|| writer.append(event).expect("appends"))
            })
            .collect(),
    );
    let cold_tips = median((0..ROUNDS).map(|_| cold(&root, "tips")).collect());
    let warm = Store::new(&root, Untrusted::none());
    warm.tips().expect("lists");
    let warm_tips = median(
        (0..ROUNDS)
            .map(|_| timed(|| warm.tips().expect("lists")))
            .collect(),
    );

    println!("a store of {OBJECTS} objects over {ENTITIES} entities");
    for (what, took) in [
        ("cold tips", cold_tips),
        ("warm tips", warm_tips),
        ("append", append),
    ] {
        println!("{what:>11}: {:9.3} ms", took.as_secs_f64() * 1e3);
    }
    let _ = fs::remove_dir_all(&root);
}
