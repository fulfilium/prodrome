//! The DAG as a value, with no files (SPEC §3). The module `store` of
//! `prodrome-core` keeps the files around a DAG.

mod below;
mod facts;
mod finding;
mod heads;
mod history;
mod index;
mod reach;

use std::collections::{BTreeMap, BTreeSet};

pub use finding::{Excluded, Finding, Unread};
pub use heads::Heads;
use history::History;
use index::Index;
pub use reach::Reach;

use crate::policy::Policy;
use crate::registers::{fold, Genesis, Node};
use crate::sign::{Proof, Proved, Registrar};
use crate::spread::spread;
use prodrome_data::change::Change;
use prodrome_data::event::{parents_of, parse_envelope, rests_on, Envelope, Hash};
use prodrome_data::husk::{Hashed, HuskObject, Sender, Stored};
use prodrome_data::literal::{Datetime, ProdromeError};
use prodrome_data::schema::Schema;

/// For this library's own code; no promise.
/// What a [`Dag`] holds, let go of: every object, every print that is not
/// one, and the names of the objects held as husks.
#[doc(hidden)]
pub struct Prints<E> {
    pub objects: BTreeMap<Hash, Envelope<E>>,
    pub unread: BTreeMap<Hash, Unread>,
    pub husks: BTreeSet<Hash>,
}

/// A set of named prints read at the schema `E`, each an object or a print
/// that is not one.
///
/// In a DAG from [`Dag::interior`] or from a store, the objects outside the
/// history wait, each resting on a name that no object of the history has.
/// In any other DAG, no object waits. [`Dag::excluded`] depends on every
/// print held, waiting or not. A husk (SPEC §3, "A husk") reads as the object
/// that it holds, and no print under its name hashes to the name.
#[derive(Debug, Clone, PartialEq)]
pub struct Dag<E> {
    history: History<E>,
    waiting: BTreeMap<Hash, Envelope<E>>,
    unread: BTreeMap<Hash, Unread>,
    husks: BTreeSet<Hash>,
}

impl<E> FromIterator<(Hash, Envelope<E>)> for Dag<E> {
    fn from_iter<I: IntoIterator<Item = (Hash, Envelope<E>)>>(objects: I) -> Self {
        Dag {
            history: History::new(objects.into_iter().collect()),
            waiting: BTreeMap::new(),
            unread: BTreeMap::new(),
            husks: BTreeSet::new(),
        }
    }
}

/// The union with more objects, the join of replicas (design §1): an object
/// is held once, and a name it is held under is no longer unread. Each
/// object given is a whole.
impl<E> Extend<(Hash, Envelope<E>)> for Dag<E> {
    fn extend<I: IntoIterator<Item = (Hash, Envelope<E>)>>(&mut self, objects: I) {
        let held = self.history.edit();
        for (name, object) in objects {
            self.unread.remove(&name);
            self.waiting.remove(&name);
            self.husks.remove(&name);
            held.insert(name, object);
        }
    }
}

/// Rehashes a stored print against its name and parses it at `schema`.
///
/// A print that does not hash to its name is read as a husk
/// ([`Schema::husk`]) only when `sender` is the keeper of record; otherwise
/// it is [`Unread::Tampered`].
pub fn decode<E: Schema>(
    schema: &E::Vocabulary,
    name: &Hash,
    bytes: &[u8],
    sender: Sender,
) -> Result<Stored<E>, Unread> {
    match Hashed::of(name, bytes) {
        Hashed::Whole(bytes) => {
            let text = std::str::from_utf8(bytes).map_err(|_| Unread::NotText)?;
            parse_envelope(schema, text)
                .map(Stored::Whole)
                .map_err(Unread::Unparsed)
        }
        Hashed::Other(other) => match sender {
            Sender::Keeper => E::husk(&other).map(Stored::Husk),
            Sender::Stranger => None,
        }
        .ok_or_else(|| Unread::Tampered(other.computed().clone())),
    }
}

impl<E> Dag<E> {
    pub fn objects(&self) -> &BTreeMap<Hash, Envelope<E>> {
        self.history.objects()
    }

