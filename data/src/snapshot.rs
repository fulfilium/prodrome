use crate::event::{field, hashes, hashes_value, optional_hash, sorted_distinct, Hash};
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
