//! The steps of a script, and the strategy that draws them.

use proptest::prelude::*;

use crate::event::Actor;
use crate::schema::Schema;

/// One of the three actors of a script. Each actor writes on a replica of
/// its own, and each has keys of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Who {
    Ada,
    Ben,
    Cai,
}

impl Who {
    /// Every actor, in order.
    pub const ALL: [Who; 3] = [Who::Ada, Who::Ben, Who::Cai];

    /// The actor's position in [`Who::ALL`], which is its replica's.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Who::Ada => 0,
            Who::Ben => 1,
            Who::Cai => 2,
        }
    }

    /// The name that the actor's events carry.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Who::Ada => "ada",
            Who::Ben => "ben",
            Who::Cai => "cai",
        }
    }

    /// The actor as its events name it.
    ///
    /// # Panics
    ///
    /// Never: every name of `Who` is an actor's name.
    #[must_use]
    pub fn actor(self) -> Actor {
        Actor::new(self.name()).expect("every name of `Who` is an actor's name")
    }
}

/// Where an actor's replica is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A store in memory.
    Memory,
    /// An overlay over a store in memory, which only a flush writes to.
    Overlay,
    /// A store on disk, in a directory that the script removes when it
    /// ends.
    Disk,
}

/// How an append is written, and what `verify` must find of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writing {
    /// Appended, and signed with the current key of the event's actor, if
    /// the actor has a key. An honest write when the writer is that actor.
    Signed,
    /// Appended and not signed. Once the actor has a key, `verify` finds
    /// the object unsigned.
    Unsigned,
    /// Appended, and given a signature that does not verify. `verify` finds
    /// the signature forged.
    Forged,
    /// Written as a change whose deps also name the `dep`-th tip that is a
    /// write to another entity, and signed as [`Writing::Signed`]. `verify`
    /// finds the dep foreign. Where no tip fits, it is written as
    /// [`Writing::Signed`].
    Foreign { dep: usize },
}

/// An object that a replica must refuse to receive, offered beside an
/// object that it can receive, so that the receipt must take neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A print under a name that is not its hash.
    Tampered,
    /// A print, under its hash, that does not parse.
    Unparsed,
    /// A snapshot of a tip that no print is.
    Orphan,
}

/// How a read is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Through {
    /// Through the script's cache of readings, keyed by the instant and the
    /// replica's frontier token.
    Memo,
    /// Folded afresh.
    Fresh,
}

/// One step of a script.
///
/// Each step means something in every state. A position (`chosen`, `at`,
/// `with`, `key`, `dep`) is taken modulo the length of what it picks from,
/// and a step with nothing to pick from does nothing. So a script with
/// steps removed is still a script, and a failure shrinks to a short one.
#[derive(Debug, Clone, PartialEq)]
pub enum Step<E> {
    /// `by`'s replica appends `event`, an event that `of` writes, then
    /// appends it again, which must change nothing. When `by` is not `of`,
    /// `by` holds `of`'s key, as a thief would.
    Append {
        by: Who,
        of: Who,
        event: E,
        writing: Writing,
    },
    /// `to` receives, one receipt for each position in `chosen` and in that
    /// order, the object at that position among `from`'s, with what it
    /// rests on: a partial sync in any order, with duplicates. Each receipt
    /// is made twice, and the second must take nothing.
    Pick {
        from: Who,
        to: Who,
        chosen: Vec<usize>,
    },
    /// `to` receives everything `from` holds, twice.
    Sync { from: Who, to: Who },
    /// Every replica receives everything every other holds.
    SyncAll,
    /// `to` is offered an object it must refuse, beside the `with`-th object
    /// that it lacks.
    Refuse {
        to: Who,
        refusal: Refusal,
        with: usize,
    },
    /// `by` revokes its key, if it has one, standing behind everything its
    /// replica holds, and vouches again: the root registers a new key, and
    /// `by` signs with it every object of its own that its replica holds.
    Rotate { by: Who },
    /// `by` reads at the `at`-th instant its events are dated (0: ever).
    Read {
        by: Who,
        at: usize,
        through: Through,
    },
    /// `by` records its reading at the `at`-th instant and at the down-set
    /// it holds. Every replica that holds that down-set later must read it
    /// the same.
    Record { by: Who, at: usize },
    /// `by` writes a snapshot of its tips, and records the reading at it.
    Snapshot { by: Who },
    /// The cache of readings forgets the `key`-th key it holds.
    Forget { key: usize },
    /// `by`'s replica, if it is an overlay, flushes into its base, twice.
    Flush { by: Who },
}

