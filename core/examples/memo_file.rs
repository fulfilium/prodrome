//! What a cache held in a file saves a new process (design §6.2). Run BY
//! HAND, in release (`cargo run --release --example memo_file -p
//! prodrome-core`); nothing under `cargo test` runs it.
//!
//! The program lays out a generated store of 40,000 objects: a genesis and
//! changes to 100 entities, each change over the entity's last. A fold over
//! the store is the digest's balanced tree of its objects
//! ([`digest::tree`]) and the measure [`digest::Measured`], which counts the
//! objects and their bytes at every node.
//!
//! The program runs itself as a new process for each phase, so that no
//! phase reads what an earlier one left in memory:
//!
//! 1. `cold`: no cache file. It reads the store, builds the tree, and folds
//!    it once with an empty cache in memory and once through a new file.
//! 2. `warm`: the file that `cold` wrote. It folds the same tree.
//! 3. `append`: one object appended to the store. It folds the new tree
//!    through the file, and prints the report's hits and computed nodes at
//!    each depth.
//!
//! Each phase prints the median of `ROUNDS` runs of its fold, and the time
//! to read the store and build the tree, which every phase pays alike.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use prodrome::change::mk_change;
use prodrome::digest::{self, Leaf, Measure, Measured};
use prodrome::event::{canonical_envelope, parents_of, seal_hash, Envelope, Hash, TodoEvent};
use prodrome::literal::codec::Literal;
use prodrome::literal::{Datetime, ProdromeError, Value};
use prodrome::memo::{fold, Cache, File, Front, Keep, Lru, Report};
use prodrome::policy::Untrusted;
use prodrome::reference::{mk_authored, Todo};
use prodrome::store::EventStore;

type Event = TodoEvent<Todo>;
type Store = EventStore<Event>;

/// The changes, and the entities they are spread over.
const CHANGES: usize = 39_999;
const ENTITIES: usize = 100;
const ROUNDS: usize = 5;

/// How many objects, and how many bytes, a node spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    objects: u64,
    bytes: u64,
}

impl Measure for Span {
    fn empty() -> Self {
        Span {
            objects: 0,
            bytes: 0,
        }
    }

    fn join(&self, other: &Self) -> Self {
        Span {
            objects: self.objects + other.objects,
            bytes: self.bytes + other.bytes,
        }
    }
}

/// A span prints as the pair of its two counts.
impl Literal for Span {
    fn to_value(&self) -> Value {
        Value::Tuple(vec![self.objects.to_value(), self.bytes.to_value()])
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        match value.as_tuple() {
            Some([objects, bytes]) => Ok(Span {
                objects: u64::from_value(objects)?,
                bytes: u64::from_value(bytes)?,
            }),
            _ => Err(ProdromeError::invalid("a span is a pair")),
        }
    }
}

