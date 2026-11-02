//! What a store has read (design §6.1): its objects, each verified once, and
//! their fold, extended by the objects it has not read yet and never rebuilt
//! from the files.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::change::mk_change;
use crate::dag::{decode, Dag, Unread};
use crate::event::{canonical, canonical_envelope, parents_of, Envelope, Hash};
use crate::literal::{Datetime, ProdromeError};
use crate::registers::{deps_for, fold, Folded, Genesis};
use crate::schema::Schema;

/// A store's objects as it last read them, and their fold.
///
/// VERIFIED, NOT TRUSTED FOR BEING HELD. An object enters only as
/// [`Verified`]: its bytes rehashed to its name, or sealed here. Its FILE is
/// only held: [`Seen`] is how it looked then, and a file that no longer looks
/// so is read and verified again before anything is read from memory.
///
/// The fold is the fold of the objects, always, taken the first time it is
/// asked for: an object arriving after that is [`Folded::insert`]ed where the
/// linearisation of the union puts it, and what insertion cannot do (an
/// object gone, one that something held names, one resting on another scope)
/// sets the fold aside, to be taken again from memory, never from the files.
///
/// ITS HISTORY IS THE LARGEST DOWN-SET OF WHAT IT HOLDS ([`Dag::interior`]).
/// An object resting on one it lacks (set aside, or not arrived yet) is held
/// and verified, and WAITS: it is in no reading, no tip and no fold until
/// what it rests on arrives, and then it joins as any arrival does.
///
/// A PRINT THAT IS NOT AN OBJECT IS HELD AS UNREAD, beside the objects, in
/// the same [`Dag`]: what rests on it waits, and [`Dag::excluded`] names it
/// with why and with what waits on it, as it names it for any replica.
///
/// INCREMENTAL EQUALS COLD: after any looks, admissions and forgettings, a
/// memory is what one fresh look at the same files makes, its DAG the
/// files' `interior()`, its fold `fold(&dag.nodes()?)`, its tips
/// `dag.tips()` and its geneses `dag.geneses()` (`core/tests/all/memory.rs`).
pub struct Memory<E: Schema> {
    /// Every print held: the history, and the objects waiting outside it.
    dag: Arc<Dag<E>>,
    folded: Option<Arc<Folded<E>>>,
    tips: BTreeSet<Hash>,
    /// What a held object names (a parent, a genesis) that is not held.
    wanted: BTreeSet<Hash>,
    /// Each held object's file as it was seen, `None` while a look could
    /// not tell a change from none.
    files: BTreeMap<Hash, Option<Seen>>,
    /// The objects whose events name each entity at each instant, which
    /// two events that print the same do: the twin check prints these and
    /// no other.
    twins: BTreeMap<(E::Key, Datetime), BTreeSet<Hash>>,
    /// Each held `Genesis`, and the least legacy root: [`Dag::geneses`].
    geneses: BTreeSet<Hash>,
}

/// An object with the print it is named by: what a replica places, sends
/// and holds.
pub type Printed<E> = (Verified<E>, Vec<u8>);

/// An object whose name is the hash of its bytes, which is the only way into
/// a [`Memory`].
pub struct Verified<E> {
    name: Hash,
    object: Envelope<E>,
}

impl<E: Schema> Verified<E> {
    /// An object sealed here, and the print it is named by, which is what
    /// its file holds.
    pub fn sealed(object: Envelope<E>) -> Printed<E> {
        let print = canonical_envelope(&object).into_bytes();
        let name = Hash::of_bytes(&print);
        (Verified { name, object }, print)
    }

    pub fn name(&self) -> &Hash {
        &self.name
    }

    pub fn object(&self) -> &Envelope<E> {
        &self.object
    }

    /// `bytes` under `name`, rehashed and parsed at the schema `schema`.
    pub fn read(schema: &E::Vocabulary, name: &Hash, bytes: &[u8]) -> Result<Verified<E>, Unread> {
        decode(schema, name, bytes).map(|object| Verified {
            name: name.clone(),
            object,
        })
    }
}

/// How long after a file changed at `modified` its `stat` may still not tell
/// a second change from none: a scheduler tick, where timestamps carry
/// fractions of a second, and FAT's two seconds where they do not (ext3 and
/// HFS+ keep one).
fn racy(modified: SystemTime) -> Duration {
    let fine = modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .is_ok_and(|since| since.subsec_nanos() != 0);
    if fine {
        Duration::from_millis(50)
    } else {
        Duration::from_secs(2)
    }
}

