//! One generic event for an application schema: a key, `at`, `actor` and a
//! body.
//!
//! An event of such a schema prints as `Constructor(<key>, at, actor, <body
//! fields>)`. The key says which entity the event is about. The field `at`
//! is the instant that the writer stamped, which is data and never an
//! order. The field `actor` names the writer. The body is what the event
//! says, and the schema defines it as a type that implements [`Says`].
//! [`Event`] adds the key, `at` and `actor`, and implements [`Schema`], so a
//! store can be opened at `Event<S>`.
//!
//! A schema whose events write one register can use
//! [`crate::fold::sole::One`] as its product.

#![warn(clippy::pedantic)]

use std::fmt::Debug;
use std::marker::PhantomData;

use crate::event::Actor;
use crate::fold::Product;
use crate::literal::codec::{record, Constructor, Fields, Literal};
use crate::literal::{Datetime, ProdromeError, Signature, Table, Value, Vocabulary};
use crate::schema::Schema;

/// One event: the key of its entity, its instant, its writer and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event<S: Says> {
    key: S::Key,
    at: Datetime,
    actor: Actor,
    said: S,
}

/// The body of an event, and the parts of the schema that the body decides.
///
/// An implementation gives:
///
/// - the type of the key, and the registers that a write can supersede in;
/// - each event constructor, with its fields in printed order: the key's
///   fields, then `at` and `actor`, then the body's fields;
/// - the registers that a body writes, and whether a policy is asked about
///   it ([`Schema::writes`] and [`Schema::asks`]);
/// - the print of the body's fields ([`Says::body`]), and the parse of a
///   whole event ([`Says::parse`]), which goes through the schema's smart
///   constructors.
pub trait Says: Clone + PartialEq + Debug + Send + Sync + Sized + 'static {
    /// The key of the entity that an event is about.
    type Key: Head;

    /// The name of a register that a write supersedes in.
    type Register: Copy + Ord + Debug + Send + Sync + 'static;

    /// The registers of one entity.
    type Registers<'a>: Product<'a, Schema = Event<Self>>;

    /// Each event constructor. Its fields are the key's fields, then `at`
    /// and `actor`, then the body's fields.
    const EVENTS: &'static [Constructor];

    /// The constructors that occur inside the fields of an event.
    const NESTED: &'static [Constructor] = &[];

    /// Every register that a write supersedes in, in the order that a
    /// reading lists them.
    fn registers() -> Vec<Self::Register>;

    /// The registers that an event with this body writes, each once.
    fn writes(&self) -> impl Iterator<Item = Self::Register>;

    /// Is an event with this body about standing? The reference policy
    /// binds an event that is not, whoever wrote it.
    #[must_use]
    fn asks(&self) -> bool {
        true
    }

    /// The body's constructor, and the values of its fields after `actor`.
    fn body(&self) -> (Constructor, Vec<Value>);

    /// The event that `fields` print, built through the schema's smart
    /// constructors.
    ///
    /// # Errors
    ///
    /// A field that is missing, or that a smart constructor refuses.
    fn parse(fields: &Fields<'_>) -> Result<Event<Self>, ProdromeError>;
}

/// The fields of a key, as the start of an event's print.
///
/// A key whose literal is one field gets this trait from [`Literal`]. A key
/// of several fields implements it directly.
pub trait Head: Clone + Ord + Debug + Send + Sync {
    /// The values of the key's fields, in printed order.
    fn head(&self) -> Vec<Value>;
}

impl<T: Literal + Clone + Ord + Debug + Send + Sync> Head for T {
    fn head(&self) -> Vec<Value> {
        vec![self.to_value()]
    }
}

impl<S: Says> Event<S> {
    /// The event about `key`, stamped `at` by `actor`, with the body `said`.
    /// A schema's smart constructors call this after they check the body.
    pub fn of(key: S::Key, at: Datetime, actor: Actor, said: S) -> Self {
        Self {
            key,
            at,
            actor,
            said,
        }
    }

    /// The key of the entity that the event is about.
    #[must_use]
    pub fn key(&self) -> &S::Key {
        &self.key
    }

    /// The body.
    #[must_use]
    pub fn said(&self) -> &S {
        &self.said
    }
}

/// The vocabulary of `Event<S>`: the event constructors of `S`, then the
/// nested ones.
pub struct Vocab<S>(PhantomData<fn() -> S>);

impl<S> Default for Vocab<S> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<S> Clone for Vocab<S> {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl<S> Debug for Vocab<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Vocab")
    }
}

impl<S: Says> Vocabulary for Vocab<S> {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        Table(S::EVENTS)
            .find(name)
            .or_else(|| Table(S::NESTED).find(name))
    }
}

impl<S: Says> Schema for Event<S> {
    type Vocabulary = Vocab<S>;
    type Key = S::Key;
    type Register = S::Register;

    fn registers(_: &Vocab<S>) -> Vec<S::Register> {
        S::registers()
    }

    type Registers<'a> = S::Registers<'a>;

    fn to_value(&self) -> Value {
        let (constructor, body) = self.said.body();
        let fields = self
            .key
            .head()
            .into_iter()
            .chain([self.at.to_value(), self.actor.to_value()])
            .chain(body)
            .collect();
        record(constructor, fields)
    }

    fn from_value(_: &Vocab<S>, value: &Value) -> Result<Self, ProdromeError> {
        S::parse(&Fields::among(value, S::EVENTS)?)
    }

    fn key(&self) -> &S::Key {
        &self.key
    }

    fn at(&self) -> Datetime {
        self.at
    }

    fn actor(&self) -> &Actor {
        &self.actor
    }

    fn writes(&self) -> impl Iterator<Item = S::Register> {
        self.said.writes()
    }

    fn asks(&self) -> bool {
        self.said.asks()
    }
}
