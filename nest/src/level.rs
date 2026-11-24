//! A nest of content-addressed histories, read from the replicas of each
//! level's schema, and its join (design §6.3).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::heads::Nests;
use super::history::History;
use super::path::{Path, Segment};
use prodrome_dag::dag::Dag;
use prodrome_data::event::{parents_of, Envelope, Hash};
use prodrome_data::literal::ProdromeError;
use prodrome_data::schema::Schema;

/// A tree of content-addressed histories: one level's history, each object
/// at its entity's key, and the nest that each of its pointers holds, at
/// the pointer's path.
///
/// A pointer's history is everything that any write to it in this level's
/// history named, so [`Nest::flatten`] is a function of the object sets
/// alone. The pointer's current value is its register's reading
/// ([`super::pointer`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nest {
    name: Hash,
    own: History<Hash>,
    /// Each own object's parents, all of them own.
    parents: BTreeMap<Hash, Vec<Hash>>,
    /// Each own object that writes a pointer: at what path, naming what.
    points: BTreeMap<Hash, Vec<(Path, BTreeSet<Hash>)>>,
    held: BTreeMap<Path, Nest>,
}

/// One level of a nest: a replica's objects at one schema, and how its
/// pointers are read.
pub trait Level {
    /// The nest of the history that `heads` rest on, at this level.
    ///
    /// # Errors
    ///
    /// A head, or a parent of an object of the history, that this level
    /// does not hold: an object rests only on objects of its own level, so
    /// a name that the level lacks is missing. A key that is not a path
    /// segment.
    fn nest(&self, heads: &BTreeSet<Hash>) -> Result<Nest, ProdromeError>;
}

/// A level whose schema holds no history: a nest's leaves.
#[derive(Debug, Clone)]
pub struct Leaf<E: Schema> {
    dag: Arc<Dag<E>>,
}

impl<E: Schema> Leaf<E> {
    /// The level of `dag`'s objects, such as the DAG that a replica of
    /// `prodrome-store` holds.
    #[must_use]
    pub fn new(dag: Arc<Dag<E>>) -> Leaf<E> {
        Leaf { dag }
    }
}

impl<E: Schema<Key: AsRef<str>>> Level for Leaf<E> {
    fn nest(&self, heads: &BTreeSet<Hash>) -> Result<Nest, ProdromeError> {
        Nest::of(&self.dag, heads, |_| Vec::new(), &BTreeMap::new())
    }
}

/// A level whose schema holds histories ([`Nests`]): its objects, and the
/// level of each held register's inner schema.
pub struct Holding<'l, E: Nests> {
    dag: Arc<Dag<E>>,
    inner: BTreeMap<E::Register, &'l dyn Level>,
}

impl<'l, E: Nests> Holding<'l, E> {
    /// The level of `dag`'s objects, holding nothing yet.
    #[must_use]
    pub fn new(dag: Arc<Dag<E>>) -> Holding<'l, E> {
        Holding {
            dag,
            inner: BTreeMap::new(),
        }
    }

    /// This level, reading the histories `register` holds from `level`, a
    /// level of the register's inner schema.
    #[must_use]
    pub fn holds(mut self, register: E::Register, level: &'l dyn Level) -> Holding<'l, E> {
        self.inner.insert(register, level);
        self
    }
}

impl<E: Nests> Level for Holding<'_, E> {
    fn nest(&self, heads: &BTreeSet<Hash>) -> Result<Nest, ProdromeError> {
        let mut levels = BTreeMap::new();
        let pointers = |event: &E| {
            let mut found = Vec::new();
            for &register in E::HELD {
                if let Some(names) = event.heads(register) {
                    found.push((E::segment(register), names.clone()));
                }
            }
            found
        };
        for &register in E::HELD {
            let level = self.inner.get(&register).ok_or_else(|| {
                ProdromeError::invalid(format!(
                    "no level is given for the held register {}",
                    E::segment(register)
                ))
            })?;
            levels.insert(E::segment(register), *level);
        }
        Nest::of(&self.dag, heads, pointers, &levels)
    }
}