/// A file as `stat` saw it: while it looks the same it holds the same bytes,
/// the test git's index makes of a worktree. Its size, its times and, on
/// unix, its device, inode and change time, which no writer sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Seen {
    /// A file as `meta` says it looks at `now`, or `None` where that says
    /// nothing: it could not be read, or it changed within [`racy`] of
    /// `now`, so a change after this look could leave it looking the same
    /// (git's "racily clean"). A file seen as `None` is read again at the
    /// next look.
    pub fn of(meta: Option<fs::Metadata>, now: SystemTime) -> Option<Seen> {
        let meta = meta?;
        let modified = meta.modified().ok();
        let settled =
            modified.is_some_and(|at| now.duration_since(at).is_ok_and(|age| age >= racy(at)));
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
        };
        settled.then_some(Seen {
            len: meta.len(),
            modified,
            #[cfg(unix)]
            identity,
        })
    }
}

impl<E: Schema> Memory<E> {
    pub fn empty() -> Memory<E> {
        Memory {
            dag: Arc::new(Dag::from_iter([])),
            folded: None,
            tips: BTreeSet::new(),
            wanted: BTreeSet::new(),
            files: BTreeMap::new(),
            twins: BTreeMap::new(),
            geneses: BTreeSet::new(),
        }
    }

    /// A memory holding `dag`, whose fold is `folded`: another replica's,
    /// shared, and copied only when this one is extended.
    pub fn over(dag: Arc<Dag<E>>, folded: Arc<Folded<E>>) -> Memory<E> {
        let mut twins: BTreeMap<(E::Key, Datetime), BTreeSet<Hash>> = BTreeMap::new();
        for (name, event) in dag
            .objects()
            .iter()
            .filter_map(|(name, object)| Some((name, object.event()?)))
        {
            twins
                .entry((event.key().clone(), event.at()))
                .or_default()
                .insert(name.clone());
        }
        Memory {
            tips: dag.tips(),
            geneses: dag.geneses(),
            wanted: dag
                .objects()
                .values()
                .flat_map(parents_of)
                .filter(|parent| dag.get(parent).is_none())
                .collect(),
            files: dag.held().map(|(name, _)| (name.clone(), None)).collect(),
            twins,
            folded: Some(folded),
            dag,
        }
    }

    pub fn dag(&self) -> &Arc<Dag<E>> {
        &self.dag
    }

    /// The genesis a writer writes into: `chosen`, which must be held, or
    /// else the only one.
    ///
    /// # Errors
    ///
    /// `chosen` not held; none held; several held and none chosen.
    pub fn writer(&self, chosen: Option<&Hash>) -> Result<Hash, ProdromeError> {
        let mut sole = self.geneses.iter();
        match (chosen, sole.next(), sole.next()) {
            (Some(mine), ..) if self.geneses.contains(mine) => Ok(mine.clone()),
            (Some(mine), ..) => Err(ProdromeError::Store(format!(
                "the store holds no genesis {}",
                mine.as_str()
            ))),
            (None, Some(one), None) => Ok(one.clone()),
            (None, None, _) => Err(ProdromeError::Store(
                "the store has no genesis: `init` it first".to_owned(),
            )),
            (None, Some(_), Some(_)) => Err(ProdromeError::Store(format!(
                "the store holds {} geneses: name the writer's with `in_genesis`",
                self.geneses.len()
            ))),
        }
    }

    /// WHAT AN APPEND OF `event` WRITES, decided on what is held and the
    /// same for every replica (SPEC §3): its name, and the object sealed
    /// with its print, or no object where the writer's prodrome holds a
    /// twin, whose name it is.
    ///
    /// # Errors
    ///
    /// [`Memory::writer`]'s, and the append's refusals: an inflationary
    /// register written below its reading, a change no constructor makes.
    pub fn change(
        &mut self,
        writer: Option<&Hash>,
        event: E,
    ) -> Result<(Hash, Option<Printed<E>>), ProdromeError> {
        let genesis = self.writer(writer)?;
        let prodrome = self.dag.key(&genesis);
        if let Some(twin) = self.twin(&prodrome, &event)? {
            return Ok((twin, None));
        }
        let deps = deps_for(self.folded()?, &prodrome, &event)?;
        let (verified, print) =
            Verified::sealed(Envelope::Change(mk_change(genesis, deps, event)?));
        Ok((verified.name.clone(), Some((verified, print))))
    }

    /// The fold of the history: taken from memory the first time, and
    /// extended since.
    ///
    /// # Errors
    ///
    /// None the history can make, which is closed under parents and has no
    /// cycle; the type is the fold's.
    pub fn folded(&mut self) -> Result<&Arc<Folded<E>>, ProdromeError> {
        if self.folded.is_none() {
            self.folded = Some(Arc::new(fold(&self.dag.nodes()?)));
        }
        Ok(self.folded.as_ref().expect("folded above"))
    }

