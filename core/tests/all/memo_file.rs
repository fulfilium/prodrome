//! Laws 51 to 58 (design §6.2): a cache held in a file is the cache held in
//! memory.
//!
//! The trees, the edits and the two algebras are those of `memo_fold.rs`.
//! A second handle on one file stands for a second process: `flock` locks
//! an open file description, so two handles in one process exclude each
//! other as two processes do.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use prodrome::event::Hash;
use prodrome::memo::file::{BATCH, SLACK};
use prodrome::memo::{
    self, fold, Algebra, Cache, File, Front, Holder, Keep, Key, Lookup, Lru, Memory, Outcome,
};
use proptest::prelude::*;
use proptest::sample::Index;

use crate::memo_fold::{
    apply, build, cata, edits, positions, shape, sum, weigh, Labels, Node, Sum,
};
use crate::memory::Scratch;

/// A file in a fresh scratch directory, removed with the directory.
fn scratch(what: &str) -> (Scratch, PathBuf) {
    let scratch = Scratch::new(what);
    fs::create_dir_all(&scratch.0).expect("a scratch directory");
    let path = scratch.0.join("cache");
    (scratch, path)
}

fn open<V: prodrome::literal::codec::Literal + Clone + Eq>(path: &Path) -> File<V> {
    File::open(path).expect("a cache file opens")
}

/// Every node's value under `algebra`, by name.
fn values<A: Algebra<Node>>(algebra: &A, tree: &Node, into: &mut BTreeMap<Hash, A::Out>) {
    into.insert(tree.name.clone(), cata(algebra, tree));
    tree.kids.iter().for_each(|kid| values(algebra, kid, into));
}

/// Every pair `holder` holds.
fn graph<V: Ord + Clone>(holder: &mut impl Holder<V>) -> BTreeSet<(Key, V)> {
    let mut pairs = BTreeSet::new();
    for key in holder.keys().collect::<Vec<_>>() {
        for value in holder.get(&key).values() {
            pairs.insert((key.clone(), value));
        }
    }
    pairs
}

/// A name for a small number, so generated keys collide.
fn hash(n: u8) -> Hash {
    Hash::new(format!("{n:064x}")).expect("64 hex")
}

fn key(function: u8, argument: u8) -> Key {
    Key {
        function: hash(function),
        argument: hash(argument),
    }
}

fn pairs() -> impl Strategy<Value = Vec<(u8, u8, u32)>> {
    prop::collection::vec((0..2u8, 0..4u8, 0..3u32), 0..12)
}

/// An algebra whose values are long and unique to their node, so that a
/// test can look for a value's bytes in a file.
struct Spell(Hash);

impl Algebra<Node> for Spell {
    type Out = String;
    fn name(&self) -> &Hash {
        &self.0
    }
    fn apply(&self, node: &Node, children: Vec<String>) -> String {
        format!("value of {} over {}", node.name, children.len())
    }
}

fn spell() -> Spell {
    Spell(memo::name(b"spell", []))
}

// --- law 57: one interface --------------------------------------------------

/// One operation on a holder.
#[derive(Debug, Clone)]
enum Op {
    Add(u8, u8, u32),
    Remove(u8, u8),
    Forget(Vec<(u8, u8)>),
    /// The holder settled, dropped and opened again, where it is a file.
    Reopen,
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    let op = prop_oneof![
        6 => (0..2u8, 0..4u8, 0..3u32).prop_map(|(f, a, v)| Op::Add(f, a, v)),
        2 => (0..2u8, 0..4u8).prop_map(|(f, a)| Op::Remove(f, a)),
        1 => prop::collection::vec((0..2u8, 0..4u8), 0..3).prop_map(Op::Forget),
        1 => Just(Op::Reopen),
    ];
    prop::collection::vec(op, 0..24)
}

/// The relation a holder must be: each key's distinct values in the order
/// they arrived.
type Model = BTreeMap<Key, Vec<u32>>;

fn lookup(model: &Model, key: &Key) -> Lookup<u32> {
    model.get(key).cloned().unwrap_or_default().into()
}

