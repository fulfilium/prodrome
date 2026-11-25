//! §2: "Anything else is a refusal (`ValueError`-shaped), never a crash:
//! parsing is fuzzed." The reference's own guard is `tests/test_fuzz.py`; this
//! is the same property in Rust, where "never a crash" also means never a
//! stack overflow — a deep nest is a refusal, not an abort (`MAX_DEPTH`).

use prodrome::event::{parse_envelope, TodoEvent};
use prodrome::literal::{parse_literal, print_literal, Open, Table};
use prodrome::reference::Todo;
use proptest::prelude::*;

/// A vocabulary shaped like a real one, so the fuzzer reaches the call arms.
static VOCABULARY: Table = Table(&[
    ("Sealed", &["prev", "event"]),
    ("Woven", &["parents", "event"]),
    ("Genesis", &["label", "nonce"]),
    ("Change", &["genesis", "deps", "event"]),
    ("Snapshot", &["genesis", "tips", "previous"]),
    ("Note", &["on", "lines"]),
]);

/// One of each of §3's Genesis, Change and Snapshot, ASCII so any byte offset is a cut.
const OBJECTS: [&str; 3] = [
    "Genesis(label='home', nonce='9f2c9f2c9f2c9f2c9f2c9f2c9f2c9f2c')",
    "Change(genesis='0000000000000000000000000000000000000000000000000000000000000000', \
     deps=('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',), \
     event=Completed(todo='a', at=datetime(2026, 9, 1, 0, 0, 0), actor='bassel', note=''))",
    "Snapshot(genesis='0000000000000000000000000000000000000000000000000000000000000000', \
     tips=('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',), previous='')",
];

proptest! {
    #![proptest_config(crate::common::cases::cases(4096))]

    #[test]
    fn arbitrary_text_never_panics(text in ".{0,200}") {
        let _ = parse_literal(&text, &VOCABULARY);
        let _ = parse_literal(&text, &Open);
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..200)) {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            let _ = parse_literal(text, &VOCABULARY);
        }
    }

    /// An object with a cut or a splice in it is a refusal or an object.
    #[test]
    fn a_damaged_object_never_panics(
        which in 0..OBJECTS.len(),
        at in any::<prop::sample::Index>(),
        cut in 0usize..8,
        splice in ".{0,8}",
    ) {
        let object = OBJECTS[which];
        let at = at.index(object.len() + 1);
        let end = (at + cut).min(object.len());
        let damaged = format!("{}{splice}{}", &object[..at], &object[end..]);
        let _ = parse_envelope::<TodoEvent<Todo>>(&Default::default(), &damaged);
    }

    /// Whatever survives the parse must print back to something the parser
    /// accepts, and the second print must equal the first — the stability that
    /// lets `seal_hash` hash a print (`literals.py`'s module docstring).
    #[test]
    fn a_parse_that_succeeds_prints_stably(text in ".{0,200}") {
        if let Ok(value) = parse_literal(&text, &VOCABULARY) {
            let once = print_literal(&value);
            let again = parse_literal(&once, &VOCABULARY).expect("a canonical print reparses");
            prop_assert_eq!(once, print_literal(&again));
        }
    }
}

#[test]
fn every_object_the_fuzzer_damages_parses_whole() {
    for object in OBJECTS {
        assert!(
            parse_envelope::<TodoEvent<Todo>>(&Default::default(), object).is_ok(),
            "{object}"
        );
    }
}

#[test]
fn a_deep_nest_is_a_refusal_and_not_a_stack_overflow() {
    let deep = format!("{}{}", "(".repeat(10_000), ")".repeat(10_000));
    assert!(parse_literal(&deep, &Open).is_err());
    let deep = format!("Note(on={}1{})", "(".repeat(10_000), ")".repeat(10_000));
    assert!(parse_literal(&deep, &VOCABULARY).is_err());
}