    pub fn unread(&self) -> &BTreeMap<Hash, Unread> {
        &self.unread
    }

    /// The objects held, in the history or waiting, whose prints are husks
    /// (SPEC §3).
    pub fn husks(&self) -> &BTreeSet<Hash> {
        &self.husks
    }

    pub fn get(&self, name: &Hash) -> Option<&Envelope<E>> {
        self.objects().get(name)
    }

    /// For this library's own code; no promise.
    /// Every object held, in the history or waiting outside it.
    #[doc(hidden)]
    pub fn held(&self) -> impl Iterator<Item = (&Hash, &Envelope<E>)> {
        self.objects().iter().chain(&self.waiting)
    }

    /// For this library's own code; no promise.
    /// Is `name` held as an object, in the history or waiting outside it?
    #[doc(hidden)]
    pub fn holds(&self, name: &Hash) -> bool {
        self.objects().contains_key(name) || self.waiting.contains_key(name)
    }

    /// For this library's own code; no promise.
    /// Every print `name` was held as, an object or unread, let go.
    #[doc(hidden)]
    pub fn forget(&mut self, name: &Hash) -> Option<Envelope<E>> {
        self.unread.remove(name);
        self.husks.remove(name);
        self.history
            .edit()
            .remove(name)
            .or_else(|| self.waiting.remove(name))
    }

    /// For this library's own code; no promise.
    /// The prints that are not objects: `unread`, in place of those held.
    #[doc(hidden)]
    pub fn set_unread(&mut self, unread: BTreeMap<Hash, Unread>) {
        self.unread = unread;
    }

    /// For this library's own code; no promise.
    /// `names`, held as objects, are held as husks.
    #[doc(hidden)]
    pub fn hold_husks(&mut self, names: impl IntoIterator<Item = Hash>) {
        self.husks.extend(names);
    }

    /// For this library's own code; no promise.
    /// The object `name` of the history replaced by its husk. The husk of
    /// the object's own `Change` rests on what the object rests on, so the
    /// kept facts (the tips, the order and its index) are kept; a husk that
    /// rests on other names lets them go. A name the history does not hold
    /// is passed over.
    #[doc(hidden)]
    pub fn husk(&mut self, name: &Hash, husk: HuskObject<E>) {
        if self.history.replace(name, husk.into_object()).is_some() {
            self.husks.insert(name.clone());
        }
    }

    /// For this library's own code; no promise.
    /// Every object held, every print that is not one, and the names of the
    /// objects held as husks.
    #[doc(hidden)]
    pub fn into_prints(self) -> Prints<E> {
        let mut objects = self.history.into_objects();
        objects.extend(self.waiting);
        Prints {
            objects,
            unread: self.unread,
            husks: self.husks,
        }
    }

    /// The history: the largest down-set of the objects held, that is, every
    /// object whose ancestors are all held.
    ///
    /// Law 40: the interior is never more than the set, idempotent and
    /// monotone, so it depends on the objects alone. A cycle, which only a
    /// hash collision makes, is in no down-set. The result keeps every print
    /// of this DAG (the rest wait or stay unread), so [`Dag::excluded`] gives
    /// the same answer for both.
    #[must_use]
    pub fn interior(&self) -> Dag<E>
    where
        E: Clone,
    {
        let mut interior = self.clone();
        interior.settle();
        interior
    }

    /// For this library's own code; no promise.
    /// Make the history the interior of every object held: each one waits,
    /// and [`Dag::grow`] takes back in what rests only on the rest.
    #[doc(hidden)]
    pub fn settle(&mut self) {
        let held = std::mem::take(self.history.edit());
        self.grow(held);
    }

