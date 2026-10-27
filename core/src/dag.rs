//! §3 — the DAG as a value: named objects, their tips, closure, order and
//! findings, with no files. [`crate::store`] is the files around one.

mod finding;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub use finding::{Excluded, Finding, Unread};

use crate::change::Change;
use crate::event::{parents_of, parse_envelope, Envelope, Hash};
use crate::literal::{Datetime, ProdromeError};
use crate::policy::Policy;
use crate::registers::{Genesis, Node};
use crate::schema::Schema;
use crate::sign::{Proof, Registrar};
use crate::topo;

/// Named prints read at the schema `E`: each an object or, `unread`, a print
/// that is not one. The objects are `objects` and `waiting`: of a DAG made
/// by [`Dag::interior`], or held by a store, `objects` is its history and
/// `waiting` the objects outside it, each resting on a name no object of
/// the history is; of any other, `waiting` is empty. So the history is
/// `objects` once read, and what it leaves out ([`Dag::excluded`]) is still
/// a function of every print held.
#[derive(Debug, Clone, PartialEq)]
pub struct Dag<E> {
    objects: BTreeMap<Hash, Envelope<E>>,
    waiting: BTreeMap<Hash, Envelope<E>>,
    unread: BTreeMap<Hash, Unread>,
    tips: Tips,
}

/// [`Dag::tips`] of the objects, derived on its first ask and kept until
/// the objects change: every method that changes them clears it. A
/// function of the objects, so it plays no part in equality.
#[derive(Debug, Clone, Default)]
struct Tips(OnceLock<BTreeSet<Hash>>);

impl PartialEq for Tips {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<E> FromIterator<(Hash, Envelope<E>)> for Dag<E> {
    fn from_iter<I: IntoIterator<Item = (Hash, Envelope<E>)>>(objects: I) -> Self {
        Dag {
            objects: objects.into_iter().collect(),
            waiting: BTreeMap::new(),
            unread: BTreeMap::new(),
            tips: Tips::default(),
        }
    }
}

/// The union with more objects, the join of replicas (design §1): an object
/// is held once, and a name it is held under is no longer unread.
impl<E> Extend<(Hash, Envelope<E>)> for Dag<E> {
    fn extend<I: IntoIterator<Item = (Hash, Envelope<E>)>>(&mut self, objects: I) {
        self.tips = Tips::default();
        for (name, object) in objects {
            self.unread.remove(&name);
            self.waiting.remove(&name);
            self.objects.insert(name, object);
        }
    }
}

/// A stored print, rehashed against its name before it is parsed at the
/// schema `schema`.
pub fn decode<E: Schema>(
    schema: &E::Vocabulary,
    name: &Hash,
    bytes: &[u8],
) -> Result<Envelope<E>, Unread> {
    let computed = Hash::of_bytes(bytes);
    if computed != *name {
        return Err(Unread::Tampered(computed));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Unread::NotText)?;
    parse_envelope(schema, text).map_err(Unread::Unparsed)
}

impl<E> Dag<E> {
    pub fn objects(&self) -> &BTreeMap<Hash, Envelope<E>> {
        &self.objects
    }

    pub fn unread(&self) -> &BTreeMap<Hash, Unread> {
        &self.unread
    }

    pub fn get(&self, name: &Hash) -> Option<&Envelope<E>> {
        self.objects.get(name)
    }

    /// Every object held, in the history or waiting outside it.
    pub(crate) fn held(&self) -> impl Iterator<Item = (&Hash, &Envelope<E>)> {
        self.objects.iter().chain(&self.waiting)
    }

    /// Is `name` held as an object, in the history or waiting outside it?
    pub(crate) fn holds(&self, name: &Hash) -> bool {
        self.objects.contains_key(name) || self.waiting.contains_key(name)
    }

    /// Every print `name` was held as, an object or unread, let go.
    pub(crate) fn forget(&mut self, name: &Hash) -> Option<Envelope<E>> {
        self.tips = Tips::default();
        self.unread.remove(name);
        self.objects
            .remove(name)
            .or_else(|| self.waiting.remove(name))
    }

    /// The prints that are not objects: `unread`, in place of those held.
    pub(crate) fn set_unread(&mut self, unread: BTreeMap<Hash, Unread>) {
        self.unread = unread;
    }

    /// Every object held, and every print that is not one.
    pub(crate) fn into_prints(self) -> (BTreeMap<Hash, Envelope<E>>, BTreeMap<Hash, Unread>) {
        let mut objects = self.objects;
        objects.extend(self.waiting);
        (objects, self.unread)
    }

