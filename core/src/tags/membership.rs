//! The membership schema: `Added` and `Removed`, each writing the one
//! register of its pair of a target and a tag.

use std::marker::PhantomData;

use crate::event::Actor;
use crate::fold::sole::{Bodies, One, Only};
use crate::fold::{Order, Product as _};
use crate::literal::codec::{no_arm, Constructor, Fields, Literal as _};
use crate::literal::{Datetime, ProdromeError, Value};
use crate::schema::headed::{Body, Constructors, Event, KeyFields};
use crate::tags::{Names, Tagging};

/// Whether a tag is on a target. `On` is above `Off`, so concurrent writes
/// read as `On` when any of them is `On`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Presence {
    /// The tag is off the target.
    Off,
    /// The tag is on the target.
    On,
}

/// The key of a membership: a target and a tag, as the writer spelled the
/// tag. An alias may spell the tag otherwise now.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pair<S: Tagging> {
    /// The target.
    pub target: S::Target,
    /// The tag.
    pub tag: S::Tag,
}

/// The body of a membership event: the presence that it writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership<S> {
    presence: Presence,
    of: PhantomData<fn() -> S>,
}

impl<S> Membership<S> {
    fn writing(presence: Presence) -> Self {
        Self {
            presence,
            of: PhantomData,
        }
    }
}

/// A tag put on a target or taken off it.
pub type MembershipEvent<S> = Event<Membership<S>>;

/// The vocabulary of the membership schema.
pub type MembershipVocabulary<S> = Constructors<Membership<S>>;

/// `tag` put on `target` by `actor`.
#[must_use]
pub fn added<S: Tagging>(
    target: S::Target,
    tag: S::Tag,
    at: Datetime,
    actor: Actor,
) -> MembershipEvent<S> {
    Event::of(
        Pair { target, tag },
        at,
        actor,
        Membership::writing(Presence::On),
    )
}

/// `tag` taken off `target` by `actor`.
#[must_use]
pub fn removed<S: Tagging>(
    target: S::Target,
    tag: S::Tag,
    at: Datetime,
    actor: Actor,
) -> MembershipEvent<S> {
    Event::of(
        Pair { target, tag },
        at,
        actor,
        Membership::writing(Presence::Off),
    )
}

/// The presence that a pair's registers read: the greatest value of the
/// frontier, and `Off` where nothing wrote it.
pub(crate) fn presence<S: Tagging>(registers: &One<'_, Bodies<Membership<S>>>) -> Presence {
    registers
        .reading(Only)
        .iter()
        .map(|stamp| stamp.event.body().presence)
        .max()
        .unwrap_or(Presence::Off)
}

impl<S: Tagging> KeyFields for Pair<S> {
    fn key_fields(&self) -> Vec<Value> {
        vec![self.target.to_value(), self.tag.to_value()]
    }
}

impl<S: Tagging> Membership<S> {
    const ADDED: Constructor = {
        let () = Names::<S>::CHECKED;
        ("Added", &[S::TARGET, S::TAG, "at", "actor"])
    };
    const REMOVED: Constructor = {
        let () = Names::<S>::CHECKED;
        ("Removed", &[S::TARGET, S::TAG, "at", "actor"])
    };
}

/// Under `Off < On`, a removal goes below an add only where it descends
/// from it.
impl<S> Order for Membership<S> {
    fn le(&self, other: &Self) -> bool {
        self.presence <= other.presence
    }
}

impl<S: Tagging> Body for Membership<S> {
    type Key = Pair<S>;
    type Register = Only;
    type Registers<'a> = One<'a, Bodies<Self>>;
    type Refusal = std::convert::Infallible;

    const EVENTS: &'static [Constructor] = &[Self::ADDED, Self::REMOVED];

    fn writes(&self) -> impl Iterator<Item = Only> {
        std::iter::once(Only)
    }

    fn fields(&self) -> (Constructor, Vec<Value>) {
        match self.presence {
            Presence::On => (Self::ADDED, Vec::new()),
            Presence::Off => (Self::REMOVED, Vec::new()),
        }
    }

    fn parse(fields: &Fields<'_>) -> Result<MembershipEvent<S>, ProdromeError> {
        let presence = match fields.name() {
            "Added" => Presence::On,
            "Removed" => Presence::Off,
            other => return Err(no_arm("a membership event", other)),
        };
        Ok(Event::of(
            Pair {
                target: fields.get(S::TARGET)?,
                tag: fields.get(S::TAG)?,
            },
            fields.get("at")?,
            fields.get("actor")?,
            Membership::writing(presence),
        ))
    }
}