    /// For this library's own code; no promise.
    /// Extend this down-set by every object `arrived` and held waiting that
    /// rests only on it and on each other, and answer the names taken; the
    /// rest wait, each resting on something neither holds. The one step
    /// [`Dag::interior`] and a store's memory share: what arrives either
    /// joins the history or waits for what it rests on, and whatever
    /// arrives later that completes it brings it in.
    ///
    /// The names taken come in order. Maps are joined whole where that
    /// costs less than an insert per object: a merge costs the sizes of
    /// both maps, and an insert the logarithm of the larger.
    #[doc(hidden)]
    pub fn grow(&mut self, mut arrived: BTreeMap<Hash, Envelope<E>>) -> Vec<Hash> {
        arrived.retain(|name, _| !self.objects().contains_key(name));
        for name in arrived.keys() {
            self.unread.remove(name);
        }
        let mut waiting = std::mem::take(&mut self.waiting);
        waiting.append(&mut arrived);
        let mut joins = self.joining(&waiting).into_iter();
        let (joined, still): (Vec<_>, Vec<_>) = waiting
            .into_iter()
            .partition(|_| joins.next().unwrap_or(false));
        self.waiting = still.into_iter().collect();
        let taken = joined.iter().map(|(name, _)| name.clone()).collect();
        let history = self.history.edit();
        if joined.len() < history.len() {
            history.extend(joined);
        } else {
            history.append(&mut joined.into_iter().collect());
        }
        taken
    }

    /// Which of `waiting`, in its order, join the history: each object
    /// that rests only on the history and on objects of `waiting` that
    /// join. Kahn's walk over the objects' places in `waiting`, so that
    /// an edge costs a binary search among the names and no name is
    /// copied.
    fn joining(&self, waiting: &BTreeMap<Hash, Envelope<E>>) -> Vec<bool> {
        let names: Vec<&Hash> = waiting.keys().collect();
        let mut short = vec![0_usize; names.len()];
        let mut above: Vec<Vec<usize>> = vec![Vec::new(); names.len()];
        for (at, object) in waiting.values().enumerate() {
            for parent in rests_on(object) {
                match names.binary_search(&parent) {
                    Ok(under) => {
                        above[under].push(at);
                        short[at] += 1;
                    }
                    // Missing: the object waits until it arrives.
                    Err(_) if !self.objects().contains_key(parent) => short[at] += 1,
                    Err(_) => {}
                }
            }
        }
        let mut ready: Vec<usize> = (0..names.len()).filter(|&at| short[at] == 0).collect();
        let mut joins = vec![false; names.len()];
        while let Some(at) = ready.pop() {
            joins[at] = true;
            for &over in &above[at] {
                short[over] -= 1;
                if short[over] == 0 {
                    ready.push(over);
                }
            }
        }
        joins
    }

    /// This DAG if every print it holds is an object; a write over the DAG
    /// refuses with the same error.
    ///
    /// # Errors
    ///
    /// The first print, by name, that is not an object. A print that fails
    /// its hash gives [`Unread::refusal`], which names the `fsck` that sets
    /// it aside; any other gives [`Finding::Unread`] as text.
    pub fn whole(&self) -> Result<&Dag<E>, ProdromeError> {
        match self.unread.iter().next() {
            Some((name, why @ Unread::Tampered(_))) => Err(why.refusal(name)),
            Some((name, why)) => Err(ProdromeError::Store(
                Finding::Unread {
                    name: name.clone(),
                    why: why.clone(),
                }
                .to_string(),
            )),
            None => Ok(self),
        }
    }

    /// The objects that no object rests on ([`parents_of`]).
    ///
    /// The DAG derives the tips on the first call and keeps them until the
    /// objects change, so a later call makes no pass over the objects
    /// (law 59: a kept fact equals the fresh one).
    pub fn tips(&self) -> BTreeSet<Hash> {
        self.history.tips().clone()
    }

    /// The tips and the count of objects of the history.
    ///
    /// Law 65: for a DAG that a store holds, the store records these heads,
    /// so a host that places its own files derives its record here.
    #[must_use]
    pub fn heads(&self) -> Heads {
        Heads::new(self.tips(), self.objects().len())
    }

    /// `seeds` and everything they rest on. A name that this DAG lacks is in
    /// the result, and the walk stops there.
    pub fn closure(&self, seeds: impl IntoIterator<Item = Hash>) -> BTreeSet<Hash> {
        let mut found = BTreeSet::new();
        let mut pending: Vec<Hash> = seeds.into_iter().collect();
        while let Some(name) = pending.pop() {
            if let Some(object) = self.objects().get(&name).filter(|_| !found.contains(&name)) {
                pending.extend(parents_of(object));
            }
            found.insert(name);
        }
        found
    }

