//! Law 48 (SPEC §9): mergeable tag sets.
//!
//! Notes are tagged with labels. Each replica holds a membership store and
//! an alias store, each begun by one genesis. It writes the events that its
//! own tag set returns, and takes in other replicas' objects by `sync`, the
//! union of two object sets. Every law is about the tag set after that union. Where a
//! law needs a register's frontier, the test computes it from the
//! prodrome's streams with `Stamp::descends`, apart from the tag set.

use crate::common;

use std::collections::{BTreeMap, BTreeSet};

use prodrome::event::{canonical, canonical_envelope, parse_event, seal_hash, Actor, Envelope};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::registers::{fold, Prodrome, Stamp};
use prodrome::schema::Schema;
use prodrome::store::{sync, MemoryStore, Replica};
use prodrome::tags::alias::{self, Alias, AliasEvent, AliasVocabulary};
use prodrome::tags::membership::{self, MembershipEvent, MembershipVocabulary};
use prodrome::tags::set::{Disagreement, TagSet};
use prodrome::tags::Tagging;
use proptest::prelude::*;

/// A note's name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Note(String);

/// A label: a word of lowercase letters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Label(String);

impl TryFrom<String> for Note {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        Ok(Note(text))
    }
}

impl TryFrom<String> for Label {
    type Error = String;
    fn try_from(text: String) -> Result<Self, String> {
        if !text.is_empty() && text.chars().all(|c| c.is_ascii_lowercase()) {
            Ok(Label(text))
        } else {
            Err(format!("a label is lowercase letters, got {text:?}"))
        }
    }
}

prodrome::text_literal!(Note => |note| note.0.clone(), Label => |label| label.0.clone());

/// Notes tagged with labels.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Labels;

impl Tagging for Labels {
    type Target = Note;
    type Tag = Label;
    const TARGET: &'static str = "note";
    const TAG: &'static str = "label";
}

/// Labels tagged with labels: a target and a tag of one type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Kinds;

impl Tagging for Kinds {
    type Target = Label;
    type Tag = Label;
    const TARGET: &'static str = "label";
    const TAG: &'static str = "kind";
}

type Memberships = MembershipEvent<Labels>;
type Aliases = AliasEvent<Labels>;
type Tags = TagSet<Labels>;

const LABELS: [&str; 4] = ["red", "blue", "green", "gold"];
const ACTORS: [&str; 3] = ["ada", "ben", "cai"];

fn label(name: &str) -> Label {
    Label(name.to_owned())
}

fn note(n: usize) -> Note {
    Note(format!("n{n}"))
}

fn at(minute: u32) -> Datetime {
    Datetime::new(2026, 10, 21, 8 + (minute / 60) % 12, minute % 60, 0, 0).expect("an instant")
}

/// The genesis of a store at schema `E`, the same on every replica.
fn genesis<E: Schema>() -> Envelope<E> {
    Envelope::Genesis(mk_genesis("tags", &"0".repeat(32)).expect("a genesis"))
}

/// A store at schema `E` holding its genesis.
fn begun<E: Schema<Vocabulary: Default>>() -> MemoryStore<E> {
    let store = MemoryStore::default();
    let genesis = genesis::<E>();
    let print = canonical_envelope(&genesis).into_bytes();
    store
        .receive([seal_hash(&genesis)].into(), &|_| Some(print.clone()))
        .expect("begins");
    store
}

/// The one prodrome of `store`, the one its genesis begins.
fn prodrome<E: Schema>(store: &MemoryStore<E>) -> Prodrome<E> {
    let nodes = store
        .held()
        .expect("memory reads")
        .dag
        .nodes()
        .expect("a store is closed under parents");
    fold(&nodes)
        .prodromes()
        .get(&Some(seal_hash(&genesis::<E>())))
        .cloned()
        .unwrap_or_default()
}

fn names<E: Schema>(store: &MemoryStore<E>) -> BTreeSet<String> {
    let held = store.held().expect("memory reads");
    held.dag
        .objects()
        .keys()
        .map(|name| name.as_str().to_owned())
        .collect()
}

/// The writes of `stream` that no other write of it descends from.
fn frontier<E: Schema>(stream: &[Stamp<E>]) -> Vec<&Stamp<E>> {
    stream
        .iter()
        .filter(|earlier| {
            !stream
                .iter()
                .any(|later| later.name != earlier.name && later.descends(earlier))
        })
        .collect()
}