    /// The first object, in the linearisation's order, of the prodrome
    /// `prodrome` whose event prints as `event` does: what an append of
    /// `event` answers instead of writing.
    ///
    /// # Errors
    ///
    /// [`Memory::folded`]'s.
    pub fn twin(&mut self, prodrome: &Genesis, event: &E) -> Result<Option<Hash>, ProdromeError> {
        let print = canonical(event);
        let twins: BTreeSet<Hash> = self
            .twins
            .get(&(event.key().clone(), event.at()))
            .into_iter()
            .flatten()
            .filter(|name| {
                self.dag
                    .get(name)
                    .and_then(Envelope::event)
                    .is_some_and(|held| canonical(held) == print)
            })
            .cloned()
            .collect();
        if twins.is_empty() {
            return Ok(None);
        }
        Ok(self
            .folded()?
            .prodromes()
            .get(prodrome)
            .and_then(|entities| entities.get(event.key()))
            .and_then(|stream| stream.iter().find(|stamp| twins.contains(&stamp.name)))
            .map(|stamp| stamp.name.clone()))
    }

    /// Is `name` in the history? A waiting object is not, so a walk that
    /// stops at what is held walks on through it to what it waits for.
    pub fn holds(&self, name: &Hash) -> bool {
        self.dag.get(name).is_some()
    }

    pub fn tips(&self) -> &BTreeSet<Hash> {
        &self.tips
    }

    /// Every prodrome's genesis, as [`Dag::geneses`] answers it.
    pub fn geneses(&self) -> &BTreeSet<Hash> {
        &self.geneses
    }

    /// The held names, each with its file as it was seen.
    pub fn files(&self) -> &BTreeMap<Hash, Option<Seen>> {
        &self.files
    }

    /// A held object's file, read again and found to hold its bytes, as it
    /// looks now.
    fn reseen(&mut self, name: &Hash, seen: Option<Seen>) {
        if let Some(held) = self.files.get_mut(name) {
            *held = seen;
        }
    }

    /// Level with a look at the files: `gone`, the held names no longer
    /// listed, forgotten; `read`, every listed file not held as it looks
    /// now, read by [`Dag::from_prints`], each with its file as it was seen
    /// before it was read (`seen`). An object read under a held name is that
    /// file seen again; one read under a new name is [`Memory::admit`]ted. A
    /// held name read as no object leaves the history, and what rests on it
    /// with it.
    ///
    /// The unread prints held are `read`'s: a file that is no object is
    /// never held as one, so every look reads it again, and its repair (its
    /// bytes restored, the file set aside) is seen at the next.
    pub fn level(
        &mut self,
        mut gone: Vec<Hash>,
        read: Dag<E>,
        seen: &BTreeMap<Hash, Option<Seen>>,
    ) {
        let (objects, unread) = read.into_prints();
        gone.extend(
            unread
                .keys()
                .filter(|name| self.files.contains_key(*name))
                .cloned(),
        );
        let mut fresh = Vec::new();
        for (name, object) in objects {
            let at = seen.get(&name).cloned().flatten();
            if self.files.contains_key(&name) {
                self.reseen(&name, at);
            } else {
                fresh.push((Verified { name, object }, at));
            }
        }
        self.admit(&gone, fresh);
        if self.dag.unread() != &unread {
            Arc::make_mut(&mut self.dag).set_unread(unread);
        }
    }

