use std::collections::BTreeSet;

use crate::change::mk_change;
use crate::event::{seal_hash, Envelope, Hash};
use crate::genesis::mk_genesis;
use crate::literal::ProdromeError;
use crate::schema::Schema;

/// The objects of a history of `events`: one genesis, and one change per
/// event. Each event is paired with a bit mask: its change rests on the
/// earlier changes to the same entity whose positions, modulo 64, have their
/// bit set.
///
/// # Errors
///
/// A genesis or a change that its smart constructor refuses.
pub fn history<E: Schema>(events: &[(E, u64)]) -> Result<Vec<(Hash, Envelope<E>)>, ProdromeError> {
    let genesis = Envelope::Genesis(mk_genesis("laws", &"0".repeat(32))?);
    let root = seal_hash(&genesis);
    let mut objects = vec![(root.clone(), genesis)];
    let mut changes: Vec<(Hash, &E)> = Vec::new();
    for (event, bits) in events {
        let deps: BTreeSet<Hash> = changes
            .iter()
            .enumerate()
            .filter(|(i, (_, earlier))| bits >> (i % 64) & 1 == 1 && earlier.key() == event.key())
            .map(|(_, (name, _))| name.clone())
            .collect();
        let object = Envelope::Change(mk_change(
            root.clone(),
            deps.into_iter().collect(),
            event.clone(),
        )?);
        let name = seal_hash(&object);
        changes.push((name.clone(), event));
        objects.push((name, object));
    }
    Ok(objects)
}
