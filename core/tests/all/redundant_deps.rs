//! SPEC §3's deps rule and law 66: `verify` reports a dep that another dep
//! of the same change rests on. The rule is stated here as SPEC §3 states
//! it, by the closure of each dep's parents, and `verify` is held to that
//! statement over generated DAGs.
//!
//! The generated objects are held in memory under names that the test
//! chooses, and not under the hashes of their bytes. So a DAG here can name
//! an absent object as a dep, and its objects can make a cycle, which no
//! store on disk can hold.

use prodrome::change::mk_change;
use prodrome::dag::{Dag, Finding};
use prodrome::event::{mk_created, parents_of, Envelope, Hash, TodoEvent};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::policy::Everything;
use prodrome::reference::Todo;
use prodrome::schema::Schema;
use proptest::prelude::*;

use crate::common::cases::cases;

type Event = TodoEvent<Todo>;

/// How the deps of a generated change are drawn.
#[derive(Debug, Clone, Copy)]
enum Shape {
    /// Each dep is a change drawn before it, so the DAG has a linear order.
    Ordered,
    /// Each dep is any name: a change drawn before or after it, the change
    /// itself, the genesis, or a name that no object holds.
    Any,
}

/// One drawn DAG: its shape, and for each change, its entity and its picks
/// of deps.
type Drawn = (Shape, Vec<(usize, Vec<usize>)>);

fn a_dag() -> impl Strategy<Value = Drawn> {
    let shape = prop_oneof![Just(Shape::Ordered), Just(Shape::Any)];
    let change = (0usize..2, prop::collection::vec(0usize..64, 0..4));
    (shape, prop::collection::vec(change, 1..14))
}

/// The name of the object at `at`: the genesis is at 0, and each change at
/// its place after it. A place after the last change names no object.
fn name(at: usize) -> Hash {
    Hash::new(format!("{at:064x}")).expect("64 hex digits")
}

/// The drawn DAG: one genesis, and a change to `alpha` or `beta` for each
/// drawn change.
fn realise((shape, changes): &Drawn) -> Dag<Event> {
    let at = Datetime::new(2026, 9, 1, 12, 0, 0, 0).expect("a real instant");
    let genesis = mk_genesis("home", &"0".repeat(32)).expect("a valid nonce");
    let mut objects = vec![(name(0), Envelope::Genesis(genesis))];
    for (i, (entity, picks)) in changes.iter().enumerate() {
        let place = i + 1;
        let mut deps: Vec<Hash> = picks
            .iter()
            .filter_map(|pick| match shape {
                Shape::Ordered => (i > 0).then(|| name(1 + pick % i)),
                Shape::Any => Some(name(pick % (changes.len() + 3))),
            })
            .collect();
        deps.sort();
        deps.dedup();
        let todo = ["alpha", "beta"][*entity];
        let event = mk_created(todo, at, "ada", "", "").expect("a valid event");
        let change = mk_change(name(0), deps, event).expect("sorted, distinct deps");
        objects.push((name(place), Envelope::Change(change)));
    }
    objects.into_iter().collect()
}

/// The deps rule as SPEC §3 states it, for one change: each dep that is
/// not a write to the change's entity, and each dep that is in the closure
/// of another dep's parents, dep by dep in the order of the deps. A dep
/// that lies on a cycle is in the closure of its own parents, so this
/// reports it as beneath itself.
fn by_closure(dag: &Dag<Event>, name: &Hash, object: &Envelope<Event>) -> Vec<Finding> {
    let Envelope::Change(change) = object else {
        return Vec::new();
    };
    let closures: Vec<_> = change
        .deps
        .iter()
        .map(|other| {
            let parents = dag.get(other).map(parents_of).unwrap_or_default();
            dag.closure(parents)
        })
        .collect();
    let mut findings = Vec::new();
    for dep in &change.deps {
        let entity = dag.get(dep).map(|held| held.event().map(Schema::key));
        if entity.is_some_and(|entity| entity != Some(change.event.key())) {
            findings.push(Finding::Foreign {
                change: name.clone(),
                dep: dep.clone(),
            });
        }
        for (other, closure) in change.deps.iter().zip(&closures) {
            if closure.contains(dep) {
                findings.push(Finding::Redundant {
                    change: name.clone(),
                    dep: dep.clone(),
                    beneath: other.clone(),
                });
            }
        }
    }
    findings
}

/// The findings of the deps rule among `verify`'s findings, in order.
fn deps_findings(dag: &Dag<Event>) -> Vec<Finding> {
    dag.verify(&Everything)
        .into_iter()
        .filter(|finding| matches!(finding, Finding::Foreign { .. } | Finding::Redundant { .. }))
        .collect()
}

proptest! {
    #![proptest_config(cases(512))]

    /// Law 66: on chains, merges of several deps, redundant deps, deps
    /// that no object holds, and cycles, `verify` reports exactly the
    /// deps findings of the closure rule, in the same order.
    #[test]
    fn the_deps_rule_is_the_closure_rule(drawn in a_dag()) {
        let dag = realise(&drawn);
        let expected: Vec<Finding> = dag
            .objects()
            .iter()
            .flat_map(|(name, object)| by_closure(&dag, name, object))
            .collect();
        prop_assert_eq!(deps_findings(&dag), expected);
    }
}

/// Law 66, on a fixed cycle: two changes that each name the other as a
/// dep, and a third change that names both. Each of the two lies beneath
/// itself, and the third reports each of the two beneath itself and
/// beneath the other: six findings, as the closure rule gives.
#[test]
fn a_dep_on_a_cycle_lies_beneath_itself() {
    let at = Datetime::new(2026, 9, 1, 12, 0, 0, 0).expect("a real instant");
    let genesis = mk_genesis("home", &"0".repeat(32)).expect("a valid nonce");
    let change = |deps: Vec<Hash>| {
        let event = mk_created("alpha", at, "ada", "", "").expect("a valid event");
        Envelope::Change(mk_change(name(0), deps, event).expect("sorted, distinct deps"))
    };
    let dag: Dag<Event> = [
        (name(0), Envelope::Genesis(genesis)),
        (name(1), change(vec![name(2)])),
        (name(2), change(vec![name(1)])),
        (name(3), change(vec![name(1), name(2)])),
    ]
    .into_iter()
    .collect();
    let redundant = |change: usize, dep: usize, beneath: usize| Finding::Redundant {
        change: name(change),
        dep: name(dep),
        beneath: name(beneath),
    };
    let expected: Vec<Finding> = dag
        .objects()
        .iter()
        .flat_map(|(name, object)| by_closure(&dag, name, object))
        .collect();
    let found = deps_findings(&dag);
    assert_eq!(found, expected);
    assert_eq!(found.len(), 6);
    for (change, dep, beneath) in [
        (1, 2, 2),
        (2, 1, 1),
        (3, 1, 1),
        (3, 1, 2),
        (3, 2, 1),
        (3, 2, 2),
    ] {
        assert!(found.contains(&redundant(change, dep, beneath)));
    }
}
