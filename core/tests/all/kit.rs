//! The law kit (`prodrome::laws`) over the reference schemas: law 1
//! (`printed`) and law 35 (`synced`, between the two halves of every drawn
//! history) for the todo, the review and the note schemas. `schema_laws.rs`
//! runs the kit's law 33 over the same schemas.
//!
//! Each law is also shown to fail: a print under another object's name
//! (law 1), a product that keeps the last write joined whatever its
//! ancestry and joins writes that name no register (law 33), a register type
//! whose value disagrees with the writes (law 33), and a replica that drops
//! an object it receives (law 35).

use std::collections::BTreeSet;

use prodrome::event::{canonical_envelope, Actor, Hash, TodoEvent};
use prodrome::fold::sole::Only;
use prodrome::fold::{Discrete, Frontier, Product, Register as _, RegisterType};
use prodrome::laws::{self, Shuffle};
use prodrome::literal::codec::{Constructor, Fields};
use prodrome::literal::{Datetime, ProdromeError, Value};
use prodrome::reference::Todo;
use prodrome::registers::Stamp;
use prodrome::schema::headed::{Body, Event};
use prodrome::schema::Schema;
use prodrome::store::{Held, MemoryStore, Replica};
use proptest::prelude::*;

use crate::common::draw::{a_draw, history, Draw};
use crate::note::{self, Entry, Note};
use crate::review::Review;
use crate::schema_laws::{a_note_event, a_review_event, a_todo_event, failed};

/// Laws 1 and 35 at one draw: the history's prints, and a sync between its
/// two halves, each held in memory.
fn kept<E: Schema<Vocabulary: Default>>(
    draw: &Draw<E>,
    cut: usize,
    partial: usize,
) -> Result<(), TestCaseError> {
    let objects = history(&draw.events);
    let prints: Vec<(Hash, String)> = objects
        .iter()
        .map(|(name, object)| (name.clone(), canonical_envelope(object)))
        .collect();
    laws::printed::<E>(&E::Vocabulary::default(), &prints).map_err(failed)?;
    let (here, there) = draw.shuffle.halves(&objects).map_err(failed)?;
    let (here, there) = (
        laws::holding(E::Vocabulary::default(), &here).expect("a half is closed under parents"),
        laws::holding(E::Vocabulary::default(), &there).expect("a half is closed under parents"),
    );
    laws::synced(&here, &there, MemoryStore::<E>::default, cut, partial).map_err(failed)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Laws 1 and 35, by the kit, over the todo schema.
    #[test]
    fn the_kit_holds_for_todos(draw in a_draw(a_todo_event()), cut in 0usize..12, partial in any::<usize>()) {
        kept::<TodoEvent<Todo>>(&draw, cut, partial)?;
    }

    /// Laws 1 and 35, by the kit, over the review schema.
    #[test]
    fn the_kit_holds_for_reviews(draw in a_draw(a_review_event()), cut in 0usize..12, partial in any::<usize>()) {
        kept::<Review>(&draw, cut, partial)?;
    }

    /// Laws 1 and 35, by the kit, over the note schema.
    #[test]
    fn the_kit_holds_for_notes(draw in a_draw(a_note_event()), cut in 0usize..12, partial in any::<usize>()) {
        kept::<Note>(&draw, cut, partial)?;
    }
}

/// Law 1 fails for a print under another object's name.
#[test]
fn printed_finds_a_print_under_another_name() {
    let objects = history(&[(note::by_ada("n1", note::at(0), Entry::Viewed), 0)]);
    let mut prints: Vec<(Hash, String)> = objects
        .iter()
        .map(|(name, object)| (name.clone(), canonical_envelope(object)))
        .collect();
    let first = prints[0].0.clone();
    prints[0].0 = prints[1].0.clone();
    prints[1].0 = first;
    let broken = laws::printed::<Note>(&Default::default(), &prints).expect_err("misnamed");
    assert_eq!(broken.law, 1);
}

const SET: Constructor = ("Set", &["mark", "at", "actor"]);
const SEEN: Constructor = ("Seen", &["mark", "at", "actor"]);

/// A MARK schema that breaks law 33: a mark is set, and a reader sees it,
/// which names no register. Its product keeps the last write joined, so its
/// reading depends on the order of the fold, and it joins a sighting too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Set,
    Seen,
}

type MarkEvent = Event<Mark>;

/// The last write joined, whatever its ancestry.
#[derive(Debug, Clone, PartialEq, Default)]
struct Last<'a>(Frontier<'a, MarkEvent>);

impl<'a> Product<'a> for Last<'a> {
    type Schema = MarkEvent;

    fn registers(&self) -> Vec<Only> {
        vec![Only]
    }

    fn join(&mut self, stamp: &'a Stamp<MarkEvent>) {
        self.0 = Frontier::default();
        self.0.join(stamp);
    }

    fn frontier(&self, _: Only) -> &Frontier<'a, MarkEvent> {
        &self.0
    }

    fn frontier_mut(&mut self, _: Only) -> &mut Frontier<'a, MarkEvent> {
        &mut self.0
    }

    fn reading(&self, _: Only) -> Vec<&'a Stamp<MarkEvent>> {
        self.0.writes().to_vec()
    }

    fn grows(&self, _: &MarkEvent) -> Result<(), ProdromeError> {
        Ok(())
    }
}

