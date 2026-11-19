//! §6.6 — the DAG as the registers read it: each object's place and ancestry,
//! and each entity's stream of writes, under any [`Schema`].
//!
//! See ../../SPEC.md §6.6. [`crate::fold`] folds a stream into its
//! registers; this module only says what descends from what.
//!
//! THE FOLD IS A MONOID ACTION. `fold(nodes) == extend(EMPTY, nodes)` and
//! `extend(extend(s, xs), ys) == extend(s, xs ++ ys)`, so a consumer keeps the
//! state for a tip and applies only the objects since.
//!
//! AND AN ACTION OF THE SET. [`Folded::insert`] puts an object where the
//! linearisation of everything folded and it would: on a linearised suffix
//! that is the end, so it is `extend`, and in any other order that puts
//! parents first it is still `fold` of the union. The state is a function of
//! the objects (§1), whatever order they arrive in, wherever each object
//! rests only on objects of its own scope ([`Folded::local`]).
//!
//! Ancestry is a [`BitSet`] over positions. A legacy object is placed among
//! every legacy object, O(n²) bits for the store; a change's deps never leave
//! its entity, so a change is placed within its `(genesis, key)`, O(Σ kᵢ²).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::event::{Envelope, Hash};
use crate::fold::{read, Product};
use crate::literal::ProdromeError;
use crate::policy::Everything;
use crate::schema::Schema;

/// The state the fold carries, compared by value.
#[derive(Debug, Clone, PartialEq)]
pub struct Folded<E: Schema> {
    index: BTreeMap<Hash, Place<E::Key>>,
    /// The legacy scope's objects in order, each with its entity if it has
    /// an event: a legacy position is an index here.
    legacy: Vec<(Hash, Option<E::Key>)>,
    attestations: BTreeSet<Hash>,
    prodromes: BTreeMap<Genesis, Prodrome<E>>,
    local: bool,
}

/// Where an object is placed: the legacy scope (`None`), or one entity of
/// one prodrome.
type Scope<K> = Option<(Hash, K)>;

/// [`Folded::insert`]'s refusal: where this object goes in the
/// linearisation depends on objects outside its scope, so only a fold of
/// everything can place it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elsewhere;

/// Which prodrome: a genesis object's name, or `None` for the legacy one.
pub type Genesis = Option<Hash>;

/// One prodrome's entities, each its stream of writes in causal order.
pub type Prodrome<E> = BTreeMap<<E as Schema>::Key, Vec<Stamp<E>>>;

/// One object that carries an event, where its entity's registers can see it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp<E: Schema> {
    pub name: Hash,
    pub event: Arc<E>,
    place: Place<E::Key>,
}

impl<E: Schema> Stamp<E> {
    /// Does this write descend from `earlier`, a write to the same entity?
    pub fn descends(&self, earlier: &Stamp<E>) -> bool {
        self.place.ancestry.contains(earlier.place.position)
    }

    /// The write's position in its scope's linear order.
    pub(crate) fn position(&self) -> usize {
        self.place.position
    }

    /// The positions of every object of its scope that the write rests on.
    pub(crate) fn ancestry(&self) -> &BitSet {
        &self.place.ancestry
    }
}

/// A position in an index, and the positions of every ancestor in it.
#[derive(Debug, Clone, PartialEq)]
struct Place<K> {
    position: usize,
    ancestry: BitSet,
    scope: Scope<K>,
}

impl<K> Place<K> {
    /// Make room at `slot`: this place, if at or after it, moves up one.
    fn open(&mut self, slot: usize) {
        if self.position >= slot {
            self.position += 1;
            self.ancestry.open(slot);
        }
    }
}

/// What the fold reads of a stored object: its name, its parents, its event
/// and its PLACE, the prodrome it is in. `genesis` is `None` for a legacy
/// object, and for a change in the legacy prodrome.
///
/// ONLY A [`crate::dag::Dag`] MAKES ONE ([`Dag::nodes`](crate::dag::Dag::nodes)).
/// An object's prodrome is a function of the SET it is in, not of the
/// object: a `Change` naming a legacy root is in the legacy prodrome, with
/// every `Sealed` of its todo, and nothing in the change's bytes says that
/// its genesis is a root rather than a `Genesis`. A node built from one
/// envelope would put that change in a prodrome of its own and split its
/// todo in two, so the fields are private and no constructor is public.
///
/// ```compile_fail
/// use prodrome::event::{Hash, TodoEvent};
/// use prodrome::reference::Todo;
/// use prodrome::registers::Node;
///
/// fn misplace(name: Hash, parents: Vec<Hash>) -> Node<TodoEvent<Todo>> {
///     // A node's place is its DAG's to give.
///     Node { name, parents, event: None, genesis: None }
/// }
/// ```
///
/// ```compile_fail
/// use prodrome::event::{Envelope, Hash, TodoEvent};
/// use prodrome::reference::Todo;
/// use prodrome::registers::Node;
///
/// fn misplace(name: Hash, object: &Envelope<TodoEvent<Todo>>) -> Node<TodoEvent<Todo>> {
///     // No node is made of one envelope, which cannot say its prodrome.
///     Node::of(name, object)
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Node<E> {
    name: Hash,
    parents: Vec<Hash>,
    event: Option<E>,
    genesis: Genesis,
}

