//! SPEC laws 19, 20, 21, 23, 28 and 29 on stores of changes: a name is
//! its genesis, event and view; append is idempotent; deps never leave a todo;
//! geneses are disjoint; a snapshot attests its closure; placement is not
//! identity.

use crate::common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{a_draft, a_log, a_replay, a_schedule, moment, realise, two_writers, Draft, WINDOW};
use prodrome::dag::{Dag, Finding};
use prodrome::event::{
    canonical, canonical_envelope, event_id, seal_hash, Actor, Envelope, Hash, TodoEvent,
};
use prodrome::fold::{Kind, Product};
use prodrome::genesis::mk_genesis;
use prodrome::policy::{Everything, Untrusted};
use prodrome::reference::Todo;
use prodrome::registers::fold;
use prodrome::store::EventStore;
use prodrome::store::Sender;
use prodrome::view;
use proptest::prelude::*;

type Event = TodoEvent<Todo>;
type Store = EventStore<TodoEvent<Todo>, Untrusted>;

fn roster() -> Untrusted {
    Untrusted::of([Actor::new("triage").expect("valid")])
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A scratch directory of stores, gone when the case is done with it.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "prodrome-change-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        Scratch(root)
    }

    /// A store begun with the genesis labelled `label`, whose nonce is fixed
    /// so that two stores of one label are one prodrome.
    fn store(&self, name: &str, label: &str) -> Store {
        let store = Store::new(self.0.join(name), roster());
        let genesis: Envelope<TodoEvent<Todo>> =
            Envelope::Genesis(mk_genesis(label, &"0".repeat(32)).expect("a genesis"));
        let digest = seal_hash(&genesis);
        let print = canonical_envelope(&genesis);
        store
            .adopt_objects(&[(digest.clone(), print)].into(), &digest, Sender::Stranger)
            .expect("begins");
        store
    }

    /// Every object file of each store, copied into one: a `git merge`.
    fn union(&self, name: &str, stores: &[&Store]) -> Store {
        let union = Store::new(self.0.join(name), roster());
        let objects = union.root().join("objects");
        fs::create_dir_all(&objects).expect("creates objects/");
        for store in stores {
            for entry in fs::read_dir(store.root().join("objects")).expect("lists") {
                let path = entry.expect("an entry").path();
                fs::copy(&path, objects.join(path.file_name().expect("a name"))).expect("copies");
            }
        }
        union
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn append(store: &Store, events: &[Event]) -> Vec<Hash> {
    events
        .iter()
        .map(|event| store.append(event.clone()).expect("appends"))
        .collect()
}

fn dag(store: &Store) -> std::sync::Arc<Dag<TodoEvent<Todo>>> {
    store.dag().expect("reads")
}

fn names(store: &Store) -> BTreeSet<Hash> {
    dag(store).objects().keys().cloned().collect()
}

fn file(store: &Store, name: &Hash) -> PathBuf {
    store
        .root()
        .join("objects")
        .join(format!("{}.py", name.as_str()))
}

fn findings(store: &Store) -> Vec<String> {
    store.verify().iter().map(ToString::to_string).collect()
}

/// A draft that writes a register, dated.
fn a_write() -> impl Strategy<Value = Event> {
    ((a_draft(), 3u8..7), 0i64..WINDOW)
        .prop_map(|((draft, roll), at)| Draft { roll, ..draft }.at_with_note(moment(at), "written"))
}

proptest! {
    #![proptest_config(crate::common::cases::cases(24))]

    /// Law 19 — A NAME IS ITS GENESIS, EVENT AND VIEW. Two writers diverge
    /// on everything but `event`'s entity, and write it as one object.
    #[test]
    fn name_ignores_other_entities(
        (shared, mine, theirs) in two_writers(),
        event in a_write(),
    ) {
        let scratch = Scratch::new();
        let elsewhere = |log: &[Event]| -> Vec<Event> {
            log.iter()
                .filter(|other| other.todo() != event.todo())
                .cloned()
                .collect()
        };
        let here = scratch.store("here", "one");
        let there = scratch.store("there", "one");
        append(&here, &shared);
        append(&there, &shared);
        append(&here, &elsewhere(&mine));
        append(&there, &elsewhere(&theirs));
        prop_assert_eq!(
            here.append(event.clone()).expect("appends"),
            there.append(event).expect("appends")
        );
    }

    /// Law 20 — APPEND IS IDEMPOTENT. Replaying appends onto the store they
    /// wrote answers the object each first wrote and writes nothing.
    #[test]
    fn replay_is_a_no_op((log, replay) in a_replay()) {
        let scratch = Scratch::new();
        let store = scratch.store("store", "one");
        let mut first: BTreeMap<String, Hash> = BTreeMap::new();
        for (event, name) in log.iter().zip(append(&store, &log)) {
            prop_assert_eq!(first.entry(canonical(event)).or_insert(name.clone()), &name);
        }
        let held = names(&store);
        for event in log.iter().chain(&replay) {
            prop_assert_eq!(&store.append(event.clone()).expect("replays"), &first[&canonical(event)]);
        }
        prop_assert_eq!(names(&store), held);
        prop_assert_eq!(findings(&store), Vec::<String>::new());
    }

    /// Law 20, §3's case — a claim `P` is overridden by a maintainer's `M`,
    /// and the delivery that carried `P` is replayed: nothing is written, `P`
    /// is the answer, and the claimed reading still reads `M`.
    #[test]
    fn replay_after_rejection_writes_nothing(
        shared in a_log(),
        (draft, rolls, at, later) in (a_draft(), (3u8..7, 4u8..7), 0i64..WINDOW, 0i64..WINDOW),
    ) {
        let scratch = Scratch::new();
        let store = scratch.store("store", "one");
        append(&store, &shared);
        let (proposed, overriding) = rolls;
        let proposal = Draft { actor: "triage", roll: proposed, ..draft.clone() }
            .at_with_note(moment(at), "proposed");
        let rejection = Draft {
            actor: "bassel",
            roll: if proposed == 3 { 3 } else { overriding },
            ..draft
        }
        .at_with_note(moment(at.max(later)), "overridden");
        let p = store.append(proposal.clone()).expect("proposes");
        let m = store.append(rejection).expect("overrides");
        let Envelope::Change(change) = store.load(&m).expect("loads") else {
            panic!("an append writes a change")
        };
        prop_assert!(change.deps.contains(&p), "the override supersedes the proposal");

        let held = names(&store);
        prop_assert_eq!(store.append(proposal).expect("replays"), p.clone());
        prop_assert_eq!(names(&store), held);
        let state = fold(&dag(&store).nodes().expect("reads"));
        let (_, stream) = state
            .entities()
            .find(|(todo, _)| *todo == change.event.todo())
            .expect("the todo");
        let claimed = prodrome::fold::read(stream, None, &Everything);
        prop_assert!(
            [Kind::State, Kind::Spec].iter().all(|kind| !claimed.frontier(*kind).names().contains(&p)),
            "the rejected proposal is not back in the claimed reading"
        );
    }

    /// Law 21 — INDEPENDENCE IS STRUCTURAL. Every dep is a write to its
    /// change's todo, so deps never leave a todo; and the store
    /// written todo by todo is the store written in time order.
    #[test]
    fn deps_name_one_todo(log in a_log()) {
        let scratch = Scratch::new();
        let store = scratch.store("store", "one");
        append(&store, &log);
        let read = dag(&store);
        for object in read.objects().values() {
            let Envelope::Change(change) = object else { continue };
            for dep in &change.deps {
                let event = read.get(dep).and_then(Envelope::event).expect("a dep carries an event");
                prop_assert_eq!(event.todo(), change.event.todo());
            }
        }
        prop_assert_eq!(findings(&store), Vec::<String>::new());

        let mut by_todo = log.clone();
        by_todo.sort_by(|a, b| a.todo().cmp(b.todo()));
        let grouped = scratch.store("grouped", "one");
        append(&grouped, &by_todo);
        prop_assert_eq!(names(&grouped), names(&store));
    }

    /// Law 23 — GENESES ARE DISJOINT. Two prodromes' files united verify
    /// clean, fold to the union of their folds keyed by genesis, and a write
    /// into one of them is the write that prodrome alone would make.
    #[test]
    fn no_edge_crosses_geneses(
        (shared, mine, theirs) in two_writers(),
        event in a_write(),
        when in 0i64..WINDOW,
    ) {
        let scratch = Scratch::new();
        let a = scratch.store("a", "a");
        let b = scratch.store("b", "b");
        append(&a, &[shared.clone(), mine].concat());
        append(&b, &[shared, theirs].concat());
        let united = scratch.union("united", &[&a, &b]);
        prop_assert_eq!(findings(&united), Vec::<String>::new());

        let read = dag(&united);
        let geneses = read.geneses();
        prop_assert_eq!(geneses.len(), 2);
        for (name, object) in read.objects() {
            let Envelope::Change(change) = object else { continue };
            for dep in &change.deps {
                let Some(Envelope::Change(held)) = read.get(dep) else {
                    panic!("{name:?} rests on {dep:?}, which is not a change")
                };
                prop_assert_eq!(&held.genesis, &change.genesis);
            }
        }

        let nodes = |store: &Store| dag(store).nodes().expect("reads");
        let mut apart = fold(&nodes(&a)).prodromes().clone();
        apart.extend(fold(&nodes(&b)).prodromes().clone());
        let whole = fold(&nodes(&united));
        prop_assert_eq!(whole.prodromes(), &apart);
        let t = moment(when);
        let mut rows = view::entries(&nodes(&a), t, &roster()).expect("folds");
        rows.extend(view::entries(&nodes(&b), t, &roster()).expect("folds"));
        rows.sort_by(|x, y| (&x.genesis, &x.key).cmp(&(&y.genesis, &y.key)));
        prop_assert_eq!(view::entries(&nodes(&united), t, &roster()).expect("folds"), rows);

        prop_assert!(united.append(event.clone()).is_err(), "a writer names its genesis");
        let genesis = dag(&a).geneses().into_iter().next().expect("a genesis");
        let written = united.in_genesis(genesis).append(event.clone()).expect("appends");
        prop_assert_eq!(written, a.append(event).expect("appends"));
    }

    /// Law 28 — A SNAPSHOT ATTESTS ITS CLOSURE: what the store held when it
    /// was written, and nothing after. Its name moves with any object in it,
    /// and `verify` reports an object it attests that the store lacks.
    #[test]
    fn snapshot_commits_to_closure(
        log in a_log(),
        later in a_schedule(0..10),
        pick in any::<prop::sample::Index>(),
    ) {
        let scratch = Scratch::new();
        let store = scratch.store("store", "one");
        append(&store, &log);
        let held = names(&store);
        let snapshot = store.snapshot().expect("attests");
        prop_assert_eq!(store.snapshot().expect("attests"), snapshot.clone(), "nothing new");
        append(&store, &realise(&later, WINDOW));
        let attested = dag(&store).closure([snapshot.clone()]);
        let mut expected = held.clone();
        expected.insert(snapshot.clone());
        prop_assert_eq!(attested, expected);
        prop_assert_eq!(findings(&store), Vec::<String>::new());

        let other = scratch.store("other", "one");
        let mut moved = log.clone();
        if !moved.is_empty() {
            let at = pick.index(moved.len());
            moved[at] = Draft {
                todo: "alpha",
                actor: "bassel",
                roll: 4,
                spec: None,
                items: 0,
                text: 0,
            }
            .at_with_note(moved[at].at(), "moved");
        }
        append(&other, &moved);
        let same = names(&other) == held;
        prop_assert_eq!(
            other.snapshot().expect("attests") == snapshot,
            same,
            "a snapshot's name is its closure"
        );

        let gone = held.iter().nth(pick.index(held.len())).expect("the genesis at least");
        fs::remove_file(store.root().join("objects").join(format!("{}.py", gone.as_str())))
            .expect("removes");
        let incomplete = Finding::Incomplete {
            snapshot,
            missing: gone.clone(),
        };
        prop_assert!(store.verify().contains(&incomplete));
    }

    /// Law 28 — ON A `previous` CHAIN, EACH CLOSURE CONTAINS THE ONE BEFORE,
    /// and what it adds is what was written since.
    #[test]
    fn snapshot_chain_grows(batches in prop::collection::vec(a_schedule(0..10), 1..4)) {
        let scratch = Scratch::new();
        let store = scratch.store("store", "one");
        let mut before: Option<(Hash, BTreeSet<Hash>)> = None;
        for (shift, batch) in (0..).step_by(WINDOW as usize).zip(&batches) {
            append(&store, &realise(batch, shift));
            let held = names(&store);
            let snapshot = store.snapshot().expect("attests");
            let attested = dag(&store).closure([snapshot.clone()]);
            if let Some((previous, was)) = &before {
                if snapshot != *previous {
                    let Envelope::Snapshot(object) = store.load(&snapshot).expect("loads") else {
                        panic!("a snapshot")
                    };
                    prop_assert_eq!(object.previous.as_ref(), Some(previous));
                }
                let earlier = dag(&store).closure([previous.clone()]);
                prop_assert!(earlier.is_subset(&attested));
                prop_assert!(attested.difference(&earlier).all(|name| !was.contains(name)));
            }
            prop_assert!(held.iter().all(|name| attested.contains(name)));
            before = Some((snapshot.clone(), names(&store)));
        }
        prop_assert_eq!(findings(&store), Vec::<String>::new());
    }

    /// Law 29 — PLACEMENT IS NOT IDENTITY. Changes written in a staging store
    /// that holds the base are accepted by moving their files into the base:
    /// every name stands, the base verifies clean once the host records its
    /// heads (a move is a door the store does not own, SPEC §3), and they
    /// are the objects the base would have written itself.
    #[test]
    fn accept_is_a_same_name_move((shared, mine, _) in two_writers()) {
        let scratch = Scratch::new();
        let base = scratch.store("base", "one");
        append(&base, &shared);
        let own = scratch.union("own", &[&base]);
        let staged = append(&own, &mine);
        let held = names(&base);
        let moved: BTreeSet<Hash> = names(&own).difference(&held).cloned().collect();
        for name in &moved {
            fs::rename(file(&own, name), file(&base, name)).expect("moves");
        }
        // A new handle reads the moved files at once; `base`, which has
        // read the store, reads them at the next hold of the lock.
        let reopened = Store::new(base.root(), roster());
        prop_assert_eq!(names(&reopened), held.union(&moved).cloned().collect::<BTreeSet<_>>());
        let lagging = base
            .verify()
            .iter()
            .all(|finding| matches!(finding, Finding::StaleHeads { .. }));
        prop_assert!(lagging, "the record lags the moved files, and nothing else is reported");
        base.record_heads().expect("records");
        prop_assert_eq!(findings(&base), Vec::<String>::new());
        prop_assert_eq!(names(&base), held.union(&moved).cloned().collect::<BTreeSet<_>>());

        let direct = scratch.store("direct", "one");
        append(&direct, &shared);
        prop_assert_eq!(append(&direct, &mine), staged);
        prop_assert_eq!(names(&direct), names(&base));
        prop_assert_eq!(
            base.heads().expect("reads the record"),
            direct.heads().expect("reads the record"),
            "the recorded heads after the move are the heads the base would have written"
        );
    }

    /// Law 29 — A RE-PROPOSAL IS RECOGNISED. Rejected, a proposal leaves the
    /// staging store and a receipt keeps its name and `event_id`. Proposed
    /// again over the same heads it is the same object; over heads the base
    /// moved, a twin with the same `event_id`.
    #[test]
    fn reproposal_is_recognised(
        shared in a_log(),
        (draft, roll, at, later) in (a_draft(), 3u8..7, 0i64..WINDOW, 0i64..WINDOW),
    ) {
        let scratch = Scratch::new();
        let base = scratch.store("base", "one");
        append(&base, &shared);
        let own = scratch.union("own", &[&base]);
        let proposal = Draft { actor: "triage", roll, ..draft.clone() }
            .at_with_note(moment(at), "proposed");
        let p = own.append(proposal.clone()).expect("proposes");
        let receipt = (p.clone(), event_id(&proposal));
        fs::remove_file(file(&own, &p)).expect("rejects");

        prop_assert_eq!(own.append(proposal.clone()).expect("re-proposes"), receipt.0.clone());

        let meanwhile = Draft { actor: "bassel", roll, ..draft }
            .at_with_note(moment(at.max(later)), "meanwhile");
        base.append(meanwhile).expect("the base moves on");
        let moved = scratch.union("moved", &[&base]);
        let twin = moved.append(proposal).expect("re-proposes");
        prop_assert_ne!(&twin, &receipt.0);
        let Envelope::Change(change) = moved.load(&twin).expect("loads") else {
            panic!("an append writes a change")
        };
        prop_assert_eq!(event_id(&change.event), receipt.1);
    }
}
