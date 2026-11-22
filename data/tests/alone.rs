//! A program whose only crate of this workspace is `prodrome-data`: it
//! builds an event and an object, prints them, parses them back and hashes
//! them, and it derives `Literal` for a type of its own.

#[path = "../src/schema/headed/example.rs"]
mod note;

use note::{by_ada, Entry, Note};
use prodrome_data::change::mk_change;
use prodrome_data::event::{
    canonical, canonical_envelope, mk_sealed, parse_envelope, parse_event, seal_hash, Actor,
    Envelope, Hash,
};
use prodrome_data::genesis::mk_genesis;
use prodrome_data::literal::codec::Literal;
use prodrome_data::literal::{parse_literal, print_literal, Datetime, Table};
use prodrome_data::schema::headed::Constructors;

fn at() -> Datetime {
    Datetime::new(2026, 9, 8, 9, 0, 0, 0).expect("a real instant")
}

/// An event prints as its constructor, and its print parses back to it.
#[test]
fn an_event_prints_and_parses_back() {
    let event = by_ada("n1", at(), Entry::Titled("first".to_owned()));
    let print = canonical(&event);
    assert_eq!(
        print,
        "Titled(note='n1', at=datetime(2026, 9, 8, 9, 0, 0), actor='ada', title='first')"
    );
    let vocabulary = Constructors::<Entry>::default();
    let back: Note = parse_event(&vocabulary, &print).expect("the print parses");
    assert_eq!(back, event);
}

/// An object's name is the SHA-256 of its print, and the print parses back
/// to the object, whether the object is a legacy `Sealed` or a `Change` of a
/// prodrome.
#[test]
fn an_object_prints_parses_back_and_hashes() {
    let vocabulary = Constructors::<Entry>::default();
    let genesis: Envelope<Note> = Envelope::Genesis(
        mk_genesis("notes", "0123456789abcdef0123456789abcdef").expect("a valid genesis"),
    );
    let event = by_ada("n1", at(), Entry::Viewed);
    let objects = [
        mk_sealed(None, event.clone()),
        Envelope::Change(mk_change(seal_hash(&genesis), Vec::new(), event).expect("no deps")),
    ];
    // Each print, and the digest that `sha256sum` gives for it.
    let expected = [
        (
            "Sealed(prev='', event=Viewed(note='n1', at=datetime(2026, 9, 8, 9, 0, 0), actor='ada'))",
            "dd67c1bc9430db82a92882567833e64f441b2388a51e694c0e0aae166b023958",
        ),
        (
            "Change(genesis='b0f34528514d2483d72396b36908d8c81d01aa3d33cbaaf1e8e8466d7bc3f5da', \
             deps=(), event=Viewed(note='n1', at=datetime(2026, 9, 8, 9, 0, 0), actor='ada'))",
            "40f336b0ed90b45e55729c4335d6c264ebf8e532e0ef4c369909d217e8ce99f4",
        ),
    ];
    for (object, (print, name)) in objects.iter().zip(expected) {
        assert_eq!(canonical_envelope(object), print);
        let back = parse_envelope::<Note>(&vocabulary, print).expect("the print parses");
        assert_eq!(&back, object);
        assert_eq!(seal_hash(&back), Hash::new(name).expect("a name"));
    }
}

/// A record that a program defines, with the path of the data crate given
/// to the derive.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
#[literal(crate = prodrome_data)]
struct Seen {
    by: Actor,
    note: String,
    twice: bool,
}

/// A type that names the data crate in `#[literal(crate = ...)]` derives
/// `Literal`, and its value prints and reads back.
#[test]
fn a_program_with_the_data_crate_alone_derives_literal() {
    let seen = Seen {
        by: Actor::new("ada").expect("a valid actor"),
        note: "n1".to_owned(),
        twice: false,
    };
    let print = print_literal(&seen.to_value());
    assert_eq!(print, "Seen(by='ada', note='n1', twice=False)");
    let value = parse_literal(&print, &Table(Seen::CONSTRUCTORS)).expect("the print parses");
    assert_eq!(Seen::from_value(&value).expect("the value reads"), seen);
}