fn record(entity: usize, n: usize) -> Event {
    let at = Datetime::new(2026, 1 + (n % 12) as u32, 1 + (n % 28) as u32, 12, 0, 0, 0)
        .expect("a real instant");
    mk_authored(
        &format!("entity-{entity}"),
        at,
        "ada",
        "line",
        at,
        &format!("line {n} of entity {entity}"),
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
    .expect("a valid record")
}

/// The object of change `n`, over `last`, the entity's last change.
fn change(genesis: &Hash, last: Option<&Hash>, n: usize) -> Envelope<Event> {
    let deps = last.into_iter().cloned().collect();
    Envelope::Change(mk_change(genesis.clone(), deps, record(n % ENTITIES, n)).expect("valid"))
}

fn put(objects: &Path, object: &Envelope<Event>) -> Hash {
    let name = seal_hash(object);
    fs::write(
        objects.join(format!("{}.py", name.as_str())),
        canonical_envelope(object),
    )
    .expect("writes");
    name
}

/// The store, laid out as a genesis and `CHANGES` appends would leave it.
fn lay_out(root: &Path) {
    let store = Store::new(root, Untrusted::none());
    let genesis = store.init("memo").expect("begins");
    let objects = root.join("objects");
    let mut last: Vec<Option<Hash>> = vec![None; ENTITIES];
    for n in 0..CHANGES {
        let object = change(&genesis, last[n % ENTITIES].as_ref(), n);
        last[n % ENTITIES] = Some(put(&objects, &object));
    }
}

/// One more change, over entity 0's last.
fn append(root: &Path) {
    let dag = Store::new(root, Untrusted::none()).dag().expect("reads");
    let genesis = dag
        .objects()
        .values()
        .find_map(Envelope::genesis)
        .cloned()
        .expect("a genesis");
    let last = dag
        .objects()
        .iter()
        .filter(|(_, object)| {
            object
                .event()
                .is_some_and(|event| event.todo().as_str() == "entity-0")
        })
        .map(|(name, _)| name)
        .find(|name| {
            !dag.objects()
                .values()
                .any(|object| parents_of(object).contains(name))
        })
        .cloned();
    put(
        &root.join("objects"),
        &change(&genesis, last.as_ref(), CHANGES),
    );
}

/// The store's tree: its objects in causal order, balanced.
fn tree(root: &Path) -> digest::Tree<Span> {
    let dag = Store::new(root, Untrusted::none()).dag().expect("reads");
    let leaves = dag
        .objects()
        .iter()
        .map(|(name, object)| {
            let bytes = canonical_envelope(object).len() as u64;
            Leaf {
                name: name.clone(),
                parents: parents_of(object),
                bytes: u32::try_from(bytes).expect("a small object"),
                measure: Span { objects: 1, bytes },
            }
        })
        .collect();
    digest::tree(leaves)
        .expect("no cycle")
        .expect("some objects")
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn ms(took: Duration) -> String {
    format!("{:8.2} ms", took.as_secs_f64() * 1e3)
}

fn print_report(report: &Report) {
    println!("    depth  hits  computed");
    for (depth, counts) in report.by_depth().iter().enumerate() {
        println!("    {depth:>5} {:>5} {:>9}", counts.hits, counts.computed);
    }
}

/// One phase, in this process.
fn phase(name: &str, root: &Path, cache: &Path) {
    let start = Instant::now();
    let tree = tree(root);
    let built = start.elapsed();
    let measured = Measured::default();
    println!(
        "{name}: {} objects; read and built the tree in {}",
        tree.lines(),
        ms(built)
    );
    match name {
        "cold" => {
            let start = Instant::now();
            let (span, _) = fold(&measured, tree.root(), &mut Cache::default());
            println!("  fold, empty cache in memory:  {}", ms(start.elapsed()));
            let start = Instant::now();
            let mut file = Cache::held(File::open(cache).expect("opens"), Keep);
            let (kept, report) = fold(&measured, tree.root(), &mut file);
            file.settle().expect("settles");
            assert_eq!(span, kept);
            println!(
                "  fold, new file (open, fold, settle): {}  ({} nodes computed, {} bytes)",
                ms(start.elapsed()),
                report.by_depth().iter().map(|c| c.computed).sum::<usize>(),
                fs::metadata(cache).expect("a file").len()
            );
        }
        "warm" => {
            let first = Instant::now();
            let file = File::open(cache).expect("opens");
            let opened = first.elapsed();
            let mut file = Cache::held(file, Keep);
            let held = first.elapsed();
            let (span, report) = fold(&measured, tree.root(), &mut file);
            println!(
                "  first fold, warm file: open {} + policy {} + fold {}  ({} visit, {:?})",
                ms(opened),
                ms(held - opened),
                ms(first.elapsed() - held),
                report.visits().len(),
                report.visits()[0].outcome
            );
            let again = median(
                (0..ROUNDS)
                    .map(|_| {
                        let start = Instant::now();
                        let (kept, _) = fold(&measured, tree.root(), &mut file);
                        assert_eq!(span, kept);
                        start.elapsed()
                    })
                    .collect(),
            );
            println!("  a later fold, same handle:  {}", ms(again));
            println!("  objects {} bytes {}", span.objects, span.bytes);
        }
        _ => {
            let start = Instant::now();
            let mut file = Front::new(
                Lru::new(4096),
                Cache::held(File::open(cache).expect("opens"), Keep),
            );
            let (span, report) = fold(&measured, tree.root(), &mut file);
            println!(
                "  fold after one append, memory in front of the warm file: {}",
                ms(start.elapsed())
            );
            println!("  objects {} bytes {}", span.objects, span.bytes);
            print_report(&report);
        }
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    if let (Some(name), Some(root), Some(cache)) = (args.next(), args.next(), args.next()) {
        let name = name.to_string_lossy().into_owned();
        phase(&name, Path::new(&root), Path::new(&cache));
        return;
    }
    let root: PathBuf =
        std::env::temp_dir().join(format!("prodrome-memo-file-{}", std::process::id()));
    let cache = root.join("cache");
    lay_out(&root);
    let me = std::env::current_exe().expect("this program");
    for name in ["cold", "warm", "append"] {
        if name == "append" {
            append(&root);
        }
        let status = Command::new(&me)
            .arg(name)
            .arg(&root)
            .arg(&cache)
            .status()
            .expect("runs");
        assert!(status.success(), "phase {name} failed");
    }
    let _ = fs::remove_dir_all(&root);
}