/// One replica: a store of each schema, and its writer.
struct Device {
    memberships: MemoryStore<Memberships>,
    aliases: MemoryStore<Aliases>,
    actor: Actor,
    clock: u32,
}

#[derive(Debug, Clone)]
enum Op {
    Add(usize, usize),
    Remove(usize, usize),
    Rename(usize, usize),
}

impl Device {
    fn new(n: usize) -> Self {
        Self {
            memberships: begun(),
            aliases: begun(),
            actor: Actor::new(ACTORS[n % ACTORS.len()]).expect("an actor"),
            clock: 0,
        }
    }

    fn read(&self) -> Tags {
        TagSet::of(&prodrome(&self.memberships), &prodrome(&self.aliases))
    }

    fn objects(&self) -> (BTreeSet<String>, BTreeSet<String>) {
        (names(&self.memberships), names(&self.aliases))
    }

    fn take(&self, other: &Device) {
        sync(&other.memberships, &self.memberships).expect("synced");
        sync(&other.aliases, &self.aliases).expect("synced");
    }

    /// Whether the pair of note `n` and the label `name`, as written, reads
    /// as on: some write of its frontier is an add.
    fn on(&self, n: usize, name: &str) -> bool {
        prodrome(&self.memberships)
            .iter()
            .filter(|(pair, _)| pair.target == note(n) && pair.tag == label(name))
            .any(|(_, stream)| {
                frontier(stream)
                    .iter()
                    .any(|stamp| canonical(&*stamp.event).starts_with("Added("))
            })
    }

    /// Each label's chosen alias: the alias of the write with the least
    /// object name in its register's frontier.
    fn chosen(&self) -> BTreeMap<Label, Alias<Labels>> {
        prodrome(&self.aliases)
            .iter()
            .filter_map(|(tag, stream)| {
                frontier(stream)
                    .into_iter()
                    .min_by(|one, two| one.name.cmp(&two.name))
                    .map(|stamp| (tag.clone(), stamp.event.body().clone()))
            })
            .collect()
    }

    /// Writes the events that this replica's tag set returns for `op`.
    /// Answers the pairs of the membership events it wrote.
    fn apply(&mut self, op: &Op) -> Vec<(Note, Label)> {
        let tags = self.read();
        self.clock += 1;
        let at = at(self.clock);
        let memberships = match op {
            Op::Add(n, name) => tags
                .add(&note(*n), &label(LABELS[*name]), at, &self.actor)
                .into_iter()
                .collect(),
            Op::Remove(n, name) => tags.remove(&note(*n), &label(LABELS[*name]), at, &self.actor),
            Op::Rename(from, to) => {
                let renamed =
                    tags.rename(&label(LABELS[*from]), &label(LABELS[*to]), at, &self.actor);
                for event in renamed {
                    self.aliases.append(event).expect("appended");
                }
                Vec::new()
            }
        };
        let mut pairs = Vec::new();
        for event in memberships {
            let pair = event.key().clone();
            self.memberships.append(event).expect("appended");
            pairs.push((pair.target, pair.tag));
        }
        pairs
    }

    /// How many alias objects a rename wrote.
    fn rename(&mut self, from: usize, to: usize) -> usize {
        let before = names(&self.aliases).len();
        self.apply(&Op::Rename(from, to));
        names(&self.aliases).len() - before
    }
}

fn an_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0usize..4, 0usize..4).prop_map(|(n, name)| Op::Add(n, name)),
        2 => (0usize..4, 0usize..4).prop_map(|(n, name)| Op::Remove(n, name)),
        1 => (0usize..4, 0usize..4).prop_map(|(from, to)| Op::Rename(from, to)),
    ]
}

/// A write over two notes and two labels, so that two replicas often write
/// one pair, or a rename among all four labels.
fn a_narrow_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0usize..2, 0usize..2).prop_map(|(n, name)| Op::Add(n, name)),
        3 => (0usize..2, 0usize..2).prop_map(|(n, name)| Op::Remove(n, name)),
        1 => (0usize..4, 0usize..4).prop_map(|(from, to)| Op::Rename(from, to)),
    ]
}

#[derive(Debug, Clone)]
enum Step {
    Write(usize, Op),
    Sync { from: usize, to: usize },
}

