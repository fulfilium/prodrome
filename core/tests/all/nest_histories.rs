//! Law 10 (design §6.3, §7) over histories held in registers: nests of two
//! and three levels, generated over four inner schemas (reviews, todos,
//! shelves of reviews, and proposals at a schema declared as data), each
//! sub-history its own prodrome in its own replica
//! and every level's objects in one replica of its schema, as one object
//! database holds a git repository's trees.
//!
//! Join renames nothing: the flattened history holds every object of every
//! level once, at its path, under the name it had. Reading it at a held
//! register's path reads the inner history there, through a replica that
//! accepts exactly those objects. A pointer's reading is the heads its
//! last write saw. And happens-before across levels is exactly what the
//! pointers say: an inner object's past is its own level's, and an outer
//! write's past reaches an inner object only through heads it, or a write
//! beneath it, named. A nest is a tree of content-addressed histories, so
//! the memoised fold folds it, the join is that fold of one algebra, and a
//! write deep in it recomputes only the spine it moved.

use crate::common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use prodrome::dag::Dag;
use prodrome::event::{
    canonical_envelope, mk_created, mk_tended, seal_hash, Envelope, Hash, TodoEvent,
};
use prodrome::genesis::mk_genesis;
use prodrome::memo::{fold, Algebra, Cache, Outcome};
use prodrome::nest::{pointer, History, Holding, Leaf, Level, Nest, Path, Segment};
use prodrome::reference::Todo;
use prodrome::registers::Folded;
use prodrome::schema::Schema;
use prodrome::store::Sender;
use prodrome::store::{accept, sync, MemoryStore, Replica};
use proptest::prelude::*;

use crate::review::{moved, opened, Review, PHASES};

#[path = "../schemas/holder.rs"]
mod holder;

use holder::{placed, Holder, Slot};

/// A sub-history: a replica in memory, writing into its own prodrome.
pub(crate) struct Sub<E: Schema> {
    pub(crate) store: MemoryStore<E>,
}

/// A replica at the schema `schema` holding a genesis of its own,
/// `label`'s, writing into it.
pub(crate) fn sub<E: Schema>(schema: E::Vocabulary, label: &str) -> Sub<E> {
    let object = Envelope::<E>::Genesis(mk_genesis(label, &nonce_of(label)).expect("a genesis"));
    let print = canonical_envelope(&object).into_bytes();
    let name = seal_hash(&object);
    let store = MemoryStore::at(schema);
    store
        .receive(
            [name.clone()].into(),
            &|_| Some(print.clone()),
            Sender::Stranger,
        )
        .expect("a genesis verifies");
    Sub {
        store: store.in_genesis(name),
    }
}

/// 32 hex digits of the label's bytes, so every label is its own prodrome.
fn nonce_of(label: &str) -> String {
    let mut digits: String = label.bytes().map(|b| format!("{b:02x}")).collect();
    digits.truncate(32);
    format!("{digits:0>32}")
}

fn segment(text: &str) -> Segment {
    Segment::new(text).expect("a segment")
}

/// The path a shelf's held history sits at.
fn held(shelf: &str) -> Path {
    Path::of([segment(shelf), segment("held")])
}

fn shelf(n: usize) -> String {
    format!("s{n}")
}

/// A level's history read straight from a replica: each object at its
/// entity's key, or at the root.
fn level<E: Schema<Key: AsRef<str>>>(replica: &impl Replica<E>) -> History<Hash> {
    let dag = replica.held().expect("reads").dag;
    dag.objects()
        .iter()
        .map(|(name, object)| {
            let at = object
                .event()
                .map_or_else(Path::root, |event| segment(event.key().as_ref()).into());
            (at, name.clone())
        })
        .collect()
}

pub(crate) fn dag<E: Schema>(replica: &impl Replica<E>) -> Arc<Dag<E>> {
    replica.held().expect("reads").dag
}

/// What a replica reads: its objects, their fold and its tips.
#[derive(Debug, PartialEq)]
struct Reading<E: Schema> {
    objects: BTreeSet<Hash>,
    folded: Folded<E>,
    tips: BTreeSet<Hash>,
}

fn reading<E: Schema>(replica: &impl Replica<E>) -> Reading<E> {
    let held = replica.held().expect("reads");
    Reading {
        objects: held.dag.objects().keys().cloned().collect(),
        folded: (*held.folded).clone(),
        tips: held.tips,
    }
}

