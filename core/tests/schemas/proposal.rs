//! The PROPOSAL schema, twice: in Rust, and as data ([`DECLARED`]), so the
//! two can be read side by side over every generated history (a
//! differential, `all/declared.rs`).
//!
//! A proposal is design §3's inbox entry. Its `state` is the machine
//! ordered by "further along", `standing < failed < sent`, `standing <
//! unknown < checked`, `standing < dismissed`, `standing < superseded`,
//! inflationary; beside it the three other orders of the declared
//! vocabulary: `attempts`, total over an integer; `tags`, sets under
//! inclusion, inflationary; and `text`, discrete. No valuation.

use std::collections::BTreeSet;

use prodrome::event::Actor;
use prodrome::fold::{
    Climb, Discrete, Frontier, Inflationary, Order, Product, Register, RegisterType, Total,
};
use prodrome::literal::{Call, Datetime, ProdromeError, Signature, Table, Value, Vocabulary};
use prodrome::payload::{datetime_field, required, string_field, strings, tuple_field};
use prodrome::registers::Stamp;
use prodrome::schema::Schema;
use proptest::prelude::*;

/// The proposal schema as data: the text a host would hand the store, in
/// its canonical print.
pub const DECLARED: &str = "Schema(key='proposal', events=(\
Event(name='Attempted', fields=(Field(name='proposal', type=Text()), Field(name='at', type=Instant()), \
Field(name='actor', type=Text()), Field(name='attempt', type=Integer()))), \
Event(name='Moved', fields=(Field(name='proposal', type=Text()), Field(name='at', type=Instant()), \
Field(name='actor', type=Text()), Field(name='state', type=Enum(alternatives=('checked', 'dismissed', \
'failed', 'sent', 'standing', 'superseded', 'unknown'))))), \
Event(name='Proposed', fields=(Field(name='proposal', type=Text()), Field(name='at', type=Instant()), \
Field(name='actor', type=Text()), Field(name='text', type=Text()))), \
Event(name='Tagged', fields=(Field(name='proposal', type=Text()), Field(name='at', type=Instant()), \
Field(name='actor', type=Text()), Field(name='tags', type=List(item=Text()))))), \
registers=(\
Register(name='attempts', order=Total(), inflationary=False, writes=(Write(event='Attempted', field='attempt'),)), \
Register(name='state', order=Machine(covers=(('failed', 'sent'), ('standing', 'dismissed'), \
('standing', 'failed'), ('standing', 'superseded'), ('standing', 'unknown'), ('unknown', 'checked'))), \
inflationary=True, writes=(Write(event='Moved', field='state'),)), \
Register(name='tags', order=Inclusion(), inflationary=True, writes=(Write(event='Tagged', field='tags'),)), \
Register(name='text', order=Discrete(), inflationary=False, writes=(Write(event='Proposed', field='text'),))))";

/// Further along is greater. No `PartialOrd`: [`Order`] is its only order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Standing,
    Failed,
    Sent,
    Unknown,
    Checked,
    Dismissed,
    Superseded,
}

pub const STATES: [State; 7] = [
    State::Standing,
    State::Failed,
    State::Sent,
    State::Unknown,
    State::Checked,
    State::Dismissed,
    State::Superseded,
];

impl Order for State {
    fn le(&self, other: &Self) -> bool {
        use State::*;
        self == other
            || matches!(
                (self, other),
                (Standing, _) | (Failed, Sent) | (Unknown, Checked)
            )
    }
}

impl Inflationary for State {}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Standing => "standing",
            State::Failed => "failed",
            State::Sent => "sent",
            State::Unknown => "unknown",
            State::Checked => "checked",
            State::Dismissed => "dismissed",
            State::Superseded => "superseded",
        }
    }

    fn parse(text: &str) -> Result<State, ProdromeError> {
        STATES
            .into_iter()
            .find(|state| state.as_str() == text)
            .ok_or_else(|| ProdromeError::invalid(format!("Moved.state: no state {text:?}")))
    }
}

