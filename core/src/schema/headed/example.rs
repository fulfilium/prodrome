// A NOTE schema: each note has a title, and a reader can record that they
// viewed a note. A title writes the note's one register. A view writes no
// register, so it never supersedes a title. The schema is an application
// schema at `schema::headed::Event`, and its product is
// `fold::sole::One` over the register type `Whole`. Its prints are
// `Titled(note='n1', at=datetime(…), actor='ada', title='…')` and
// `Viewed(note='n1', at=datetime(…), actor='ada')`.
//
// The crate's tests include this file, and the module documentation of
// `schema::headed` shows it as its example.

use prodrome::event::Actor;
use prodrome::fold::sole::{One, Only, Whole};
use prodrome::literal::codec::{Constructor, Fields, Literal};
use prodrome::literal::{Datetime, ProdromeError, Value};
use prodrome::schema::headed::{Body, Event};

/// `Titled(note, at, actor, title)`.
pub const TITLED: Constructor = ("Titled", &["note", "at", "actor", "title"]);
/// `Viewed(note, at, actor)`.
pub const VIEWED: Constructor = ("Viewed", &["note", "at", "actor"]);

/// What an event says of its note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// The note has a new title. It writes the note's register.
    Titled(String),
    /// A reader viewed the note. It writes no register.
    Viewed,
}

/// The schema.
pub type Note = Event<Entry>;

/// A note's name is empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unnamed;

impl std::fmt::Display for Unnamed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a note has a name")
    }
}

impl Body for Entry {
    type Key = String;
    type Register = Only;
    type Registers<'a> = One<'a, Whole<Note>>;
    type Refusal = Unnamed;

    const EVENTS: &'static [Constructor] = &[TITLED, VIEWED];

    fn writes(&self) -> impl Iterator<Item = Only> {
        match self {
            Entry::Titled(_) => Some(Only),
            Entry::Viewed => None,
        }
        .into_iter()
    }

    fn admits(note: &String, _: &Entry) -> Result<(), Unnamed> {
        if note.is_empty() {
            Err(Unnamed)
        } else {
            Ok(())
        }
    }

    fn fields(&self) -> (Constructor, Vec<Value>) {
        match self {
            Entry::Titled(title) => (TITLED, vec![title.to_value()]),
            Entry::Viewed => (VIEWED, Vec::new()),
        }
    }

    fn parse(fields: &Fields<'_>) -> Result<Note, ProdromeError> {
        let entry = match fields.name() {
            "Titled" => Entry::Titled(fields.get("title")?),
            "Viewed" => Entry::Viewed,
            other => return Err(prodrome::literal::codec::no_arm("a note event", other)),
        };
        Event::new(
            fields.get("note")?,
            fields.get("at")?,
            fields.get("actor")?,
            entry,
        )
        .map_err(|refused| ProdromeError::invalid(refused.to_string()))
    }
}

/// The event by `ada` with `entry` about `note` at `at`.
pub fn by_ada(note: &str, at: Datetime, entry: Entry) -> Note {
    let ada = Actor::new("ada").expect("a valid actor");
    Event::new(note.to_owned(), at, ada, entry).expect("a named note")
}