    /// THE HISTORY THESE OBJECTS HOLD: the largest down-set among them, every
    /// object whose ancestors are all here. Of a store closed under parents
    /// it is the store; of one missing an object (set aside, never arrived)
    /// it is the store without that object and everything resting on it,
    /// which is the history a replica holding exactly those objects has. An
    /// interior operator on sets of objects: never more than the set,
    /// idempotent and monotone, so a function of the objects alone (law 40).
    /// A cycle, which only a hash collision makes, is in no down-set.
    ///
    /// NOTHING IS LOST: the objects outside it are held as waiting, and the
    /// unread prints as unread, so the interior holds the prints this DAG
    /// holds and [`Dag::excluded`] answers of it what it answers of this.
    #[must_use]
    pub fn interior(&self) -> Dag<E>
    where
        E: Clone,
    {
        let mut interior = self.clone();
        interior.settle();
        interior
    }

    /// Make the history the interior of every object held: each one waits,
    /// and [`Dag::grow`] takes back in what rests only on the rest.
    pub(crate) fn settle(&mut self) {
        let held = std::mem::take(&mut self.objects);
        self.grow(held);
    }

    /// Extend this down-set by every object `arrived` and held waiting that
    /// rests only on it and on each other, and answer the names taken; the
    /// rest wait, each resting on something neither holds. The one step
    /// [`Dag::interior`] and a store's memory share: what arrives either
    /// joins the history or waits for what it rests on, and whatever
    /// arrives later that completes it brings it in.
    pub(crate) fn grow(&mut self, arrived: BTreeMap<Hash, Envelope<E>>) -> BTreeSet<Hash> {
        self.tips = Tips::default();
        let mut waiting = std::mem::take(&mut self.waiting);
        for (name, object) in arrived {
            if !self.objects.contains_key(&name) {
                self.unread.remove(&name);
                waiting.insert(name, object);
            }
        }
        let mut children: BTreeMap<Hash, Vec<Hash>> = BTreeMap::new();
        let mut short: BTreeMap<Hash, usize> = BTreeMap::new();
        let mut ready: Vec<Hash> = Vec::new();
        for (name, object) in &waiting {
            let mut count = 0;
            for parent in parents_of(object) {
                if waiting.contains_key(&parent) {
                    children.entry(parent).or_default().push(name.clone());
                    count += 1;
                } else if !self.objects.contains_key(&parent) {
                    // Missing: this object waits until it arrives.
                    count += 1;
                }
            }
            if count == 0 {
                ready.push(name.clone());
            } else {
                short.insert(name.clone(), count);
            }
        }
        let mut taken = BTreeSet::new();
        while let Some(name) = ready.pop() {
            let object = waiting.remove(&name).expect("a ready object waits");
            self.objects.insert(name.clone(), object);
            for child in children.remove(&name).into_iter().flatten() {
                let count = short.get_mut(&child).expect("a child waits");
                *count -= 1;
                if *count == 0 {
                    short.remove(&child);
                    ready.push(child);
                }
            }
            taken.insert(name);
        }
        self.waiting = waiting;
        taken
    }

    /// The DAG, where every print it holds is an object; or the refusal of
    /// the first, by name, that is not: what a write over it refuses with.
    ///
    /// # Errors
    ///
    /// That print named: a file failing its hash by [`Unread::refusal`],
    /// which names the `fsck` that sets it aside, and any other in
    /// [`Dag::verify`]'s words ([`Finding::Unread`]).
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

    /// Every object no object rests on: the maximal objects of the causal
    /// order ([`parents_of`]). Derived from the objects on the first ask
    /// and kept until they change, so a DAG that a store of record keeps
    /// answers it without a pass over its objects.
    pub fn tips(&self) -> BTreeSet<Hash> {
        self.tips
            .0
            .get_or_init(|| {
                let graph: Vec<(&Hash, Vec<Hash>)> = self
                    .objects
                    .iter()
                    .map(|(name, object)| (name, parents_of(object)))
                    .collect();
                tips_among(
                    graph
                        .iter()
                        .map(|(name, parents)| (*name, parents.as_slice())),
                )
            })
            .clone()
    }