/// A proposal's name: any text, as the declared key is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub String);

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The events: what each says beside its proposal, instant and actor.
#[derive(Debug, Clone, PartialEq)]
pub enum Says {
    Proposed(String),
    Moved(State),
    Attempted(i64),
    Tagged(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub proposal: Id,
    pub at: Datetime,
    pub actor: Actor,
    pub says: Says,
}

const SIGNATURES: &[(&str, &[&str])] = &[
    ("Attempted", &["proposal", "at", "actor", "attempt"]),
    ("Moved", &["proposal", "at", "actor", "state"]),
    ("Proposed", &["proposal", "at", "actor", "text"]),
    ("Tagged", &["proposal", "at", "actor", "tags"]),
];

#[derive(Debug, Clone, Default)]
pub struct ProposalVocabulary;

impl Vocabulary for ProposalVocabulary {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        Table(SIGNATURES).find(name)
    }
}

/// The registers, in the order their names sort, as a declaration lists
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    Attempts,
    State,
    Tags,
    Text,
}

pub const FIELDS: [Field; 4] = [Field::Attempts, Field::State, Field::Tags, Field::Text];

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Field::Attempts => "attempts",
            Field::State => "state",
            Field::Tags => "tags",
            Field::Text => "text",
        }
    }
}

pub struct StateRegister;
pub struct AttemptsRegister;
pub struct TagsRegister;
pub struct TextRegister;

impl RegisterType<Proposal> for StateRegister {
    type Value<'e> = State;
    fn value(event: &Proposal) -> Option<State> {
        match event.says {
            Says::Moved(state) => Some(state),
            _ => None,
        }
    }
}

impl RegisterType<Proposal> for AttemptsRegister {
    type Value<'e> = Total<i64>;
    fn value(event: &Proposal) -> Option<Total<i64>> {
        match event.says {
            Says::Attempted(attempt) => Some(Total(attempt)),
            _ => None,
        }
    }
}

impl RegisterType<Proposal> for TagsRegister {
    type Value<'e> = BTreeSet<&'e str>;
    fn value(event: &Proposal) -> Option<BTreeSet<&str>> {
        match &event.says {
            Says::Tagged(tags) => Some(tags.iter().map(String::as_str).collect()),
            _ => None,
        }
    }
}

impl RegisterType<Proposal> for TextRegister {
    type Value<'e> = Discrete<&'e str>;
    fn value(event: &Proposal) -> Option<Discrete<&str>> {
        match &event.says {
            Says::Proposed(text) => Some(Discrete(text.as_str())),
            _ => None,
        }
    }
}

/// A proposal's registers: two frontiers, and the two inflationary
/// registers, which climb.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Registers<'a> {
    attempts: Frontier<'a, Proposal>,
    state: Climb<'a, Proposal>,
    tags: Climb<'a, Proposal>,
    text: Frontier<'a, Proposal>,
}

impl<'a> Product<'a> for Registers<'a> {
    type Schema = Proposal;

    fn registers(&self) -> Vec<Field> {
        FIELDS.to_vec()
    }

    fn frontier(&self, field: Field) -> &Frontier<'a, Proposal> {
        match field {
            Field::Attempts => &self.attempts,
            Field::State => self.state.frontier(),
            Field::Tags => self.tags.frontier(),
            Field::Text => &self.text,
        }
    }

    fn join_to(&mut self, field: Field, stamp: &'a Stamp<Proposal>) {
        match field {
            Field::Attempts => self.attempts.join(stamp),
            Field::State => self.state.join::<StateRegister>(stamp),
            Field::Tags => self.tags.join::<TagsRegister>(stamp),
            Field::Text => self.text.join(stamp),
        }
    }

    fn reading(&self, field: Field) -> Vec<&'a Stamp<Proposal>> {
        match field {
            Field::Attempts => self.attempts.reading::<AttemptsRegister>(),
            Field::State => self.state.reading::<StateRegister>(),
            Field::Tags => self.tags.reading::<TagsRegister>(),
            Field::Text => self.text.reading::<TextRegister>(),
        }
    }

    /// The state and the tags are inflationary.
    fn grows(&self, event: &Proposal) -> Result<(), ProdromeError> {
        self.state.grows::<StateRegister>(event)?;
        self.tags.grows::<TagsRegister>(event)
    }

    /// The state and the tags never fall (law 32).
    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError> {
        self.state.climbed::<StateRegister>(&earlier.state)?;
        self.tags.climbed::<TagsRegister>(&earlier.tags)
    }
}