/// One replica holding every sub-history of a level.
pub(crate) fn union<E: Schema>(subs: &[&Sub<E>]) -> MemoryStore<E> {
    let union = MemoryStore::at(subs[0].store.schema().clone());
    for sub in subs {
        sync(&sub.store, &union, Sender::Stranger).expect("syncs");
    }
    union
}

/// A review event: opened, or moved to a phase, at its own instant. A move
/// below its review's reading is refused, and the step writes nothing.
fn a_review(n: usize, pick: u8) -> Review {
    let pr = ["r1", "r2"][usize::from(pick) % 2];
    let at = common::moment(i64::try_from(n).expect("small") * 60);
    match usize::from(pick / 2) % 5 {
        0 => opened(pr, at, "bassel", "a review"),
        phase => moved(pr, at, "bassel", PHASES[phase - 1]),
    }
}

/// A todo event: created or tended, at its own instant.
fn a_todo(n: usize, pick: u8) -> TodoEvent<Todo> {
    let todo = ["t1", "t2"][usize::from(pick) % 2];
    let at = common::moment(i64::try_from(n).expect("small") * 60);
    if pick % 4 < 2 {
        mk_created(todo, at, "bassel", "a todo", "").expect("an event")
    } else {
        mk_tended(todo, at, "bassel", "").expect("an event")
    }
}

/// Point `shelf` of `outer` at the heads `inner` holds now, and remember
/// what that write saw.
fn point<I: Schema>(
    outer: &Sub<Holder<I>>,
    shelf: &str,
    inner: &Sub<I>,
    n: usize,
    seen: &mut BTreeMap<Hash, BTreeSet<Hash>>,
) {
    let heads = inner.store.tips().expect("derives");
    let at = common::moment(i64::try_from(n).expect("small") * 60);
    let name = outer
        .store
        .append(placed(shelf, at, "bassel", heads.clone()))
        .expect("a pointer is written");
    seen.insert(name, heads);
}

/// One thing that happens to a nest of two levels.
#[derive(Debug, Clone)]
enum Step {
    /// An event written into a shelf's history.
    Inner(usize, u8),
    /// A shelf pointed at what its history holds now.
    Point(usize),
}

fn steps(shelves: usize) -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        prop_oneof![
            3 => (0..shelves, any::<u8>()).prop_map(|(s, pick)| Step::Inner(s, pick)),
            1 => (0..shelves).prop_map(Step::Point),
        ],
        0..24,
    )
}

/// A nest of two levels: an outer replica of shelves, a sub-history per
/// shelf, and what each pointer write saw.
struct Two<I: Schema> {
    outer: Sub<Holder<I>>,
    shelves: Vec<Sub<I>>,
    seen: BTreeMap<Hash, BTreeSet<Hash>>,
}

fn two<I: Schema>(
    schema: &I::Vocabulary,
    shelves: usize,
    steps: &[Step],
    make: fn(usize, u8) -> I,
) -> Two<I> {
    let two = Two {
        outer: sub(Default::default(), "outer"),
        shelves: (0..shelves)
            .map(|s| sub(schema.clone(), &format!("inner {s}")))
            .collect(),
        seen: BTreeMap::new(),
    };
    let mut seen = two.seen;
    for (n, step) in steps.iter().enumerate() {
        match step {
            Step::Inner(s, pick) => {
                let _ = two.shelves[*s].store.append(make(n, *pick));
            }
            Step::Point(s) => point(&two.outer, &shelf(*s), &two.shelves[*s], n, &mut seen),
        }
    }
    for (s, inner) in two.shelves.iter().enumerate() {
        point(&two.outer, &shelf(s), inner, steps.len() + s, &mut seen);
    }
    Two { seen, ..two }
}

impl<I: Schema<Key: AsRef<str>>> Two<I> {
    /// The nest the outer replica's tips rest on, its inner level one
    /// replica of every shelf's history; and that replica.
    fn nest(&self) -> (Nest, MemoryStore<I>) {
        let inner = union(&self.shelves.iter().collect::<Vec<_>>());
        let leaf = Leaf::new(dag(&inner));
        let root = Holding::new(dag(&self.outer.store)).holds(Slot::Held, &leaf);
        let nest = root
            .nest(&self.outer.store.tips().expect("derives"))
            .expect("a nest");
        (nest, inner)
    }
}

