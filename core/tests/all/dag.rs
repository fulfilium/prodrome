//! SPEC §9 law 8 for §3, on `conformance/dag.py`: linearisations, tips,
//! parents and `verify` findings EXACTLY.
//!
//! Each vector is a store the reference built by appending, adopting another
//! replica's objects, and sometimes merging — so the DAGs have two heads,
//! merges, and triage-actor events dated behind what they rest on. Each is
//! rebuilt here by writing the object files alone, which is the honest way to
//! test a READER: nothing about the order, the tips or the findings may depend
//! on this side having been the writer. The vectors' `tips` were the heads the
//! writer's `HEAD` and `refs/` named; that the objects alone derive them is
//! SPEC §9.17, and the second test lays those files out too and says so.

use crate::common;

use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::vectors::{each, field, integer, strings, text_at, vectors};
use prodrome::event::{parents_of, seal_hash, Actor, Hash, TodoEvent};
use prodrome::literal::Value;
use prodrome::policy::Untrusted;
use prodrome::reference::Todo;
use prodrome::store::EventStore;

/// The vectors were taken with the reference payload, so the store these read
/// them into holds that record shape.
type Store = EventStore<TodoEvent<Todo>, Untrusted>;
fn roster() -> Untrusted {
    // The policy is the DEPLOYMENT's, never the engine's (§5) — it arrives as
    // a parameter here exactly as it does at every other call site. These
    // vectors were generated under the reference policy with `{"triage"}` on
    // its roster, so that is what their `verify` findings were taken under.
    Untrusted::of([Actor::new("triage").expect("valid")])
}