/// The interpreters a law runs over, each a fresh holder: what each is,
/// and how to open it again after a reopen.
trait Interpreter {
    type Holder: Holder<u32>;
    fn make(&self) -> Self::Holder;
    fn reopen(&self, holder: Self::Holder) -> Self::Holder;
}

struct InMemory;
impl Interpreter for InMemory {
    type Holder = Memory<u32>;
    fn make(&self) -> Memory<u32> {
        Memory::default()
    }
    fn reopen(&self, holder: Memory<u32>) -> Memory<u32> {
        holder
    }
}

struct InFile(PathBuf);
impl Interpreter for InFile {
    type Holder = File<u32>;
    fn make(&self) -> File<u32> {
        let _ = fs::remove_file(&self.0);
        open(&self.0)
    }
    fn reopen(&self, mut holder: File<u32>) -> File<u32> {
        holder.settle().expect("settles");
        drop(holder);
        open(&self.0)
    }
}

struct CacheInFile(PathBuf);
impl Interpreter for CacheInFile {
    type Holder = Cache<u32, Keep, File<u32>>;
    fn make(&self) -> Self::Holder {
        let _ = fs::remove_file(&self.0);
        Cache::held(open(&self.0), Keep)
    }
    fn reopen(&self, mut holder: Self::Holder) -> Self::Holder {
        holder.settle().expect("settles");
        drop(holder);
        Cache::held(open(&self.0), Keep)
    }
}

struct FrontOfFile(PathBuf, usize);
impl Interpreter for FrontOfFile {
    type Holder = Front<u32, Lru, File<u32>>;
    fn make(&self) -> Self::Holder {
        let _ = fs::remove_file(&self.0);
        Front::new(Lru::new(self.1), open(&self.0))
    }
    fn reopen(&self, mut holder: Self::Holder) -> Self::Holder {
        holder.settle().expect("settles");
        drop(holder);
        Front::new(Lru::new(self.1), open(&self.0))
    }
}

/// Runs `ops` on a holder and on the model, and checks after each that
/// every key reads alike, that `add` answers a finding exactly when the
/// model's key comes to hold its second value, and that the keys agree.
fn bisimulates<I: Interpreter>(interpreter: &I, ops: &[Op]) -> Result<(), TestCaseError> {
    let mut holder = interpreter.make();
    let mut model = Model::new();
    for op in ops {
        match op {
            Op::Add(f, a, v) => {
                let values = model.entry(key(*f, *a)).or_default();
                let second = !values.contains(v) && values.len() == 1;
                if !values.contains(v) {
                    values.push(*v);
                }
                prop_assert_eq!(holder.add(key(*f, *a), *v).is_err(), second);
            }
            Op::Remove(f, a) => {
                let held = model.remove(&key(*f, *a)).is_some();
                prop_assert_eq!(holder.remove(&key(*f, *a)), held);
            }
            Op::Forget(keys) => {
                let doomed: BTreeSet<Key> = keys.iter().map(|(f, a)| key(*f, *a)).collect();
                model.retain(|key, _| !doomed.contains(key));
                holder.forget(&doomed).expect("forgets");
            }
            Op::Reopen => holder = interpreter.reopen(holder),
        }
        for f in 0..2 {
            for a in 0..4 {
                prop_assert_eq!(holder.get(&key(f, a)), lookup(&model, &key(f, a)));
            }
        }
        let keys: BTreeSet<Key> = holder.keys().collect();
        prop_assert_eq!(&keys, &model.keys().cloned().collect::<BTreeSet<_>>());
        prop_assert_eq!(holder.len(), model.len());
    }
    Ok(())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 57: every holder is one relation. In memory, in a file, as a
    /// cache over a file, and as memory in front of a file, a holder reads
    /// every key as the set of pairs added and not removed or forgotten,
    /// across a reopen of the file, and its `add` answers a finding exactly
    /// when a key comes to hold a second value.
    #[test]
    fn every_holder_is_one_relation(ops in ops(), front in 0..4usize) {
        let (_scratch, path) = scratch("relation");
        bisimulates(&InMemory, &ops)?;
        bisimulates(&InFile(path.clone()), &ops)?;
        bisimulates(&CacheInFile(path.clone()), &ops)?;
        bisimulates(&FrontOfFile(path, front), &ops)?;
    }
}