    /// `seeds` and everything they rest on; a name this DAG lacks is in it,
    /// and not walked past.
    pub fn closure(&self, seeds: impl IntoIterator<Item = Hash>) -> BTreeSet<Hash> {
        let mut found = BTreeSet::new();
        let mut pending: Vec<Hash> = seeds.into_iter().collect();
        while let Some(name) = pending.pop() {
            if let Some(object) = self.objects.get(&name).filter(|_| !found.contains(&name)) {
                pending.extend(parents_of(object));
            }
            found.insert(name);
        }
        found
    }

    /// `seeds` and every object resting on them: [`Dag::closure`]'s dual,
    /// an up-set of the causal order. A seed this DAG lacks is in it, and
    /// what names it as a parent is walked from.
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

    /// WHAT THE HISTORY LEAVES OUT, AND WHY: every name these prints hold
    /// or their objects rest on that is no object here, a print that is not
    /// one ([`Unread`]) or a parent no print is, in name order, each with
    /// the objects resting on it ([`Dag::above`]). The history,
    /// [`Dag::interior`], is exactly the objects no `resting` names (a
    /// cycle aside, which only a hash collision makes): this says, object
    /// by object, why the rest are not in it. A function of the prints, as
    /// the interior is (law 40), and so the same of a DAG and of its
    /// interior; the one reading of "unreadable" that [`Dag::verify`]
    /// reports and every replica's reading leaves out (laws 44 and 45).
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

    /// Kahn's algorithm with a min-heap on the name: parents first, and
    /// incomparable objects in name order. A missing parent or a cycle is a
    /// refusal.
    pub fn linearise(&self) -> Result<Vec<Hash>, ProdromeError> {
        for (name, object) in &self.objects {
            if let Some(parent) = parents_of(object)
                .into_iter()
                .find(|parent| !self.objects.contains_key(parent))
            {
                return Err(ProdromeError::Store(format!(
                    "missing object {}, named as a parent by {}",
                    parent.as_str(),
                    name.as_str()
                )));
            }
        }
        self.order_among(&self.objects.keys().collect())
    }

