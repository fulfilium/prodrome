//! The NOTE schema (`core/src/schema/headed/example.rs`): an application schema at
//! the generic event of `schema::headed`, whose product is
//! `fold::sole::One` over `fold::sole::Whole`. The laws run over it in
//! `schema_laws.rs`, `order_laws.rs` and `causal.rs`. This file checks its
//! stored form (the key, `at` and `actor` print before the body, and the
//! parse reads that print back and refuses any other), that `Event::new`
//! refuses what the schema refuses, and that a view, which writes no
//! register, never supersedes a title.

use std::collections::BTreeMap;

use prodrome::event::{
    canonical, canonical_envelope, parse_event, seal_hash, Actor, Envelope, Hash,
};
use prodrome::fold::sole::Only;
use prodrome::fold::{read, Product};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::schema::headed::{Constructors, Event};
use prodrome::store::{sync, Held, MemoryStore, Replica};

#[path = "../../src/schema/headed/example.rs"]
mod schema;

pub use schema::*;

/// The instant `seconds` after the start of 2026-10-01.
pub fn at(seconds: u32) -> Datetime {
    Datetime::new(
        2026,
        10,
        1 + seconds / 86_400,
        0,
        0,
        seconds % 86_400 / 60,
        0,
    )
    .expect("a real instant")
}

/// The body that gives a note the title `title`.
pub fn titled(title: &str) -> Entry {
    Entry::Titled(title.to_owned())
}

/// The prints are the key, `at`, `actor`, then the body, and they read
/// back.
#[test]
fn a_note_prints_its_key_before_its_body() {
    let vocabulary = Constructors::default();
    let title = by_ada("n1", at(0), titled("first"));
    let print = "Titled(note='n1', at=datetime(2026, 10, 1, 0, 0, 0), actor='ada', title='first')";
    assert_eq!(canonical(&title), print);
    assert_eq!(parse_event::<Note>(&vocabulary, print), Ok(title));
    let view = by_ada("n1", at(0), Entry::Viewed);
    let print = "Viewed(note='n1', at=datetime(2026, 10, 1, 0, 0, 0), actor='ada')";
    assert_eq!(canonical(&view), print);
    assert_eq!(parse_event::<Note>(&vocabulary, print), Ok(view));
}

/// The parse refuses a constructor that the schema does not name, and a
/// key that `Body::admits` refuses. `Event::new` refuses the same key.
#[test]
fn the_parse_refuses_what_the_schema_does_not_name() {
    let vocabulary = Constructors::default();
    let moved = "Moved(note='n1', at=datetime(2026, 10, 1, 0, 0, 0), actor='ada', title='x')";
    assert!(parse_event::<Note>(&vocabulary, moved).is_err());
    let unnamed = "Titled(note='', at=datetime(2026, 10, 1, 0, 0, 0), actor='ada', title='x')";
    assert!(parse_event::<Note>(&vocabulary, unnamed).is_err());
    let ada = Actor::new("ada").expect("a valid actor");
    assert_eq!(
        Event::new(String::new(), at(0), ada, Entry::Viewed),
        Err(Unnamed)
    );
    let positional = "Titled('n1', datetime(2026, 10, 1, 0, 0, 0), 'ada', 'x')";
    assert_eq!(
        parse_event::<Note>(&vocabulary, positional),
        Ok(by_ada("n1", at(0), titled("x")))
    );
}

/// Two replicas of one genesis that hold nothing else.
fn two_replicas() -> [MemoryStore<Note>; 2] {
    let genesis: Envelope<Note> =
        Envelope::Genesis(mk_genesis("notes", &"0".repeat(32)).expect("a genesis"));
    let name = seal_hash(&genesis);
    let print = canonical_envelope(&genesis).into_bytes();
    let begun = || {
        let memory = MemoryStore::default();
        memory
            .receive([name.clone()].into(), &|held| {
                (*held == name).then(|| print.clone())
            })
            .expect("receives its genesis");
        memory
    };
    [begun(), begun()]
}

/// The one note's reading and conflicts in `replica`, as object names.
fn held(replica: &MemoryStore<Note>) -> (Vec<Hash>, BTreeMap<Only, Vec<Hash>>) {
    let Held { folded, .. } = replica.held().expect("reads");
    let (_, stream) = folded.entities().next().expect("one note");
    let registers = read(stream, None, &Everything);
    let reading = registers
        .reading(Only)
        .iter()
        .map(|stamp| stamp.name.clone())
        .collect();
    (reading, registers.conflicts())
}

/// Two writers that give a note a title concurrently make a conflict on
/// `Only`, and a third title that rests on both settles it.
#[test]
fn a_third_title_settles_two_concurrent_ones() {
    let [mine, theirs] = two_replicas();
    mine.append(by_ada("n1", at(0), titled("a")))
        .expect("appends");
    sync(&mine, &theirs).expect("syncs");
    let b = mine
        .append(by_ada("n1", at(60), titled("b")))
        .expect("appends");
    let c = theirs
        .append(by_ada("n1", at(120), titled("c")))
        .expect("appends");
    sync(&theirs, &mine).expect("syncs");

    let mut both = vec![b, c];
    both.sort();
    assert_eq!(held(&mine), (both.clone(), BTreeMap::from([(Only, both)])));

    let d = mine
        .append(by_ada("n1", at(180), titled("d")))
        .expect("appends");
    assert_eq!(held(&mine), (vec![d], BTreeMap::new()));
}

/// A view rests on the title before it and writes no register, so the
/// title stays the reading.
#[test]
fn a_view_supersedes_no_title() {
    let [mine, _] = two_replicas();
    let title = mine
        .append(by_ada("n1", at(0), titled("a")))
        .expect("appends");
    mine.append(by_ada("n1", at(60), Entry::Viewed))
        .expect("appends");
    assert_eq!(held(&mine), (vec![title], BTreeMap::new()));
}