impl<E: Clone> Node<E> {
    /// `envelope` under `name`, in the prodrome `genesis`, which the DAG
    /// holding it decides.
    pub(crate) fn placed(name: Hash, envelope: &Envelope<E>, genesis: Genesis) -> Node<E> {
        Node {
            parents: crate::event::parents_of(envelope),
            event: envelope.event().cloned(),
            name,
            genesis,
        }
    }
}

impl<E> Node<E> {
    #[must_use]
    pub fn name(&self) -> &Hash {
        &self.name
    }

    #[must_use]
    pub fn parents(&self) -> &[Hash] {
        &self.parents
    }

    #[must_use]
    pub fn event(&self) -> Option<&E> {
        self.event.as_ref()
    }

    /// The prodrome the object is in: `None` for the legacy one.
    #[must_use]
    pub fn genesis(&self) -> &Genesis {
        &self.genesis
    }

    #[must_use]
    pub fn into_event(self) -> Option<E> {
        self.event
    }
}

impl<E: Schema> Folded<E> {
    pub fn empty() -> Folded<E> {
        Folded {
            index: BTreeMap::new(),
            legacy: Vec::new(),
            attestations: BTreeSet::new(),
            prodromes: BTreeMap::new(),
            local: true,
        }
    }

    pub fn holds(&self, name: &Hash) -> bool {
        self.index.contains_key(name) || self.attestations.contains(name)
    }

    /// Does every object folded rest only on objects of its own scope and
    /// on its genesis? Then each scope's order is decided within it, and
    /// [`Folded::insert`] can place an object without the rest: every
    /// object of a scope rests on its genesis, which rests on nothing, so
    /// the genesis orders nothing within one. True of every store an append
    /// wrote: a change's deps never leave its entity, a legacy object's
    /// parents are legacy.
    pub fn local(&self) -> bool {
        self.local
    }

    /// Is `earlier` among `later`'s ancestors in one index? False for a name
    /// this state has never placed.
    pub fn descends(&self, later: &Hash, earlier: &Hash) -> bool {
        match (self.index.get(later), self.index.get(earlier)) {
            (Some(later), Some(earlier)) => {
                later.scope == earlier.scope && later.ancestry.contains(earlier.position)
            }
            _ => false,
        }
    }

    pub fn prodromes(&self) -> &BTreeMap<Genesis, Prodrome<E>> {
        &self.prodromes
    }

    /// The stream of the entity `key` in the prodrome `genesis`: the empty
    /// stream where nothing is written to the entity, because an entity
    /// that no write names reads as the empty product.
    pub fn stream(&self, genesis: &Genesis, key: &E::Key) -> &[Stamp<E>] {
        self.prodromes
            .get(genesis)
            .and_then(|entities| entities.get(key))
            .map_or(&[], Vec::as_slice)
    }

    /// Every prodrome's entities, for a reader keyed by entity alone.
    pub fn entities(&self) -> impl Iterator<Item = (&E::Key, &[Stamp<E>])> {
        self.prodromes
            .values()
            .flatten()
            .map(|(key, stream)| (key, stream.as_slice()))
    }

    /// Fold `node` in after everything folded: the monoid action's step.
    pub fn push(&mut self, node: &Node<E>) {
        if self.holds(&node.name) {
            return;
        }
        let Some(scope) = scope(node) else {
            self.attestations.insert(node.name.clone());
            return;
        };
        self.local &= self.within(node, &scope);
        let slot = self.size(&scope);
        self.put(node, scope, slot);
    }