/// Law 10 over two levels of the inner schema `I`.
fn two_levels<I: Schema<Key: AsRef<str>>>(two: &Two<I>) -> Result<(), TestCaseError> {
    let (nest, inner) = two.nest();
    let flat = nest.flatten();

    // Nothing renamed: every object of every replica, once, at one path.
    let mut every: BTreeSet<Hash> = reading(&two.outer.store).objects;
    let mut count = every.len();
    for sub in &two.shelves {
        let objects = reading(&sub.store).objects;
        count += objects.len();
        every.extend(objects);
    }
    prop_assert_eq!(flat.len(), count);
    prop_assert_eq!(flat.values().cloned().collect::<BTreeSet<Hash>>(), every);
    prop_assert_eq!(flat.at(&Path::root()).len(), flat.len());

    let folded = reading(&two.outer.store).folded;
    let pointed = |s: usize| {
        folded
            .entities()
            .find(|(key, _)| key.as_ref() == shelf(s))
            .map(|(_, stream)| pointer(stream, Slot::Held))
            .expect("every shelf is pointed")
    };
    for (s, sub) in two.shelves.iter().enumerate() {
        let path = held(&shelf(s));
        // Reading commutes with join.
        prop_assert_eq!(&flat.at(&path), &level(&sub.store));
        prop_assert_eq!(&flat.at(&path), &nest.held()[&path].flatten());
        let read = MemoryStore::<I>::at(inner.schema().clone());
        accept(
            &inner,
            &read,
            flat.at(&path).values().cloned().collect(),
            Sender::Stranger,
        )
        .expect("accepts");
        prop_assert_eq!(reading(&read), reading(&sub.store));
        // The pointer reads as the heads its last write saw.
        prop_assert_eq!(pointed(s), sub.store.tips().expect("derives"));
    }

    // Happens-before across levels is what the pointers say.
    let outer = dag(&two.outer.store);
    let shelves: Vec<Arc<Dag<I>>> = two.shelves.iter().map(|sub| dag(&sub.store)).collect();
    let inner_objects: BTreeSet<Hash> = shelves
        .iter()
        .flat_map(|dag| dag.objects().keys().cloned())
        .collect();
    for name in outer.objects().keys() {
        let past = nest.past(name).expect("in the nest");
        let mut beneath = outer.closure([name.clone()]);
        beneath.remove(name);
        let mut saw = BTreeSet::new();
        for write in outer.closure([name.clone()]) {
            if let Some(heads) = two.seen.get(&write) {
                for dag in &shelves {
                    let known: BTreeSet<Hash> = heads
                        .iter()
                        .filter(|head| dag.get(head).is_some())
                        .cloned()
                        .collect();
                    saw.extend(dag.closure(known));
                }
            }
        }
        prop_assert_eq!(
            past.intersection(&inner_objects)
                .cloned()
                .collect::<BTreeSet<_>>(),
            saw
        );
        prop_assert_eq!(
            past.difference(&inner_objects)
                .cloned()
                .collect::<BTreeSet<_>>(),
            beneath
        );
    }
    for dag in &shelves {
        for name in dag.objects().keys() {
            let mut beneath = dag.closure([name.clone()]);
            beneath.remove(name);
            prop_assert_eq!(nest.past(name), Some(beneath), "an inner past is inner");
        }
    }
    Ok(())
}

/// One thing that happens to a nest of three levels: shelves of shelves of
/// reviews.
#[derive(Debug, Clone)]
enum Deep {
    Leaf(usize, usize, u8),
    Middle(usize, usize),
    Outer(usize),
}

fn deep_steps() -> impl Strategy<Value = Vec<Deep>> {
    prop::collection::vec(
        prop_oneof![
            3 => (0..2usize, 0..2usize, any::<u8>()).prop_map(|(o, m, pick)| Deep::Leaf(o, m, pick)),
            1 => (0..2usize, 0..2usize).prop_map(|(o, m)| Deep::Middle(o, m)),
            1 => (0..2usize).prop_map(Deep::Outer),
        ],
        0..30,
    )
}

/// A nest of three levels, and what each pointer write at each level saw.
struct Three {
    outer: Sub<Holder<Holder<Review>>>,
    middles: Vec<Sub<Holder<Review>>>,
    leaves: Vec<Vec<Sub<Review>>>,
    seen: BTreeMap<Hash, BTreeSet<Hash>>,
}

