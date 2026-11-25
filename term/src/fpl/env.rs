//! The environment that a term reads: a snapshot of history (SPEC §6.1).

use std::collections::{BTreeMap, BTreeSet};

use prodrome_data::instant::Instant;

/// The end of a named event that history records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    Completed(Instant),
    Cancelled(Instant),
}

impl Outcome {
    #[inline]
    pub fn at(&self) -> Instant {
        match self {
            Outcome::Completed(at) | Outcome::Cancelled(at) => *at,
        }
    }

    /// The constructor name the reference prints for this outcome.
    pub fn kind(&self) -> &'static str {
        match self {
            Outcome::Completed(_) => "Completed",
            Outcome::Cancelled(_) => "Cancelled",
        }
    }
}

/// A register's candidate bindings (SPEC §6.6), with `None` for an open
/// candidate. One member is a value, and several members are a conflict.
pub type Candidates = BTreeSet<Option<Outcome>>;

/// A snapshot of history (SPEC §6.1): the candidate bindings that `After`
/// reads for each todo that is not simply open, and the tendings that `Recur`
/// reads for each todo.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Env {
    pub outcomes: BTreeMap<String, Candidates>,
    pub tended: BTreeMap<String, BTreeSet<Instant>>,
}

impl Env {
    /// The environment that binds nothing and records no tending.
    pub fn new() -> Env {
        Env::default()
    }

    /// Binds `event` to exactly `outcome`, in place of its earlier candidates.
    pub fn bind(&mut self, event: impl Into<String>, outcome: Outcome) {
        self.outcomes.insert(event.into(), [Some(outcome)].into());
    }
}
