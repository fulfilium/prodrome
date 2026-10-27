//! THE HISTORY GENERATOR: a drawn history of any schema's events, and the
//! kit's `Shuffle` of it. One generator, read by the core's laws
//! (`all/schema_laws.rs`, `all/declared.rs`, `all/kit.rs`), and by any other
//! crate's tests that include it, never two. The objects of a drawn history
//! are `prodrome::laws::history`'s, and its shuffle is
//! `prodrome::laws::a_shuffle`'s.

use prodrome::event::{Envelope, Hash};
use prodrome::laws::{a_shuffle, Shuffle};
use prodrome::schema::Schema;
use proptest::prelude::*;

/// What one history is drawn as: each event with the earlier changes to its
/// entity it supersedes (a bit per earlier change), and the `Shuffle` the
/// laws fold it by, one record per object.
#[derive(Debug, Clone)]
pub struct Draw<E> {
    pub events: Vec<(E, u64)>,
    pub shuffle: Shuffle,
}

/// A history of one to eleven of `event`'s draws, with a shuffle of its
/// objects.
pub fn a_draw<E: Schema>(event: impl Strategy<Value = E>) -> impl Strategy<Value = Draw<E>> {
    prop::collection::vec((event, any::<u64>()), 1..12).prop_flat_map(|events| {
        let objects = history(&events).len();
        a_shuffle(objects).prop_map(move |shuffle| Draw {
            events: events.clone(),
            shuffle,
        })
    })
}

/// The objects of `events`' history.
pub fn history<E: Schema>(events: &[(E, u64)]) -> Vec<(Hash, Envelope<E>)> {
    prodrome::laws::history(events).expect("a drawn history is a history")
}