// --- law 51: reading through a kept cache is computing the function ------

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 51: a fold through a cache answers the plain fold, with an empty
    /// cache, a warm cache in memory, a warm file, the file opened again by
    /// a new handle, a file that a second handle wrote while the first was
    /// open, and memory in front of a file; and a warm file opened again
    /// hits at the root.
    #[test]
    fn a_kept_cache_reads_as_the_function(
        shape in shape(),
        edits in edits(),
        front in 0..8usize,
    ) {
        let (_scratch, path) = scratch("function");
        let mut labels = Labels::shared();
        let tree = build(&shape, &mut labels);
        let edited = apply(&tree, &positions(&tree, &edits), &mut labels).0;
        let mut memory = Cache::default();
        let mut file = Cache::held(open::<u64>(&path), Keep);
        for tree in [&tree, &edited, &tree] {
            prop_assert_eq!(fold(&sum(), tree, &mut Cache::default()).0, cata(&sum(), tree));
            prop_assert_eq!(fold(&sum(), tree, &mut memory).0, cata(&sum(), tree));
            prop_assert_eq!(fold(&sum(), tree, &mut file).0, cata(&sum(), tree));
            prop_assert_eq!(fold(&weigh(), tree, &mut file).0, cata(&weigh(), tree));
        }
        // A second handle, open beside the first, writes the edited tree's
        // other algebra; the first reads it after its next write.
        let mut second: File<u64> = open(&path);
        prop_assert_eq!(fold(&sum(), &edited, &mut second).0, cata(&sum(), &edited));
        second.settle().expect("settles");
        file.settle().expect("settles");
        drop(file);
        drop(second);
        let mut again = open::<u64>(&path);
        for tree in [&tree, &edited] {
            let (value, report) = fold(&sum(), tree, &mut again);
            prop_assert_eq!(value, cata(&sum(), tree));
            prop_assert_eq!(report.visits().len(), 1, "a warm file hits at the root");
            prop_assert_eq!(report.visits()[0].outcome, Outcome::Hit);
        }
        let mut fronted = Front::new(Lru::new(front), again);
        for tree in [&tree, &edited, &tree] {
            prop_assert_eq!(fold(&sum(), tree, &mut fronted).0, cata(&sum(), tree));
            prop_assert_eq!(fold(&weigh(), tree, &mut fronted).0, cata(&weigh(), tree));
        }
    }
}

// --- law 52: a bad entry is a miss -----------------------------------------

/// What happens to a file between two runs.
#[derive(Debug, Clone)]
enum Damage {
    /// Cut short at this place after the header.
    Cut(Index),
    /// This byte after the header flipped by this mask.
    Flip(Index, u8),
    /// These bytes put in at this place after the header.
    Insert(Index, Vec<u8>),
}

fn damages() -> impl Strategy<Value = Vec<Damage>> {
    let damage = prop_oneof![
        any::<Index>().prop_map(Damage::Cut),
        (any::<Index>(), 1..=255u8).prop_map(|(at, mask)| Damage::Flip(at, mask)),
        (any::<Index>(), prop::collection::vec(any::<u8>(), 1..40))
            .prop_map(|(at, bytes)| Damage::Insert(at, bytes)),
    ];
    prop::collection::vec(damage, 1..4)
}