    /// Fold `node` in WHERE THE LINEARISATION OF THE UNION PUTS IT: after its
    /// parents, and before the first object of its scope after them whose
    /// name is greater, which is where Kahn's walk, taking the least ready
    /// name, takes it. Everything of its scope after that moves up one.
    ///
    /// So for objects given parents first, in any order, inserting each is
    /// `fold` of them all, and on a linearised suffix it is
    /// [`Folded::push`]. An object already folded changes nothing.
    ///
    /// # Errors
    ///
    /// [`Elsewhere`], changing nothing, where `node` or anything folded
    /// rests on an object of another scope ([`Folded::local`]).
    pub fn insert(&mut self, node: &Node<E>) -> Result<(), Elsewhere> {
        if self.holds(&node.name) {
            return Ok(());
        }
        let Some(scope) = scope(node) else {
            self.attestations.insert(node.name.clone());
            return Ok(());
        };
        if !self.local || !self.within(node, &scope) {
            return Err(Elsewhere);
        }
        let after = node
            .parents
            .iter()
            .filter_map(|parent| self.index.get(parent))
            .map(|parent| parent.position + 1)
            .max()
            .unwrap_or(0);
        let slot = self
            .members(&scope, after)
            .position(|name| *name > node.name)
            .map_or_else(|| self.size(&scope), |later| after + later);
        self.open(&scope, slot);
        self.put(node, scope, slot);
        Ok(())
    }

    /// Does every parent of `node` this state holds lie in `scope`, or is it
    /// the genesis beneath all of it?
    fn within(&self, node: &Node<E>, scope: &Scope<E::Key>) -> bool {
        node.parents
            .iter()
            .all(|parent| match self.index.get(parent) {
                Some(place) => place.scope == *scope,
                None => {
                    !self.attestations.contains(parent)
                        || scope.as_ref().is_some_and(|(genesis, _)| genesis == parent)
                }
            })
    }

    /// The names of `scope`'s objects from position `from`, in order.
    fn members<'s>(
        &'s self,
        scope: &Scope<E::Key>,
        from: usize,
    ) -> Box<dyn Iterator<Item = &'s Hash> + 's> {
        match scope {
            None => Box::new(self.legacy[from..].iter().map(|(name, _)| name)),
            Some((genesis, key)) => Box::new(
                self.prodromes
                    .get(&Some(genesis.clone()))
                    .and_then(|entities| entities.get(key))
                    .map_or(&[][..], |stream| &stream[from..])
                    .iter()
                    .map(|stamp| &stamp.name),
            ),
        }
    }

    /// How many objects `scope` holds.
    fn size(&self, scope: &Scope<E::Key>) -> usize {
        match scope {
            None => self.legacy.len(),
            Some((genesis, key)) => self
                .prodromes
                .get(&Some(genesis.clone()))
                .and_then(|entities| entities.get(key))
                .map_or(0, Vec::len),
        }
    }

    /// Make room at `slot` in `scope`: every place at or after it moves up.
    fn open(&mut self, scope: &Scope<E::Key>, slot: usize) {
        let streams: Vec<&mut Vec<Stamp<E>>> = match scope {
            None => self
                .prodromes
                .get_mut(&None)
                .map_or_else(Vec::new, |entities| entities.values_mut().collect()),
            Some((genesis, key)) => self
                .prodromes
                .get_mut(&Some(genesis.clone()))
                .and_then(|entities| entities.get_mut(key))
                .into_iter()
                .collect(),
        };
        for stream in streams {
            let moved = stream.partition_point(|stamp| stamp.place.position < slot);
            for stamp in &mut stream[moved..] {
                stamp.place.open(slot);
            }
        }
        let moved: Vec<Hash> = self.members(scope, slot).cloned().collect();
        for name in &moved {
            if let Some(place) = self.index.get_mut(name) {
                place.open(slot);
            }
        }
    }

    /// Place `node` at `slot` of `scope`, which is free.
    fn put(&mut self, node: &Node<E>, scope: Scope<E::Key>, slot: usize) {
        let mut ancestry = BitSet::default();
        for parent in node.parents.iter().filter_map(|p| self.index.get(p)) {
            if parent.scope == scope {
                ancestry.insert(parent.position);
                ancestry.union_with(&parent.ancestry);
            }
        }
        let place = Place {
            position: slot,
            ancestry,
            scope,
        };
        if place.scope.is_none() {
            let key = node.event.as_ref().map(|event| event.key().clone());
            self.legacy.insert(slot, (node.name.clone(), key));
        }
        self.index.insert(node.name.clone(), place.clone());
        if let Some(event) = &node.event {
            let stream = self
                .prodromes
                .entry(node.genesis.clone())
                .or_default()
                .entry(event.key().clone())
                .or_default();
            let at = stream.partition_point(|stamp| stamp.place.position < slot);
            stream.insert(
                at,
                Stamp {
                    name: node.name.clone(),
                    event: Arc::new(event.clone()),
                    place,
                },
            );
        }
    }
}

/// Where `node` is placed: `None` for an attestation, which has no place.
fn scope<E: Schema>(node: &Node<E>) -> Option<Scope<E::Key>> {
    match (&node.genesis, &node.event) {
        (None, _) => Some(None),
        (Some(genesis), Some(event)) => Some(Some((genesis.clone(), event.key().clone()))),
        (Some(_), None) => None,
    }
}