fn steps(devices: usize) -> impl Strategy<Value = Vec<Step>> {
    prop::collection::vec(
        prop_oneof![
            3 => (0..devices, an_op()).prop_map(|(device, op)| Step::Write(device, op)),
            1 => (0..devices, 0..devices).prop_map(|(from, to)| Step::Sync { from, to }),
        ],
        0..30,
    )
}

fn devices(n: usize) -> Vec<Device> {
    (0..n).map(Device::new).collect()
}

fn run(devices: &mut [Device], script: &[Step]) {
    for step in script {
        match step {
            Step::Write(device, op) => {
                devices[*device].apply(op);
            }
            Step::Sync { from, to } => {
                if from != to {
                    devices[*to].take(&devices[*from]);
                }
            }
        }
    }
}

/// A new replica holding the union of `parts`, taken in that order.
fn union(parts: &[&Device]) -> Device {
    let merged = Device::new(0);
    for part in parts {
        merged.take(part);
    }
    merged
}

fn tags_of(tags: &Tags, n: usize) -> BTreeSet<Label> {
    tags.tags(&note(n))
}

proptest! {
    #![proptest_config(common::cases::cases(64))]

    /// Law 48 (a): three replicas write and sync in any interleaving.
    /// However their objects are then merged, the tag set is one value:
    /// the union commutes, associates and is idempotent, and every replica
    /// reads it once each has taken in the others' objects.
    #[test]
    fn merging_is_commutative_associative_and_idempotent(script in steps(3)) {
        let mut all = devices(3);
        run(&mut all, &script);
        let [a, b, c] = [&all[0], &all[1], &all[2]];
        let ab = union(&[a, b]).read();
        prop_assert_eq!(&ab, &union(&[b, a]).read(), "commutative");
        let left = union(&[&union(&[a, b]), c]).read();
        let right = union(&[a, &union(&[b, c])]).read();
        prop_assert_eq!(&left, &right, "associative");
        let twice = union(&[a, a, b, b]);
        prop_assert_eq!(&twice.read(), &ab, "idempotent");
        prop_assert_eq!(twice.objects(), union(&[a, b]).objects(), "idempotent in objects");
        a.take(b);
        a.take(c);
        b.take(c);
        b.take(a);
        c.take(a);
        for device in [a, b, c] {
            prop_assert_eq!(&device.read(), &left, "converged");
        }
    }

    /// Law 48 (a): after any history and a full sync, each label reads
    /// through its chosen alias. The chosen alias is the alias of the
    /// frontier's write with the least object name. A label and the label
    /// that its chosen alias points to read alike. Every label reads as a
    /// label that reads as itself, and that is a label with no alias or the
    /// least name of a ring.
    #[test]
    fn every_name_reads_through_its_chosen_alias(script in steps(3)) {
        let mut all = devices(3);
        run(&mut all, &script);
        let merged = union(&[&all[0], &all[1], &all[2]]);
        let tags = merged.read();
        let chosen = merged.chosen();
        let mut rings: BTreeMap<Label, BTreeSet<Label>> = BTreeMap::new();
        for disagreement in tags.disagreements() {
            match disagreement {
                Disagreement::Ring { names, spelled } => {
                    rings.insert(spelled, names);
                }
                Disagreement::Aliases { tag, chosen: read, .. } => {
                    prop_assert_eq!(chosen.get(&tag), Some(&read), "the chosen alias of {:?}", tag);
                }
            }
        }
        for name in LABELS.map(label) {
            let spelled = tags.spelling(&name);
            prop_assert_eq!(tags.spelling(&spelled), spelled.clone(), "idempotent for {:?}", name);
            if let Some(Alias::As(to)) = chosen.get(&name) {
                prop_assert_eq!(tags.spelling(to), spelled.clone(), "{:?} follows its alias", name);
            }
            let terminal = !matches!(chosen.get(&spelled), Some(Alias::As(_)));
            let least_of_a_ring = rings
                .get(&spelled)
                .is_some_and(|ring| ring.iter().min() == Some(&spelled));
            prop_assert!(terminal || least_of_a_ring, "{:?} reads as {:?}", name, spelled);
        }
    }

    /// Law 48 (b): two replicas share a history, then write apart,
    /// renames included. After a merge in either direction, a pair, as
    /// written, reads as the one replica that wrote it after the fork left
    /// it. Where both did, it is on if either left it on: a removal takes
    /// off only the adds that its writer saw. A note carries the spelling
    /// of every name whose pair is on.
    #[test]
    fn an_add_and_a_remove_apart_merge_the_same_way_in_either_order(
        base in prop::collection::vec(a_narrow_op(), 0..8),
        mine in prop::collection::vec(a_narrow_op(), 0..6),
        theirs in prop::collection::vec(a_narrow_op(), 0..6),
    ) {
        let mut all = devices(2);
        for op in &base {
            all[0].apply(op);
        }
        all[1].take(&all[0]);
        let before = union(&[&all[0]]);
        let mut wrote: [BTreeSet<(Note, Label)>; 2] = Default::default();
        for (device, ops) in [(0, &mine), (1, &theirs)] {
            for op in ops {
                let pairs = all[device].apply(op);
                wrote[device].extend(pairs);
            }
        }
        let one = union(&[&all[0], &all[1]]);
        prop_assert_eq!(&one.read(), &union(&[&all[1], &all[0]]).read());
        let tags = one.read();
        for n in 0..4 {
            for name in LABELS {
                let pair = (note(n), label(name));
                let expected = match (wrote[0].contains(&pair), wrote[1].contains(&pair)) {
                    (false, false) => before.on(n, name),
                    (true, false) => all[0].on(n, name),
                    (false, true) => all[1].on(n, name),
                    (true, true) => all[0].on(n, name) || all[1].on(n, name),
                };
                prop_assert_eq!(one.on(n, name), expected, "note {} label {}", n, name);
            }
            let spelled: BTreeSet<Label> = LABELS
                .iter()
                .filter(|name| one.on(n, name))
                .map(|name| tags.spelling(&label(name)))
                .collect();
            prop_assert_eq!(tags_of(&tags, n), spelled);
        }
    }

    /// Law 48 (c): on a replica that merged three replicas' histories,
    /// after a rename, every note that carried the renamed tag or the tag
    /// that `to` stood for carries `to`, every other tag stays where it
    /// was, the rename settles the disagreements on the names it writes,
    /// and the same rename again writes nothing.
    #[test]
    fn a_rename_keeps_every_membership(
        script in steps(3),
        from in 0usize..4,
        to in 0usize..4,
    ) {
        let mut all = devices(3);
        run(&mut all, &script);
        let mut device = union(&[&all[0], &all[1], &all[2]]);
        device.clock = 100;
        let before = device.read();
        let (from_label, to_label) = (label(LABELS[from]), label(LABELS[to]));
        let merged = [before.spelling(&from_label), before.spelling(&to_label)];
        let memberships = device.objects().0;
        device.rename(from, to);
        let after = device.read();
        prop_assert_eq!(device.objects().0, memberships, "a rename writes no membership");
        for n in 0..4 {
            let expected: BTreeSet<Label> = tags_of(&before, n)
                .into_iter()
                .map(|tag| if merged.contains(&tag) { to_label.clone() } else { tag })
                .collect();
            prop_assert_eq!(tags_of(&after, n), expected);
        }
        for name in before.names(&from_label).iter().chain(&before.names(&to_label)) {
            prop_assert_eq!(after.spelling(name), to_label.clone());
        }
        prop_assert!(after.disagreements().iter().all(|disagreement| match disagreement {
            Disagreement::Aliases { tag, .. } => after.spelling(tag) != to_label,
            Disagreement::Ring { names, .. } => !names.contains(&to_label),
        }), "the rename settles the names it writes");
        prop_assert_eq!(device.rename(from, to), 0, "a second rename writes nothing");
    }

    /// Law 48: a set's memberships are its notes with their tags, and a
    /// filter shows exactly the notes that carry every chosen tag, each
    /// read in its current spelling.
    #[test]
    fn a_filter_shows_exactly_the_notes_with_every_chosen_tag(
        script in steps(2),
        chosen in prop::collection::btree_set(0usize..4, 0..=4),
    ) {
        let mut all = devices(2);
        run(&mut all, &script);
        let tags = union(&[&all[0], &all[1]]).read();
        let chosen: BTreeSet<Label> = chosen.iter().map(|n| label(LABELS[*n])).collect();
        let memberships = tags.memberships();
        for n in 0..6 {
            let on = tags_of(&tags, n);
            prop_assert_eq!(memberships.get(&note(n)).cloned().unwrap_or_default(), on.clone());
            prop_assert!(tags.shows(&BTreeSet::new(), &note(n)));
            prop_assert_eq!(
                tags.shows(&chosen, &note(n)),
                chosen.iter().all(|name| on.contains(&tags.spelling(name)))
            );
        }
        prop_assert!(memberships.values().all(|tags| !tags.is_empty()));
    }

    /// Every event of a script prints and parses back to itself, at its
    /// schema's vocabulary.
    #[test]
    fn every_event_prints_and_parses_back(script in prop::collection::vec(an_op(), 0..20)) {
        let mut device = Device::new(0);
        for op in &script {
            device.apply(op);
        }
        for stream in prodrome(&device.memberships).values() {
            for stamp in stream {
                let parsed: Memberships =
                    parse_event(&MembershipVocabulary::default(), &canonical(&*stamp.event))
                        .expect("parses");
                prop_assert_eq!(&parsed, &*stamp.event);
            }
        }
        for stream in prodrome(&device.aliases).values() {
            for stamp in stream {
                let parsed: Aliases =
                    parse_event(&AliasVocabulary::default(), &canonical(&*stamp.event))
                        .expect("parses");
                prop_assert_eq!(&parsed, &*stamp.event);
            }
        }
    }
}

