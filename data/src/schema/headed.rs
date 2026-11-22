//! A generic event type for an application schema: a key, `at`, `actor` and
//! a body.
//!
//! An event of such a schema prints as `Constructor(<key fields>, at,
//! actor, <body fields>)`. A schema implements [`Body`], and a program
//! opens a store at `Event<B>`. Every event passed [`Body::admits`]:
//! [`Event::new`] asks it, and [`Event::of`] serves only a body whose
//! `admits` refuses nothing. A schema whose events write one register can
//! use [`crate::fold::sole::One`] as its product.
//!
//! # Example
//!
//! A complete schema of notes with one register, as the crate's tests use
//! it:
//!
#![doc = concat!("```\n", include_str!("headed/example.rs"), "```")]
#![warn(clippy::pedantic)]

use std::convert::Infallible;
use std::fmt::{Debug, Display};
use std::marker::PhantomData;

use crate::event::Actor;
use crate::fold::Product;
use crate::literal::codec::{record, Constructor, Fields, Literal};
use crate::literal::{Datetime, ProdromeError, Signature, Table, Value, Vocabulary};
use crate::schema::Schema;

/// One event: the key of its entity, its instant, its writer and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event<B: Body> {
    key: B::Key,
    at: Datetime,
    actor: Actor,
    body: B,
}

/// The part of an event that a schema defines, and the parts of the schema
/// that it decides.
pub trait Body: Clone + PartialEq + Debug + Send + Sync + 'static {
    /// The key of the entity that an event is about.
    type Key: KeyFields;

    /// The name of a register that a write supersedes in.
    type Register: Copy + Ord + Debug + Send + Sync + 'static;

    /// The registers of one entity.
    type Registers<'a>: Product<'a, Schema = Event<Self>>;

    /// Why [`Body::admits`] refuses a key and a body.
    /// [`std::convert::Infallible`] for a schema that refuses nothing.
    type Refusal: Display;

    /// The name of the body's type, which a refusal of an unknown event
    /// constructor names, as [`no_arm`](crate::literal::codec::no_arm)
    /// words it: `Other is not one of Entry's constructors`.
    const NAME: &'static str;

    /// Each event constructor. Its fields are the key's fields, then `at`
    /// and `actor`, then the body's fields. No constructor may have an
    /// envelope's name ([`crate::event::ENVELOPE_SIGNATURES`]), because a
    /// parse looks up the envelopes first.
    const EVENTS: &'static [Constructor];

    /// The constructors that occur inside the fields of an event.
    const NESTED: &'static [Constructor] = &[];

    /// The registers that an event with this body writes, each once.
    fn writes(&self) -> impl Iterator<Item = Self::Register>;

    /// Whether an event with this body is about standing (SPEC §5).
    #[must_use]
    fn asks(&self) -> bool {
        true
    }

    /// Checks that `key` and `body` can make an event. By default every pair
    /// can.
    ///
    /// # Errors
    ///
    /// A key and a body that the schema refuses together.
    fn admits(_key: &Self::Key, _body: &Self) -> Result<(), Self::Refusal> {
        Ok(())
    }

    /// The body's constructor, and the values of its fields after `actor`.
    fn fields(&self) -> (Constructor, Vec<Value>);

    /// The event that `fields` print. An implementation builds it with
    /// [`Event::new`], so the parse refuses what [`Body::admits`] refuses.
    ///
    /// # Errors
    ///
    /// A field that is missing or does not read, or a refusal of
    /// [`Body::admits`].
    fn parse(fields: &Fields<'_>) -> Result<Event<Self>, ProdromeError>;
}

/// The fields of a key, at the start of an event's print.
///
/// A key whose literal is one field gets this trait from [`Literal`]. A key
/// of several fields implements it directly.
pub trait KeyFields: Clone + Ord + Debug + Send + Sync {
    /// The values of the key's fields, in printed order.
    fn key_fields(&self) -> Vec<Value>;
}

impl<T: Literal + Clone + Ord + Debug + Send + Sync> KeyFields for T {
    fn key_fields(&self) -> Vec<Value> {
        vec![self.to_value()]
    }
}

impl<B: Body> Event<B> {
    /// The event about `key`, stamped `at` by `actor`, with the body `body`.
    ///
    /// # Errors
    ///
    /// The refusal of [`Body::admits`].
    pub fn new(key: B::Key, at: Datetime, actor: Actor, body: B) -> Result<Self, B::Refusal> {
        B::admits(&key, &body)?;
        Ok(Self {
            key,
            at,
            actor,
            body,
        })
    }

    #[must_use]
    pub fn body(&self) -> &B {
        &self.body
    }
}

impl<B: Body<Refusal = Infallible>> Event<B> {
    /// The total [`Event::new`], for a schema that refuses nothing.
    #[must_use]
    pub fn of(key: B::Key, at: Datetime, actor: Actor, body: B) -> Self {
        Self {
            key,
            at,
            actor,
            body,
        }
    }
}

/// The vocabulary of `Event<B>`: the event constructors of `B`, then the
/// nested ones.
pub struct Constructors<B>(PhantomData<fn() -> B>);

impl<B> Default for Constructors<B> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<B> Clone for Constructors<B> {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl<B> Debug for Constructors<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Constructors")
    }
}

impl<B: Body> Vocabulary for Constructors<B> {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        Table(B::EVENTS)
            .find(name)
            .or_else(|| Table(B::NESTED).find(name))
    }
}

impl<B: Body> Schema for Event<B> {
    type Vocabulary = Constructors<B>;
    type Key = B::Key;
    type Register = B::Register;

    fn registers(_: &Constructors<B>) -> Vec<B::Register> {
        B::Registers::default().registers()
    }

    type Registers<'a> = B::Registers<'a>;

    fn to_value(&self) -> Value {
        let (constructor, body) = self.body.fields();
        let fields = self
            .key
            .key_fields()
            .into_iter()
            .chain([self.at.to_value(), self.actor.to_value()])
            .chain(body)
            .collect();
        record(constructor, fields)
    }

    fn from_value(_: &Constructors<B>, value: &Value) -> Result<Self, ProdromeError> {
        B::parse(&Fields::among(value, B::NAME, B::EVENTS)?)
    }

    fn key(&self) -> &B::Key {
        &self.key
    }

    fn at(&self) -> Datetime {
        self.at
    }

    fn actor(&self) -> &Actor {
        &self.actor
    }

    fn writes(&self) -> impl Iterator<Item = B::Register> {
        self.body.writes()
    }

    fn asks(&self) -> bool {
        self.body.asks()
    }
}
