//! The REVIEW schema, beside the tests rather than in one: a review, whose
//! one register is a machine ordered by "further along", `Draft < Review <
//! Merged` and `Draft < Closed`, declared inflationary, priced by a flat
//! term per phase that does not change with its history ([`Price`]).
//! The core's tests (`all/review.rs`, `all/schema_laws.rs`) and the wasm's
//! read the same schema, so it depends on nothing but the core.

use std::collections::HashSet;

use prodrome::event::Actor;
use prodrome::fold::{Climb, Frontier, Inflationary, Order, Product, RegisterType};
use prodrome::fpl::{mk_flat, FplError};
use prodrome::literal::{Call, Datetime, ProdromeError, Signature, Table, Value, Vocabulary};
use prodrome::payload::{datetime_field, string_field};
use prodrome::registers::Stamp;
use prodrome::schema::{Price, Schema};
use prodrome::term::Term;

/// Further along is greater; `Merged` and `Closed` are the one incomparable
/// pair, so a merge on one replica and a close on another is the one real
/// conflict. No `PartialOrd`: [`Order`] is the only order it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Draft,
    Review,
    Merged,
    Closed,
}

pub const PHASES: [Phase; 4] = [Phase::Draft, Phase::Review, Phase::Merged, Phase::Closed];

impl Order for Phase {
    fn le(&self, other: &Self) -> bool {
        use Phase::*;
        matches!(
            (self, other),
            (Draft, _) | (Review, Review | Merged) | (Merged, Merged) | (Closed, Closed)
        )
    }
}

impl Inflationary for Phase {}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Draft => "draft",
            Phase::Review => "review",
            Phase::Merged => "merged",
            Phase::Closed => "closed",
        }
    }

    fn parse(text: &str) -> Result<Phase, ProdromeError> {
        PHASES
            .into_iter()
            .find(|phase| phase.as_str() == text)
            .ok_or_else(|| ProdromeError::invalid(format!("Moved.phase: no phase {text:?}")))
    }
}

/// A review's name: `^[a-z0-9-]+$`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pr(String);

impl Pr {
    pub fn new(text: &str) -> Result<Pr, ProdromeError> {
        let named = !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if named {
            Ok(Pr(text.to_owned()))
        } else {
            Err(ProdromeError::invalid(format!(
                "Pr must match ^[a-z0-9-]+$, got {text:?}"
            )))
        }
    }
}

impl AsRef<str> for Pr {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The review schema's events: a review opened, and a review moved to a
/// phase. Nothing else is a value of the type, so nothing else parses.
#[derive(Debug, Clone, PartialEq)]
pub enum Review {
    Opened {
        pr: Pr,
        at: Datetime,
        actor: Actor,
        title: String,
    },
    Moved {
        pr: Pr,
        at: Datetime,
        actor: Actor,
        phase: Phase,
    },
}

pub fn opened(pr: &str, at: Datetime, actor: &str, title: &str) -> Review {
    Review::Opened {
        pr: Pr::new(pr).expect("a review name"),
        at,
        actor: Actor::new(actor).expect("an actor"),
        title: title.to_owned(),
    }
}

pub fn moved(pr: &str, at: Datetime, actor: &str, phase: Phase) -> Review {
    Review::Moved {
        pr: Pr::new(pr).expect("a review name"),
        at,
        actor: Actor::new(actor).expect("an actor"),
        phase,
    }
}

const REVIEW_SIGNATURES: &[(&str, &[&str])] = &[
    ("Opened", &["pr", "at", "actor", "title"]),
    ("Moved", &["pr", "at", "actor", "phase"]),
];

#[derive(Debug, Clone, Default)]
pub struct ReviewVocabulary;

impl Vocabulary for ReviewVocabulary {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        Table(REVIEW_SIGNATURES).find(name)
    }
}

/// The one register a write supersedes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    Phase,
}

/// The phase register's type: a [`Phase`], under its order.
pub struct PhaseRegister;

impl RegisterType<Review> for PhaseRegister {
    type Value<'e> = Phase;

    fn value(event: &Review) -> Option<Phase> {
        match event {
            Review::Moved { phase, .. } => Some(*phase),
            Review::Opened { .. } => None,
        }
    }
}

/// A review's registers: a product of one, its phase, which climbs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Phases<'a>(Climb<'a, Review>);

impl Phases<'_> {
    /// The reading: the greatest phases of all its writes.
    pub fn phases(&self) -> HashSet<Phase> {
        self.0
            .reading::<PhaseRegister>()
            .into_iter()
            .filter_map(|stamp| PhaseRegister::value(&stamp.event))
            .collect()
    }
}

