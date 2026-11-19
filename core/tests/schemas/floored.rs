//! The FLOORED review schema: the review's phases (`review.rs`) in one
//! inflationary register, with one rule that their order does not state. A
//! merge stands only on a review: the append refuses a `merged` unless the
//! reading it supersedes holds `review` or `merged`. So the append refuses
//! a merge as the first write to a review, where the reading is empty.
//! Under the order alone, the empty reading admits every phase, so a schema
//! whose refusal is its order cannot show what an append does with a first
//! write. This schema can (law 70).
//!
//! An event prints as `Moved(review='r-1', at=…, actor='ada', phase='merged')`.

use std::convert::Infallible;

use prodrome::fold::sole::{Hold, One, Only, Sole};
use prodrome::fold::{Climb, Frontier, RegisterType};
use prodrome::literal::codec::{Constructor, Fields};
use prodrome::literal::{ProdromeError, Value};
use prodrome::registers::Stamp;
use prodrome::schema::headed::{Body, Event};

use crate::review::Phase;

/// `Moved(review, at, actor, phase)`.
pub const MOVED: Constructor = ("Moved", &["review", "at", "actor", "phase"]);

/// What an event says of its review: the phase it moves the review to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move(pub Phase);

/// The schema.
pub type Floored = Event<Move>;

/// The phases that `phase` stands on, beside its order: a merge stands only
/// on a review or a merge. `None` for a phase that the order alone admits.
fn floor(phase: Phase) -> Option<[Phase; 2]> {
    match phase {
        Phase::Merged => Some([Phase::Review, Phase::Merged]),
        Phase::Draft | Phase::Review | Phase::Closed => None,
    }
}

/// The register type of the phase.
pub struct Phases;

impl RegisterType<Floored> for Phases {
    type Value<'e> = Phase;

    fn value(event: &Floored) -> Option<Phase> {
        Some(event.body().0)
    }
}

impl Sole for Phases {
    type Schema = Floored;
    type Held<'a> = Floor<'a>;
}

/// The phase, held in a [`Climb`] as an inflationary register is, with the
/// floor rule added to the append's refusal.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Floor<'a>(Climb<'a, Floored>);

impl<'a> Hold<'a, Phases> for Floor<'a> {
    fn join(&mut self, stamp: &'a Stamp<Floored>) {
        self.0.join::<Phases>(stamp);
    }

    fn frontier(&self) -> &Frontier<'a, Floored> {
        self.0.frontier()
    }

    fn reading(&self) -> Vec<&'a Stamp<Floored>> {
        self.0.reading::<Phases>()
    }

    /// A move stands at or above the reading it supersedes, under the
    /// order, and a merge also stands on a review or a merge in that
    /// reading.
    fn grows(&self, event: &Floored) -> Result<(), ProdromeError> {
        self.0.grows::<Phases>(event)?;
        let Some(floor) = floor(event.body().0) else {
            return Ok(());
        };
        let reading: Vec<Phase> = self
            .0
            .reading::<Phases>()
            .into_iter()
            .map(|stamp| stamp.event.body().0)
            .collect();
        if reading.iter().any(|phase| floor.contains(phase)) {
            Ok(())
        } else {
            Err(ProdromeError::invalid(format!(
                "a merge stands only on a review, and the reading is {reading:?}"
            )))
        }
    }

    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError> {
        self.0.climbed::<Phases>(&earlier.0)
    }

    fn climbing(&self) -> Vec<Only> {
        vec![Only]
    }
}

impl Body for Move {
    type Key = String;
    type Register = Only;
    type Registers<'a> = One<'a, Phases>;
    type Refusal = Infallible;

    const NAME: &'static str = "Move";

    const EVENTS: &'static [Constructor] = &[MOVED];

    fn writes(&self) -> impl Iterator<Item = Only> {
        std::iter::once(Only)
    }

    fn fields(&self) -> (Constructor, Vec<Value>) {
        (MOVED, vec![Value::str(self.0.as_str())])
    }

    fn parse(fields: &Fields<'_>) -> Result<Floored, ProdromeError> {
        let phase: String = fields.get("phase")?;
        Ok(Event::of(
            fields.get("review")?,
            fields.get("at")?,
            fields.get("actor")?,
            Move(Phase::parse(&phase)?),
        ))
    }
}

/// The stored names of the floored review schema. A changed name of a
/// constructor or a field, or a changed order of the fields, fails this
/// test.
#[test]
fn the_floored_schema_pins_its_stored_names() {
    prodrome::literal::codec::assert_pinned(Move::EVENTS, "Moved(review, at, actor, phase)");
}