/// The two replicas of the cases below, sharing what `shared` writes on
/// the first.
fn forked(shared: &[Op]) -> [Device; 2] {
    let [mut a, b] = [Device::new(0), Device::new(1)];
    for op in shared {
        a.apply(op);
    }
    b.take(&a);
    [a, b]
}

fn merged_both_ways(a: &Device, b: &Device) -> Tags {
    let one = union(&[a, b]).read();
    assert_eq!(one, union(&[b, a]).read(), "either order");
    one
}

const RED: usize = 0;
const BLUE: usize = 1;
const GREEN: usize = 2;
const GOLD: usize = 3;

/// Law 48 (d): a note that one replica tags under a name that another
/// replica renamed meanwhile carries the new name after the merge.
#[test]
fn a_tag_under_the_old_name_lands_in_the_renamed_tag() {
    let [mut a, mut b] = forked(&[Op::Add(0, GREEN)]);
    a.apply(&Op::Rename(GREEN, GOLD));
    b.apply(&Op::Add(1, GREEN));
    let tags = merged_both_ways(&a, &b);
    for n in [0, 1] {
        assert_eq!(
            tags_of(&tags, n),
            BTreeSet::from([label("gold")]),
            "note {n}"
        );
    }
}

/// Law 48 (d): a merge of one tag into another, concurrent with a removal
/// of the other under its own name, leaves the merged tag on, because the
/// removal did not see the add under the merged name.
#[test]
fn a_merge_concurrent_with_a_removal_brings_the_tag_back() {
    let [mut a, mut b] = forked(&[Op::Add(0, RED), Op::Add(0, GOLD)]);
    a.apply(&Op::Rename(GOLD, RED));
    b.apply(&Op::Remove(0, RED));
    let tags = merged_both_ways(&a, &b);
    assert_eq!(tags_of(&tags, 0), BTreeSet::from([label("red")]));
}

