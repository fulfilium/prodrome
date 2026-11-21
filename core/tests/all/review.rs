//! A second schema, in the tests only: a REVIEW (`../schemas/review.rs`),
//! whose one register is a machine ordered by "further along", `Draft <
//! Review < Merged` and `Draft < Closed`, declared inflationary, with no
//! valuation. It shows that a schema needs no FPL: it is stored, folded and verified through the same
//! store and fold as the todo's, and the append path refuses a move back
//! (design §3.1, law 4).

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use prodrome::dag::Finding;
use prodrome::event::{mk_created, parse_envelope, Hash, TodoEvent};
use prodrome::fold::{maximal, read};
use prodrome::policy::Everything;
use prodrome::reference::Todo;
use prodrome::store::EventStore;
use prodrome::store::Sender;
use proptest::prelude::*;

#[path = "../schemas/review.rs"]
mod schema;

pub use schema::*;

type Store = EventStore<Review, Everything>;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A store at the review schema, removed when dropped.
struct Scratch(Store);

impl Scratch {
    fn new() -> Scratch {
        let root: PathBuf = std::env::temp_dir().join(format!(
            "prodrome-review-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        Scratch(Store::new(root, Everything))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.root());
    }
}

/// The phases a store reads for one review.
fn phases(store: &Store, pr: &str) -> HashSet<Phase> {
    let nodes = store.dag().expect("reads").nodes().expect("orders");
    let state = prodrome::registers::fold(&nodes);
    let pr = Pr::new(pr).expect("a review name");
    let phases = state
        .entities()
        .find(|(key, _)| **key == pr)
        .map(|(_, stream)| read(stream, None, &Everything).phases())
        .unwrap_or_default();
    phases
}

fn objects(store: &Store) -> usize {
    store.dag().expect("reads").objects().len()
}

/// Law 4 through the real append path: a move below, or beside, the phase
/// it supersedes is refused before any object exists, and a move further
/// along is written over it.
#[test]
fn the_append_refuses_a_move_back() {
    let scratch = Scratch::new();
    let store = &scratch.0;
    store.init("reviews").expect("begins");
    store
        .append(opened("pr-1", day(1), "ana", "a change"))
        .expect("an opening writes no register");
    store
        .append(moved("pr-1", day(2), "ana", Phase::Review))
        .expect("the first move supersedes nothing");
    let held = objects(store);

    let back = store.append(moved("pr-1", day(3), "ana", Phase::Draft));
    assert!(back.is_err(), "Draft is below Review");
    assert_eq!(objects(store), held, "a refused append writes nothing");

    store
        .append(moved("pr-1", day(4), "ana", Phase::Merged))
        .expect("Merged is above Review");
    let beside = store.append(moved("pr-1", day(5), "ana", Phase::Closed));
    assert!(beside.is_err(), "Closed is beside Merged, not above it");
    assert_eq!(phases(store, "pr-1"), HashSet::from([Phase::Merged]));

    store
        .append(moved("pr-2", day(5), "ana", Phase::Closed))
        .expect("another review's register is its own");
    assert!(store.verify().is_empty(), "{:?}", store.verify());
}

/// The schema is the parse: a review store reads no todo object, and a todo
/// print is not a review event.
#[test]
fn an_event_the_schema_does_not_name_is_refused_at_the_boundary() {
    let todo =
        "Created(todo='alpha', at=datetime(2026, 10, 1, 12, 0, 0), actor='ana', text='', note='')";
    assert!(prodrome::event::parse_event::<Review>(&Default::default(), todo).is_err());
    let envelope = format!("Sealed(prev='', event={todo})");
    assert!(parse_envelope::<Review>(&Default::default(), &envelope).is_err());
    let review =
        "Moved(pr='pr-1', at=datetime(2026, 10, 1, 12, 0, 0), actor='ana', phase='merged')";
    assert_eq!(
        prodrome::event::parse_event::<Review>(&Default::default(), review),
        Ok(moved("pr-1", day(1), "ana", Phase::Merged))
    );
    assert_eq!(
        prodrome::event::canonical(&moved("pr-1", day(1), "ana", Phase::Merged)),
        review
    );
    let unnamed = "Moved(pr='pr-1', at=datetime(2026, 10, 1, 12, 0, 0), actor='ana', phase='lost')";
    assert!(prodrome::event::parse_event::<Review>(&Default::default(), unnamed).is_err());

    let scratch = Scratch::new();
    let store = &scratch.0;
    store.init("reviews").expect("begins");
    let todos: EventStore<TodoEvent<Todo>, Everything> = EventStore::new(store.root(), Everything);
    todos
        .append(mk_created("alpha", day(1), "ana", "", "").expect("valid"))
        .expect("a genesis is any schema's");
    let dag = store.dag().expect("reads");
    assert_eq!(
        dag.unread().len(),
        1,
        "a todo object is not a review store's"
    );
    assert!(store
        .append(moved("pr-1", day(2), "ana", Phase::Draft))
        .is_err());
}

fn a_phase() -> impl Strategy<Value = Phase> {
    prop::sample::select(PHASES.to_vec())
}

/// Each move appended in turn, a refused one skipped: what a writer who
/// tries them all is left with.
fn try_all(store: &Store, from: u32, phases: &[Phase]) {
    for (i, phase) in phases.iter().enumerate() {
        let at = day(from + u32::try_from(i).expect("a few"));
        let _ = store.append(moved("pr-1", at, "ana", *phase));
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(24))]

    /// Law 4, the homomorphism: two replicas that each only grew, merged by
    /// putting their objects in one directory, read the join of their
    /// readings, the maximal phases of both. The copy is a door the store
    /// does not own, so the store verifies clean once the host records its
    /// heads (SPEC §3).
    #[test]
    fn the_reading_of_a_union_is_the_join_of_the_readings(
        shared in prop::collection::vec(a_phase(), 0..3),
        mine in prop::collection::vec(a_phase(), 0..4),
        theirs in prop::collection::vec(a_phase(), 0..4),
    ) {
        let (here, there) = (Scratch::new(), Scratch::new());
        let genesis = here.0.init("reviews").expect("begins");
        try_all(&here.0, 1, &shared);
        let tips: Vec<Hash> = here.0.tips().expect("derive").into_iter().collect();
        for tip in std::iter::once(&genesis).chain(&tips) {
            there.0.adopt(&here.0, tip, Sender::Stranger).expect("adopts");
        }
        try_all(&here.0, 10, &mine);
        try_all(&there.0, 10, &theirs);
        let (a, b) = (phases(&here.0, "pr-1"), phases(&there.0, "pr-1"));
        for entry in fs::read_dir(there.0.root().join("objects")).expect("lists") {
            let path = entry.expect("an entry").path();
            fs::copy(&path, here.0.root().join("objects").join(path.file_name().expect("a name")))
                .expect("copies");
        }
        let both: Vec<Phase> = a.iter().chain(&b).copied().collect();
        let joined: HashSet<Phase> = maximal(&both, |phase| phase).into_iter().collect();
        // A new handle reads the copied files at once; `here`, which has
        // read the store, reads them at the next hold of the lock.
        let reopened = Store::new(here.0.root(), Everything);
        prop_assert_eq!(phases(&reopened, "pr-1"), joined.clone());
        let lagging = here
            .0
            .verify()
            .iter()
            .all(|finding| matches!(finding, Finding::StaleHeads { .. }));
        prop_assert!(lagging);
        here.0.record_heads().expect("records");
        prop_assert!(here.0.verify().is_empty());
        prop_assert_eq!(phases(&here.0, "pr-1"), joined);
        let recorded = here.0.heads().expect("reads the record");
        prop_assert_eq!(
            recorded.tips(),
            &here.0.tips().expect("derive"),
            "the recorded heads are the union's"
        );
    }
}