    /// `seeds` and every object that rests on them, the dual of
    /// [`Dag::closure`]. A seed that this DAG lacks is in the result, and the
    /// walk continues from the objects that name it as a parent.
    #[must_use]
    pub fn above(&self, seeds: impl IntoIterator<Item = Hash>) -> BTreeSet<Hash> {
        up(&self.children(), seeds)
    }

    /// Each name the objects held rest on, and the objects naming it as a
    /// parent.
    fn children(&self) -> BTreeMap<Hash, Vec<&Hash>> {
        let mut children: BTreeMap<Hash, Vec<&Hash>> = BTreeMap::new();
        for (name, object) in self.held() {
            for parent in parents_of(object) {
                children.entry(parent).or_default().push(name);
            }
        }
        children
    }

    /// Each name, in name order, that is held or rested on but is not an
    /// object here, with the objects that rest on it.
    ///
    /// A name is either an [`Unread`] print or a parent that no print has.
    /// Apart from a cycle, [`Dag::interior`] is exactly the objects that no
    /// `resting` set names. The answer depends only on the prints (law 40), so a DAG and its
    /// interior give the same answer. [`Dag::verify`] reports these names,
    /// and every reading of a replica leaves them out (laws 44 and 45).
    #[must_use]
    pub fn excluded(&self) -> Vec<Excluded> {
        let children = self.children();
        let lacking: BTreeSet<&Hash> = self
            .unread
            .keys()
            .chain(children.keys())
            .filter(|name| !self.holds(name))
            .collect();
        lacking
            .into_iter()
            .map(|name| {
                let mut resting = up(&children, [name.clone()]);
                resting.remove(name);
                Excluded {
                    name: name.clone(),
                    why: self.unread.get(name).cloned(),
                    resting,
                }
            })
            .collect()
    }

    /// The causal order: parents first, and incomparable objects in name
    /// order (Kahn's algorithm with a min-heap on the name).
    ///
    /// The DAG derives the order on the first call and keeps it until the
    /// objects change, as for [`Dag::tips`] (law 59).
    ///
    /// # Errors
    ///
    /// A parent that no object of the history is, or a cycle.
    pub fn linearise(&self) -> Result<&[Hash], ProdromeError> {
        self.history.order()
    }

    /// The empty [`Reach`] over the order of [`Dag::linearise`], with an
    /// index of the order that the DAG keeps (law 64).
    ///
    /// # Errors
    ///
    /// The errors of [`Dag::linearise`].
    pub fn reach(&self) -> Result<Reach<'_>, ProdromeError> {
        let (order, index) = self.history.indexed()?;
        Ok(Reach::empty(order, index))
    }

    /// For this library's own code; no promise.
    /// `among`, parents first, by Kahn's walk over them alone: a parent
    /// outside them is passed over. A cycle is a refusal.
    #[doc(hidden)]
    pub fn order_among(&self, among: &BTreeSet<&Hash>) -> Result<Vec<Hash>, ProdromeError> {
        self.history.order_among(among)
    }

    /// For this library's own code; no promise.
    /// Is `name` a legacy root, a `Sealed` with no `prev`?
    #[doc(hidden)]
    pub fn is_root(&self, name: &Hash) -> bool {
        matches!(
            self.objects().get(name),
            Some(Envelope::Sealed { prev: None, .. })
        )
    }

    /// The genesis of every prodrome: each `Genesis`, and the least legacy
    /// root, whose prodrome holds every legacy object (SPEC §3).
    pub fn geneses(&self) -> BTreeSet<Hash> {
        let genesis = self
            .objects()
            .iter()
            .filter(|(_, object)| matches!(object, Envelope::Genesis(_)))
            .map(|(name, _)| name.clone());
        let root = self
            .objects()
            .keys()
            .find(|name| self.is_root(name))
            .cloned();
        genesis.chain(root).collect()
    }

    /// The registers' key for the prodrome that `genesis` starts: `None` for
    /// a legacy root.
    pub fn key(&self, genesis: &Hash) -> Genesis {
        (!self.is_root(genesis)).then(|| genesis.clone())
    }

    /// The prodrome `object` is in: a signature's is what it signs'.
    pub(crate) fn prodrome(&self, name: &Hash, object: &Envelope<E>) -> Genesis {
        match object {
            Envelope::Sealed { .. } | Envelope::Woven { .. } => None,
            Envelope::Genesis(_) => self.key(name),
            Envelope::Signed(signed) => self
                .objects()
                .get_key_value(&signed.object)
                .and_then(|(name, object)| self.prodrome(name, object)),
            other => other.genesis().and_then(|genesis| self.key(genesis)),
        }
    }

    /// The genesis of the prodrome of the object `name` (SPEC §3); for the
    /// legacy prodrome, its least root. `None` if the DAG lacks the object.
    #[must_use]
    pub fn genesis_of(&self, name: &Hash) -> Option<Hash> {
        let object = self.objects().get(name)?;
        match self.prodrome(name, object) {
            Some(genesis) => Some(genesis),
            None => self
                .objects()
                .keys()
                .find(|name| self.is_root(name))
                .cloned(),
        }
    }

    /// The tips of one prodrome's objects.
    pub fn tips_in(&self, prodrome: &Genesis) -> BTreeSet<Hash> {
        self.tips()
            .into_iter()
            .filter(|tip| self.prodrome(tip, &self.objects()[tip]) == *prodrome)
            .collect()
    }
}

