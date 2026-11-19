//! The HOLDER schema, beside the tests rather than in one: a shelf, whose
//! one register holds a history of the schema `I` by its heads (design
//! §6.3). `Holder<Review>` is a shelf of reviews, and `Holder<Holder<Review>>`
//! a shelf of shelves, so one schema makes nests of any depth over any
//! inner schema.

use std::collections::BTreeSet;

use prodrome::event::{Actor, Hash};
use prodrome::fold::{Frontier, Product, Register, RegisterType};
use prodrome::literal::{Call, Datetime, ProdromeError, Signature, Table, Value, Vocabulary};
use prodrome::nest::{Heads, Nests};
use prodrome::payload::{datetime_field, string_field};
use prodrome::registers::Stamp;
use prodrome::schema::Schema;

/// A shelf's name: `^[a-z0-9-]+$`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Shelf(String);

impl Shelf {
    pub fn new(text: &str) -> Result<Shelf, ProdromeError> {
        let named = !text.is_empty()
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if named {
            Ok(Shelf(text.to_owned()))
        } else {
            Err(ProdromeError::invalid(format!(
                "Shelf must match ^[a-z0-9-]+$, got {text:?}"
            )))
        }
    }
}

impl AsRef<str> for Shelf {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The holder schema's one event: a shelf pointed at the heads of a history
/// of `I`, the heads its writer saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Holder<I> {
    pub shelf: Shelf,
    pub at: Datetime,
    pub actor: Actor,
    pub heads: Heads<I>,
}

pub fn placed<I>(shelf: &str, at: Datetime, actor: &str, heads: BTreeSet<Hash>) -> Holder<I> {
    Holder {
        shelf: Shelf::new(shelf).expect("a shelf name"),
        at,
        actor: Actor::new(actor).expect("an actor"),
        heads: Heads::new(heads),
    }
}

const HOLDER_SIGNATURES: &[(&str, &[&str])] = &[("Placed", &["shelf", "at", "actor", "heads"])];

#[derive(Debug, Clone, Default)]
pub struct HolderVocabulary;

impl Vocabulary for HolderVocabulary {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        Table(HOLDER_SIGNATURES).find(name)
    }
}

/// The one register: the history a shelf holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    Held,
}

/// The held register's type: heads of a history of `I`, under inclusion.
pub struct HeldRegister;

impl<I: Schema> RegisterType<Holder<I>> for HeldRegister {
    type Value<'e> = &'e Heads<I>;

    fn value(event: &Holder<I>) -> Option<&Heads<I>> {
        Some(&event.heads)
    }
}

/// A shelf's registers: a product of one, what it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Slots<'a, I: Schema>(Frontier<'a, Holder<I>>);

impl<I: Schema> Default for Slots<'_, I> {
    fn default() -> Self {
        Slots(Frontier::default())
    }
}

impl<'a, I: Schema> Product<'a> for Slots<'a, I> {
    type Schema = Holder<I>;

    fn registers(&self) -> Vec<Slot> {
        vec![Slot::Held]
    }

    fn join(&mut self, stamp: &'a Stamp<Holder<I>>) {
        self.0.join(stamp);
    }

    fn frontier(&self, _held: Slot) -> &Frontier<'a, Holder<I>> {
        &self.0
    }

    fn join_to(&mut self, _held: Slot, stamp: &'a Stamp<Holder<I>>) {
        self.0.join(stamp);
    }

    fn reading(&self, _held: Slot) -> Vec<&'a Stamp<Holder<I>>> {
        self.0.reading::<HeldRegister>()
    }

    /// Not inflationary: a pointer moves to what its writer saw.
    fn grows(&self, _event: &Holder<I>) -> Result<(), ProdromeError> {
        Ok(())
    }

    fn climbed(&self, _: &Self) -> Result<(), ProdromeError> {
        Ok(())
    }

    fn climbing(&self) -> Vec<Slot> {
        Vec::new()
    }
}

fn field(name: &str, value: Value) -> (String, Value) {
    (name.to_owned(), value)
}

impl<I: Schema> Schema for Holder<I> {
    type Vocabulary = HolderVocabulary;
    type Key = Shelf;
    type Register = Slot;

    fn registers(_: &HolderVocabulary) -> Vec<Slot> {
        vec![Slot::Held]
    }

    type Registers<'a> = Slots<'a, I>;

    fn to_value(&self) -> Value {
        Value::call(
            "Placed",
            vec![
                field("shelf", Value::str(self.shelf.0.as_str())),
                field("at", Value::Datetime(self.at)),
                field("actor", Value::str(self.actor.as_str())),
                field("heads", self.heads.to_value()),
            ],
        )
    }

    fn from_value(_: &HolderVocabulary, value: &Value) -> Result<Holder<I>, ProdromeError> {
        let call: &Call = prodrome::literal::codec::call(value)?;
        if call.name != "Placed" {
            return Err(prodrome::literal::codec::no_arm("Holder", &call.name));
        }
        Ok(Holder {
            shelf: Shelf::new(&string_field(call, "shelf")?)?,
            at: datetime_field(call, "at")?,
            actor: Actor::new(string_field(call, "actor")?)?,
            heads: Heads::from_call(call, "heads")?,
        })
    }

    fn key(&self) -> &Shelf {
        &self.shelf
    }

    fn at(&self) -> Datetime {
        self.at
    }

    fn actor(&self) -> &Actor {
        &self.actor
    }

    fn writes(&self) -> impl Iterator<Item = Slot> {
        std::iter::once(Slot::Held)
    }
}

impl<I: Schema> Nests for Holder<I> {
    const HELD: &'static [Slot] = &[Slot::Held];

    fn segment(_held: Slot) -> &'static str {
        "held"
    }

    fn heads(&self, _held: Slot) -> Option<&BTreeSet<Hash>> {
        Some(self.heads.names())
    }
}

/// The stored names of the holder schema. A changed name of a constructor or
/// a field, or a changed order of the fields, fails this test.
#[test]
fn the_holder_schema_pins_its_stored_names() {
    prodrome::literal::codec::assert_pinned(
        HOLDER_SIGNATURES,
        "
        Placed(shelf, at, actor, heads)
        ",
    );
}
