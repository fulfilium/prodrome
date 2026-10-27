//! The tag set of one membership prodrome and one alias prodrome read
//! together. It is a function of the two prodromes' objects alone, so
//! every replica that holds the same objects reads the same tags. Where
//! concurrent renames disagree, it still chooses one tag for every name by
//! a rule that every replica applies alike, and lists each
//! [`Disagreement`].

use std::collections::{BTreeMap, BTreeSet};

use crate::event::Actor;
use crate::fold::sole::{Bodies, One, Only};
use crate::fold::Product as _;
use crate::literal::Datetime;
use crate::policy::Everything;
use crate::registers::{Prodrome, Stamp};
use crate::tags::alias::{self, Alias, AliasEvent};
use crate::tags::membership::{self, Membership, MembershipEvent, Presence};
use crate::tags::Tagging;

/// The tags on each target, and what each tag's name stands for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagSet<S: Tagging> {
    on: BTreeMap<S::Target, BTreeSet<S::Tag>>,
    aliases: BTreeMap<S::Tag, Candidates<S>>,
    spelled: BTreeMap<S::Tag, S::Tag>,
    rings: BTreeMap<S::Tag, BTreeSet<S::Tag>>,
}

impl<S: Tagging> Default for TagSet<S> {
    fn default() -> Self {
        Self {
            on: BTreeMap::new(),
            aliases: BTreeMap::new(),
            spelled: BTreeMap::new(),
            rings: BTreeMap::new(),
        }
    }
}

/// A place where concurrent renames disagree. The set still reads one tag
/// for every name, and lists the disagreement so that a person can settle
/// it with one rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disagreement<S: Tagging> {
    /// A tag's register holds several aliases. The set follows `chosen`,
    /// the alias of the write with the least object name.
    Aliases {
        /// The tag whose aliases disagree.
        tag: S::Tag,
        /// The alias that the set follows.
        chosen: Alias<S>,
        /// The other aliases, in the order of their least writes' names.
        others: Vec<Alias<S>>,
    },
    /// The chosen aliases form a ring. Every name in it, and every name
    /// whose chain enters it, reads as `spelled`, the least name of the
    /// ring.
    Ring {
        /// The names of the ring.
        names: BTreeSet<S::Tag>,
        /// The name that the ring reads as.
        spelled: S::Tag,
    },
}

/// The aliases that a tag's register reads, the chosen one first.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidates<S: Tagging> {
    chosen: Alias<S>,
    others: Vec<Alias<S>>,
}

impl<S: Tagging> TagSet<S> {
    /// The set that `memberships` and `aliases` hold, each one prodrome of
    /// its store, read under the policy that binds every writer. A store
    /// begun by one genesis holds one prodrome; an empty map reads as no
    /// tags or no aliases.
    #[must_use]
    pub fn of(
        memberships: &Prodrome<MembershipEvent<S>>,
        aliases: &Prodrome<AliasEvent<S>>,
    ) -> Self {
        let mut on: BTreeMap<S::Target, BTreeSet<S::Tag>> = BTreeMap::new();
        for (pair, stream) in memberships {
            let registers: One<'_, Bodies<Membership<S>>> =
                crate::fold::read(stream, None, &Everything);
            if membership::presence(&registers) == Presence::On {
                on.entry(pair.target.clone())
                    .or_default()
                    .insert(pair.tag.clone());
            }
        }
        let aliases: BTreeMap<S::Tag, Candidates<S>> = aliases
            .iter()
            .filter_map(|(tag, stream)| {
                let registers: One<'_, Bodies<Alias<S>>> =
                    crate::fold::read(stream, None, &Everything);
                let mut stamps: Vec<&Stamp<AliasEvent<S>>> = registers.reading(Only);
                stamps.sort_by(|one, two| one.name.cmp(&two.name));
                let mut values = stamps.into_iter().map(|stamp| stamp.event.body().clone());
                let chosen = values.next()?;
                Some((
                    tag.clone(),
                    Candidates {
                        chosen,
                        others: values.collect(),
                    },
                ))
            })
            .collect();
        let known: BTreeSet<&S::Tag> = on
            .values()
            .flatten()
            .chain(aliases.keys())
            .chain(
                aliases
                    .values()
                    .filter_map(|candidates| match &candidates.chosen {
                        Alias::As(to) => Some(to),
                        Alias::Itself => None,
                    }),
            )
            .collect();
        let mut spelled = BTreeMap::new();
        let mut rings = BTreeMap::new();
        for name in known {
            let (tag, ring) = follow(&aliases, name);
            if let Some(ring) = ring {
                rings.insert(tag.clone(), ring);
            }
            spelled.insert(name.clone(), tag);
        }
        Self {
            on,
            aliases,
            spelled,
            rings,
        }
    }

    /// The tag that `name` reads as: the end of its chain of chosen
    /// aliases, or the least name of the ring that the chain enters.
    #[must_use]
    pub fn spelling(&self, name: &S::Tag) -> S::Tag {
        self.spelled.get(name).unwrap_or(name).clone()
    }

    /// Every name that reads as the tag that `name` reads as, that tag
    /// included.
    #[must_use]
    pub fn names(&self, name: &S::Tag) -> BTreeSet<S::Tag> {
        let tag = self.spelling(name);
        self.spelled
            .iter()
            .filter(|(_, spelled)| **spelled == tag)
            .map(|(name, _)| name.clone())
            .chain([tag.clone()])
            .collect()
    }