impl<E: Schema> Dag<E> {
    /// Decodes each print with [`decode`], and keeps each print that is not
    /// an object as [`Unread`]. A print that does not hash to its name is a
    /// husk only from the keeper of record (SPEC §3, "A husk").
    pub fn from_prints(
        schema: &E::Vocabulary,
        prints: impl IntoIterator<Item = (Hash, Result<Vec<u8>, ProdromeError>)>,
        sender: Sender,
    ) -> Dag<E> {
        Dag::decoded(prints.into_iter().collect(), |(name, bytes)| {
            let read = bytes
                .map_err(Unread::Io)
                .and_then(|bytes| decode(schema, &name, &bytes, sender));
            (name, read)
        })
    }

    /// For this library's own code; no promise.
    /// [`Dag::from_prints`] of the prints `read` answers for `names`. Each
    /// print is read on the thread that decodes it, so the bytes of a print
    /// are let go once it is decoded, and the reads of a large store run on
    /// every core as the decoding does.
    #[doc(hidden)]
    pub fn read_prints(
        schema: &E::Vocabulary,
        names: Vec<Hash>,
        read: impl Fn(&Hash) -> Result<Vec<u8>, ProdromeError> + Sync,
    ) -> Dag<E> {
        Dag::decoded(names, |name| {
            let read = read(&name)
                .map_err(Unread::Io)
                .and_then(|bytes| decode(schema, &name, &bytes, Sender::Keeper));
            (name, read)
        })
    }

    /// Each item decoded by `decode` ([`spread`] over the cores), an object
    /// held under its name and a print that is not one kept with why.
    fn decoded<T: Send>(
        items: Vec<T>,
        decode: impl Fn(T) -> (Hash, Result<Stored<E>, Unread>) + Sync,
    ) -> Dag<E> {
        let mut dag: Dag<E> = Dag::from_iter([]);
        for (name, read) in spread(items, decode) {
            match read {
                Ok(Stored::Whole(object)) => {
                    dag.history.edit().insert(name, object);
                }
                Ok(Stored::Husk(husk)) => {
                    dag.history.edit().insert(name.clone(), husk.into_object());
                    dag.husks.insert(name);
                }
                Err(why) => {
                    dag.unread.insert(name, why);
                }
            }
        }
        dag
    }

    /// The objects as the registers read them, in the order of
    /// [`Dag::linearise`].
    ///
    /// # Errors
    ///
    /// A DAG that lacks a parent refuses; call this on its [`Dag::interior`].
    pub fn nodes(&self) -> Result<Vec<Node<E>>, ProdromeError> {
        Ok(self
            .linearise()?
            .iter()
            .filter_map(|name| self.node(name))
            .collect())
    }

