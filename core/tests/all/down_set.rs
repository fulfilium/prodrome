//! SPEC law 40, the interior operator: the history of a set of objects is
//! its largest down-set. The store's half of the law, and quarantine, run
//! over every store of record in `store_laws/reading.rs`.
//!
//! A history is a down-set of the causal order (design §1), and a set of
//! files need not be one: an object set aside, or one that has not arrived,
//! leaves what rests on it naming a parent nobody holds. [`Dag::interior`]
//! is the history such a set holds, every object whose ancestors are all
//! there, and it is an interior operator: never more than the set,
//! idempotent and monotone, the whole of a set closed under parents, and so
//! a function of the objects alone. It is the right adjoint of including
//! histories among sets of objects, so it preserves meets.

use crate::common;
use crate::memory::{replicas, Scratch};

use std::collections::BTreeSet;

use prodrome::dag::Dag;
use prodrome::event::{parents_of, Hash, TodoEvent};
use prodrome::reference::Todo;
use prodrome::store::EventStore;
use prodrome::store::Sender;
use proptest::prelude::*;

use common::{a_log, seal, two_writers};

type Event = TodoEvent<Todo>;
type Store = EventStore<Event, prodrome::policy::Untrusted>;

/// Two writers' changes over one genesis, unioned into the first replica.
pub(crate) fn changes(scratch: &Scratch, logs: &(Vec<Event>, Vec<Event>, Vec<Event>)) -> Store {
    let (here, there) = replicas(scratch, logs);
    for tip in there.tips().expect("the tips derive") {
        here.adopt(&there, &tip, Sender::Stranger).expect("adopts");
    }
    here
}

/// A log as legacy objects, each sealed on every tip.
pub(crate) fn legacy(scratch: &Scratch, log: &[Event]) -> Store {
    let store = scratch.store("legacy");
    for event in log {
        seal(&store, event.clone());
    }
    store
}

/// The objects of `dag` whose bit in `mask` is set, by their place in name
/// order (modulo 64).
pub(crate) fn chosen(dag: &Dag<Event>, mask: u64) -> BTreeSet<Hash> {
    dag.objects()
        .keys()
        .enumerate()
        .filter(|(i, _)| mask >> (i % 64) & 1 == 1)
        .map(|(_, name)| name.clone())
        .collect()
}

/// `dag` without `removed`.
pub(crate) fn without(dag: &Dag<Event>, removed: &BTreeSet<Hash>) -> Dag<Event> {
    dag.objects()
        .iter()
        .filter(|(name, _)| !removed.contains(*name))
        .map(|(name, object)| (name.clone(), object.clone()))
        .collect()
}

pub(crate) fn names(dag: &Dag<Event>) -> BTreeSet<Hash> {
    dag.objects().keys().cloned().collect()
}

/// The interior laws at one history and one subset of it.
fn is_interior(whole: &Dag<Event>, mask: u64, more: u64) -> Result<(), TestCaseError> {
    prop_assert_eq!(&whole.interior(), whole, "a history is its own interior");
    let removed = chosen(whole, mask);
    let set = without(whole, &removed);
    let interior = set.interior();
    // Exactly the objects nothing removed is beneath: each object with its
    // ancestors (`closure` holds its seed) misses every removed one.
    let expected: BTreeSet<Hash> = names(&set)
        .into_iter()
        .filter(|name| whole.closure([name.clone()]).is_disjoint(&removed))
        .collect();
    prop_assert_eq!(names(&interior), expected);
    prop_assert!(names(&interior).is_subset(&names(&set)), "deflationary");
    // And what it leaves out is said: each name the set's objects rest on
    // and lack, never one that is not a print here, with exactly the
    // objects above it, which together are the set less its interior.
    let excluded = set.excluded();
    let lacked: BTreeSet<Hash> = excluded.iter().map(|out| out.name.clone()).collect();
    let rested_on: BTreeSet<Hash> = set
        .objects()
        .values()
        .flat_map(parents_of)
        .filter(|parent| !set.objects().contains_key(parent))
        .collect();
    prop_assert_eq!(&lacked, &rested_on, "the names lacked");
    let mut left_out = BTreeSet::new();
    for out in &excluded {
        prop_assert!(out.why.is_none(), "every print here is an object");
        let above: BTreeSet<Hash> = names(&set)
            .into_iter()
            .filter(|name| set.closure([name.clone()]).contains(&out.name))
            .collect();
        prop_assert_eq!(&out.resting, &above, "exactly what rests on {}", out.name);
        left_out.extend(out.resting.iter().cloned());
    }
    prop_assert_eq!(
        left_out,
        names(&set)
            .difference(&names(&interior))
            .cloned()
            .collect::<BTreeSet<_>>(),
        "the history is the objects nothing lacked is beneath"
    );
    prop_assert!(whole.excluded().is_empty(), "a history lacks nothing");
    prop_assert_eq!(
        &interior.excluded(),
        &excluded,
        "the interior lacks what the set lacks"
    );
    prop_assert_eq!(&interior.interior(), &interior, "idempotent");
    prop_assert!(
        interior.nodes().is_ok(),
        "closed under parents, so it reads"
    );
    let smaller = without(&set, &chosen(&set, more));
    prop_assert!(
        names(&smaller.interior()).is_subset(&names(&interior)),
        "monotone"
    );
    // The right adjoint of including down-sets among sets, so it preserves
    // meets: the history two sets share is the meet of their histories.
    let other = without(whole, &chosen(whole, more));
    let apart: BTreeSet<Hash> = names(whole)
        .into_iter()
        .filter(|name| !(names(&set).contains(name) && names(&other).contains(name)))
        .collect();
    let both = without(whole, &apart);
    prop_assert_eq!(
        names(&both.interior()),
        names(&interior)
            .intersection(&names(&other.interior()))
            .cloned()
            .collect::<BTreeSet<_>>(),
        "meets"
    );
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(32))]

    /// Law 40, the operator: over two writers' changes and over a legacy
    /// chain, the interior of any subset is exactly the objects nothing
    /// removed is beneath, and it is deflationary, idempotent, monotone and
    /// the whole of a history.
    #[test]
    fn the_interior_is_the_largest_down_set(
        logs in two_writers(),
        log in a_log(),
        mask in any::<u64>(),
        more in any::<u64>(),
    ) {
        let scratch = Scratch::new("interior");
        is_interior(&changes(&scratch, &logs).dag().expect("reads"), mask, more)?;
        is_interior(&legacy(&scratch, &log).dag().expect("reads"), mask, more)?;
    }
}