impl<E> Step<E> {
    /// Is the step one that an honest actor takes? A write with a fault,
    /// or under another actor's name, is not.
    pub fn honest(&self) -> bool {
        match self {
            Step::Append {
                by, of, writing, ..
            } => by == of && *writing == Writing::Signed,
            Step::Pick { .. }
            | Step::Sync { .. }
            | Step::SyncAll
            | Step::Refuse { .. }
            | Step::Rotate { .. }
            | Step::Read { .. }
            | Step::Record { .. }
            | Step::Snapshot { .. }
            | Step::Forget { .. }
            | Step::Flush { .. } => true,
        }
    }
}

fn a_who() -> impl Strategy<Value = Who> {
    prop::sample::select(Who::ALL.to_vec())
}

/// How a write is written: honestly, or, with a weight of `faults` for
/// each kind of fault against 20 for an honest write, with a fault.
fn a_writing(faults: u32) -> BoxedStrategy<Writing> {
    if faults == 0 {
        return Just(Writing::Signed).boxed();
    }
    prop_oneof![
        20 => Just(Writing::Signed),
        faults => Just(Writing::Unsigned),
        faults => Just(Writing::Forged),
        faults => (0..8usize).prop_map(|dep| Writing::Foreign { dep }),
    ]
    .boxed()
}

/// The steps a script draws, with `faults` the weight of each kind of
/// fault, and `writes` the events each actor writes.
fn a_step<E, S>(
    writes: impl Fn(Who) -> S + Clone + 'static,
    faults: u32,
) -> impl Strategy<Value = Step<E>>
where
    E: Schema,
    S: Strategy<Value = E> + 'static,
{
    let position = || 0..64usize;
    let append = (a_who(), a_who(), 0..4u8, a_writing(faults)).prop_flat_map(
        move |(by, other, roll, writing)| {
            // One write in four is under another actor's name, when faults
            // are drawn at all.
            let of = if faults > 0 && roll == 0 { other } else { by };
            writes(of).prop_map(move |event| Step::Append {
                by,
                of,
                event,
                writing,
            })
        },
    );
    prop_oneof![
        12 => append,
        4 => (a_who(), a_who(), prop::collection::vec(position(), 1..4))
            .prop_map(|(from, to, chosen)| Step::Pick { from, to, chosen }),
        3 => (a_who(), a_who()).prop_map(|(from, to)| Step::Sync { from, to }),
        1 => Just(Step::SyncAll),
        1 => (
            a_who(),
            prop::sample::select(vec![Refusal::Tampered, Refusal::Unparsed, Refusal::Orphan]),
            position(),
        )
            .prop_map(|(to, refusal, with)| Step::Refuse { to, refusal, with }),
        3 => a_who().prop_map(|by| Step::Rotate { by }),
        2 => (
            a_who(),
            position(),
            prop::sample::select(vec![Through::Memo, Through::Fresh]),
        )
            .prop_map(|(by, at, through)| Step::Read { by, at, through }),
        2 => (a_who(), position()).prop_map(|(by, at)| Step::Record { by, at }),
        1 => a_who().prop_map(|by| Step::Snapshot { by }),
        1 => position().prop_map(|key| Step::Forget { key }),
        1 => a_who().prop_map(|by| Step::Flush { by }),
    ]
}

fn a_kind() -> impl Strategy<Value = Kind> {
    prop_oneof![
        3 => Just(Kind::Memory),
        2 => Just(Kind::Overlay),
        1 => Just(Kind::Disk),
    ]
}

/// A script of one to `steps` steps over replicas of drawn kinds, with
/// faults among the writes. `writes` draws the events that an actor
/// writes, under the actor's name ([`Who::actor`]) so that the actor's keys
/// sign them.
pub fn a_script<E, S>(
    writes: impl Fn(Who) -> S + Clone + 'static,
    steps: usize,
) -> impl Strategy<Value = super::Script<E>>
where
    E: Schema,
    S: Strategy<Value = E> + 'static,
{
    scripts(writes, steps, 1)
}

/// [`a_script`] without faults: every step is [`Step::honest`], so `verify`
/// must find nothing after any step.
pub fn an_honest_script<E, S>(
    writes: impl Fn(Who) -> S + Clone + 'static,
    steps: usize,
) -> impl Strategy<Value = super::Script<E>>
where
    E: Schema,
    S: Strategy<Value = E> + 'static,
{
    scripts(writes, steps, 0)
}

fn scripts<E, S>(
    writes: impl Fn(Who) -> S + Clone + 'static,
    steps: usize,
    faults: u32,
) -> impl Strategy<Value = super::Script<E>>
where
    E: Schema,
    S: Strategy<Value = E> + 'static,
{
    (
        [a_kind(), a_kind(), a_kind()],
        prop::collection::vec(a_step(writes, faults), 1..=steps.max(1)),
    )
        .prop_map(|(kinds, steps)| super::Script { kinds, steps })
}