    /// For this library's own code; no promise.
    /// One object as the registers read it, in this DAG's prodromes.
    #[doc(hidden)]
    pub fn node(&self, name: &Hash) -> Option<Node<E>> {
        let (name, object) = self.objects().get_key_value(name)?;
        Some(Node::placed(
            name.clone(),
            object,
            self.prodrome(name, object),
        ))
    }

    /// The findings of SPEC §3, "`verify`": every print that is not an
    /// object, by name; then every parent that no object is, or
    /// else a cycle, or else the dating rule; then the genesis, deps and
    /// snapshot rules, object by object.
    ///
    /// The answer depends only on the prints, so the interior of a DAG
    /// verifies as the DAG does.
    pub fn verify(&self, policy: &impl Policy<E>) -> Vec<Finding> {
        if !self.waiting.is_empty() {
            let mut held = self.clone();
            let waiting = std::mem::take(&mut held.waiting);
            held.history.edit().extend(waiting);
            return held.verify(policy);
        }
        let excluded = self.excluded();
        let mut findings: Vec<Finding> = excluded
            .iter()
            .filter_map(|lacked| {
                lacked.why.clone().map(|why| Finding::Unread {
                    name: lacked.name.clone(),
                    why,
                })
            })
            .collect();
        // A name something rests on is a parent no object is.
        let broken: Vec<Finding> = excluded
            .into_iter()
            .filter(|lacked| !lacked.resting.is_empty())
            .map(|lacked| Finding::Broken {
                at: lacked.name,
                why: lacked.why,
            })
            .collect();
        if broken.is_empty() {
            match self.linearise() {
                Ok(order) => findings.extend(self.dated(order, policy)),
                Err(error) => findings.push(Finding::Cycle(error)),
            }
        }
        findings.extend(broken);
        findings.extend(self.signature_findings());
        let ordered = self.history.indexed();
        for (name, object) in self.objects() {
            findings.extend(self.genesis_findings(name, object));
            match object {
                Envelope::Change(change) => {
                    findings.extend(self.deps_findings(name, change, &ordered));
                }
                Envelope::Snapshot(_) => findings.extend(
                    self.closure(parents_of(object))
                        .into_iter()
                        .filter(|held| !self.objects().contains_key(held))
                        .map(|missing| Finding::Incomplete {
                            snapshot: name.clone(),
                            missing,
                        }),
                ),
                _ => {}
            }
        }
        findings.extend(self.below_findings());
        findings
    }

    /// §6: each write to an inflationary register stands at or above the
    /// reading of the writes it rests on, as the append requires
    /// ([`below::findings`]). Of a DAG with no history (a cycle), it finds
    /// nothing.
    fn below_findings(&self) -> Vec<Finding> {
        // Every event of a DAG is read at one vocabulary, so the product of
        // any one write says whether the schema has an inflationary
        // register; with none, no write can fall, and there is no fold.
        let first = self
            .objects()
            .iter()
            .find(|(_, object)| object.event().is_some())
            .and_then(|(name, _)| self.node(name));
        let Some(first) = first else {
            return Vec::new();
        };
        if below::climbing(&fold(std::slice::from_ref(&first))).is_empty() {
            return Vec::new();
        }
        match self.nodes().or_else(|_| self.interior().nodes()) {
            Ok(nodes) => below::findings(&fold(&nodes)),
            Err(_) => Vec::new(),
        }
    }

    /// §3 and §5: a signature that does not verify, and an object whose
    /// actor has a key and which no key of its actor's signed, keys read as
    /// they are written ([`Registrar::Anyone`]).
    fn signature_findings(&self) -> Vec<Finding> {
        let proof = Proof::of(self, &Registrar::Anyone);
        let mut findings = Vec::new();
        for (name, object) in self.objects() {
            if matches!(object, Envelope::Signed(signed) if !signed.verifies()) {
                findings.push(Finding::Forged {
                    signed: name.clone(),
                });
            }
            if let Some(event) = object.event() {
                match proof.standing(name) {
                    Proved::Proven | Proved::Vouched => {}
                    Proved::Unsigned => {
                        if proof.keyed(&self.prodrome(name, object), event.actor()) {
                            findings.push(Finding::Unsigned {
                                object: name.clone(),
                                actor: event.actor().clone(),
                            });
                        }
                    }
                }
            }
        }
        findings
    }