impl Body for Mark {
    type Key = String;
    type Register = Only;
    type Registers<'a> = Last<'a>;
    type Refusal = std::convert::Infallible;

    const EVENTS: &'static [Constructor] = &[SET, SEEN];

    fn writes(&self) -> impl Iterator<Item = Only> {
        match self {
            Mark::Set => Some(Only),
            Mark::Seen => None,
        }
        .into_iter()
    }

    fn fields(&self) -> (Constructor, Vec<Value>) {
        match self {
            Mark::Set => (SET, Vec::new()),
            Mark::Seen => (SEEN, Vec::new()),
        }
    }

    fn parse(fields: &Fields<'_>) -> Result<MarkEvent, ProdromeError> {
        let mark = match fields.name() {
            "Set" => Mark::Set,
            "Seen" => Mark::Seen,
            other => return Err(prodrome::literal::codec::no_arm("a mark event", other)),
        };
        Ok(Event::of(
            fields.get("mark")?,
            fields.get("at")?,
            fields.get("actor")?,
            mark,
        ))
    }
}

/// A register type that values every event, a sighting included.
struct Always;

impl RegisterType<MarkEvent> for Always {
    type Value<'e> = Discrete<&'e MarkEvent>;

    fn value(event: &MarkEvent) -> Option<Discrete<&MarkEvent>> {
        Some(Discrete(event))
    }
}

fn mark(actor: &str, mark: Mark) -> MarkEvent {
    let at = Datetime::new(2026, 10, 1, 0, 0, 0, 0).expect("a real instant");
    Event::of(
        "m1".to_owned(),
        at,
        Actor::new(actor).expect("a valid actor"),
        mark,
    )
}

/// Law 33's first clause fails where the reading depends on the order of
/// the fold: two concurrent marks at one instant, folded in both orders.
#[test]
fn free_finds_a_reading_that_depends_on_the_order() {
    let objects = history(&[(mark("ada", Mark::Set), 0), (mark("ben", Mark::Set), 0)]);
    let broken: Vec<laws::Broken> = [[0, 0, 1], [0, 1, 0]]
        .into_iter()
        .filter_map(|ranks| {
            let shuffle = Shuffle {
                objects: ranks.into_iter().map(|rank| (rank, true)).collect(),
                twice: Vec::new(),
            };
            laws::free(&objects, &shuffle).err()
        })
        .collect();
    assert!(!broken.is_empty(), "one of the two orders reads otherwise");
    assert!(broken.iter().all(|broken| broken.law == 33), "{broken:?}");
}

/// Law 33's second clause fails for a product that joins a sighting.
#[test]
fn routed_finds_a_write_joined_where_it_names_no_register() {
    let broken = laws::routed(&mark("ada", Mark::Seen)).expect_err("a sighting is joined");
    assert_eq!(broken.law, 33);
    laws::routed(&mark("ada", Mark::Set)).expect("a set mark joins the register it names");
}

/// Law 33's third clause fails for a register type that values a sighting.
#[test]
fn valued_finds_a_value_where_the_event_names_no_register() {
    let broken = laws::valued::<_, Always>(&mark("ada", Mark::Seen), Only).expect_err("valued");
    assert_eq!(broken.law, 33);
    laws::valued::<_, Always>(&mark("ada", Mark::Set), Only).expect("named and valued");
}

/// A replica in memory that drops the greatest name of each receipt.
struct Forgetful<E: Schema>(MemoryStore<E>);

impl<E: Schema<Vocabulary: Default>> Default for Forgetful<E> {
    fn default() -> Self {
        Forgetful(MemoryStore::default())
    }
}

impl<E: Schema> Replica<E> for Forgetful<E> {
    fn schema(&self) -> &E::Vocabulary {
        self.0.schema()
    }

    fn held(&self) -> Result<Held<E>, ProdromeError> {
        self.0.held()
    }

    fn tips(&self) -> Result<BTreeSet<Hash>, ProdromeError> {
        self.0.tips()
    }

    fn print(&self, name: &Hash) -> Option<Vec<u8>> {
        self.0.print(name)
    }

    fn append(&self, event: E) -> Result<Hash, ProdromeError> {
        self.0.append(event)
    }

    fn receive(
        &self,
        mut seeds: BTreeSet<Hash>,
        print_of: &dyn Fn(&Hash) -> Option<Vec<u8>>,
    ) -> Result<Vec<Hash>, ProdromeError> {
        seeds.pop_last();
        self.0.receive(seeds, print_of)
    }
}

/// Law 35 fails for a replica that drops an object it receives.
#[test]
fn synced_finds_a_replica_that_drops_an_object() {
    let objects = history(&[
        (note::by_ada("n1", note::at(0), note::titled("a")), 0),
        (note::by_ada("n2", note::at(60), note::titled("b")), 0),
    ]);
    let here = laws::holding(Default::default(), &objects[..2]).expect("a down-set");
    let there = laws::holding(Default::default(), &objects).expect("a history");
    let broken = laws::synced(&here, &there, Forgetful::<Note>::default, 0, 0)
        .expect_err("an object is dropped");
    assert_eq!(broken.law, 35, "{broken}");
    laws::synced(&here, &there, MemoryStore::<Note>::default, 0, 0)
        .expect("a store in memory keeps what it receives");
}