impl<'a> Product<'a> for Phases<'a> {
    type Schema = Review;

    fn registers(&self) -> Vec<Field> {
        vec![Field::Phase]
    }

    fn frontier(&self, _phase: Field) -> &Frontier<'a, Review> {
        self.0.frontier()
    }

    fn join_to(&mut self, _phase: Field, stamp: &'a Stamp<Review>) {
        self.0.join::<PhaseRegister>(stamp);
    }

    fn reading(&self, _phase: Field) -> Vec<&'a Stamp<Review>> {
        self.0.reading::<PhaseRegister>()
    }

    /// The phase is inflationary: a move must not fall below, or beside,
    /// the phase it supersedes.
    fn grows(&self, event: &Review) -> Result<(), ProdromeError> {
        self.0.grows::<PhaseRegister>(event)
    }

    /// The phase never falls (law 32).
    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError> {
        self.0.climbed::<PhaseRegister>(&earlier.0)
    }

    fn climbing(&self) -> Vec<Field> {
        vec![Field::Phase]
    }
}

fn field(name: &str, value: Value) -> (String, Value) {
    (name.to_owned(), value)
}

impl Schema for Review {
    type Vocabulary = ReviewVocabulary;
    type Key = Pr;
    type Register = Field;

    fn registers(_: &ReviewVocabulary) -> Vec<Field> {
        vec![Field::Phase]
    }

    type Registers<'a> = Phases<'a>;

    fn to_value(&self) -> Value {
        match self {
            Review::Opened {
                pr,
                at,
                actor,
                title,
            } => Value::call(
                "Opened",
                vec![
                    field("pr", Value::str(pr.0.as_str())),
                    field("at", Value::Datetime(*at)),
                    field("actor", Value::str(actor.as_str())),
                    field("title", Value::str(title.as_str())),
                ],
            ),
            Review::Moved {
                pr,
                at,
                actor,
                phase,
            } => Value::call(
                "Moved",
                vec![
                    field("pr", Value::str(pr.0.as_str())),
                    field("at", Value::Datetime(*at)),
                    field("actor", Value::str(actor.as_str())),
                    field("phase", Value::str(phase.as_str())),
                ],
            ),
        }
    }

    fn from_value(_: &ReviewVocabulary, value: &Value) -> Result<Review, ProdromeError> {
        let call: &Call = prodrome::literal::codec::call(value)?;
        let pr = Pr::new(&string_field(call, "pr")?)?;
        let at = datetime_field(call, "at")?;
        let actor = Actor::new(string_field(call, "actor")?)?;
        match call.name.as_str() {
            "Opened" => Ok(Review::Opened {
                pr,
                at,
                actor,
                title: string_field(call, "title")?,
            }),
            "Moved" => Ok(Review::Moved {
                pr,
                at,
                actor,
                phase: Phase::parse(&string_field(call, "phase")?)?,
            }),
            other => Err(prodrome::literal::codec::no_arm("Review", other)),
        }
    }

    fn key(&self) -> &Pr {
        match self {
            Review::Opened { pr, .. } | Review::Moved { pr, .. } => pr,
        }
    }

    fn at(&self) -> Datetime {
        match self {
            Review::Opened { at, .. } | Review::Moved { at, .. } => *at,
        }
    }

    fn actor(&self) -> &Actor {
        match self {
            Review::Opened { actor, .. } | Review::Moved { actor, .. } => actor,
        }
    }

    fn writes(&self) -> impl Iterator<Item = Field> {
        PhaseRegister::value(self).map(|_| Field::Phase).into_iter()
    }
}

/// Each maximal phase prices as a flat term: a draft as 0.25, a review as
/// 0.5, a merge as fulfilled and a close as nothing fulfilled, so the one
/// real conflict prices as the close. A review only opened has no price.
impl Price for Review {
    fn terms(phases: &Phases<'_>) -> Result<Vec<Term>, FplError> {
        phases
            .0
            .reading::<PhaseRegister>()
            .into_iter()
            .filter_map(|stamp| PhaseRegister::value(&stamp.event))
            .map(|phase| {
                mk_flat(match phase {
                    Phase::Draft => 0.25,
                    Phase::Review => 0.5,
                    Phase::Merged => 1.0,
                    Phase::Closed => 0.0,
                })
            })
            .collect()
    }
}

pub fn day(day: u32) -> Datetime {
    Datetime::new(2026, 10, day, 12, 0, 0, 0).expect("a real instant")
}

/// The stored names of the review schema. A changed name of a constructor or
/// a field, or a changed order of the fields, fails this test.
#[test]
fn the_review_schema_pins_its_stored_names() {
    prodrome::literal::codec::assert_pinned(
        REVIEW_SIGNATURES,
        "
        Opened(pr, at, actor, title)
        Moved(pr, at, actor, phase)
        ",
    );
}
