use crate::dag::Dag;
use crate::event::{field, hashes, hashes_value, optional_hash, sorted_distinct, Envelope, Hash};
use crate::literal::{Call, ProdromeError, Value};
use crate::payload::string_field;

/// `Snapshot(genesis, tips, previous)`: an attestation of everything its tips
/// rest on, chained to the snapshot before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub genesis: Hash,
    pub tips: Vec<Hash>,
    pub previous: Option<Hash>,
}

pub fn mk_snapshot(
    genesis: Hash,
    tips: Vec<Hash>,
    previous: Option<Hash>,
) -> Result<Snapshot, ProdromeError> {
    if tips.is_empty() {
        return Err(ProdromeError::invalid("Snapshot.tips must name a tip"));
    }
    Ok(Snapshot {
        genesis,
        tips: sorted_distinct("Snapshot.tips", "tip", tips)?,
        previous,
    })
}

impl Snapshot {
    pub fn to_value(&self) -> Value {
        Value::call(
            "Snapshot",
            vec![
                field("genesis", Value::str(self.genesis.as_str())),
                field("tips", hashes_value(&self.tips)),
                field(
                    "previous",
                    Value::str(self.previous.as_ref().map_or("", Hash::as_str)),
                ),
            ],
        )
    }

    pub fn from_call(call: &Call) -> Result<Snapshot, ProdromeError> {
        mk_snapshot(
            Hash::new(string_field(call, "genesis")?)?,
            hashes(call, "tips")?,
            optional_hash(call, "previous")?,
        )
    }
}

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
