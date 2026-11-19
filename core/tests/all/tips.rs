//! SPEC §9.17 — THE TIPS ARE DERIVED (§3), as properties over random DAGs.
//!
//! A store is its object files and nothing else, and its tips are the objects
//! no object names as a parent. Three things make that safe to rely on, and
//! each is a property here, over the pure `tips_of` AND over a real store on
//! disk, because the claim is about directories as much as about sets:
//!
//! - **A union's tips compose** (§9.17 a). The tips of `A ∪ B` are the tips of
//!   either side that the union does not name as a parent — so putting two
//!   stores' files in one directory (which is all a `git merge` of two clones
//!   of a store does) derives exactly the union's tips.
//! - **The listing does not matter** (§9.17 c). The answer is the same in
//!   whatever order the objects arrive or the files were written.
//! - **The tips cover.** Over a set closed under parents, every object is a
//!   tip or beneath one, and no tip is beneath another: nothing is unreachable,
//!   and no head is stale, by construction — which is why `verify` lost those
//!   two findings.
//!
//! §9.17 b, that two writers' unioned directories FOLD as the Prodrome's own
//! replica merge does, is about folds and lives with them in
//! `tests/all/fold_laws.rs`; §9.17's "no stored byte moves" is `tests/all/dag.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use prodrome::dag::Dag;
use prodrome::event::{
    canonical_envelope, mk_created, mk_sealed, mk_woven, parents_of, seal_hash, Envelope, Hash,
    TodoEvent,
};
use prodrome::literal::Datetime;
use prodrome::policy::Untrusted;
use prodrome::reference::Todo;
use prodrome::store::EventStore;
use proptest::prelude::*;

type Object = Envelope<TodoEvent<Todo>>;
type Objects = BTreeMap<Hash, Object>;
type Store = EventStore<TodoEvent<Todo>, Untrusted>;

/// A DAG as drawn: for each object in turn, raw picks among the objects before
/// it, which [`realise`] reduces to its parents. Genesis names none; any later
/// object naming none is another genesis, which is a real store too (two
/// unrelated replicas, unioned).
fn a_dag() -> impl Strategy<Value = Vec<Vec<usize>>> {
    prop::collection::vec(prop::collection::vec(0usize..1024, 0..4), 1..24)
}

/// The drawn DAG as objects: `Sealed` for one parent or none, `Woven` for
/// more, each carrying its own event so no two objects share a name.
fn realise(drawn: &[Vec<usize>]) -> (Vec<Hash>, Objects) {
    let at = Datetime::new(2026, 9, 27, 12, 0, 0, 0).expect("an instant");
    let mut names: Vec<Hash> = Vec::with_capacity(drawn.len());
    let mut objects = Objects::new();
    for (i, picks) in drawn.iter().enumerate() {
        let parents: BTreeSet<Hash> = if i == 0 {
            BTreeSet::new()
        } else {
            picks.iter().map(|pick| names[pick % i].clone()).collect()
        };
        let event = mk_created(&format!("t{i}"), at, "bassel", "", "").expect("valid");
        let object = if parents.len() > 1 {
            mk_woven(parents.into_iter().collect(), Some(event)).expect("distinct parents")
        } else {
            mk_sealed(parents.into_iter().next(), event)
        };
        let name = seal_hash(&object);
        names.push(name.clone());
        objects.insert(name, object);
    }
    (names, objects)
}

/// The tips of objects collected in the order given.
fn tips_of<'a>(objects: impl IntoIterator<Item = (&'a Hash, &'a Object)>) -> BTreeSet<Hash> {
    objects
        .into_iter()
        .map(|(name, object)| (name.clone(), object.clone()))
        .collect::<Dag<TodoEvent<Todo>>>()
        .tips()
}

/// Everything `seeds` rest on, and the seeds: a set closed under parents,
/// which is what every store is (and every clone of one).
fn closure(objects: &Objects, seeds: impl IntoIterator<Item = Hash>) -> Objects {
    let mut out = Objects::new();
    let mut pending: Vec<Hash> = seeds.into_iter().collect();
    while let Some(name) = pending.pop() {
        if out.contains_key(&name) {
            continue;
        }
        let object = objects[&name].clone();
        pending.extend(parents_of(&object));
        out.insert(name, object);
    }
    out
}

/// A DAG and two stores cut from it — each a random set of objects with
/// everything they rest on — as two clones that each wrote their own.
fn two_clones() -> impl Strategy<Value = (Vec<Vec<usize>>, Vec<usize>, Vec<usize>)> {
    (
        a_dag(),
        prop::collection::vec(0usize..1024, 0..4),
        prop::collection::vec(0usize..1024, 0..4),
    )
}

fn cut(names: &[Hash], objects: &Objects, picks: &[usize]) -> Objects {
    closure(
        objects,
        picks.iter().map(|pick| names[pick % names.len()].clone()),
    )
}

