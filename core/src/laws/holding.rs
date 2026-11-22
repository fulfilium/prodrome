use std::collections::BTreeMap;

use crate::event::{canonical_envelope, Envelope, Hash};
use crate::literal::ProdromeError;
use crate::schema::Schema;
use crate::store::{MemoryStore, Replica, Sender};

/// A store in memory, opened at `vocabulary`, that receives `objects` as
/// any replica receives them.
///
/// # Errors
///
/// An object whose print does not verify, or that rests on an object that
/// `objects` does not hold.
pub fn holding<E: Schema>(
    vocabulary: E::Vocabulary,
    objects: &[(Hash, Envelope<E>)],
) -> Result<MemoryStore<E>, ProdromeError> {
    let prints: BTreeMap<&Hash, Vec<u8>> = objects
        .iter()
        .map(|(name, object)| (name, canonical_envelope(object).into_bytes()))
        .collect();
    let store = MemoryStore::at(vocabulary);
    store.receive(
        objects.iter().map(|(name, _)| name.clone()).collect(),
        &|name| prints.get(name).cloned(),
        Sender::Stranger,
    )?;
    Ok(store)
}
