use crate::event::{field, hashes, hashes_value, sorted_distinct, Hash};
use crate::literal::{Call, ProdromeError, Value};
use crate::payload::{required, string_field};
use crate::schema::Schema;

/// `Change(genesis, deps, event)`: an event named by its prodrome and the
/// writes it supersedes, never by where it was written.
#[derive(Debug, Clone, PartialEq)]
pub struct Change<E> {
    pub genesis: Hash,
    pub deps: Vec<Hash>,
    pub event: E,
}

pub fn mk_change<E>(genesis: Hash, deps: Vec<Hash>, event: E) -> Result<Change<E>, ProdromeError> {
    Ok(Change {
        genesis,
        deps: sorted_distinct("Change.deps", "dep", deps)?,
        event,
    })
}

impl<E: Schema> Change<E> {
    pub(crate) fn to_value(&self) -> Value {
        Value::call(
            "Change",
            vec![
                field("genesis", Value::str(self.genesis.as_str())),
                field("deps", hashes_value(&self.deps)),
                field("event", self.event.to_value()),
            ],
        )
    }

    /// Parses the event at the vocabulary `schema`.
    pub(crate) fn from_call(
        schema: &E::Vocabulary,
        call: &Call,
    ) -> Result<Change<E>, ProdromeError> {
        mk_change(
            Hash::new(string_field(call, "genesis")?)?,
            hashes(call, "deps")?,
            E::from_value(schema, required(call, "event")?)?,
        )
    }
}
