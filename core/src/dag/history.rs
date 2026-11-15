//! The objects of a DAG's history, and the facts that a DAG derives from
//! them: its tips and its linear order.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use crate::event::{rests_on, Envelope, Hash};
use crate::literal::ProdromeError;
use crate::topo;

/// The objects of a history, with each derived fact kept once a reader
/// asks for it. A fact is a function of the objects, so a [`History`]
/// derives it on the first ask and answers later asks from the kept value.
///
/// A program reads the objects through [`History::objects`] and changes
/// them only through [`History::edit`]. `edit` lets go of every kept fact
/// before it gives out the objects, so a kept fact always describes the
/// objects that are held. Equality compares the objects alone.
///
/// The kept order costs one [`Hash`] for each object: 32 bytes of words, a
/// `String` of 24 bytes, and its 64 bytes of text, about 120 bytes for each
/// object, or about 3.6 MB for 30,000 objects.
#[derive(Debug, Clone)]
pub(super) struct History<E> {
    objects: BTreeMap<Hash, Envelope<E>>,
    tips: OnceLock<BTreeSet<Hash>>,
    order: OnceLock<Result<Vec<Hash>, ProdromeError>>,
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
            tips: OnceLock::new(),
            order: OnceLock::new(),
        }
    }

    pub(super) fn objects(&self) -> &BTreeMap<Hash, Envelope<E>> {
        &self.objects
    }

    pub(super) fn into_objects(self) -> BTreeMap<Hash, Envelope<E>> {
        self.objects
    }

    /// The objects, to change. Every kept fact is let go first.
    pub(super) fn edit(&mut self) -> &mut BTreeMap<Hash, Envelope<E>> {
        self.tips = OnceLock::new();
        self.order = OnceLock::new();
        &mut self.objects
    }

    /// Every object no object rests on.
    pub(super) fn tips(&self) -> &BTreeSet<Hash> {
        self.tips.get_or_init(|| {
            super::tips_among(
                self.objects
                    .iter()
                    .map(|(name, object)| (name, rests_on(object))),
            )
        })
    }

    /// Every object, parents first, by [`topo::linear`]. A parent that is
    /// not held is a refusal, and so is a cycle.
    pub(super) fn order(&self) -> Result<&[Hash], ProdromeError> {
        self.order
            .get_or_init(|| {
                for (name, object) in &self.objects {
                    if let Some(parent) = rests_on(object)
                        .into_iter()
                        .find(|parent| !self.objects.contains_key(*parent))
                    {
                        return Err(ProdromeError::Store(format!(
                            "missing object {}, named as a parent by {}",
                            parent.as_str(),
                            name.as_str()
                        )));
                    }
                }
                self.order_among(&self.objects.keys().collect())
            })
            .as_deref()
            .map_err(Clone::clone)
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
        match topo::linear(&rests_on) {
            Ok(order) => Ok(order.into_iter().cloned().collect()),
            Err(stuck) => Err(ProdromeError::Store(format!(
                "cycle in the object graph at {}",
                stuck.as_str()
            ))),
        }
    }
}