    /// §3: an object names a genesis the store holds, and rests on
    /// nothing of another. The edge to a stranger is that finding alone.
    fn genesis_findings(&self, name: &Hash, object: &Envelope<E>) -> Vec<Finding> {
        let stranger = object.genesis().filter(|genesis| {
            !self.is_root(genesis)
                && !matches!(self.objects().get(*genesis), Some(Envelope::Genesis(_)))
        });
        let prodrome = self.prodrome(name, object);
        let crossings = parents_of(object).into_iter().filter(|parent| {
            Some(parent) != stranger
                && self
                    .objects()
                    .get(parent)
                    .is_some_and(|held| self.prodrome(parent, held) != prodrome)
        });
        stranger
            .map(|genesis| Finding::Stranger {
                object: name.clone(),
                genesis: genesis.clone(),
            })
            .into_iter()
            .chain(crossings.map(|parent| Finding::Crossing {
                object: name.clone(),
                parent,
            }))
            .collect()
    }

    /// §3: each dep is a write to the change's entity, and no dep rests on
    /// another. `ordered` is the DAG's linear order and its index, or the
    /// refusal of the order.
    fn deps_findings(
        &self,
        name: &Hash,
        change: &Change<E>,
        ordered: &Result<(&[Hash], &Index), ProdromeError>,
    ) -> Vec<Finding> {
        let above = self.deps_above(&change.deps, ordered);
        let mut findings = Vec::new();
        for dep in &change.deps {
            let held = self.objects().get(dep);
            if held.is_some_and(|held| held.event().map(Schema::key) != Some(change.event.key())) {
                findings.push(Finding::Foreign {
                    change: name.clone(),
                    dep: dep.clone(),
                });
            }
            for &other in above.get(dep).into_iter().flatten() {
                findings.push(Finding::Redundant {
                    change: name.clone(),
                    dep: dep.clone(),
                    beneath: other.clone(),
                });
            }
        }
        findings
    }

    /// For each of `deps`, the deps that it lies beneath, in the order of
    /// `deps`. A dep lies beneath `other` when it is in the closure of the
    /// parents of `other` ([`Dag::closure`]).
    ///
    /// Where the DAG has a linear order, it has no cycle, so no dep lies
    /// beneath itself, and a change with fewer than two deps has no pair to
    /// find. For more deps, one walk over the positions of the
    /// order answers for all of them ([`Index::beneath`]), and it reaches
    /// only the objects between the least dep and the others. Where the DAG
    /// has no linear order, because a parent is missing or the objects make
    /// a cycle, the closure of each dep's parents answers. In a cycle, a dep
    /// can lie beneath itself, and this answer keeps that pair.
    fn deps_above<'c>(
        &self,
        deps: &'c [Hash],
        ordered: &Result<(&[Hash], &Index), ProdromeError>,
    ) -> BTreeMap<&'c Hash, Vec<&'c Hash>> {
        match (ordered, deps) {
            (Ok(_), [] | [_]) => BTreeMap::new(),
            (Ok((order, index)), _) => {
                // The order holds every parent, so each dep has a position.
                let placed: Vec<(&Hash, usize)> = deps
                    .iter()
                    .filter_map(|dep| index.position(order, dep).map(|at| (dep, at)))
                    .collect();
                let tops: Vec<usize> = placed.iter().map(|&(_, at)| at).collect();
                placed
                    .iter()
                    .zip(index.beneath(&tops))
                    .map(|(&(dep, _), above)| {
                        (dep, above.iter().map(|top| placed[top].0).collect())
                    })
                    .collect()
            }
            (Err(_), _) => {
                let below: Vec<BTreeSet<Hash>> = deps
                    .iter()
                    .map(|other| {
                        let parents = self
                            .objects()
                            .get(other)
                            .map(parents_of)
                            .unwrap_or_default();
                        self.closure(parents)
                    })
                    .collect();
                deps.iter()
                    .map(|dep| {
                        let above: Vec<&Hash> = deps
                            .iter()
                            .zip(&below)
                            .filter(|(_, below)| below.contains(dep))
                            .map(|(other, _)| other)
                            .collect();
                        (dep, above)
                    })
                    .collect()
            }
        }
    }

    /// An event the policy does not confirm, dated before the latest stamp
    /// anywhere beneath it: a forced clock that ran backwards.
    fn dated(&self, order: &[Hash], policy: &impl Policy<E>) -> Vec<Finding> {
        let mut high: BTreeMap<&Hash, Datetime> = BTreeMap::new();
        let mut findings = Vec::new();
        for name in order {
            let (name, object) = self
                .objects()
                .get_key_value(name)
                .expect("the order names objects");
            let behind = parents_of(object)
                .iter()
                .filter_map(|parent| high.get(parent).copied())
                .max();
            let event = object.event();
            if let (Some(event), Some(behind)) = (event, behind) {
                if !policy.confirms(name, event) && event.at() < behind {
                    findings.push(Finding::Dated {
                        name: name.clone(),
                        at: event.at(),
                        behind,
                    });
                }
            }
            if let Some(stamp) = behind.into_iter().chain(event.map(Schema::at)).max() {
                high.insert(name, stamp);
            }
        }
        findings
    }
}