fn three(steps: &[Deep]) -> Three {
    let mut three = Three {
        outer: sub(Default::default(), "outer"),
        middles: (0..2)
            .map(|o| sub(Default::default(), &format!("middle {o}")))
            .collect(),
        leaves: (0..2)
            .map(|o| {
                (0..2)
                    .map(|m| sub(Default::default(), &format!("leaf {o} {m}")))
                    .collect()
            })
            .collect(),
        seen: BTreeMap::new(),
    };
    for (n, step) in steps.iter().enumerate() {
        three.step(n, step);
    }
    let mut n = steps.len();
    for o in 0..2 {
        for m in 0..2 {
            three.step(n, &Deep::Middle(o, m));
            n += 1;
        }
        three.step(n, &Deep::Outer(o));
        n += 1;
    }
    three
}

impl Three {
    fn step(&mut self, n: usize, step: &Deep) {
        match step {
            Deep::Leaf(o, m, pick) => {
                let _ = self.leaves[*o][*m].store.append(a_review(n, *pick));
            }
            Deep::Middle(o, m) => point(
                &self.middles[*o],
                &shelf(*m),
                &self.leaves[*o][*m],
                n,
                &mut self.seen,
            ),
            Deep::Outer(o) => point(
                &self.outer,
                &shelf(*o),
                &self.middles[*o],
                n,
                &mut self.seen,
            ),
        }
    }

    fn nest(&self) -> Nest {
        let leaves = union(&self.leaves.iter().flatten().collect::<Vec<_>>());
        let middles = union(&self.middles.iter().collect::<Vec<_>>());
        let leaf = Leaf::new(dag(&leaves));
        let middle = Holding::new(dag(&middles)).holds(Slot::Held, &leaf);
        let root = Holding::new(dag(&self.outer.store)).holds(Slot::Held, &middle);
        root.nest(&self.outer.store.tips().expect("derives"))
            .expect("a nest")
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(24))]

    /// Two levels, a shelf of reviews.
    #[test]
    fn a_nest_of_reviews_flattens_and_reads_as_its_parts(steps in steps(3)) {
        two_levels(&two(&Default::default(), 3, &steps, a_review))?;
    }

    /// Two levels, a shelf of todos.
    #[test]
    fn a_nest_of_todos_flattens_and_reads_as_its_parts(steps in steps(2)) {
        two_levels(&two(&Default::default(), 2, &steps, a_todo))?;
    }

    /// Two levels, a shelf of proposals at a schema declared as data.
    #[test]
    fn a_nest_of_declared_proposals_flattens_and_reads_as_its_parts(steps in steps(2)) {
        let schema = crate::declared::proposals();
        let two = two(schema, 2, &steps, crate::declared::a_declared_proposal);
        two_levels(&two)?;
        // And the memoised fold of the join's algebra is the join.
        let (nest, _) = two.nest();
        let algebra = Flatten(prodrome::memo::name(b"nest flatten", []));
        let (value, _) = fold(&algebra, &nest, &mut Cache::default());
        prop_assert_eq!(value, nest.flatten());
    }

    /// Three levels, shelves of shelves of reviews. The flattened history
    /// holds every object of every level once; reading it at a shelf's path
    /// reads the middle history there, flattened, and at a path two shelves
    /// deep reads the reviews there, as a replica that accepts exactly them
    /// reads; an outer write's past reaches a review only through what the
    /// middle writes beneath what it named had named.
    #[test]
    fn three_levels_flatten_and_read_as_their_parts(steps in deep_steps()) {
        let three = three(&steps);
        let nest = three.nest();
        let flat = nest.flatten();
        let mut count = reading(&three.outer.store).objects.len();
        for (o, middle) in three.middles.iter().enumerate() {
            count += reading(&middle.store).objects.len();
            let at = held(&shelf(o));
            prop_assert_eq!(&flat.at(&at), &nest.held()[&at].flatten());
            prop_assert_eq!(&nest.held()[&at].own().clone(), &level(&middle.store));
            for (m, leaf) in three.leaves[o].iter().enumerate() {
                count += reading(&leaf.store).objects.len();
                let path = at.then(&held(&shelf(m)));
                prop_assert_eq!(&flat.at(&path), &level(&leaf.store));
                let read = MemoryStore::<Review>::default();
                accept(&leaf.store, &read, flat.at(&path).values().cloned().collect(), Sender::Stranger)
                    .expect("accepts");
                prop_assert_eq!(reading(&read), reading(&leaf.store));
            }
        }
        prop_assert_eq!(flat.len(), count);

        let outer = dag(&three.outer.store);
        let middles: Vec<Arc<Dag<Holder<Review>>>> = three.middles.iter().map(|sub| dag(&sub.store)).collect();
        let leaves: Vec<Arc<Dag<Review>>> = three.leaves.iter().flatten().map(|sub| dag(&sub.store)).collect();
        let reviews: BTreeSet<Hash> = leaves.iter().flat_map(|dag| dag.objects().keys().cloned()).collect();
        let named = |seen: &BTreeSet<Hash>, dags: &[Arc<Dag<Review>>]| -> BTreeSet<Hash> {
            dags.iter()
                .flat_map(|dag| {
                    dag.closure(seen.iter().filter(|head| dag.get(head).is_some()).cloned())
                })
                .collect()
        };
        for name in outer.objects().keys() {
            let past = nest.past(name).expect("in the nest");
            let mut saw = BTreeSet::new();
            for write in outer.closure([name.clone()]) {
                if let Some(heads) = three.seen.get(&write) {
                    for middle in &middles {
                        let known = heads.iter().filter(|head| middle.get(head).is_some()).cloned();
                        for beneath in middle.closure(known) {
                            if let Some(leaf_heads) = three.seen.get(&beneath) {
                                saw.extend(named(leaf_heads, &leaves));
                            }
                        }
                    }
                }
            }
            prop_assert_eq!(past.intersection(&reviews).cloned().collect::<BTreeSet<_>>(), saw);
        }
    }
}