    /// Forget `gone` and take in `fresh`, each with its file as it was seen
    /// before it was read. What joins the history is what [`Dag::grow`]
    /// takes: the fresh objects and the waiting ones that now rest only on
    /// the history. The tips, the geneses and the fold follow, each joining
    /// object in turn, parents first: the fold by [`Folded::insert`]. Where
    /// that cannot say (an object gone, one a held object names, one resting
    /// on another scope), the history is grown again from nothing, the tips
    /// and the geneses taken again from it, and the fold set aside until it
    /// is asked for.
    pub fn admit(&mut self, gone: &[Hash], fresh: Vec<(Verified<E>, Option<Seen>)>) {
        if gone.is_empty() && fresh.is_empty() {
            return;
        }
        // A memory that held nothing derives its tips and geneses from the
        // history in one pass, which costs less than placing every object
        // in turn.
        let extends = gone.is_empty()
            && self.dag.held().next().is_some()
            && fresh
                .iter()
                .all(|(verified, _)| !self.wanted.contains(&verified.name));
        let dag = Arc::make_mut(&mut self.dag);
        for name in gone {
            let object = dag.forget(name);
            if let Some(event) = object.as_ref().and_then(Envelope::event) {
                if let Some(twins) = self.twins.get_mut(&(event.key().clone(), event.at())) {
                    twins.remove(name);
                }
            }
            self.files.remove(name);
        }
        let mut arrived = BTreeMap::new();
        for (Verified { name, object }, seen) in fresh {
            if let Some(event) = object.event() {
                self.twins
                    .entry((event.key().clone(), event.at()))
                    .or_default()
                    .insert(name.clone());
            }
            self.files.insert(name.clone(), seen);
            arrived.insert(name, object);
        }
        let names = dag.grow(arrived);
        if !gone.is_empty() {
            // What rested on a gone object leaves the history with it.
            dag.settle();
        }
        let placed = if extends {
            dag.order_among(&names.iter().collect()).ok()
        } else {
            None
        };
        let order = match placed {
            Some(order) => order,
            None => {
                self.folded = None;
                self.tips = dag.tips();
                self.geneses = dag.geneses();
                self.wanted = dag
                    .objects()
                    .values()
                    .flat_map(parents_of)
                    .filter(|parent| dag.get(parent).is_none())
                    .collect();
                return;
            }
        };
        for name in &order {
            let object = dag.get(name).expect("admitted above");
            for parent in parents_of(object) {
                self.tips.remove(&parent);
            }
            self.tips.insert(name.clone());
            self.wanted.extend(
                parents_of(object)
                    .into_iter()
                    .filter(|parent| dag.get(parent).is_none()),
            );
            match object {
                Envelope::Genesis(_) => {
                    self.geneses.insert(name.clone());
                }
                // A legacy root: `geneses` keeps the least.
                Envelope::Sealed { prev: None, .. } => {
                    let root = self.geneses.iter().find(|held| dag.is_root(held)).cloned();
                    if root.as_ref().is_none_or(|root| name < root) {
                        self.geneses.insert(name.clone());
                        if let Some(root) = root {
                            self.geneses.remove(&root);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(folded) = &mut self.folded {
            let folded = Arc::make_mut(folded);
            let placed = order
                .iter()
                .filter_map(|name| dag.node(name))
                .all(|node| folded.insert(&node).is_ok());
            if !placed {
                self.folded = None;
            }
        }
    }
}

/// WHAT A REPLICA LACKS of what `seeds` rest on, VERIFIED ON RECEIPT: a
/// walk down from `seeds` through what each object names (its parents and
/// its genesis), ending each branch at an object `held` holds, since a
/// replica is closed under parents, and reading every other one by
/// `print_of` and rehashing it against its name. Parents first, so every
/// prefix of the answer, placed, is closed under parents.
///
/// # Errors
///
/// A name `print_of` has no print for ("missing object …, named as a
/// parent"), and a print that does not hash to its name or does not parse
/// at the schema `schema`. Nothing is answered unless everything verified.
pub fn receive<E: Schema>(
    schema: &E::Vocabulary,
    held: impl Fn(&Hash) -> bool,
    seeds: impl IntoIterator<Item = Hash>,
    print_of: impl Fn(&Hash) -> Option<Vec<u8>>,
) -> Result<Vec<Printed<E>>, ProdromeError> {
    /// A depth-first walk, emitting an object once its parents are.
    enum Visit<E> {
        Enter(Hash),
        Leave(Verified<E>, Vec<u8>),
    }
    let mut entered: BTreeSet<Hash> = BTreeSet::new();
    let mut taken = Vec::new();
    let mut pending: Vec<Visit<E>> = seeds.into_iter().map(Visit::Enter).collect();
    while let Some(visit) = pending.pop() {
        let name = match visit {
            Visit::Leave(verified, print) => {
                taken.push((verified, print));
                continue;
            }
            Visit::Enter(name) => name,
        };
        if !entered.insert(name.clone()) || held(&name) {
            continue;
        }
        let Some(print) = print_of(&name) else {
            return Err(ProdromeError::Store(format!(
                "missing object {}, named as a parent",
                name.as_str()
            )));
        };
        let verified = Verified::read(schema, &name, &print).map_err(|why| why.refusal(&name))?;
        let names = parents_of(verified.object());
        pending.push(Visit::Leave(verified, print));
        pending.extend(names.into_iter().map(Visit::Enter));
    }
    Ok(taken)
}

/// Counts, not contents: a memory is the whole store.
impl<E: Schema> fmt::Debug for Memory<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memory")
            .field("objects", &self.files.len())
            .field("tips", &self.tips.len())
            .finish_non_exhaustive()
    }
}
