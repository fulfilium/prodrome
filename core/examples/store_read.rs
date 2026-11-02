//! What a cold read of a large store costs, phase by phase. Run BY HAND, in
//! release (`cargo run --release --example store_read -p prodrome-core`);
//! nothing under `cargo test` runs it.
//!
//! The store has the shape of a host's message log: 15,900 records whose
//! body is about 8.7 kB of prose (139 MB in all), and 8,000 records of about
//! 200 bytes. The prose holds newlines, both quote characters and some
//! characters outside ASCII, so the string printer escapes some of it and
//! the parser must undo the escapes. Each record is a change over the one
//! before it to its entity, as appends would have written it.
//!
//! The program prints the time of each phase of one cold read, each the
//! median of `ROUNDS`, so that the cost of the parse can be told apart from
//! the cost of the file system and of the hash:
//!
//! - `read`: every file read into memory, and nothing else;
//! - `hash`: the SHA-256 of every file's bytes, which a read checks against
//!   the file's name;
//! - `parse`: every print parsed into a [`Value`] at the store's vocabulary;
//! - `decode`: every value turned into an [`Envelope`] by the smart
//!   constructors;
//! - `dag`: [`EventStore::dag`] from a handle that has read nothing, which is
//!   all of the above and the graph.
//!
//! Pass a directory as the first argument to keep the store there and read
//! it again on the next run instead of writing it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use prodrome::change::mk_change;
use prodrome::event::{canonical_envelope, seal_hash, Envelope, EventVocabulary, Hash, TodoEvent};
use prodrome::literal::{parse_literal, Datetime, Value};
use prodrome::policy::Untrusted;
use prodrome::reference::{mk_authored, Todo};
use prodrome::store::EventStore;
use prodrome::todo::TodoVocabulary;
use sha2::{Digest as _, Sha256};

type Event = TodoEvent<Todo>;
type Store = EventStore<Event>;

/// The large records, and the size of each one's body in bytes.
const LARGE: usize = 15_900;
const BODY: usize = 8_600;
/// The small records.
const SMALL: usize = 8_000;
/// The entities the records are spread over.
const ENTITIES: usize = 100;
const ROUNDS: usize = 5;

/// Prose of about `bytes` bytes, different for each `seed`: words, newlines,
/// both quotes, a backslash now and then, and characters outside ASCII.
fn prose(seed: usize, bytes: usize) -> String {
    const WORDS: &[&str] = &[
        "the",
        "permit",
        "answer",
        "question",
        "about",
        "a",
        "form",
        "was",
        "filed",
        "on",
        "Monday",
        "and",
        "it's",
        "\"quoted\"",
        "café",
        "naïve",
        "—",
        "→",
        "line\n",
        "tab\t",
        "path\\to",
        "résumé",
        "日本",
        "ok.",
        "{\"key\": 1}",
        "deadline",
        "next",
        "week",
    ];
    let mut state = seed as u64 * 6_364_136_223_846_793_005 + 1_442_695_040_888_963_407;
    let mut out = String::with_capacity(bytes + 16);
    while out.len() < bytes {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        out.push_str(WORDS[(state >> 33) as usize % WORDS.len()]);
        out.push(' ');
    }
    out
}

fn record(entity: usize, n: usize, body: &str) -> Event {
    let at = Datetime::new(2026, 1 + (n % 12) as u32, 1 + (n % 28) as u32, 12, 0, 0, 0)
        .expect("a real instant");
    mk_authored(
        &format!("entity-{entity}"),
        at,
        "ada",
        "line",
        at,
        body,
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

/// The store, laid out as `LARGE + SMALL` appends would have left it.
fn lay_out(root: &Path) {
    let store = Store::new(root, Untrusted::none());
    let genesis = store.init("read").expect("begins");
    let objects = root.join("objects");
    let mut last: Vec<Option<Hash>> = vec![None; ENTITIES];
    for n in 0..LARGE + SMALL {
        let entity = n % ENTITIES;
        let body = if n < LARGE {
            prose(n, BODY)
        } else {
            prose(n, 150)
        };
        let deps = last[entity].iter().cloned().collect();
        let object = Envelope::Change(
            mk_change(genesis.clone(), deps, record(entity, n, &body)).expect("valid"),
        );
        let name = seal_hash(&object);
        fs::write(
            objects.join(format!("{}.py", name.as_str())),
            canonical_envelope(&object),
        )
        .expect("writes");
        last[entity] = Some(name);
    }
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// The median time of `work`, without the time to drop what it answers: a
/// program that reads a store keeps what it read.
fn timed<T>(mut work: impl FnMut() -> T) -> Duration {
    median(
        (0..ROUNDS)
            .map(|_| {
                let start = Instant::now();
                let answer = work();
                let took = start.elapsed();
                drop(answer);
                took
            })
            .collect(),
    )
}

fn main() {
    let (root, keep): (PathBuf, bool) = match std::env::args_os().nth(1) {
        Some(dir) => (dir.into(), true),
        None => (
            std::env::temp_dir().join(format!("prodrome-store-read-{}", std::process::id())),
            false,
        ),
    };
    if !root.join("objects").is_dir() {
        lay_out(&root);
    }
    let files: Vec<PathBuf> = fs::read_dir(root.join("objects"))
        .expect("lists")
        .map(|entry| entry.expect("an entry").path())
        .collect();
    let prints: Vec<Vec<u8>> = files
        .iter()
        .map(|file| fs::read(file).expect("reads"))
        .collect();
    let bytes: usize = prints.iter().map(Vec::len).sum();
    println!(
        "a store of {} objects, {:.1} MB",
        files.len(),
        bytes as f64 / 1e6
    );

    let schema = TodoVocabulary::<Todo>::default();
    let vocabulary = EventVocabulary::<Event>::new(&schema);
    let texts: Vec<&str> = prints
        .iter()
        .map(|print| std::str::from_utf8(print).expect("text"))
        .collect();
    let values: Vec<Value> = texts
        .iter()
        .map(|text| parse_literal(text, &vocabulary).expect("parses"))
        .collect();

    let phases = [
        (
            "read",
            timed(|| {
                for file in &files {
                    std::hint::black_box(fs::read(file).expect("reads"));
                }
            }),
        ),
        (
            "hash",
            timed(|| {
                for print in &prints {
                    std::hint::black_box(Sha256::digest(print));
                }
            }),
        ),
        (
            "parse",
            timed(|| {
                for text in &texts {
                    std::hint::black_box(parse_literal(text, &vocabulary).expect("parses"));
                }
            }),
        ),
        (
            "decode",
            timed(|| {
                for value in &values {
                    std::hint::black_box(
                        Envelope::<Event>::from_value(&schema, value).expect("decodes"),
                    );
                }
            }),
        ),
        (
            "dag",
            timed(|| {
                let store = Store::new(&root, Untrusted::none());
                std::hint::black_box(store.dag().expect("reads"));
                store
            }),
        ),
    ];
    for (what, took) in phases {
        println!(
            "{what:>8}: {:8.1} ms  {:7.0} MB/s",
            took.as_secs_f64() * 1e3,
            bytes as f64 / 1e6 / took.as_secs_f64()
        );
    }
    if !keep {
        let _ = fs::remove_dir_all(&root);
    }
}
