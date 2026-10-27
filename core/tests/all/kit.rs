//! The law kit (`prodrome::laws`) over the reference schemas: law 1
//! (`printed`) and law 35 (`synced`) on every drawn history of the todo, the
//! review and the note schemas. `schema_laws.rs` runs the kit's law 33 over
//! the same schemas.

use std::collections::BTreeMap;

use prodrome::event::{canonical_envelope, Envelope, Hash, TodoEvent};
use prodrome::laws;
use prodrome::reference::Todo;
use prodrome::schema::Schema;
use prodrome::store::{MemoryStore, Replica};
use proptest::prelude::*;

use crate::common::draw::{a_draw, history, Draw};
use crate::note::Note;
use crate::review::Review;
use crate::schema_laws::{a_note_event, a_review_event, a_todo_event, failed};

/// A store in memory that holds `objects`.
fn holding<E: Schema<Vocabulary: Default>>(objects: &[(Hash, Envelope<E>)]) -> MemoryStore<E> {
    let prints: BTreeMap<Hash, Vec<u8>> = objects
        .iter()
        .map(|(name, object)| (name.clone(), canonical_envelope(object).into_bytes()))
        .collect();
    let store = MemoryStore::default();
    store
        .receive(prints.keys().cloned().collect(), &|name| {
            prints.get(name).cloned()
        })
        .expect("a history verifies");
    store
}

/// Laws 1 and 35 at one draw.
fn kept<E: Schema<Vocabulary: Default>>(draw: &Draw<E>) -> Result<(), TestCaseError> {
    let objects = history(&draw.events);
    let prints: Vec<(Hash, String)> = objects
        .iter()
        .map(|(name, object)| (name.clone(), canonical_envelope(object)))
        .collect();
    laws::printed::<E>(&E::Vocabulary::default(), &prints).map_err(failed)?;
    laws::synced(&holding(&objects), &MemoryStore::default()).map_err(failed)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Laws 1 and 35, by the kit, over the todo schema.
    #[test]
    fn the_kit_holds_for_todos(draw in a_draw(a_todo_event())) {
        kept::<TodoEvent<Todo>>(&draw)?;
    }

    /// Laws 1 and 35, by the kit, over the review schema.
    #[test]
    fn the_kit_holds_for_reviews(draw in a_draw(a_review_event())) {
        kept::<Review>(&draw)?;
    }

    /// Laws 1 and 35, by the kit, over the note schema.
    #[test]
    fn the_kit_holds_for_notes(draw in a_draw(a_note_event())) {
        kept::<Note>(&draw)?;
    }
}

/// The kit reports a print that is not its object's.
#[test]
fn the_kit_finds_a_print_that_hashes_elsewhere() {
    let objects = history(&[(
        crate::note::by_ada("n1", crate::note::at(0), crate::note::Entry::Viewed),
        0,
    )]);
    let mut prints: Vec<(Hash, String)> = objects
        .iter()
        .map(|(name, object)| (name.clone(), canonical_envelope(object)))
        .collect();
    let first = prints[0].0.clone();
    prints[0].0 = prints[1].0.clone();
    prints[1].0 = first;
    let broken = laws::printed::<Note>(&Default::default(), &prints).expect_err("misnamed");
    assert_eq!(broken.law, 1);
}