/// A write's past holds what its writer saw and not what came after: a
/// review written after a shelf was pointed is in the nest only once a
/// later write points at it, and only that write's past holds it.
#[test]
fn an_outer_write_saw_exactly_the_heads_it_names() {
    let steps = [Step::Inner(0, 0), Step::Point(0), Step::Inner(0, 1)];
    let two = two(&Default::default(), 1, &steps, a_review);
    let (nest, _) = two.nest();
    let reviews = dag(&two.shelves[0].store);
    let first = reviews
        .objects()
        .iter()
        .find(|(_, object)| {
            object
                .event()
                .is_some_and(|event| event.key().as_ref() == "r1")
        })
        .map(|(name, _)| name.clone())
        .expect("the first review");
    let second = reviews
        .objects()
        .iter()
        .find(|(_, object)| {
            object
                .event()
                .is_some_and(|event| event.key().as_ref() == "r2")
        })
        .map(|(name, _)| name.clone())
        .expect("the second review");
    let (early, late): (Vec<_>, Vec<_>) = two
        .seen
        .iter()
        .partition(|(_, heads)| !heads.contains(&second));
    assert_eq!((early.len(), late.len()), (1, 1));
    let early = nest.past(early[0].0).expect("in the nest");
    let late = nest.past(late[0].0).expect("in the nest");
    assert!(early.contains(&first) && !early.contains(&second));
    assert!(late.contains(&first) && late.contains(&second));
    assert_eq!(
        nest.past(&second).map(|past| past.contains(&first)),
        Some(false)
    );
}

/// An inner object's deps are inner: a pointer naming an object its inner
/// level does not hold (here, the outer genesis) is a missing object, never
/// a cross-level edge.
#[test]
fn a_pointer_to_another_levels_object_is_missing() {
    let two = two(&Default::default(), 1, &[Step::Inner(0, 0)], a_review);
    let (_, inner) = two.nest();
    let outer: BTreeSet<Hash> = reading(&two.outer.store).objects;
    let leaf = Leaf::new(dag(&inner));
    let refused = leaf.nest(&outer).expect_err("refused");
    assert!(refused.to_string().contains("missing object"), "{refused}");
}

/// The join as an algebra: a level's own objects, and each child's result
/// under the path that held it.
struct Flatten(Hash);

impl Algebra<Nest> for Flatten {
    type Out = History<Hash>;

    fn name(&self) -> &Hash {
        &self.0
    }

    fn apply(&self, node: &Nest, children: Vec<History<Hash>>) -> History<Hash> {
        std::iter::once((Path::root(), node.own().clone()))
            .chain(node.held().keys().cloned().zip(children))
            .collect::<History<History<Hash>>>()
            .join()
    }
}