/// Apply `nodes` (parents first) to `state`. Structure is folded for every
/// object; standing and time are the readers' business.
pub fn extend<E: Schema>(state: &Folded<E>, nodes: &[Node<E>]) -> Folded<E> {
    let mut next = state.clone();
    for node in nodes {
        next.push(node);
    }
    next
}

pub fn fold<E: Schema>(nodes: &[Node<E>]) -> Folded<E> {
    extend(&Folded::empty(), nodes)
}

/// The nodes `state` has not folded yet, in their given order.
pub fn since<'a, E: Schema>(state: &Folded<E>, nodes: &'a [Node<E>]) -> Vec<&'a Node<E>> {
    nodes
        .iter()
        .filter(|node| !state.holds(&node.name))
        .collect()
}

/// A change's deps: its entity's HEADS, the writes to it, in any register,
/// that no other write to it descends from, read structurally — everything
/// binds, at no moment. What the writer saw of the entity, as a commit's
/// parents are what its author had: each written register's frontier is
/// beneath them, and so is every other write the writer held.
///
/// # Errors
///
/// The append's refusal, before any object exists: `event` does not stand
/// on the reading it supersedes ([`Product::grows`]). For the first write
/// to an entity, that reading is the empty product's, so the append asks
/// the same question of every write that `verify` asks.
pub fn deps_for<E: Schema>(
    state: &Folded<E>,
    genesis: &Genesis,
    event: &E,
) -> Result<Vec<Hash>, ProdromeError> {
    let stream = state.stream(genesis, event.key());
    read(stream, None, &Everything).grows(event)?;
    Ok(heads(stream))
}

/// The writes of one entity's stream that no other write of it descends
/// from, by name.
pub fn heads<E: Schema>(stream: &[Stamp<E>]) -> Vec<Hash> {
    let mut beneath = BitSet::default();
    for stamp in stream {
        beneath.union_with(&stamp.place.ancestry);
    }
    let mut heads: Vec<Hash> = stream
        .iter()
        .filter(|stamp| !beneath.contains(stamp.place.position))
        .map(|stamp| stamp.name.clone())
        .collect();
    heads.sort();
    heads
}

/// A set of small non-negative integers as a bitmap: set, test, union.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitSet(Vec<u64>);

impl BitSet {
    pub fn contains(&self, bit: usize) -> bool {
        self.0
            .get(bit / 64)
            .is_some_and(|word| word >> (bit % 64) & 1 == 1)
    }

    pub fn insert(&mut self, bit: usize) {
        let word = bit / 64;
        if self.0.len() <= word {
            self.0.resize(word + 1, 0);
        }
        self.0[word] |= 1 << (bit % 64);
    }

    /// Insert a clear bit at `at`: every bit at or above it moves up one.
    pub fn open(&mut self, at: usize) {
        let bits: Vec<usize> = self.iter().collect();
        *self = BitSet::default();
        for bit in bits {
            self.insert(if bit >= at { bit + 1 } else { bit });
        }
    }

    /// How many positions this set and `mask` both hold.
    pub fn count_within(&self, mask: &BitSet) -> usize {
        self.0
            .iter()
            .zip(&mask.0)
            .map(|(mine, theirs)| (mine & theirs).count_ones() as usize)
            .sum()
    }

    pub fn union_with(&mut self, other: &BitSet) {
        if self.0.len() < other.0.len() {
            self.0.resize(other.0.len(), 0);
        }
        for (mine, theirs) in self.0.iter_mut().zip(&other.0) {
            *mine |= theirs;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().enumerate().flat_map(|(index, word)| {
            (0..64).filter_map(move |bit| (word >> bit & 1 == 1).then_some(index * 64 + bit))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bitset_is_a_set_of_positions() {
        let mut bits = BitSet::default();
        assert!(!bits.contains(0));
        bits.insert(0);
        bits.insert(130);
        assert!(bits.contains(0) && bits.contains(130));
        assert!(!bits.contains(1) && !bits.contains(129) && !bits.contains(999));
        assert_eq!(bits.iter().collect::<Vec<_>>(), vec![0, 130]);

        let mut other = BitSet::default();
        other.insert(64);
        other.union_with(&bits);
        assert_eq!(other.iter().collect::<Vec<_>>(), vec![0, 64, 130]);
        let mut shorter = BitSet::default();
        shorter.insert(1);
        other.union_with(&shorter);
        assert_eq!(other.iter().collect::<Vec<_>>(), vec![0, 1, 64, 130]);
        other.open(64);
        assert_eq!(other.iter().collect::<Vec<_>>(), vec![0, 1, 65, 131]);
    }
}
