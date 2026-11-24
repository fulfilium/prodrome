//! The reading of an entity's registers from its events under a policy
//! (SPEC §6). The register algebra, such as [`Product`], is defined in
//! `prodrome-data`.
//!
//! A write joins its register when it is dated at or before the moment asked
//! and the policy binds it. Only ancestry decides what a write supersedes: no
//! clock and no linearisation picks a winner.

use std::collections::BTreeMap;

use prodrome_data::fold::Product;
use prodrome_data::instant::{instant_of, Instant};
use prodrome_data::schema::Schema;
use prodrome_data::stamp::Stamp;

use crate::policy::Policy;

/// An entity's registers at `at` (`None`: ever) under `policy`, which is the
/// last value of `fold::readings` of `prodrome-core`.
#[must_use]
pub fn read<'a, E: Schema>(
    stream: &'a [Stamp<E>],
    at: Option<Instant>,
    policy: &impl Policy<E>,
) -> E::Registers<'a> {
    scan(stream, at, policy, |_, _| {})
}

/// The fold: the admitted writes joined instant by instant, `each` shown
/// the registers as they stand at every instant one is dated. The product is
/// a semilattice, so the order writes join in within an instant, or across
/// the stream, changes nothing.
pub(crate) fn scan<'a, E: Schema>(
    stream: &'a [Stamp<E>],
    at: Option<Instant>,
    policy: &impl Policy<E>,
    mut each: impl FnMut(Instant, &E::Registers<'a>),
) -> E::Registers<'a> {
    let mut dated: BTreeMap<Instant, Vec<&'a Stamp<E>>> = BTreeMap::new();
    for stamp in stream.iter().filter(|stamp| admits(stamp, at, policy)) {
        dated
            .entry(instant_of(stamp.event.at()))
            .or_default()
            .push(stamp);
    }
    let mut registers = E::Registers::default();
    for (instant, stamps) in dated {
        for stamp in stamps {
            registers.join(stamp);
        }
        each(instant, &registers);
    }
    registers
}

/// Does `stamp` join its registers at `at` (`None`: ever) under `policy`?
/// Where it is dated by then and the policy binds it: the reference policy
/// binds an event the schema does not ask about whoever wrote it (a todo's
/// record, §6.2, §6.3), and a policy that requires signatures binds only
/// what the history proves (§5).
pub(crate) fn admits<E: Schema>(
    stamp: &Stamp<E>,
    at: Option<Instant>,
    policy: &impl Policy<E>,
) -> bool {
    at.is_none_or(|at| instant_of(stamp.event.at()) <= at)
        && policy.standing(&stamp.name, &stamp.event).binds()
}
