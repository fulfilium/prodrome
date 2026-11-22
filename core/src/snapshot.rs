//! The snapshot object of SPEC §3, which `prodrome-data` defines and this
//! module re-exports, and what a writer attests of a prodrome's DAG.

pub use prodrome_data::snapshot::*;

use crate::dag::Dag;
use crate::event::{Envelope, Hash};
use crate::literal::ProdromeError;

/// What a writer attests of a prodrome: the snapshot that already attests
/// its tips, or a new one to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attestation {
    /// The last snapshot among the tips, with nothing written since it.
    Standing(Hash),
    /// A snapshot of the tips, chained to the last snapshot among them.
    Fresh(Snapshot),
}

/// What a writer attests of the prodrome that `genesis` begins in `dag`
/// (SPEC §3, `snapshot()`).
///
/// A new snapshot names the prodrome's tips without the last snapshot among
/// them, and names that snapshot as its `previous`. If no other tip is left,
/// the answer is that snapshot, and nothing new is attested.
///
/// # Errors
///
/// A `dag` that does not linearise, and a snapshot that [`mk_snapshot`]
/// refuses.
pub fn attest<E>(dag: &Dag<E>, genesis: Hash) -> Result<Attestation, ProdromeError> {
    let tips = dag.tips_in(&dag.key(&genesis));
    let previous =
        dag.linearise()?.iter().rev().find(|name| {
            tips.contains(*name) && matches!(dag.get(name), Some(Envelope::Snapshot(_)))
        });
    let tips: Vec<Hash> = tips
        .into_iter()
        .filter(|tip| Some(tip) != previous)
        .collect();
    match previous {
        Some(previous) if tips.is_empty() => Ok(Attestation::Standing(previous.clone())),
        previous => mk_snapshot(genesis, tips, previous.cloned()).map(Attestation::Fresh),
    }
}
