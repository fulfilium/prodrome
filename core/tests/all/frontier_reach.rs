//! Law 64 (SPEC §9): an answer reads a frontier's reach as positions in the
//! store's linear order, and that reading is the walk over names.
//!
//! The walk over names is the specification, and this file keeps it for
//! itself: the closure of a set of names (`Dag::closure`), and the members
//! of a set that no member names as a parent. A store of record answers one
//! puller over the generated two-writer histories. The puller takes pieces
//! in the store's order with `record`, or posts any names it likes with
//! `record_with`, which covers pieces in another linear extension and posts
//! between pieces. The store can grow between two pieces. Every answer and
//! every record is compared with the specification.

use crate::common;

use std::collections::BTreeSet;

use prodrome::dag::Dag;
use prodrome::event::{canonical_envelope, parents_of, seal_hash, Envelope, Hash, TodoEvent};
use prodrome::genesis::mk_genesis;
use prodrome::reference::Todo;
use prodrome::store::frontier::{Frontiers, Reached, Since, Token, KEPT};
use prodrome::store::{MemoryStore, Replica};
use proptest::prelude::*;

use common::two_writers;

type Event = TodoEvent<Todo>;

const STORE: &str = "notes";

/// A store that begins from one genesis.
fn begun() -> MemoryStore<Event> {
    let genesis: Envelope<Event> =
        Envelope::Genesis(mk_genesis(STORE, &"0".repeat(32)).expect("32 hex digits are a nonce"));
    let (name, print) = (
        seal_hash(&genesis),
        canonical_envelope(&genesis).into_bytes(),
    );
    let store = MemoryStore::default();
    store
        .receive([name.clone()].into(), &|asked| {
            (*asked == name).then(|| print.clone())
        })
        .expect("a genesis is an object");
    store
}

fn dag(store: &MemoryStore<Event>) -> Dag<Event> {
    (*store.held().expect("memory reads").dag).clone()
}

/// The specification of the tips of `held`: the members that no member
/// names as a parent.
fn tips(dag: &Dag<Event>, held: &BTreeSet<Hash>) -> BTreeSet<Hash> {
    let named: BTreeSet<Hash> = held
        .iter()
        .filter_map(|name| dag.get(name))
        .flat_map(parents_of)
        .collect();
    held.difference(&named).cloned().collect()
}

/// One step of a pull: an answer and its record, or a growth of the store.
#[derive(Debug, Clone)]
enum Step {
    Answer(Record),
    /// The store appends the next event of the other writer.
    Grow,
}

/// How the puller records an answer.
#[derive(Debug, Clone)]
enum Record {
    /// The next `size` objects that the answer lacks, with `record`.
    Piece { size: usize },
    /// The lacking objects at the drawn places, and a name the store does
    /// not hold, with `record_with`.
    Post { picks: Vec<usize> },
}

fn a_step() -> impl Strategy<Value = Step> {
    prop_oneof![
        prop_oneof![0usize..6, Just(usize::MAX)]
            .prop_map(|size| Step::Answer(Record::Piece { size })),
        prop::collection::vec(0usize..64, 0..6)
            .prop_map(|picks| Step::Answer(Record::Post { picks })),
        Just(Step::Grow),
    ]
}

proptest! {
    #![proptest_config(common::cases::cases(64))]

    /// Law 64: for any token whose tips the store holds, the answer's
    /// reach is the closure of those tips, it lacks the rest of the
    /// store's linear order, and each `record` and `record_with` records
    /// the tips and the count of what it reaches joined to what was sent,
    /// all as the walk over names says.
    #[test]
    fn a_reach_in_positions_is_the_reach_in_names(
        (shared, mine, theirs) in two_writers(),
        steps in prop::collection::vec(a_step(), 1..16),
    ) {
        let store = begun();
        for event in shared.into_iter().chain(mine) {
            store.append(event).expect("the history appends");
        }
        let mut theirs = theirs.into_iter();
        let frontiers = Frontiers::remembered(KEPT);
        let mut since = Since::Nothing;
        let mut named: BTreeSet<Hash> = BTreeSet::new();
        for step in steps {
            let record = match step {
                Step::Grow => {
                    if let Some(event) = theirs.next() {
                        store.append(event).expect("the history appends");
                    }
                    continue;
                }
                Step::Answer(record) => record,
            };
            let dag = dag(&store);
            let held: BTreeSet<Hash> = dag.closure(named.iter().cloned());
            let lacking = frontiers.answer(STORE, &since, &dag).expect("memory reads");
            let order = dag.linearise().expect("a store is closed under parents");
            let expected: Vec<&Hash> = order.iter().filter(|name| !held.contains(*name)).collect();
            let listed: Vec<&Hash> = lacking.lacking().iter().map(|(name, _)| *name).collect();
            prop_assert_eq!(&listed, &expected, "it lacks the rest of the order");
            match lacking.reached() {
                Reached::Known(reach) => {
                    prop_assert_eq!(reach.len(), held.len());
                    prop_assert!(held.iter().all(|name| reach.contains(name)));
                    prop_assert_eq!(&reach.iter().cloned().collect::<BTreeSet<Hash>>(), &held);
                    prop_assert!(order.iter().filter(|name| !held.contains(*name)).all(|name| !reach.contains(name)));
                }
                Reached::Everything => {
                    prop_assert_eq!(held.len(), dag.objects().len(), "everything is every object");
                }
                Reached::Unknown => {
                    return Err(TestCaseError::fail("a kept token whose tips are held is known"));
                }
            }
            let (now, recorded) = match record {
                Record::Piece { size } => {
                    let mut now = held.clone();
                    now.extend(expected.iter().take(size).map(|name| (*name).clone()));
                    (now, lacking.record(size).expect("memory keeps"))
                }
                Record::Post { picks } => {
                    let mut taken: Vec<Hash> = picks
                        .iter()
                        .filter_map(|&pick| expected.get(pick % expected.len().max(1)).map(|name| (*name).clone()))
                        .collect();
                    taken.push(Hash::new("f".repeat(64)).expect("64 hex digits are a name"));
                    let mut now = held.clone();
                    now.extend(dag.closure(taken.iter().filter(|name| dag.get(name).is_some()).cloned()));
                    (now, lacking.record_with(taken).expect("memory keeps"))
                }
            };
            named = tips(&dag, &now);
            prop_assert_eq!(&recorded.token, &Token::of(&named), "the record's tips");
            prop_assert_eq!(recorded.count, now.len(), "the record's count");
            since = Since::Frontier(recorded.token);
        }
    }
}