/// The tips of a graph given as each name with the parents it names: the
/// names no parent list names.
pub(crate) fn tips_among<'a>(
    graph: impl IntoIterator<Item = (&'a Hash, Vec<&'a Hash>)>,
) -> BTreeSet<Hash> {
    apart(graph).0.into_iter().cloned().collect()
}

/// For this library's own code; no promise.
/// What a graph given as each name with the parents it names lacks: the
/// names a parent list names that are not names of the graph.
#[doc(hidden)]
pub fn lacked_among<'a>(
    graph: impl IntoIterator<Item = (&'a Hash, Vec<&'a Hash>)>,
) -> BTreeSet<Hash> {
    apart(graph).1.into_iter().cloned().collect()
}

/// The names of a graph that no parent list names, and the names the
/// parent lists name that are not names of the graph, each in order. Both
/// lists of names are sorted and merged once, so a parent costs no lookup.
fn apart<'a>(
    graph: impl IntoIterator<Item = (&'a Hash, Vec<&'a Hash>)>,
) -> (Vec<&'a Hash>, Vec<&'a Hash>) {
    let mut names: Vec<&Hash> = Vec::new();
    let mut named: Vec<&Hash> = Vec::new();
    for (name, parents) in graph {
        names.push(name);
        named.extend(parents);
    }
    for list in [&mut names, &mut named] {
        list.sort_unstable();
        list.dedup();
    }
    let (mut unnamed, mut lacked) = (Vec::new(), Vec::new());
    let (mut at, mut on) = (0, 0);
    while let (Some(&name), Some(&parent)) = (names.get(at), named.get(on)) {
        match name.cmp(parent) {
            std::cmp::Ordering::Less => {
                unnamed.push(name);
                at += 1;
            }
            std::cmp::Ordering::Greater => {
                lacked.push(parent);
                on += 1;
            }
            std::cmp::Ordering::Equal => {
                at += 1;
                on += 1;
            }
        }
    }
    unnamed.extend(&names[at..]);
    lacked.extend(&named[on..]);
    (unnamed, lacked)
}

/// `seeds` and everything above them, through each name's `children`.
fn up(
    children: &BTreeMap<Hash, Vec<&Hash>>,
    seeds: impl IntoIterator<Item = Hash>,
) -> BTreeSet<Hash> {
    let mut found = BTreeSet::new();
    let mut pending: Vec<Hash> = seeds.into_iter().collect();
    while let Some(name) = pending.pop() {
        if !found.contains(&name) {
            pending.extend(
                children
                    .get(&name)
                    .into_iter()
                    .flatten()
                    .map(|&child| child.clone()),
            );
            found.insert(name);
        }
    }
    found
}