/// A set of prodromes: a level whose nest holds each prodrome of a replica,
/// as its member level reads it, at its genesis (design §6.3.1, law 41).
///
/// The set has no objects of its own, so [`Nest::flatten`] puts each
/// member's objects at `genesis/key`. No edge crosses geneses (SPEC §3), so
/// no member's nest reaches another's objects. A genesis is a global name,
/// so two sets join by union, and a prodrome in both is one member.
pub struct Set<'l, E: Schema> {
    dag: Arc<Dag<E>>,
    member: &'l dyn Level,
}

impl<'l, E: Schema> Set<'l, E> {
    /// The set of `dag`'s prodromes, each read by `member`, a level of the
    /// same objects.
    #[must_use]
    pub fn new(dag: Arc<Dag<E>>, member: &'l dyn Level) -> Set<'l, E> {
        Set { dag, member }
    }
}

impl<E: Schema> Level for Set<'_, E> {
    fn nest(&self, heads: &BTreeSet<Hash>) -> Result<Nest, ProdromeError> {
        let mut members: BTreeMap<Hash, BTreeSet<Hash>> = BTreeMap::new();
        for head in heads {
            let genesis = self.dag.genesis_of(head).ok_or_else(|| {
                ProdromeError::Store(format!(
                    "missing object {}, named in a set's heads",
                    head.as_str()
                ))
            })?;
            members.entry(genesis).or_default().insert(head.clone());
        }
        let mut held = BTreeMap::new();
        for (genesis, heads) in members {
            held.insert(
                Segment::new(genesis.as_str())?.into(),
                self.member.nest(&heads)?,
            );
        }
        Ok(Nest::holding(held))
    }
}

/// An object's key at its level: its entity's, or the root for one with no
/// event (a genesis, a snapshot).
fn key<E: Schema<Key: AsRef<str>>>(object: &Envelope<E>) -> Result<Path, ProdromeError> {
    match object.event() {
        Some(event) => Ok(Segment::new(event.key().as_ref())?.into()),
        None => Ok(Path::root()),
    }
}