/// Law 48 (d): two renames into each other form a ring, which every name
/// reads as its least name, until a later rename settles it.
#[test]
fn two_renames_into_each_other_read_as_the_least_name_until_one_settles_them() {
    let [mut a, mut b] = forked(&[Op::Add(0, RED), Op::Add(1, BLUE)]);
    a.apply(&Op::Rename(RED, BLUE));
    b.apply(&Op::Rename(BLUE, RED));
    let tags = merged_both_ways(&a, &b);
    assert_eq!(
        tags.disagreements(),
        vec![Disagreement::Ring {
            names: BTreeSet::from([label("blue"), label("red")]),
            spelled: label("blue"),
        }]
    );
    for n in [0, 1] {
        assert_eq!(tags_of(&tags, n), BTreeSet::from([label("blue")]));
    }
    let mut settled = union(&[&a, &b]);
    settled.clock = 100;
    settled.apply(&Op::Rename(RED, RED));
    let tags = settled.read();
    assert!(tags.disagreements().is_empty());
    for n in [0, 1] {
        assert_eq!(tags_of(&tags, n), BTreeSet::from([label("red")]));
    }
}

/// Law 48 (d): two renames of one tag apart are a disagreement that every
/// replica reads alike, following the alias of the write with the least
/// object name.
#[test]
fn two_renames_of_one_tag_apart_are_a_disagreement_every_replica_reads_alike() {
    let [mut a, mut b] = forked(&[Op::Add(0, RED)]);
    a.apply(&Op::Rename(RED, BLUE));
    b.apply(&Op::Rename(RED, GREEN));
    let merged = union(&[&a, &b]);
    let tags = merged_both_ways(&a, &b);
    let disagreements = tags.disagreements();
    let [Disagreement::Aliases {
        tag,
        chosen,
        others,
    }] = disagreements.as_slice()
    else {
        panic!("one disagreement, got {disagreements:?}");
    };
    assert_eq!(tag, &label("red"));
    assert_eq!(merged.chosen().get(&label("red")), Some(chosen));
    let both: BTreeSet<Alias<Labels>> = [chosen.clone()]
        .into_iter()
        .chain(others.iter().cloned())
        .collect();
    assert_eq!(
        both,
        BTreeSet::from([Alias::As(label("blue")), Alias::As(label("green"))])
    );
    let Alias::As(spelled) = chosen else {
        panic!("an alias");
    };
    assert_eq!(tags_of(&tags, 0), BTreeSet::from([spelled.clone()]));
}