fn damage(bytes: &mut Vec<u8>, damage: &Damage) {
    let body = bytes.len() - 16;
    match damage {
        Damage::Cut(at) => bytes.truncate(16 + at.index(body + 1)),
        Damage::Flip(at, mask) if body > 0 => bytes[16 + at.index(body)] ^= mask,
        Damage::Flip(..) => {}
        Damage::Insert(at, more) => {
            let at = 16 + at.index(body + 1);
            bytes.splice(at..at, more.iter().copied());
        }
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// Law 52: a dropped, cut short, damaged or foreign entry is a miss.
    /// After a file is cut short, has bytes flipped, has bytes put in, and
    /// holds records of the same keys that another program wrote as text,
    /// every key reads as a miss or as the node's own value, never as
    /// another value and never as a finding, and a fold answers the plain
    /// fold.
    #[test]
    fn a_bad_entry_is_a_miss(shape in shape(), damages in damages(), foreign in any::<Index>()) {
        let (_scratch, path) = scratch("damage");
        let tree = build(&shape, &mut Labels::unique());
        let mut file = open::<u64>(&path);
        let _ = fold(&sum(), &tree, &mut file);
        file.settle().expect("settles");
        let names: Vec<Hash> = file.keys().map(|key| key.argument).collect();
        drop(file);
        // Another program's records: text at the keys of some nodes.
        let mut text = open::<String>(&path);
        for name in names.iter().take(foreign.index(names.len() + 1)) {
            let key = Key { function: sum().0, argument: name.clone() };
            let _ = text.add(key, format!("not a number at {name}"));
        }
        text.settle().expect("settles");
        drop(text);
        let mut bytes = fs::read(&path).expect("reads");
        for each in &damages {
            damage(&mut bytes, each);
        }
        fs::write(&path, &bytes).expect("writes");
        let mut expected = BTreeMap::new();
        values(&sum(), &tree, &mut expected);
        let mut file = open::<u64>(&path);
        for (name, value) in &expected {
            let key = Key { function: sum().0, argument: name.clone() };
            match file.get(&key) {
                Lookup::Miss => {}
                Lookup::Hit(held) => prop_assert_eq!(held, *value),
                Lookup::Finding(held) => prop_assert!(false, "a finding {:?}", held),
            }
        }
        let (value, report) = fold(&sum(), &tree, &mut file);
        prop_assert_eq!(value, cata(&sum(), &tree));
        prop_assert_eq!(report.names(Outcome::Finding).count(), 0);
    }
}

// --- law 53: another name is never a hit ------------------------------------

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 53: an entry written under another name of the function is never
    /// a hit. The same code under a name that covers other inputs, read
    /// through a warm file opened again, computes every node it looks at.
    #[test]
    fn another_name_is_never_a_hit(shape in shape()) {
        let (_scratch, path) = scratch("names");
        let tree = build(&shape, &mut Labels::unique());
        let mut file = open::<u64>(&path);
        let _ = fold(&sum(), &tree, &mut file);
        let _ = fold(&weigh(), &tree, &mut file);
        file.settle().expect("settles");
        drop(file);
        let other = Sum(memo::name(b"sum", [&memo::name(b"another input", [])]));
        let mut file = open::<u64>(&path);
        let (value, report) = fold(&other, &tree, &mut file);
        prop_assert_eq!(value, cata(&sum(), &tree));
        prop_assert_eq!(report.names(Outcome::Hit).count(), 0);
    }
}

// --- law 54: a stop leaves a subset ------------------------------------------