impl Nest {
    /// The nest of what `heads` rest on in `dag`, reading each object's
    /// pointers by `pointers` and the history each holds from the level
    /// its register's segment names.
    fn of<E: Schema<Key: AsRef<str>>>(
        dag: &Dag<E>,
        heads: &BTreeSet<Hash>,
        pointers: impl Fn(&E) -> Vec<(&'static str, BTreeSet<Hash>)>,
        levels: &BTreeMap<&'static str, &dyn Level>,
    ) -> Result<Nest, ProdromeError> {
        let closure = dag.closure(heads.iter().cloned());
        let mut own = Vec::new();
        let mut parents = BTreeMap::new();
        let mut points: BTreeMap<Hash, Vec<(Path, BTreeSet<Hash>)>> = BTreeMap::new();
        let mut named: BTreeMap<Path, (&dyn Level, BTreeSet<Hash>)> = BTreeMap::new();
        for name in closure {
            let object = dag.get(&name).ok_or_else(|| {
                ProdromeError::Store(format!(
                    "missing object {}, named in a nest's level",
                    name.as_str()
                ))
            })?;
            let at = key(object)?;
            for (segment, heads) in object.event().map(&pointers).unwrap_or_default() {
                let path = at.then(&Segment::new(segment)?.into());
                let level = levels.get(segment).ok_or_else(|| {
                    ProdromeError::invalid(format!("no level is given for {segment}"))
                })?;
                named
                    .entry(path.clone())
                    .or_insert_with(|| (*level, BTreeSet::new()))
                    .1
                    .extend(heads.iter().cloned());
                points.entry(name.clone()).or_default().push((path, heads));
            }
            parents.insert(name.clone(), parents_of(object));
            own.push((at, name));
        }
        let mut held = BTreeMap::new();
        for (path, (level, heads)) in named {
            held.insert(path, level.nest(&heads)?);
        }
        let own: History<Hash> = own.into_iter().collect();
        Ok(Nest {
            name: Nest::name_of(&own, &held),
            own,
            parents,
            points,
            held,
        })
    }

    /// A nest with no objects of its own, holding `held`.
    fn holding(held: BTreeMap<Path, Nest>) -> Nest {
        let own = History::default();
        Nest {
            name: Nest::name_of(&own, &held),
            own,
            parents: BTreeMap::new(),
            points: BTreeMap::new(),
            held,
        }
    }

    /// [`crate::memo::name`] over this level's objects, each at its key, and
    /// the paths of what it holds, with the held nests' names as children.
    fn name_of(own: &History<Hash>, held: &BTreeMap<Path, Nest>) -> Hash {
        let mut bytes = Vec::new();
        for (path, name) in own.iter() {
            let path = path.to_string();
            bytes.extend(format!("{} {path} {}\n", path.len(), name.as_str()).bytes());
        }
        for path in held.keys() {
            let path = path.to_string();
            bytes.extend(format!("held {} {path}\n", path.len()).bytes());
        }
        prodrome_memo::name(&bytes, held.values().map(|nest| &nest.name))
    }

    /// The nest's content name, over its own objects, the paths that it
    /// holds and the names of the nests held there, so that it changes
    /// exactly when something in the tree below it changes.
    #[must_use]
    pub fn name(&self) -> &Hash {
        &self.name
    }

    /// This level's objects, each at its entity's key, or at the root.
    #[must_use]
    pub fn own(&self) -> &History<Hash> {
        &self.own
    }

    /// The nest each pointer of this level holds, by its path.
    #[must_use]
    pub fn held(&self) -> &BTreeMap<Path, Nest> {
        &self.held
    }

    /// The join: every object of the tree at its path, this level's at
    /// their keys and each held nest's under the path that held it. A name
    /// is global, so the join renames no object and rewrites no dep.
    #[must_use]
    pub fn flatten(&self) -> History<Hash> {
        std::iter::once((Path::root(), self.own.clone()))
            .chain(
                self.held
                    .iter()
                    .map(|(path, nest)| (path.clone(), nest.flatten())),
            )
            .collect::<History<History<Hash>>>()
            .join()
    }

    /// What `heads` rest on in this nest, the heads included: this level's
    /// objects beneath them and, at every depth, what each pointer among
    /// those objects named.
    #[must_use]
    pub fn below(&self, heads: &BTreeSet<Hash>) -> BTreeSet<Hash> {
        let mut found = BTreeSet::new();
        let mut pending: Vec<&Hash> = heads.iter().collect();
        while let Some(name) = pending.pop() {
            let Some(parents) = self.parents.get(name) else {
                continue;
            };
            if !found.insert(name.clone()) {
                continue;
            }
            pending.extend(parents);
            for (path, named) in self.points.get(name).into_iter().flatten() {
                found.extend(self.held[path].below(named));
            }
        }
        found
    }

    /// Everything that happened before the object `name`, wherever it sits
    /// in the nest, or `None` where it is not in the nest. The past holds
    /// the object's ancestors in its own level and, through every pointer
    /// among the object and those ancestors, the inner history that the
    /// pointer named. Causality crosses a level only where a write names the
    /// inner heads that it saw, because an inner object's deps are inner.
    #[must_use]
    pub fn past(&self, name: &Hash) -> Option<BTreeSet<Hash>> {
        let Some(parents) = self.parents.get(name) else {
            return self.held.values().find_map(|nest| nest.past(name));
        };
        let mut past = self.below(&parents.iter().cloned().collect());
        for (path, named) in self.points.get(name).into_iter().flatten() {
            past.extend(self.held[path].below(named));
        }
        Some(past)
    }
}

/// A NEST IS A TREE OF CONTENT-ADDRESSED HISTORIES, so the memoised fold
/// (design §6.2) folds it with no further mechanism: a node is one level's
/// history, its children the nests its pointers hold in path order, and its
/// name covers both. A write deep in the nest, once the pointers above it
/// name it, renames exactly the nests from it to the root, and a fold
/// computes those and hits every other. [`Nest::flatten`] is such a fold,
/// its algebra a level's own objects joined with its children's results
/// under their paths.
impl prodrome_memo::Tree for Nest {
    fn name(&self) -> &Hash {
        &self.name
    }

    fn children(&self) -> impl Iterator<Item = &Self> {
        self.held.values()
    }
}
