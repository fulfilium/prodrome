//! An INBOX of proposals, each priced by the todo it serves (design §0's
//! first fake, §6.3.1): a proposal says what it proposes (`text`, discrete)
//! and which entity of which store it serves (`serves`, discrete), and its
//! price is a qualified reference to that entity, `RefIn(store, todo)`.
//! A proposal that serves nothing has no price, which is not `Absent`.

use prodrome::event::Actor;
use prodrome::fold::{Discrete, Frontier, Product, Register, RegisterType};
use prodrome::fpl::{self, FplError};
use prodrome::literal::{Call, Datetime, ProdromeError, Signature, Table, Value, Vocabulary};
use prodrome::payload::{datetime_field, string_field};
use prodrome::registers::Stamp;
use prodrome::schema::{Price, Schema};
use prodrome::term::Term;

/// A proposal's name: any text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(pub String);

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// What an event says beside its proposal, instant and actor.
#[derive(Debug, Clone, PartialEq)]
pub enum Says {
    Proposed(String),
    /// The proposal serves `todo` in the store `store` names.
    Serves {
        store: String,
        todo: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Inbox {
    pub proposal: Id,
    pub at: Datetime,
    pub actor: Actor,
    pub says: Says,
}

const SIGNATURES: &[(&str, &[&str])] = &[
    ("Proposed", &["proposal", "at", "actor", "text"]),
    ("Serves", &["proposal", "at", "actor", "store", "todo"]),
];

#[derive(Debug, Clone, Default)]
pub struct InboxVocabulary;

impl Vocabulary for InboxVocabulary {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        Table(SIGNATURES).find(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    Serves,
    Text,
}

pub struct ServesRegister;
pub struct TextRegister;

impl RegisterType<Inbox> for ServesRegister {
    type Value<'e> = Discrete<(&'e str, &'e str)>;
    fn value(event: &Inbox) -> Option<Discrete<(&str, &str)>> {
        match &event.says {
            Says::Serves { store, todo } => Some(Discrete((store.as_str(), todo.as_str()))),
            Says::Proposed(_) => None,
        }
    }
}

impl RegisterType<Inbox> for TextRegister {
    type Value<'e> = Discrete<&'e str>;
    fn value(event: &Inbox) -> Option<Discrete<&str>> {
        match &event.says {
            Says::Proposed(text) => Some(Discrete(text.as_str())),
            Says::Serves { .. } => None,
        }
    }
}

/// A proposal's registers: a product of two frontiers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Registers<'a> {
    serves: Frontier<'a, Inbox>,
    text: Frontier<'a, Inbox>,
}

impl<'a> Product<'a> for Registers<'a> {
    type Schema = Inbox;

    fn registers(&self) -> Vec<Field> {
        vec![Field::Serves, Field::Text]
    }

    fn join(&mut self, stamp: &'a Stamp<Inbox>) {
        for field in stamp.event.writes() {
            self.frontier_mut(field).join(stamp);
        }
    }

    fn frontier(&self, field: Field) -> &Frontier<'a, Inbox> {
        match field {
            Field::Serves => &self.serves,
            Field::Text => &self.text,
        }
    }

    fn frontier_mut(&mut self, field: Field) -> &mut Frontier<'a, Inbox> {
        match field {
            Field::Serves => &mut self.serves,
            Field::Text => &mut self.text,
        }
    }

    fn reading(&self, field: Field) -> Vec<&'a Stamp<Inbox>> {
        match field {
            Field::Serves => self.serves.reading::<ServesRegister>(),
            Field::Text => self.text.reading::<TextRegister>(),
        }
    }

    fn grows(&self, _: &Inbox) -> Result<(), ProdromeError> {
        Ok(())
    }
}

fn field(name: &str, value: Value) -> (String, Value) {
    (name.to_owned(), value)
}

impl Schema for Inbox {
    type Vocabulary = InboxVocabulary;
    type Key = Id;
    type Register = Field;
    type Registers<'a> = Registers<'a>;

    fn registers(_: &InboxVocabulary) -> Vec<Field> {
        vec![Field::Serves, Field::Text]
    }

    fn to_value(&self) -> Value {
        let (name, rest) = match &self.says {
            Says::Proposed(text) => ("Proposed", vec![field("text", Value::str(text.as_str()))]),
            Says::Serves { store, todo } => (
                "Serves",
                vec![
                    field("store", Value::str(store.as_str())),
                    field("todo", Value::str(todo.as_str())),
                ],
            ),
        };
        let mut fields = vec![
            field("proposal", Value::str(self.proposal.0.as_str())),
            field("at", Value::Datetime(self.at)),
            field("actor", Value::str(self.actor.as_str())),
        ];
        fields.extend(rest);
        Value::call(name, fields)
    }

    fn from_value(_: &InboxVocabulary, value: &Value) -> Result<Inbox, ProdromeError> {
        let call: &Call = prodrome::literal::codec::call(value)?;
        let says = match call.name.as_str() {
            "Proposed" => Says::Proposed(string_field(call, "text")?),
            "Serves" => Says::Serves {
                store: string_field(call, "store")?,
                todo: string_field(call, "todo")?,
            },
            other => return Err(prodrome::literal::codec::no_arm("Inbox", other)),
        };
        Ok(Inbox {
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
            Says::Serves { .. } => Field::Serves,
        })
    }
}

/// A proposal's price is the price of what it serves: one qualified
/// reference per candidate, so two devices that disagree on what it serves
/// price it as the more urgent of the two (`Least`, law 34).
impl Price for Inbox {
    fn terms(registers: &Registers<'_>) -> Result<Vec<Term>, FplError> {
        registers
            .serves
            .reading::<ServesRegister>()
            .into_iter()
            .filter_map(|stamp| match &stamp.event.says {
                Says::Serves { store, todo } => Some(fpl::mk_ref_in(store.clone(), todo.clone())),
                Says::Proposed(_) => None,
            })
            .collect()
    }
}