fn field(name: &str, value: Value) -> (String, Value) {
    (name.to_owned(), value)
}

impl Schema for Proposal {
    type Vocabulary = ProposalVocabulary;
    type Key = Id;
    type Register = Field;
    type Registers<'a> = Registers<'a>;

    fn registers(_: &ProposalVocabulary) -> Vec<Field> {
        FIELDS.to_vec()
    }

    fn to_value(&self) -> Value {
        let (name, last) = match &self.says {
            Says::Proposed(text) => ("Proposed", field("text", Value::str(text.as_str()))),
            Says::Moved(state) => ("Moved", field("state", Value::str(state.as_str()))),
            Says::Attempted(attempt) => ("Attempted", field("attempt", Value::int(*attempt))),
            Says::Tagged(tags) => (
                "Tagged",
                field(
                    "tags",
                    Value::Tuple(tags.iter().map(|tag| Value::str(tag.as_str())).collect()),
                ),
            ),
        };
        Value::call(
            name,
            vec![
                field("proposal", Value::str(self.proposal.0.as_str())),
                field("at", Value::Datetime(self.at)),
                field("actor", Value::str(self.actor.as_str())),
                last,
            ],
        )
    }

    fn from_value(_: &ProposalVocabulary, value: &Value) -> Result<Proposal, ProdromeError> {
        let call: &Call = value
            .as_call()
            .ok_or_else(|| ProdromeError::invalid("a proposal event is a constructor call"))?;
        let says = match call.name.as_str() {
            "Proposed" => Says::Proposed(string_field(call, "text")?),
            "Moved" => Says::Moved(State::parse(&string_field(call, "state")?)?),
            "Attempted" => Says::Attempted(
                match required(call, "attempt")? {
                    Value::Int(int) => int.as_i64(),
                    _ => None,
                }
                .ok_or_else(|| ProdromeError::invalid("Attempted.attempt must be an integer"))?,
            ),
            "Tagged" => Says::Tagged(strings(tuple_field(call, "tags")?, "Tagged.tags")?),
            other => {
                return Err(ProdromeError::invalid(format!(
                    "{other} is not a proposal event"
                )))
            }
        };
        Ok(Proposal {
            proposal: Id(string_field(call, "proposal")?),
            at: datetime_field(call, "at")?,
            actor: Actor::new(string_field(call, "actor")?)?,
            says,
        })
    }

    fn key(&self) -> &Id {
        &self.proposal
    }

    fn at(&self) -> Datetime {
        self.at
    }

    fn actor(&self) -> &Actor {
        &self.actor
    }

    fn writes(&self) -> impl Iterator<Item = Field> {
        std::iter::once(match self.says {
            Says::Proposed(_) => Field::Text,
            Says::Moved(_) => Field::State,
            Says::Attempted(_) => Field::Attempts,
            Says::Tagged(_) => Field::Tags,
        })
    }
}

/// A proposal event of either of two proposals, on one of five days, of
/// any constructor: the generator `all/declared.rs` and the wasm exports
/// draw histories of.
pub fn a_proposal() -> impl Strategy<Value = Proposal> {
    let says = prop_oneof![
        (0..3u8).prop_map(|n| Says::Proposed(format!("text {n}"))),
        prop::sample::select(STATES.to_vec()).prop_map(Says::Moved),
        (-2..4i64).prop_map(Says::Attempted),
        prop::collection::vec(prop::sample::select(vec!["a", "b", "c"]), 0..3)
            .prop_map(|tags| Says::Tagged(tags.into_iter().map(str::to_owned).collect())),
    ];
    (0..2usize, 1..6u32, says).prop_map(|(p, d, says)| Proposal {
        proposal: Id(["p-1", "p-2"][p].to_owned()),
        at: Datetime::new(2026, 10, d, 12, 0, 0, 0).expect("a real instant"),
        actor: Actor::new("ana").expect("an actor"),
        says,
    })
}