    /// `among`, parents first, by Kahn's walk over them alone: a parent
    /// outside them is passed over. A cycle is a refusal.
    pub(crate) fn order_among(&self, among: &BTreeSet<&Hash>) -> Result<Vec<Hash>, ProdromeError> {
        let rests_on: BTreeMap<&Hash, Vec<&Hash>> = among
            .iter()
            .filter_map(|name| self.objects.get_key_value(*name))
            .map(|(name, object)| {
                let parents = parents_of(object)
                    .into_iter()
                    .filter_map(|parent| among.get(&parent).copied())
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

    /// Is `name` a legacy root, a `Sealed` with no `prev`?
    pub(crate) fn is_root(&self, name: &Hash) -> bool {
        matches!(
            self.objects.get(name),
            Some(Envelope::Sealed { prev: None, .. })
        )
    }

    /// Every prodrome's genesis: each `Genesis`, and the least legacy root,
    /// whose prodrome every legacy object is in (§3).
    pub fn geneses(&self) -> BTreeSet<Hash> {
        let genesis = self
            .objects
            .iter()
            .filter(|(_, object)| matches!(object, Envelope::Genesis(_)))
            .map(|(name, _)| name.clone());
        let root = self.objects.keys().find(|name| self.is_root(name)).cloned();
        genesis.chain(root).collect()
    }

    /// The registers' key for the prodrome `genesis` starts: `None` for a
    /// legacy root.
    pub fn key(&self, genesis: &Hash) -> Genesis {
        (!self.is_root(genesis)).then(|| genesis.clone())
    }

    /// The prodrome `object` is in: a signature's is what it signs'.
    pub(crate) fn prodrome(&self, name: &Hash, object: &Envelope<E>) -> Genesis {
        match object {
            Envelope::Sealed { .. } | Envelope::Woven { .. } => None,
            Envelope::Genesis(_) => self.key(name),
            Envelope::Signed(signed) => self
                .objects
                .get_key_value(&signed.object)
                .and_then(|(name, object)| self.prodrome(name, object)),
            other => other.genesis().and_then(|genesis| self.key(genesis)),
        }
    }

    /// The genesis of the prodrome the object `name` is in (§3): a
    /// `Genesis`'s own name, the one a change or a snapshot names, and for
    /// the legacy prodrome its least root. `None` where the DAG lacks it.
    #[must_use]
    pub fn genesis_of(&self, name: &Hash) -> Option<Hash> {
        let object = self.objects.get(name)?;
        match self.prodrome(name, object) {
            Some(genesis) => Some(genesis),
            None => self.objects.keys().find(|name| self.is_root(name)).cloned(),
        }
    }

    /// The tips of one prodrome's objects.
    pub fn tips_in(&self, prodrome: &Genesis) -> BTreeSet<Hash> {
        self.tips()
            .into_iter()
            .filter(|tip| self.prodrome(tip, &self.objects[tip]) == *prodrome)
            .collect()
    }
}

impl<E: Schema> Dag<E> {
    /// Each print decoded under its name at the schema `schema`, a print
    /// that is not an object kept with why.
    pub fn from_prints(
        schema: &E::Vocabulary,
        prints: impl IntoIterator<Item = (Hash, Result<Vec<u8>, ProdromeError>)>,
    ) -> Dag<E> {
        let mut dag: Dag<E> = Dag::from_iter([]);
        for (name, bytes) in prints {
            match bytes
                .map_err(Unread::Io)
                .and_then(|bytes| decode(schema, &name, &bytes))
            {
                Ok(object) => {
                    dag.objects.insert(name, object);
                }
                Err(why) => {
                    dag.unread.insert(name, why);
                }
            }
        }
        dag
    }

    /// The objects as the registers read them, in the linearisation's order.
    /// A DAG missing a parent refuses: its [`Dag::interior`] is the history
    /// it holds.
    pub fn nodes(&self) -> Result<Vec<Node<E>>, ProdromeError> {
        Ok(self
            .linearise()?
            .iter()
            .filter_map(|name| self.node(name))
            .collect())
    }

    /// One object as the registers read it, in this DAG's prodromes.
    pub(crate) fn node(&self, name: &Hash) -> Option<Node<E>> {
        let (name, object) = self.objects.get_key_value(name)?;
        Some(Node::placed(
            name.clone(),
            object,
            self.prodrome(name, object),
        ))
    }

    /// §3's findings: every print that is not an object, by name, then every
    /// parent no object is, or else a cycle, or else the dating rule; then
    /// §3's genesis, deps and snapshot rules, object by object. Of every
    /// object held, a function of the prints as [`Dag::excluded`] is: a
    /// DAG's interior verifies as the DAG does.
    pub fn verify(&self, policy: &impl Policy<E>) -> Vec<Finding> {
        if !self.waiting.is_empty() {
            let mut held = self.clone();
            let waiting = std::mem::take(&mut held.waiting);
            held.objects.extend(waiting);
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
                Ok(order) => findings.extend(self.dated(&order, policy)),
                Err(error) => findings.push(Finding::Cycle(error)),
            }
        }
        findings.extend(broken);
        findings.extend(self.signature_findings());
        for (name, object) in &self.objects {
            findings.extend(self.genesis_findings(name, object));
            match object {
                Envelope::Change(change) => findings.extend(self.deps_findings(name, change)),
                Envelope::Snapshot(_) => findings.extend(
                    self.closure(parents_of(object))
                        .into_iter()
                        .filter(|held| !self.objects.contains_key(held))
                        .map(|missing| Finding::Incomplete {
                            snapshot: name.clone(),
                            missing,
                        }),
                ),
                _ => {}
            }
        }
        findings
    }

    /// §3 and §5: a signature that does not verify, and an object whose
    /// actor has a key and which no key of its actor's signed, keys read as
    /// they are written ([`Registrar::Anyone`]).
    fn signature_findings(&self) -> Vec<Finding> {
        let proof = Proof::of(self, &Registrar::Anyone);
        let mut findings = Vec::new();
        for (name, object) in &self.objects {
            if matches!(object, Envelope::Signed(signed) if !signed.verifies()) {
                findings.push(Finding::Forged {
                    signed: name.clone(),
                });
            }
            if let Some(event) = object.event() {
                if proof.keyed(&self.prodrome(name, object), event.actor()) && !proof.proves(name) {
                    findings.push(Finding::Unsigned {
                        object: name.clone(),
                        actor: event.actor().clone(),
                    });
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
                && !matches!(self.objects.get(*genesis), Some(Envelope::Genesis(_)))
        });
        let prodrome = self.prodrome(name, object);
        let crossings = parents_of(object).into_iter().filter(|parent| {
            Some(parent) != stranger
                && self
                    .objects
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
    /// another.
    fn deps_findings(&self, name: &Hash, change: &Change<E>) -> Vec<Finding> {
        let beneath: Vec<(&Hash, BTreeSet<Hash>)> = change
            .deps
            .iter()
            .map(|dep| {
                let parents = self.objects.get(dep).map(parents_of).unwrap_or_default();
                (dep, self.closure(parents))
            })
            .collect();
        let mut findings = Vec::new();
        for (dep, _) in &beneath {
            let held = self.objects.get(dep);
            if held.is_some_and(|held| held.event().map(Schema::key) != Some(change.event.key())) {
                findings.push(Finding::Foreign {
                    change: name.clone(),
                    dep: (*dep).clone(),
                });
            }
            for (other, below) in &beneath {
                if below.contains(*dep) {
                    findings.push(Finding::Redundant {
                        change: name.clone(),
                        dep: (*dep).clone(),
                        beneath: (*other).clone(),
                    });
                }
            }
        }
        findings
    }

    /// An event the policy does not confirm, dated before the latest stamp
    /// anywhere beneath it: a forced clock that ran backwards.
    fn dated(&self, order: &[Hash], policy: &impl Policy<E>) -> Vec<Finding> {
        let mut high: BTreeMap<&Hash, Datetime> = BTreeMap::new();
        let mut findings = Vec::new();
        for name in order {
            let (name, object) = self
                .objects
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

/// The tips of a graph given as each name with the parents it names.
pub(crate) fn tips_among<'a>(
    graph: impl IntoIterator<Item = (&'a Hash, &'a [Hash])>,
) -> BTreeSet<Hash> {
    let mut names: Vec<&Hash> = Vec::new();
    let mut named: BTreeSet<&Hash> = BTreeSet::new();
    for (name, parents) in graph {
        names.push(name);
        named.extend(parents);
    }
    names
        .into_iter()
        .filter(|name| !named.contains(name))
        .cloned()
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{canonical_envelope, mk_created, mk_sealed, seal_hash, TodoEvent};
    use crate::reference::Todo;

    #[test]
    fn linearise_refuses_a_missing_parent() {
        let at = Datetime::new(2026, 9, 1, 12, 0, 0, 0).expect("a real instant");
        let event = mk_created("alpha", at, "bassel", "", "").expect("valid");
        let absent = Hash::new("a".repeat(64)).expect("hex");
        let orphaned: Envelope<TodoEvent<Todo>> = mk_sealed(Some(absent.clone()), event);
        let name = seal_hash(&orphaned);
        let dag: Dag<TodoEvent<Todo>> = [(name.clone(), orphaned)].into_iter().collect();
        assert_eq!(
            dag.linearise().unwrap_err().to_string(),
            format!(
                "missing object {}, named as a parent by {}",
                absent.as_str(),
                name.as_str()
            )
        );
    }

    /// A print that is not an object, and the chain on it: the history
    /// leaves out the print and both objects above it, and `excluded` says
    /// so, with the parse's own refusal, as `verify` reports it.
    #[test]
    fn excluded_names_an_unread_print_and_what_rests_on_it() {
        let at = Datetime::new(2026, 9, 1, 12, 0, 0, 0).expect("a real instant");
        let junk = b"Sealed(prev=None, event=Nonsense())".to_vec();
        let unread = Hash::of_bytes(&junk);
        let first: Envelope<TodoEvent<Todo>> = mk_sealed(
            Some(unread.clone()),
            mk_created("alpha", at, "bassel", "", "").expect("valid"),
        );
        let second: Envelope<TodoEvent<Todo>> = mk_sealed(
            Some(seal_hash(&first)),
            mk_created("beta", at, "bassel", "", "").expect("valid"),
        );
        let print = |object: &Envelope<TodoEvent<Todo>>| {
            (
                seal_hash(object),
                Ok(canonical_envelope(object).into_bytes()),
            )
        };
        let dag: Dag<TodoEvent<Todo>> = Dag::from_prints(
            &crate::todo::TodoVocabulary::default(),
            [(unread.clone(), Ok(junk)), print(&first), print(&second)],
        );
        let excluded = dag.excluded();
        assert_eq!(excluded.len(), 1);
        assert_eq!(excluded[0].name, unread);
        assert!(matches!(excluded[0].why, Some(Unread::Unparsed(_))));
        assert_eq!(
            excluded[0].resting,
            [seal_hash(&first), seal_hash(&second)].into()
        );
        assert!(dag.interior().objects().is_empty());
        let findings = dag.verify(&crate::policy::Everything);
        assert!(matches!(&findings[0], Finding::Unread { name, .. } if *name == unread));
        assert!(matches!(&findings[1], Finding::Broken { at, why: Some(_) } if *at == unread));
    }
}