/// Law 48: one actor's add, removal and add again with one `at` print the
/// first add's event again, so the store writes nothing for the second
/// add, and the tag stays off.
#[test]
fn an_add_again_in_one_tick_is_the_first_add() {
    let device = Device::new(0);
    let ada = Actor::new("ada").expect("an actor");
    let add = |tags: &Tags| {
        tags.add(&note(0), &label("red"), at(1), &ada)
            .expect("an add")
    };
    let first = add(&device.read());
    device.memberships.append(first.clone()).expect("appended");
    for event in device.read().remove(&note(0), &label("red"), at(1), &ada) {
        device.memberships.append(event).expect("appended");
    }
    let again = add(&device.read());
    assert_eq!(canonical(&again), canonical(&first));
    let held = names(&device.memberships).len();
    device.memberships.append(again).expect("appended");
    assert_eq!(
        names(&device.memberships).len(),
        held,
        "the store writes nothing"
    );
    assert!(tags_of(&device.read(), 0).is_empty());
}

/// The events print with the tagging's field names, a target and a tag can
/// be of one type, and a tag is no alias of itself, built or parsed.
#[test]
fn the_events_print_with_the_tagging_s_field_names() {
    let ada = Actor::new("ada").expect("an actor");
    let added = membership::added::<Labels>(note(1), label("red"), at(1), ada.clone());
    assert_eq!(
        canonical(&added),
        "Added(note='n1', label='red', at=datetime(2026, 10, 21, 8, 1, 0), actor='ada')"
    );
    let removed = membership::removed::<Labels>(note(1), label("red"), at(2), ada.clone());
    assert_eq!(
        canonical(&removed),
        "Removed(note='n1', label='red', at=datetime(2026, 10, 21, 8, 2, 0), actor='ada')"
    );
    let aliased = alias::aliased::<Labels>(label("red"), label("blue"), at(3), ada.clone())
        .expect("an alias");
    assert_eq!(
        canonical(&aliased),
        "Aliased(label='red', at=datetime(2026, 10, 21, 8, 3, 0), actor='ada', to='blue')"
    );
    let unaliased =
        alias::unaliased::<Labels>(label("red"), at(4), ada.clone()).expect("its own name");
    assert_eq!(
        canonical(&unaliased),
        "Unaliased(label='red', at=datetime(2026, 10, 21, 8, 4, 0), actor='ada')"
    );
    assert_eq!(
        alias::aliased::<Labels>(label("red"), label("red"), at(5), ada.clone()),
        Err(alias::OfItself(label("red")))
    );
    let itself = "Aliased(label='red', at=datetime(2026, 10, 21, 8, 5, 0), actor='ada', to='red')";
    assert!(parse_event::<Aliases>(&AliasVocabulary::default(), itself).is_err());
    let kind = membership::added::<Kinds>(label("red"), label("color"), at(6), ada);
    let print = canonical(&kind);
    assert_eq!(
        print,
        "Added(label='red', kind='color', at=datetime(2026, 10, 21, 8, 6, 0), actor='ada')"
    );
    let parsed: MembershipEvent<Kinds> =
        parse_event(&MembershipVocabulary::default(), &print).expect("parses");
    assert_eq!(parsed, kind);
}
