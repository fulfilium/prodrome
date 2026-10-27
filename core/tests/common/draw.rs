//! THE HISTORY GENERATOR: a drawn history of any schema's events, and how the
//! laws reorder it. One generator, read by the core's laws
//! (`all/schema_laws.rs`, `all/declared.rs`), and by any other crate's
//! tests that include it, never two. The objects of a drawn history are
//! `prodrome::laws::history`'s.

use prodrome::laws::Shuffle;
use prodrome::schema::Schema;
use proptest::prelude::*;

/// What one history is drawn as: each event with the earlier changes to its
/// entity it supersedes (a bit per earlier change), and the `Shuffle` the
/// laws fold it by.
#[derive(Debug, Clone)]
pub struct Draw<E> {
    pub events: Vec<(E, u64)>,
    pub shuffle: Shuffle,
}

/// A `Shuffle` for a history of up to thirteen objects.
pub fn a_shuffle() -> impl Strategy<Value = Shuffle> {
    (
        prop::collection::vec(any::<u32>(), 13),
        prop::collection::vec(0usize..13, 0..4),
        any::<u64>(),
    )
        .prop_map(|(ranks, twice, replica)| Shuffle {
            ranks,
            twice,
            replica,
        })
}

/// A history of one to eleven of `event`'s draws, with the `Shuffle` the
/// laws fold it by.
pub fn a_draw<E: Schema>(event: impl Strategy<Value = E>) -> impl Strategy<Value = Draw<E>> {
    (
        prop::collection::vec((event, any::<u64>()), 1..12),
        a_shuffle(),
    )
        .prop_map(|(events, shuffle)| Draw { events, shuffle })
}

/// The objects of `events`' history.
pub fn history<E: Schema>(
    events: &[(E, u64)],
) -> Vec<(prodrome::event::Hash, prodrome::event::Envelope<E>)> {
    prodrome::laws::history(events).expect("a drawn history is a history")
}
