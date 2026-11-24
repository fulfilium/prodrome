//! The objects of a DAG's history, and the facts that a DAG derives from
//! them: its tips, its linear order, and the order's index.

use std::collections::{BTreeMap, BTreeSet};

use super::facts::{linear, Facts, Graph};
use super::index::Index;
use prodrome_data::event::{rests_on, Envelope, Hash};
use prodrome_data::literal::ProdromeError;

/// The objects of a history, with the facts derived from them ([`Facts`]),
/// each kept once a reader asks for it.
///
/// A program reads the objects through [`History::objects`] and changes
/// them only through [`History::edit`]. `edit` replaces the facts whole
/// before it gives out the objects, so a kept fact always describes the
/// objects that are held. Equality compares the objects alone.
///
/// The facts read the objects through [`History::graph`], the one part
/// that depends on the schema, so their code is compiled once and not once
/// for each schema. The kept order costs one [`Hash`] for each object
/// (about 120 bytes), and its index about 16 bytes for each object and 8
/// for each parent edge: about 4.3 MB for 30,000 objects.
#[derive(Debug, Clone)]
pub(super) struct History<E> {
    objects: BTreeMap<Hash, Envelope<E>>,
    facts: Facts,
}

impl<E> PartialEq for History<E>
where
    Envelope<E>: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.objects == other.objects
    }
}

impl<E> History<E> {
    pub(super) fn new(objects: BTreeMap<Hash, Envelope<E>>) -> Self {
        History {
            objects,
            facts: Facts::default(),
        }
    }

    pub(super) fn objects(&self) -> &BTreeMap<Hash, Envelope<E>> {
        &self.objects
    }

    pub(super) fn into_objects(self) -> BTreeMap<Hash, Envelope<E>> {
        self.objects
    }

    /// The objects, to change. The kept facts are let go first.
    pub(super) fn edit(&mut self) -> &mut BTreeMap<Hash, Envelope<E>> {
        self.facts = Facts::default();
        &mut self.objects
    }

    /// The object `name` replaced by `object`, and the object it replaced;
    /// `None`, and nothing replaced, for a name not held. The kept facts
    /// read only the names and what each rests on, so they are kept where
    /// `object` rests on what the held object rests on, and let go
    /// otherwise.
    pub(super) fn replace(&mut self, name: &Hash, object: Envelope<E>) -> Option<Envelope<E>> {
        if rests_on(self.objects.get(name)?) != rests_on(&object) {
            self.facts = Facts::default();
        }
        let held = self.objects.get_mut(name)?;
        Some(std::mem::replace(held, object))
    }

    /// Each object's name and the names it rests on, in name order.
    fn graph(&self) -> Graph<'_> {
        self.objects
            .iter()
            .map(|(name, object)| (name, rests_on(object)))
            .collect()
    }

    /// Every object no object rests on.
    pub(super) fn tips(&self) -> &BTreeSet<Hash> {
        self.facts.tips(&|| self.graph())
    }

    /// Every object, parents first. A parent that is not held is a
    /// refusal, and so is a cycle.
    pub(super) fn order(&self) -> Result<&[Hash], ProdromeError> {
        self.indexed().map(|(order, _)| order)
    }

    /// The order, and its [`Index`]: each object's parents as positions in
    /// the order.
    pub(super) fn indexed(&self) -> Result<(&[Hash], &Index), ProdromeError> {
        self.facts.ordered(&|| self.graph())
    }

    /// `among`, parents first, by Kahn's walk over them alone: a parent
    /// outside them is passed over. A cycle is a refusal.
    pub(super) fn order_among(&self, among: &BTreeSet<&Hash>) -> Result<Vec<Hash>, ProdromeError> {
        let rests_on: BTreeMap<&Hash, Vec<&Hash>> = among
            .iter()
            .filter_map(|name| self.objects.get_key_value(*name))
            .map(|(name, object)| {
                let parents = rests_on(object)
                    .into_iter()
                    .filter_map(|parent| among.get(parent).copied())
                    .collect();
                (name, parents)
            })
            .collect();
        linear(&rests_on)
    }
}