fn findings_of(store: &Store) -> Vec<String> {
    store.verify().iter().map(ToString::to_string).collect()
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// One vector's objects, by name — the map the JSON keyed and the literal
/// holds as a tuple of `Object(name=, literal=)`.
fn objects_of(dag: &Value) -> BTreeMap<String, String> {
    each(dag, "objects")
        .iter()
        .map(|o| {
            (
                text_at(o, "name").to_owned(),
                text_at(o, "literal").to_owned(),
            )
        })
        .collect()
}

/// Write a vector's objects out as a store on disk — the object files and
/// NOTHING ELSE, because that is all a store is (§3): its tips are derived.
fn materialise(dag: &Value) -> Store {
    let root = std::env::temp_dir().join(format!(
        "prodrome-dag-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("objects")).expect("creates the store");
    for (name, text) in &objects_of(dag) {
        fs::write(root.join("objects").join(format!("{name}.py")), text).expect("writes an object");
    }
    Store::new(root, roster())
}

/// The same store as the 0.8 writer left it: the objects, plus `HEAD` naming
/// one head and `refs/` naming every head while there were several — the
/// shape its `set_heads` produced, and the shape every store written before
/// the tips were derived is in.
fn materialise_as_written_before(dag: &Value) -> Store {
    let store = materialise(dag);
    let root = store.root();
    let tips = strings(dag, "tips");
    if tips.len() > 1 {
        fs::create_dir_all(root.join("refs")).expect("creates refs/");
        for tip in &tips {
            fs::write(root.join("refs").join(tip), tip).expect("writes a ref");
        }
    }
    fs::write(root.join("HEAD"), &tips[0]).expect("writes HEAD");
    store
}

/// What the 0.8 reader took for the heads: `refs/` when it named any, HEAD
/// otherwise. Kept here, in the test, as the one statement of the old reading
/// the law below compares against — the crate no longer has it.
fn heads_as_read_before(root: &std::path::Path) -> Vec<String> {
    let mut named: Vec<String> = fs::read_dir(root.join("refs"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    named.sort();
    if named.is_empty() {
        let head = fs::read_to_string(root.join("HEAD")).expect("HEAD reads");
        named.push(head.trim().to_owned());
    }
    named
}

/// SPEC §9.17 — NO STORED BYTE MOVES. For every conformance store, laid out
/// the way the 0.8 writer left it, the tips derived from the objects are
/// EXACTLY the heads its `HEAD` and `refs/` named; and `verify` finds what
/// the vector found, plus the two files as leftovers and nothing else.
#[test]
fn every_dag_vector_derives_the_heads_its_old_files_named() {
    let data = vectors("dag.py");
    let mut forked = 0;
    for dag in each(&data, "dags") {
        let seed = integer(field(dag, "seed"));
        let store = materialise_as_written_before(dag);
        let derived: Vec<String> = store
            .tips()
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"))
            .iter()
            .map(|tip| tip.as_str().to_owned())
            .collect();
        let named = heads_as_read_before(store.root());
        assert_eq!(
            derived, named,
            "seed {seed}: derived tips against HEAD/refs"
        );
        if named.len() > 1 {
            forked += 1;
        }
        let mut expected = strings(dag, "verify");
        let mut leftovers = vec![
            "HEAD is left from before tips were derived (SPEC §3): nothing reads it, delete it"
                .to_owned(),
        ];
        if named.len() > 1 {
            leftovers.push(
                "refs/ is left from before tips were derived (SPEC §3): nothing reads it, delete it"
                    .to_owned(),
            );
        }
        expected.splice(0..0, leftovers);
        assert_eq!(findings_of(&store), expected, "seed {seed}: verify");
        let _ = fs::remove_dir_all(store.root());
    }
    assert!(forked > 0, "some vectors' old files named two heads");
}

#[test]
fn every_dag_vector_linearises_tips_parents_and_verifies_alike() {
    let data = vectors("dag.py");
    let dags = each(&data, "dags");
    assert!(!dags.is_empty());
    let mut merges = 0;
    let mut forked = 0;
    let mut findings = 0;
    for dag in dags {
        let seed = integer(field(dag, "seed"));
        let objects = objects_of(dag);
        let store = materialise(dag);
        let read = store.dag().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let read: Vec<(Hash, &prodrome::event::Envelope<TodoEvent<Todo>>)> = read
            .linearise()
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"))
            .iter()
            .map(|name| {
                let object = read.get(name).expect("the order names objects");
                (name.clone(), object)
            })
            .collect();

        let order: Vec<&str> = read.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            order,
            strings(dag, "linearisation"),
            "seed {seed}: linearisation"
        );

        let tips: Vec<String> = store
            .tips()
            .unwrap_or_else(|e| panic!("seed {seed}: {e}"))
            .iter()
            .map(|tip| tip.as_str().to_owned())
            .collect();
        assert_eq!(tips, strings(dag, "tips"), "seed {seed}: tips");
        if tips.len() > 1 {
            forked += 1;
        }

        let parents_of_name: BTreeMap<String, Vec<String>> = each(dag, "parents")
            .iter()
            .map(|p| (text_at(p, "name").to_owned(), strings(p, "parents")))
            .collect();
        for (name, object) in &read {
            let parents: Vec<String> = parents_of(object)
                .iter()
                .map(|parent| parent.as_str().to_owned())
                .collect();
            assert_eq!(
                &parents,
                &parents_of_name[name.as_str()],
                "seed {seed}: parents"
            );
            // Every object is still named by its own print.
            assert_eq!(seal_hash(object).as_str(), name.as_str());
            assert_eq!(
                &prodrome::event::canonical_envelope(object),
                &objects[name.as_str()]
            );
            if object.event().is_none() {
                merges += 1;
            }
        }

        let verify = strings(dag, "verify");
        assert_eq!(findings_of(&store), verify, "seed {seed}: verify");
        findings += verify.len();
        let _ = fs::remove_dir_all(store.root());
    }
    // The corpus exercises what it is for: forked stores, merge objects, and
    // real findings. A zero here would mean the vectors stopped covering a case
    // and this file went quietly green.
    assert!(forked > 0, "some vectors have two heads");
    assert!(merges > 0, "some vectors hold a merge envelope");
    assert!(findings > 0, "some vectors have verify findings");
}
