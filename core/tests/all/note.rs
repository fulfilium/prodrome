//! The tests' NOTE schema (`../schemas/note.rs`): an application schema at
//! the generic event of `schema::envelope`, whose one register is
//! `fold::sole::One`. The schema laws run over it in `schema_laws.rs`,
//! `order_laws.rs` and `causal.rs`. This file checks its stored form: the
//! envelope prints the key, `at` and `actor` before the body, and the parse
//! reads that print back and refuses any other.

use std::collections::BTreeMap;

use prodrome::event::{canonical, canonical_envelope, parse_event, seal_hash, Envelope, Hash};
use prodrome::fold::sole::Only;
use prodrome::fold::{read, Product};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::schema::envelope::Vocab;
use prodrome::store::{sync, Held, MemoryStore, Replica};

#[path = "../schemas/note.rs"]
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

/// The print is the key, `at`, `actor`, then the body, and it reads back.
#[test]
fn a_note_prints_its_head_before_its_body() {
    let note = by_ada("n1", at(0), "first");
    let print = "Titled(note='n1', at=datetime(2026, 10, 1, 0, 0, 0), actor='ada', title='first')";
    assert_eq!(canonical(&note), print);
    assert_eq!(parse_event::<Note>(&Vocab::default(), print), Ok(note));
}

/// The parse refuses a constructor that the schema does not name, and a
/// field that the smart constructor refuses.
#[test]
fn the_parse_refuses_what_the_schema_does_not_name() {
    let vocabulary = Vocab::default();
    let moved = "Moved(note='n1', at=datetime(2026, 10, 1, 0, 0, 0), actor='ada', title='x')";
    assert!(parse_event::<Note>(&vocabulary, moved).is_err());
    let unnamed = "Titled(note='', at=datetime(2026, 10, 1, 0, 0, 0), actor='ada', title='x')";
    assert!(parse_event::<Note>(&vocabulary, unnamed).is_err());
    let positional = "Titled('n1', datetime(2026, 10, 1, 0, 0, 0), 'ada', 'x')";
    assert_eq!(
        parse_event::<Note>(&vocabulary, positional),
        Ok(by_ada("n1", at(0), "x"))
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

/// The one note's frontier and conflicts in `replica`, as object names.
fn held(replica: &MemoryStore<Note>) -> (Vec<Hash>, BTreeMap<Only, Vec<Hash>>) {
    let Held { folded, .. } = replica.held().expect("reads");
    let (_, stream) = folded.entities().next().expect("one note");
    let registers = read(stream, None, &Everything);
    let reading = registers
        .reading()
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
    mine.append(by_ada("n1", at(0), "a")).expect("appends");
    sync(&mine, &theirs).expect("syncs");
    let b = mine.append(by_ada("n1", at(60), "b")).expect("appends");
    let c = theirs.append(by_ada("n1", at(120), "c")).expect("appends");
    sync(&theirs, &mine).expect("syncs");

    let mut both = vec![b, c];
    both.sort();
    assert_eq!(held(&mine), (both.clone(), BTreeMap::from([(Only, both)])));

    let d = mine.append(by_ada("n1", at(180), "d")).expect("appends");
    assert_eq!(held(&mine), (vec![d], BTreeMap::new()));
}
