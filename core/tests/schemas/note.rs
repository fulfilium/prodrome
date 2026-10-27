//! A NOTE schema, in the tests only: a note has a title, and each event
//! gives a note a new title. The schema is an application schema at the
//! generic event (`schema::envelope::Event`), and its one register is read
//! by `fold::sole::One` under the discrete order on events. So the schema
//! laws that run over it cover the envelope and the one-register product.
//!
//! An event prints as `Titled(note='n1', at=datetime(…), actor='ada',
//! title='…')`.

use prodrome::event::Actor;
use prodrome::fold::sole::{One, Only, Sole};
use prodrome::fold::Frontier;
use prodrome::literal::codec::{Constructor, Fields, Literal};
use prodrome::literal::{Datetime, ProdromeError, Value};
use prodrome::registers::Stamp;
use prodrome::schema::envelope::{Event, Says};

/// The one event constructor.
pub const TITLED: Constructor = ("Titled", &["note", "at", "actor", "title"]);

/// The body of an event: the note's new title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title(String);

/// The schema.
pub type Note = Event<Title>;

/// The event that gives `note` the title `title`, stamped `at` by `actor`.
///
/// # Errors
///
/// An empty note name.
pub fn titled(
    note: String,
    at: Datetime,
    actor: Actor,
    title: String,
) -> Result<Note, ProdromeError> {
    if note.is_empty() {
        return Err(ProdromeError::invalid("a note has a name"));
    }
    Ok(Event::of(note, at, actor, Title(title)))
}

/// The event by `ada` that gives `note` the title `title` at `at`.
pub fn by_ada(note: &str, at: Datetime, title: &str) -> Note {
    let ada = Actor::new("ada").expect("a valid actor");
    titled(note.to_owned(), at, ada, title.to_owned()).expect("a named note")
}

impl Says for Title {
    type Key = String;
    type Register = Only;
    type Registers<'a> = One<'a, Title>;

    const EVENTS: &'static [Constructor] = &[TITLED];

    fn registers() -> Vec<Only> {
        vec![Only]
    }

    fn writes(&self) -> impl Iterator<Item = Only> {
        std::iter::once(Only)
    }

    fn body(&self) -> (Constructor, Vec<Value>) {
        (TITLED, vec![self.0.to_value()])
    }

    fn parse(fields: &Fields<'_>) -> Result<Note, ProdromeError> {
        titled(
            fields.get("note")?,
            fields.get("at")?,
            fields.get("actor")?,
            fields.get("title")?,
        )
    }
}

/// The title register reads its candidates: the discrete order on events.
impl Sole for Title {
    type Schema = Note;

    fn reading<'a>(frontier: &Frontier<'a, Note>) -> Vec<&'a Stamp<Note>> {
        frontier.candidates()
    }
}