fn named_as_parents(objects: &Objects) -> BTreeSet<Hash> {
    objects.values().flat_map(parents_of).collect()
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A scratch directory that goes when the case is done with it.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let root = std::env::temp_dir().join(format!(
            "prodrome-tips-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        Scratch(root)
    }

    fn store(&self, name: &str) -> Store {
        Store::new(self.0.join(name), Untrusted::none())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Lay objects out as a store's files, in the order given — the only thing a
/// store on disk is.
fn write<'a>(root: &Path, objects: impl IntoIterator<Item = (&'a Hash, &'a Object)>) {
    let dir = root.join("objects");
    fs::create_dir_all(&dir).expect("creates objects/");
    for (name, object) in objects {
        fs::write(
            dir.join(format!("{}.py", name.as_str())),
            canonical_envelope(object),
        )
        .expect("writes an object");
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// §9.17 a — THE TIPS OF A UNION ARE THE UNION'S TIPS, and they compose: the tips
    /// of either side that the union does not name as a parent. On disk, two
    /// clones' files copied into one directory — a `git merge` of two clones —
    /// derive exactly that set.
    #[test]
    fn the_tips_of_a_union_compose((drawn, mine, theirs) in two_clones()) {
        let (names, objects) = realise(&drawn);
        let a = cut(&names, &objects, &mine);
        let b = cut(&names, &objects, &theirs);
        let mut union = a.clone();
        union.extend(b.clone());

        let composed: BTreeSet<Hash> = tips_of(&a)
            .union(&tips_of(&b))
            .filter(|tip| !named_as_parents(&union).contains(*tip))
            .cloned()
            .collect();
        prop_assert_eq!(tips_of(&union), composed);

        let scratch = Scratch::new();
        let (here, there, merged) =
            (scratch.store("here"), scratch.store("there"), scratch.store("merged"));
        write(here.root(), &a);
        write(there.root(), &b);
        prop_assert_eq!(here.tips().expect("derives"), tips_of(&a));
        prop_assert_eq!(there.tips().expect("derives"), tips_of(&b));
        write(merged.root(), &a);
        write(merged.root(), &b);
        prop_assert_eq!(merged.tips().expect("derives"), tips_of(&union));
        prop_assert!(merged.verify().is_empty(), "{:?}", merged.verify());
    }

    /// THE TIPS COVER, AND NONE IS STALE. Over a set closed under parents
    /// every object is a tip or an ancestor of one, and no tip is an ancestor
    /// of another — so the store's reads, which start from nothing but its
    /// files, miss nothing, and `verify` has no unreachable object or stale
    /// head left to report.
    #[test]
    fn the_tips_cover_the_store_and_none_rests_on_another((drawn, mine, _) in two_clones()) {
        let (names, objects) = realise(&drawn);
        let a = cut(&names, &objects, &mine);
        let tips = tips_of(&a);
        prop_assert!(a.is_empty() || !tips.is_empty());

        let scratch = Scratch::new();
        let store = scratch.store("store");
        write(store.root(), &a);
        let mut covered: BTreeSet<Hash> = tips.clone();
        for tip in &tips {
            let beneath = store.ancestors(tip).expect("walks");
            prop_assert!(
                tips.iter().all(|other| !beneath.contains(other)),
                "a tip rests on another tip"
            );
            covered.extend(beneath);
        }
        prop_assert_eq!(covered, a.keys().cloned().collect::<BTreeSet<Hash>>());
        prop_assert_eq!(store.dag().expect("reads").objects().len(), a.len());
    }

    /// §9.17 c — THE LISTING DOES NOT MATTER. The same objects, handed over in any
    /// order or written to disk in any order, derive the same tips.
    #[test]
    fn the_tips_do_not_depend_on_the_listing_order(
        (drawn, order) in a_dag().prop_flat_map(|drawn| {
            let indices: Vec<usize> = (0..drawn.len()).collect();
            (Just(drawn), Just(indices).prop_shuffle())
        })
    ) {
        let (names, objects) = realise(&drawn);
        let shuffled: Vec<(&Hash, &Object)> =
            order.iter().map(|i| (&names[*i], &objects[&names[*i]])).collect();
        let reversed: Vec<(&Hash, &Object)> = shuffled.iter().rev().copied().collect();
        prop_assert_eq!(tips_of(shuffled.iter().copied()), tips_of(&objects));
        prop_assert_eq!(tips_of(reversed), tips_of(&objects));

        let scratch = Scratch::new();
        let store = scratch.store("store");
        write(store.root(), shuffled.iter().copied());
        prop_assert_eq!(store.tips().expect("derives"), tips_of(&objects));
    }
}

/// The stored names of a record of heads (SPEC §3). A changed name or order
/// fails this test.
#[test]
fn the_record_of_heads_pins_its_stored_names() {
    use prodrome::dag::Heads;
    use prodrome::literal::codec::{assert_pinned, Literal};
    assert_pinned(Heads::CONSTRUCTORS, "Heads(count, tips)");
}