/// The bytes of a file after a stop in the write from `before` to `after`:
/// the first `cut` bytes of `after` over `before`, or, with a `mask`, each
/// byte from either, as a write that reached the disk out of order leaves
/// them; at any length from the shorter's to the longer's.
fn stopped(
    before: &[u8],
    after: &[u8],
    cut: Index,
    length: Index,
    mask: Option<&[bool]>,
) -> Vec<u8> {
    let short = before.len().min(after.len());
    let long = before.len().max(after.len());
    let length = short + length.index(long - short + 1);
    let cut = cut.index(long + 1);
    (0..length)
        .map(|at| {
            let new = mask.map_or(at < cut, |mask| mask[at % mask.len()]);
            let (first, second) = if new {
                (after, before)
            } else {
                (before, after)
            };
            first.get(at).or(second.get(at)).copied().unwrap_or(0)
        })
        .collect()
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 54: after a stop at any point of a write, the file reads as a
    /// cache that holds a subset of what was written. The writes are an
    /// append of a fold, an append of a fold of an edited tree, a forget,
    /// and a compaction; a stop leaves a prefix of the new bytes over the
    /// old, or any mix of the two, at any length between.
    #[test]
    fn a_stop_leaves_a_subset(
        shape in shape(),
        edits in edits(),
        doomed in prop::collection::vec(any::<bool>(), 1..8),
        cut in any::<Index>(),
        length in any::<Index>(),
        mask in prop::collection::vec(any::<bool>(), 1..64),
    ) {
        let (scratch, path) = scratch("stop");
        let mut labels = Labels::shared();
        let tree = build(&shape, &mut labels);
        let edited = apply(&tree, &positions(&tree, &edits), &mut labels).0;
        let mut written = BTreeMap::new();
        values(&sum(), &tree, &mut written);
        values(&sum(), &edited, &mut written);
        let mut file = Cache::held(open::<u64>(&path), Keep);
        let mut states = vec![fs::read(&path).expect("reads")];
        for tree in [&tree, &edited] {
            let _ = fold(&sum(), tree, &mut file);
            file.settle().expect("settles");
            states.push(fs::read(&path).expect("reads"));
        }
        let keys: Vec<Key> = file.keys().collect();
        let forgotten: BTreeSet<Key> = keys
            .iter()
            .zip(doomed.iter().cycle())
            .filter(|(_, doom)| **doom)
            .map(|(key, _)| key.clone())
            .collect();
        file.forget(&forgotten).expect("forgets");
        states.push(fs::read(&path).expect("reads"));
        drop(file);
        let mixed = scratch.0.join("mixed");
        for pair in states.windows(2) {
            for mask in [None, Some(mask.as_slice())] {
                fs::write(&mixed, stopped(&pair[0], &pair[1], cut, length, mask)).expect("writes");
                let mut file = open::<u64>(&mixed);
                for (key, value) in graph(&mut file) {
                    prop_assert_eq!(&key.function, &sum().0);
                    prop_assert_eq!(Some(&value), written.get(&key.argument));
                }
            }
        }
    }
}

// --- law 55: a forget erases ----------------------------------------------

/// Whether `needle` occurs in `haystack`.
fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

proptest! {
    #![proptest_config(crate::common::cases::cases(64))]

    /// Law 55: after a forget, no byte of a forgotten value is in the file.
    /// The values are long and unique to their nodes. After a forget of
    /// some keys, through memory in front of the file, with a second handle
    /// open on it that wrote some of the same pairs, the file holds no
    /// forgotten value's text, and every other value it held still reads;
    /// the forgotten keys miss through the front and through the second
    /// handle.
    #[test]
    fn a_forget_erases(shape in shape(), doomed in prop::collection::vec(any::<bool>(), 1..8)) {
        let (_scratch, path) = scratch("forget");
        let tree = build(&shape, &mut Labels::unique());
        let mut second: File<String> = open(&path);
        let _ = fold(&spell(), &tree, &mut second);
        second.settle().expect("settles");
        let mut front = Front::new(Lru::new(4), open::<String>(&path));
        let _ = fold(&spell(), &tree, &mut front);
        front.settle().expect("settles");
        let mut expected = BTreeMap::new();
        values(&spell(), &tree, &mut expected);
        let forgotten: BTreeSet<Key> = expected
            .keys()
            .zip(doomed.iter().cycle())
            .filter(|(_, doom)| **doom)
            .map(|(name, _)| Key { function: spell().0, argument: name.clone() })
            .collect();
        front.forget(&forgotten).expect("forgets");
        let bytes = fs::read(&path).expect("reads");
        for (name, value) in &expected {
            let key = Key { function: spell().0, argument: name.clone() };
            if forgotten.contains(&key) {
                prop_assert!(!holds(&bytes, value.as_bytes()), "a forgotten value is in the file");
                prop_assert_eq!(front.get(&key), Lookup::Miss);
                prop_assert_eq!(second.get(&key), Lookup::Miss);
            } else {
                prop_assert!(holds(&bytes, value.as_bytes()));
                prop_assert_eq!(second.get(&key), Lookup::Hit(value.clone()));
            }
        }
    }
}

// --- law 56: join on files is the union ---------------------------------------