    /// The tags on `target`, each in its current spelling.
    #[must_use]
    pub fn tags(&self, target: &S::Target) -> BTreeSet<S::Tag> {
        self.on
            .get(target)
            .into_iter()
            .flatten()
            .map(|name| self.spelling(name))
            .collect()
    }

    /// Every target that carries a tag, with its tags in their current
    /// spellings.
    #[must_use]
    pub fn memberships(&self) -> BTreeMap<S::Target, BTreeSet<S::Tag>> {
        self.on
            .keys()
            .map(|target| (target.clone(), self.tags(target)))
            .filter(|(_, tags)| !tags.is_empty())
            .collect()
    }

    /// Whether `target` carries every tag of `chosen`, each read in its
    /// current spelling. An empty `chosen` holds for every target.
    #[must_use]
    pub fn shows(&self, chosen: &BTreeSet<S::Tag>, target: &S::Target) -> bool {
        let on = self.tags(target);
        chosen.iter().all(|tag| on.contains(&self.spelling(tag)))
    }

    /// Every disagreement: the tags whose aliases disagree, then the rings.
    #[must_use]
    pub fn disagreements(&self) -> Vec<Disagreement<S>> {
        let aliases = self
            .aliases
            .iter()
            .filter(|(_, candidates)| !candidates.others.is_empty())
            .map(|(tag, candidates)| Disagreement::Aliases {
                tag: tag.clone(),
                chosen: candidates.chosen.clone(),
                others: candidates.others.clone(),
            });
        let rings = self
            .rings
            .iter()
            .map(|(spelled, names)| Disagreement::Ring {
                names: names.clone(),
                spelled: spelled.clone(),
            });
        aliases.chain(rings).collect()
    }

    /// The event that puts `tag` on `target` in its current spelling, or
    /// `None` where the target already carries it.
    ///
    /// An event's name is its key, `at`, `actor` and body. So when one
    /// actor adds a tag, takes it off and adds it again with one `at`, the
    /// second add is the first add's event again, and the store writes
    /// nothing for it.
    #[must_use]
    pub fn add(
        &self,
        target: &S::Target,
        tag: &S::Tag,
        at: Datetime,
        actor: &Actor,
    ) -> Option<MembershipEvent<S>> {
        let spelled = self.spelling(tag);
        (!self.tags(target).contains(&spelled))
            .then(|| membership::added(target.clone(), spelled, at, actor.clone()))
    }

    /// The events that take off every name of `tag`'s tag that this set
    /// holds on `target`. They take off no add that the set does not hold.
    #[must_use]
    pub fn remove(
        &self,
        target: &S::Target,
        tag: &S::Tag,
        at: Datetime,
        actor: &Actor,
    ) -> Vec<MembershipEvent<S>> {
        let spelled = self.spelling(tag);
        self.on
            .get(target)
            .into_iter()
            .flatten()
            .filter(|name| self.spelling(name) == spelled)
            .map(|name| membership::removed(target.clone(), name.clone(), at, actor.clone()))
            .collect()
    }

    /// The events after which every name of `from`'s tag and of `to`'s tag
    /// stands for `to`. A rename to a name of another tag merges the two
    /// tags, and renaming back undoes a rename. Each event supersedes what
    /// this set holds for its name, so it settles the disagreements on
    /// those names. A rename that changes nothing writes nothing.
    #[must_use]
    pub fn rename(
        &self,
        from: &S::Tag,
        to: &S::Tag,
        at: Datetime,
        actor: &Actor,
    ) -> Vec<AliasEvent<S>> {
        // `unaliased` refuses nothing.
        let itself = (!self.settled(to, &Alias::Itself))
            .then(|| alias::unaliased(to.clone(), at, actor.clone()).ok())
            .flatten();
        let pointed = Alias::As(to.clone());
        let mut names = self.names(from);
        names.extend(self.names(to));
        let aliased = names
            .into_iter()
            .filter(|name| !self.settled(name, &pointed))
            // `aliased` refuses `to` itself, the one name that is no alias
            // afterwards.
            .filter_map(|name| alias::aliased(name, to.clone(), at, actor.clone()).ok());
        itself.into_iter().chain(aliased).collect()
    }

    /// Whether `name`'s register reads `alias` alone. A register that no
    /// event wrote reads `Itself`.
    fn settled(&self, name: &S::Tag, alias: &Alias<S>) -> bool {
        match self.aliases.get(name) {
            None => *alias == Alias::Itself,
            Some(candidates) => candidates.chosen == *alias && candidates.others.is_empty(),
        }
    }
}

/// The tag that `name` reads as: the end of its chain of chosen aliases, or
/// the least name of the ring that the chain enters, with that ring.
fn follow<S: Tagging>(
    aliases: &BTreeMap<S::Tag, Candidates<S>>,
    name: &S::Tag,
) -> (S::Tag, Option<BTreeSet<S::Tag>>) {
    let mut path: Vec<&S::Tag> = Vec::new();
    let mut at = name;
    loop {
        if let Some(start) = path.iter().position(|seen| *seen == at) {
            let ring = &path[start..];
            let least = ring.iter().fold(at, |least, name| least.min(*name));
            return (
                least.clone(),
                Some(ring.iter().map(|name| (*name).clone()).collect()),
            );
        }
        path.push(at);
        match aliases.get(at).map(|candidates| &candidates.chosen) {
            Some(Alias::As(to)) => at = to,
            Some(Alias::Itself) | None => return (at.clone(), None),
        }
    }
}
