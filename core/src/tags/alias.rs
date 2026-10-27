//! The alias schema: `Aliased` makes a tag a name of another tag, and
//! `Unaliased` makes it a name of its own again. Each writes the tag's one
//! register, under the discrete order, so two different aliases written
//! concurrently for one tag are a conflict, which the reading shows and the
//! next rename settles.

use crate::event::Actor;
use crate::fold::sole::{Bodies, One, Only};
use crate::fold::Order;
use crate::literal::codec::{no_arm, Constructor, Fields, Literal as _};
use crate::literal::{Datetime, ProdromeError, Value};
use crate::schema::headed::{Body, Constructors, Event};
use crate::tags::{Names, Tagging};

/// What a tag's name stands for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Alias<S: Tagging> {
    /// The name is a tag of its own.
    Itself,
    /// The name stands for another tag. Never the tag itself: [`aliased`]
    /// refuses that.
    As(S::Tag),
}

/// A tag's alias event.
pub type AliasEvent<S> = Event<Alias<S>>;

/// The vocabulary of the alias schema.
pub type AliasVocabulary<S> = Constructors<Alias<S>>;

/// A tag given as an alias of itself.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} cannot be an alias of itself; it is a name of its own")]
pub struct OfItself<G: std::fmt::Debug>(pub G);

/// `tag` made an alias of `to` by `actor`.
///
/// # Errors
///
/// [`OfItself`] where `to` is `tag`, so that "its own name" has one
/// spelling, [`unaliased`].
pub fn aliased<S: Tagging>(
    tag: S::Tag,
    to: S::Tag,
    at: Datetime,
    actor: Actor,
) -> Result<AliasEvent<S>, OfItself<S::Tag>> {
    Event::new(tag, at, actor, Alias::As(to))
}

/// `tag` made a name of its own again by `actor`.
///
/// # Errors
///
/// None: the schema refuses only an alias of itself. The result is the
/// type of [`Event::new`], the schema's one constructor, which asks
/// [`Body::admits`] of every event.
pub fn unaliased<S: Tagging>(
    tag: S::Tag,
    at: Datetime,
    actor: Actor,
) -> Result<AliasEvent<S>, OfItself<S::Tag>> {
    Event::new(tag, at, actor, Alias::Itself)
}

impl<S: Tagging> Alias<S> {
    const ALIASED: Constructor = {
        let () = Names::<S>::CHECKED;
        ("Aliased", &[S::TAG, "at", "actor", "to"])
    };
    const UNALIASED: Constructor = {
        let () = Names::<S>::CHECKED;
        ("Unaliased", &[S::TAG, "at", "actor"])
    };
}

/// The discrete order: two different aliases are incomparable.
impl<S: Tagging> Order for Alias<S> {
    fn le(&self, other: &Self) -> bool {
        self == other
    }
}

impl<S: Tagging> Body for Alias<S> {
    type Key = S::Tag;
    type Register = Only;
    type Registers<'a> = One<'a, Bodies<Self>>;
    type Refusal = OfItself<S::Tag>;

    const EVENTS: &'static [Constructor] = &[Self::ALIASED, Self::UNALIASED];

    fn writes(&self) -> impl Iterator<Item = Only> {
        std::iter::once(Only)
    }

    fn admits(tag: &S::Tag, alias: &Self) -> Result<(), OfItself<S::Tag>> {
        match alias {
            Alias::As(to) if to == tag => Err(OfItself(tag.clone())),
            Alias::As(_) | Alias::Itself => Ok(()),
        }
    }

    fn fields(&self) -> (Constructor, Vec<Value>) {
        match self {
            Alias::As(to) => (Self::ALIASED, vec![to.to_value()]),
            Alias::Itself => (Self::UNALIASED, Vec::new()),
        }
    }

    fn parse(fields: &Fields<'_>) -> Result<AliasEvent<S>, ProdromeError> {
        let alias = match fields.name() {
            "Aliased" => Alias::As(fields.get("to")?),
            "Unaliased" => Alias::Itself,
            other => return Err(no_arm("an alias event", other)),
        };
        Event::new(
            fields.get(S::TAG)?,
            fields.get("at")?,
            fields.get("actor")?,
            alias,
        )
        .map_err(|refused| ProdromeError::invalid(refused.to_string()))
    }
}
