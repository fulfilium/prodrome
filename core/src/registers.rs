//! Each object's place and ancestry in the DAG, and each entity's stream of
//! writes, under any [`Schema`] (SPEC §6.6).
//!
//! The fold is a monoid action: `fold(nodes) == extend(EMPTY, nodes)` and
//! `extend(extend(s, xs), ys) == extend(s, xs ++ ys)`, so a consumer can keep
//! the state for a tip and apply only the newer objects.
//!
//! Ancestry is a [`BitSet`] over positions. A legacy object is placed among
//! every legacy object, which costs O(n²) bits for the store. A change's deps
//! never leave its entity, so a change is placed within its `(genesis, key)`,
//! which costs O(Σ kᵢ²) bits.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub use prodrome_data::stamp::Stamp;

use crate::event::{Envelope, Hash};
use crate::fold::{read, Product};
use crate::literal::ProdromeError;
use crate::policy::Everything;
use crate::schema::Schema;
use prodrome_data::bitset::BitSet;
use prodrome_data::stamp::{Place, Scope};

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

/// The refusal of [`Folded::insert`]: the place of the object in the
/// linearisation depends on objects outside its scope, so only a fold of
/// everything can place it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elsewhere;

/// Which prodrome: a genesis object's name, or `None` for the legacy one.
pub type Genesis = Option<Hash>;

/// One prodrome's entities, each with its stream of writes in causal order.
pub type Prodrome<E> = BTreeMap<<E as Schema>::Key, Vec<Stamp<E>>>;

/// What the fold reads of a stored object: its name, its parents, its event
/// and the prodrome that it is in.
///
/// Only [`crate::dag::Dag::nodes`] makes a node. The prodrome of an object
/// depends on the set of objects that it is in: a `Change` that names a
/// legacy root is in the legacy prodrome, and nothing in the change's bytes
/// tells whether its genesis is a root or a `Genesis`. A node built from one
/// envelope would put that change in a prodrome of its own and split its todo
/// in two, so the fields are private and no constructor is public.
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

    /// The stamp of `name` reads `event` in place of its event: the husk of
    /// the object it was (SPEC §3), whose key is the same, so it stays in
    /// its entity's stream where it was. A name with no stamp is passed
    /// over.
    pub(crate) fn rewrite(&mut self, name: &Hash, event: &E) {
        for prodrome in self.prodromes.values_mut() {
            if let Some(stamp) = prodrome
                .get_mut(event.key())
                .and_then(|stream| stream.iter_mut().find(|stamp| stamp.name == *name))
            {
                stamp.event = Arc::new(event.clone());
                return;
            }
        }
    }

    /// Whether every object folded rests only on objects of its own scope and
    /// on its genesis.
    ///
    /// Where it does, each scope's order is decided within the scope, and
    /// [`Folded::insert`] can place an object without the rest. The genesis
    /// orders nothing within a scope, because every object of the scope rests
    /// on it and it rests on nothing. Every store
    /// that appends wrote is local: a change's deps never leave its entity,
    /// and a legacy object's parents are legacy.
    pub fn local(&self) -> bool {
        self.local
    }

    /// Whether `earlier` is among the ancestors of `later` in one index.
    /// `false` for a name that this state has never placed.
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

    /// The stream of the entity `key` in the prodrome `genesis`, empty where
    /// no write names the entity.
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

    /// Folds `node` in after everything folded, as one step of the monoid
    /// action.
    ///
    /// The caller must give the nodes parents first.
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

    /// Folds `node` in where the linearisation of the union puts it: after
    /// its parents, and before the first later object of its scope whose
    /// name is greater, where Kahn's walk takes it.
    ///
    /// For objects given parents first, in any order, inserting each gives
    /// the `fold` of them all, so the state depends only on the set of
    /// objects. An object already folded changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`Elsewhere`], and changes nothing, where `node` or anything
    /// folded rests on an object of another scope ([`Folded::local`]).
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
            let moved = stream.partition_point(|stamp| stamp.position() < slot);
            for stamp in &mut stream[moved..] {
                stamp.open(slot);
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
            let at = stream.partition_point(|stamp| stamp.position() < slot);
            stream.insert(
                at,
                Stamp::placed(node.name.clone(), Arc::new(event.clone()), place),
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

/// Applies `nodes`, parents first, to `state`.
///
/// The function folds structure for every object. The readers apply standing
/// and time.
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

/// The deps of a change: the heads of its entity, which are the writes to the
/// entity, in any register, that no other write to it descends from.
///
/// The function reads the heads under the policy that binds everything and
/// at no moment, so the deps cover every write to the entity that the writer
/// holds.
///
/// # Errors
///
/// Returns the refusal of the append, before any object exists, where
/// `event` does not stand on the reading that it supersedes
/// ([`Product::grows`]). For the first write to an entity, that reading is
/// the empty product's, so the append asks every write the same question
/// that `verify` asks.
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
        beneath.union_with(stamp.ancestry());
    }
    let mut heads: Vec<Hash> = stream
        .iter()
        .filter(|stamp| !beneath.contains(stamp.position()))
        .map(|stamp| stamp.name.clone())
        .collect();
    heads.sort();
    heads
}
