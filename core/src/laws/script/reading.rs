//! What a script compares: a replica's registers at an instant, under the
//! policy where everything binds and under the policy that requires the
//! root's keys.

use std::collections::BTreeSet;

use super::keys::root;
use crate::dag::Dag;
use crate::event::Hash;
use crate::fpl::{instant_of, Instant};
use crate::laws::reading::Registers;
use crate::policy::{Everything, Proven};
use crate::registers::{fold, Folded};
use crate::schema::Schema;

/// Every entity's registers at an instant: the CLAIMED reading, under the
/// policy where everything binds, and the CONFIRMED one, under the policy
/// that binds only what the root's keys prove (SPEC §5), taken at the
/// history it reads.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Reading<E: Schema> {
    claimed: Registers<E>,
    confirmed: Registers<E>,
}

/// Both halves are `Eq`.
impl<E: Schema> Eq for Reading<E> {}

impl<E: Schema> Reading<E> {
    /// What `dag`, whose fold is `folded`, reads at `at` (`None`: ever).
    pub(super) fn of(dag: &Dag<E>, folded: &Folded<E>, at: Option<Instant>) -> Self {
        let proven = Proven::new(Everything, [root().public()], dag);
        Reading {
            claimed: Registers::at(folded, at, &Everything),
            confirmed: Registers::at(folded, at, &proven),
        }
    }

    /// What the down-set of `tips` in `dag` reads at `at`, folded afresh:
    /// the objects the tips rest on, with the policy taken at them.
    pub(super) fn at_down_set(
        dag: &Dag<E>,
        tips: &BTreeSet<Hash>,
        at: Option<Instant>,
    ) -> Result<Self, String> {
        let held = dag.closure(tips.iter().cloned());
        let down: Dag<E> = dag
            .objects()
            .iter()
            .filter(|(name, _)| held.contains(*name))
            .map(|(name, object)| (name.clone(), object.clone()))
            .collect();
        let nodes = down
            .nodes()
            .map_err(|refused| format!("the down-set is no history: {refused}"))?;
        Ok(Reading::of(&down, &fold(&nodes), at))
    }

    /// The first place where `self` and `expected` part, with both sides.
    pub(super) fn differs(&self, expected: &Self) -> Option<String> {
        self.claimed
            .differs(&expected.claimed)
            .map(|difference| format!("claimed: {difference}"))
            .or_else(|| {
                self.confirmed
                    .differs(&expected.confirmed)
                    .map(|difference| format!("confirmed: {difference}"))
            })
    }
}

/// The instant at position `at` among the distinct instants that the
/// events of `dag` are dated, in order, after `None` (ever) at position 0;
/// taken modulo their number.
pub(super) fn instant<E: Schema>(dag: &Dag<E>, at: usize) -> Option<Instant> {
    let instants: BTreeSet<Instant> = dag
        .objects()
        .values()
        .filter_map(|object| object.event().map(|event| instant_of(event.at())))
        .collect();
    match at % (instants.len() + 1) {
        0 => None,
        n => instants.into_iter().nth(n - 1),
    }
}