proptest! {
    #![proptest_config(crate::common::cases::cases(128))]

    /// Law 56: `join` on files is the union, as it is in memory. Two files
    /// of drawn pairs, the second joined into the first, hold the union of
    /// their pairs, which is what the join of the same pairs in memory
    /// holds, and the joined file opened again holds it too.
    #[test]
    fn join_on_files_is_the_union(a in pairs(), b in pairs()) {
        let (scratch, left) = scratch("join");
        let right = scratch.0.join("right");
        let fill = |path: &Path, pairs: &[(u8, u8, u32)]| {
            let mut file = Cache::held(open::<u32>(path), Keep);
            let mut memory = Cache::default();
            for &(f, a, v) in pairs {
                let _ = file.insert(key(f, a), v);
                let _ = memory.insert(key(f, a), v);
            }
            (file, memory)
        };
        let (mut one, mut one_memory) = fill(&left, &a);
        let (mut two, mut two_memory) = fill(&right, &b);
        let union: BTreeSet<_> = graph(&mut one).union(&graph(&mut two)).cloned().collect();
        let _ = one.join(&mut two);
        let _ = one_memory.join(&mut two_memory);
        prop_assert_eq!(&graph(&mut one), &union);
        prop_assert_eq!(&graph(&mut one_memory), &union);
        one.settle().expect("settles");
        drop(one);
        prop_assert_eq!(&graph(&mut open::<u32>(&left)), &union);
    }
}

// --- law 58: a policy bounds the file ----------------------------------------

proptest! {
    #![proptest_config(crate::common::cases::cases(8))]

    /// Law 58: an eviction policy means on a file what it means in memory.
    /// Under a policy that keeps at most `capacity` keys, a file holds at
    /// most that many after any run of inserts and after it is opened
    /// again, and it is never longer than twice its live records, the
    /// slack and one buffer of new records.
    #[test]
    fn a_policy_bounds_the_file(capacity in 1..64usize, inserts in 600..4000usize) {
        let (_scratch, path) = scratch("bound");
        let mut file = Cache::held(open::<u64>(&path), Lru::new(capacity));
        // A record is 104 bytes and the print of its value.
        let largest = 104 + u64::MAX.to_string().len() as u64;
        for n in 0..inserts {
            let key = Key {
                function: memo::name(b"bound", []),
                argument: memo::name(&n.to_be_bytes(), []),
            };
            let _ = file.insert(key, n as u64 * 7919);
            prop_assert!(file.len() <= capacity);
            let length = fs::metadata(&path).expect("a file").len();
            prop_assert!(length <= 16 + 2 * capacity as u64 * largest + SLACK + BATCH as u64);
        }
        file.settle().expect("settles");
        drop(file);
        let again = Cache::held(open::<u64>(&path), Lru::new(capacity));
        prop_assert!(again.len() <= capacity);
    }
}

proptest! {
    #![proptest_config(crate::common::cases::cases(16))]

    /// Law 51, with two writers at once: two threads, each with its own
    /// handle on one file, fold different trees and forget and settle in
    /// turn, many times over. A third handle opened afterwards folds both
    /// trees to the plain fold, and every key it holds reads as its node's
    /// value.
    #[test]
    fn two_writers_at_once_leave_one_cache(one in shape(), two in shape(), rounds in 1..6usize) {
        let (_scratch, path) = scratch("writers");
        let mut labels = Labels::unique();
        let trees = [build(&one, &mut labels), build(&two, &mut labels)];
        std::thread::scope(|scope| {
            for tree in &trees {
                let path = path.clone();
                scope.spawn(move || {
                    let mut file = Front::new(Lru::new(2), Cache::held(open::<u64>(&path), Lru::new(64)));
                    for round in 0..rounds {
                        assert_eq!(fold(&sum(), tree, &mut file).0, cata(&sum(), tree));
                        if round % 2 == 1 {
                            let doomed = BTreeSet::from([Key { function: sum().0, argument: tree.name.clone() }]);
                            file.forget(&doomed).expect("forgets");
                        }
                        file.settle().expect("settles");
                    }
                });
            }
        });
        let mut expected = BTreeMap::new();
        for tree in &trees {
            values(&sum(), tree, &mut expected);
        }
        let mut file = open::<u64>(&path);
        for (key, value) in graph(&mut file) {
            prop_assert_eq!(Some(&value), expected.get(&key.argument));
        }
        for tree in &trees {
            prop_assert_eq!(fold(&sum(), tree, &mut file).0, cata(&sum(), tree));
        }
    }
}