/// Every nest's name in the tree.
fn names(nest: &Nest, into: &mut BTreeSet<Hash>) {
    into.insert(nest.name().clone());
    for held in nest.held().values() {
        names(held, into);
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(24))]

    /// THE MEMOISED FOLD FOLDS A NEST (law 8 on law 10's trees). The
    /// memoised fold of the join's algebra is the join, cold and warm; and
    /// after a review is written two shelves deep and the pointers above it
    /// name it, a fold through the same cache computes exactly the nests
    /// whose names moved, which are the spine from that review's history to
    /// the root, one per level, and hits the top of each one beside it.
    #[test]
    fn the_memoised_fold_recomputes_only_the_spine_a_write_moved(
        steps in deep_steps(),
        o in 0..2usize,
        m in 0..2usize,
    ) {
        let mut three = three(&steps);
        let before = three.nest();
        let algebra = Flatten(prodrome::memo::name(b"nest flatten", []));
        let mut cache: Cache<History<Hash>> = Cache::default();
        let (value, _) = fold(&algebra, &before, &mut cache);
        prop_assert_eq!(&value, &before.flatten());

        let n = steps.len() + 10;
        three.step(n, &Deep::Leaf(o, m, 0));
        three.step(n + 1, &Deep::Middle(o, m));
        three.step(n + 2, &Deep::Outer(o));
        let after = three.nest();
        let (value, report) = fold(&algebra, &after, &mut cache);
        prop_assert_eq!(&value, &after.flatten());
        prop_assert_eq!(&value, &fold(&algebra, &after, &mut Cache::default()).0);

        let (mut old, mut new) = (BTreeSet::new(), BTreeSet::new());
        names(&before, &mut old);
        names(&after, &mut new);
        let moved: BTreeSet<Hash> = new.difference(&old).cloned().collect();
        let middle = &after.held()[&held(&shelf(o))];
        let spine: BTreeSet<Hash> = [
            after.name().clone(),
            middle.name().clone(),
            middle.held()[&held(&shelf(m))].name().clone(),
        ]
        .into();
        prop_assert_eq!(&moved, &spine);
        prop_assert_eq!(report.names(Outcome::Computed).cloned().collect::<BTreeSet<_>>(), spine);
        let depths: Vec<(usize, usize)> = report
            .by_depth()
            .iter()
            .map(|counts| (counts.computed, counts.hits))
            .collect();
        prop_assert_eq!(depths, vec![(1, 0), (1, 1), (1, 1)]);
    }
}

/// CONCURRENT POINTERS READ AS THE UNION. Two replicas of one shelf each
/// point it at a different branch of the same inner history, neither seeing
/// the other's; once synced, the pointer reads both branches' heads, the
/// nest holds both, and each write's past holds its own branch alone.
#[test]
fn concurrent_pointers_read_as_the_union_of_their_histories() {
    let base: Sub<Review> = sub(Default::default(), "inner 0");
    base.store
        .append(a_review(0, 0))
        .expect("a review is opened");
    let fork = || {
        let store = MemoryStore::default();
        sync(&base.store, &store, Sender::Stranger).expect("syncs");
        let genesis = genesis_of(&store);
        Sub {
            store: store.in_genesis(genesis),
        }
    };
    let (left, right) = (fork(), fork());
    let x = left
        .store
        .append(a_review(1, 1))
        .expect("r2 opened on the left");
    let y = right
        .store
        .append(a_review(2, 2))
        .expect("r1 opened again on the right");

    let mut seen = BTreeMap::new();
    let here: Sub<Holder<Review>> = sub(Default::default(), "outer");
    let there = MemoryStore::default();
    sync(&here.store, &there, Sender::Stranger).expect("syncs");
    let there = Sub {
        store: there.in_genesis(genesis_of(&here.store)),
    };
    point(&here, "s0", &left, 3, &mut seen);
    point(&there, "s0", &right, 4, &mut seen);
    sync(&there.store, &here.store, Sender::Stranger).expect("syncs");

    let two = Two {
        outer: here,
        shelves: vec![left, right],
        seen,
    };
    let (nest, _) = two.nest();
    let folded = reading(&two.outer.store).folded;
    let (_, stream) = folded.entities().next().expect("one shelf");
    let heads: BTreeSet<Hash> = two.seen.values().flatten().cloned().collect();
    assert!(heads.contains(&x) && heads.contains(&y));
    assert_eq!(
        pointer(stream, Slot::Held),
        heads,
        "the reading is the union"
    );
    let flat = nest.flatten().at(&held("s0"));
    assert!(flat.values().any(|name| *name == x) && flat.values().any(|name| *name == y));
    for (write, saw) in &two.seen {
        let past = nest.past(write).expect("in the nest");
        let (mine, theirs) = if saw.contains(&x) { (&x, &y) } else { (&y, &x) };
        assert!(past.contains(mine) && !past.contains(theirs));
    }
}

fn genesis_of<E: Schema>(store: &MemoryStore<E>) -> Hash {
    store
        .held()
        .expect("reads")
        .dag
        .geneses()
        .into_iter()
        .next()
        .expect("one genesis")
}
