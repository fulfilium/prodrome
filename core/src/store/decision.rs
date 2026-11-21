//! A coordinated decision (design §6, §6.1): a non-monotone question asked
//! of the replica of record under its lock, and the record of the answer
//! written through, before anything is performed.

use std::fs::{self, File};
use std::sync::{Arc, MutexGuard};
use std::time::SystemTime;

use super::husk::{HuskObject, Husking};
use super::memory::{Memory, Printed, Seen};
use super::record::{Record, Removal};
use super::{replace, sync_dir, EventStore};
use crate::dag::Dag;
use crate::event::{Envelope, Hash};
use crate::literal::ProdromeError;
use crate::policy::Policy;
use crate::registers::Folded;
use crate::schema::Schema;

/// THE STORE OF RECORD, LOCKED: [`EventStore::decide`]'s answer, and the
/// only value that can answer a question no monotone reading can ("is this
/// proposal still standing, so I may send?"), because while it lives no
/// writer of the store, in this process or another, finishes a write.
///
/// What it reads is everything any writer finished; what it appends is on
/// disk, synced, when the append returns (WRITTEN THROUGH). So a host
/// decides, appends the record of its decision, and only then performs the
/// effect: a crash at any point leaves either no record and no effect, or a
/// record a later decision reads. Dropping it lets the lock go, and
/// first writes the store's record of its heads (SPEC §3), once for the
/// whole hold.
///
/// Only [`EventStore`] makes one. A store in memory and an overlay hold no
/// lock any other writer takes and do not write through, so a decision
/// cannot be written against either: the CALM line (§6), as a type. Monotone
/// writes need none of this, and every replica appends them.
///
/// ```
/// # use prodrome::event::{mk_completed, TodoEvent, TodoId};
/// # use prodrome::literal::Datetime;
/// # use prodrome::policy::Untrusted;
/// # use prodrome::reference::Todo;
/// # use prodrome::store::EventStore;
/// # fn main() -> Result<(), prodrome::literal::ProdromeError> {
/// # let dir = std::env::temp_dir().join("prodrome-decision-doc");
/// # let _ = std::fs::remove_dir_all(&dir);
/// let store = EventStore::<TodoEvent<Todo>>::new(&dir, Untrusted::none());
/// store.init("decisions")?;
/// let at = Datetime::new(2026, 10, 1, 9, 0, 0, 0)?;
/// let todo = TodoId::new("send-it")?;
/// let mut decision = store.decide()?;
/// let unsent = !decision
///     .folded()?
///     .entities()
///     .any(|(key, _)| *key == todo);
/// if unsent {
///     decision.append(mk_completed("send-it", at, "host", "sent")?)?;
/// }
/// drop(decision);
/// // Only now is the effect performed: its record is on disk.
/// # let _ = std::fs::remove_dir_all(&dir);
/// # Ok(())
/// # }
/// ```
///
/// ```compile_fail
/// use prodrome::event::TodoEvent;
/// use prodrome::reference::Todo;
/// use prodrome::store::{EventStore, Overlay};
///
/// type Store = EventStore<TodoEvent<Todo>>;
/// fn send(overlay: &Overlay<TodoEvent<Todo>, Store>) {
///     // An overlay writes to memory: it has no lock to decide under.
///     let decision = overlay.decide();
/// }
/// ```
///
/// ```compile_fail
/// use prodrome::event::TodoEvent;
/// use prodrome::reference::Todo;
/// use prodrome::store::MemoryStore;
///
/// fn send(replica: &MemoryStore<TodoEvent<Todo>>) {
///     // A store in memory has no lock any other writer takes.
///     let decision = replica.decide();
/// }
/// ```
///
/// While it lives, read through it and not through the store: the store's
/// own reads wait for it, on this thread too.
pub struct Decision<'s, E: Schema, Pol> {
    pub(super) store: &'s EventStore<E, Pol>,
    pub(super) memory: MutexGuard<'s, Memory<E>>,
    /// What this hold did to the record of heads.
    pub(super) record: Record,
    /// The count of holds that the hold read when it began.
    pub(super) prior: Option<super::writes::Count>,
    pub(super) _locked: Option<File>,
}

/// The lock is let go after the record of heads is written, since the
/// fields drop after this. A record that cannot be written is left absent,
/// which every reader reads as "derive": the objects of the hold are in the
/// store, so the hold's writes stand.
impl<E: Schema, Pol> Drop for Decision<'_, E, Pol> {
    fn drop(&mut self) {
        let _ = self.record.finish(&self.store.root, &self.memory.heads());
        self.store.end_hold(self.prior);
    }
}

impl<E: Schema, Pol: Policy<E>> Decision<'_, E, Pol> {
    /// Every object the store holds, as of the lock.
    #[must_use]
    pub fn dag(&self) -> &Arc<Dag<E>> {
        self.memory.dag()
    }

    /// The fold of every object the store holds ([`EventStore::folded`]).
    ///
    /// # Errors
    ///
    /// A cycle among what is held, which only a hash collision could make.
    pub fn folded(&mut self) -> Result<&Arc<Folded<E>>, ProdromeError> {
        self.memory.folded()
    }

    /// [`EventStore::append`] under this lock, written through: the object
    /// is on disk when this returns. A run of one ([`Decision::append_all`]).
    ///
    /// # Errors
    ///
    /// [`EventStore::append`]'s.
    pub fn append(&mut self, event: E) -> Result<Hash, ProdromeError> {
        let mut run = Run::Nothing;
        let name = self.decide(&mut run, event)?;
        self.place_run(run)?;
        Ok(name)
    }

    /// A RUN OF APPENDS, decided over this one look and placed at its end.
    /// Each event is decided as [`Decision::append`] decides it, over the
    /// memory that holds every change decided before it, so the run writes
    /// the objects that one append after another writes. The new objects
    /// are placed once every event is decided, in the order decided, with
    /// one directory sync. Answers each event's name: the change written, or
    /// the twin that stands for it.
    ///
    /// # Errors
    ///
    /// [`Stopped`] at the first event refused, with the names of the events
    /// decided before it, which are placed; or where the placement fails,
    /// with no names, and the objects renamed into place before the failure
    /// are in the store, as an adoption cut short is.
    pub fn append_all(
        &mut self,
        events: impl IntoIterator<Item = E>,
    ) -> Result<Vec<Hash>, Stopped> {
        let mut run = Run::Nothing;
        let mut names = Vec::new();
        let mut refusal = None;
        for event in events {
            match self.decide(&mut run, event) {
                Ok(name) => names.push(name),
                Err(why) => {
                    refusal = Some(why);
                    break;
                }
            }
        }
        self.place_run(run).map_err(|why| Stopped {
            placed: Vec::new(),
            why,
        })?;
        match refusal {
            None => Ok(names),
            Some(why) => Err(Stopped { placed: names, why }),
        }
    }

    /// One event of a run decided over the memory, and its name answered:
    /// the change, [`Decision::take`]n into `run`, or the twin that stands
    /// for it.
    fn decide(&mut self, run: &mut Run, event: E) -> Result<Hash, ProdromeError> {
        let (name, sealed) = self.memory.change(self.store.genesis.as_ref(), event)?;
        if let Some(printed) = sealed {
            self.take(run, printed)?;
        }
        Ok(name)
    }

    /// One object, sealed here or received, into `run`. Where the store
    /// keeps its name, in the history or waiting outside it, nothing is
    /// taken when what it keeps is the same object: the same bytes, or a
    /// husk of it, whose whole is never written back (SPEC §3). Other bytes
    /// under the name are a sha256 collision, and the object is refused
    /// rather than clobber them.
    /// Otherwise it is admitted to the memory, so that what is decided next
    /// sees it, and its print joins what `run` places. Before the first, the
    /// record of heads is removed, so that the memory is never ahead of the
    /// disk while a record is due.
    fn take(&mut self, run: &mut Run, (verified, print): Printed<E>) -> Result<(), ProdromeError> {
        let name = verified.name().clone();
        if self.memory.holds(&name) {
            return Ok(());
        }
        if self.memory.keeps(&name) {
            // Held outside the history, waiting for a parent: its husk, or
            // the same bytes, are already placed, and other bytes are a
            // collision.
            if self.memory.dag().husks().contains(&name) {
                return Ok(());
            }
            let path = self.store.object_path(&name);
            let held = fs::read(&path).map_err(|e| ProdromeError::io(&path, &e))?;
            if held == print {
                return Ok(());
            }
            return Err(ProdromeError::Store(format!(
                "object {} exists with different bytes — sha256 collision or corruption",
                name.as_str()
            )));
        }
        match run {
            Run::Nothing => {
                let removal = self.record.remove(&self.store.root)?;
                self.record = removal.placing();
                *run = Run::Placing(removal, vec![(name, print)]);
            }
            Run::Placing(_, prints) => prints.push((name, print)),
        }
        self.memory.admit(&[], vec![(verified, None)]);
        Ok(())
    }

    /// The prints of `run` onto disk, in order, with one directory sync
    /// ([`EventStore::place`]), and each file then seen in the memory; the
    /// record of heads is due after. A placement that fails leaves the
    /// memory as a cold read of the files reads them (SPEC law 79): what the
    /// run admitted is forgotten, and the files are looked at again, since
    /// the objects renamed into place before the failure are in the store.
    /// The record stays removed for the rest of the hold.
    ///
    /// # Errors
    ///
    /// The placement's. Where `objects/` then cannot be listed either, the
    /// refusal names both, and the memory holds what it held before the run
    /// until the next look.
    fn place_run(&mut self, run: Run) -> Result<(), ProdromeError> {
        match run {
            Run::Nothing => Ok(()),
            Run::Placing(removal, prints) => {
                let placed = self
                    .store
                    .place(prints.iter().map(|(name, print)| (name, print.as_slice())));
                match placed {
                    Ok(()) => {
                        self.reseen(prints.into_iter().map(|(name, _)| name));
                        self.record = removal.placed();
                        Ok(())
                    }
                    Err(why) => {
                        self.forget(prints);
                        match self.store.look(&mut self.memory) {
                            Ok(()) => Err(why),
                            Err(looked) => Err(ProdromeError::Store(format!(
                                "{why}; and objects/ was not listed after it: {looked}"
                            ))),
                        }
                    }
                }
            }
        }
    }

    /// What `run` admitted, forgotten, and nothing placed: a run given up
    /// before its placement.
    fn give_up(&mut self, run: Run) {
        match run {
            Run::Nothing => {}
            Run::Placing(_, prints) => self.forget(prints),
        }
    }

    /// The objects of a run's `prints`, admitted to the memory, forgotten.
    fn forget(&mut self, prints: Vec<(Hash, Vec<u8>)>) {
        let names: Vec<Hash> = prints.into_iter().map(|(name, _)| name).collect();
        self.memory.admit(&names, Vec::new());
    }

    /// [`EventStore::husk`] under this lock: each object `names` names that
    /// the history holds whole, as a `Change` whose event holds a sealed
    /// part, replaced by its husk, file by file, and in the memory. The
    /// husk is made from the object in memory, which the look under this
    /// lock verified against its name. Every other name is passed over.
    /// Answers the names husked.
    ///
    /// The heads do not change, so the record of heads stays true at every
    /// stop, and is not removed.
    ///
    /// # Errors
    ///
    /// A file not written or not renamed. The files renamed before it are
    /// husks, and the memory holds them so.
    pub fn husk(
        &mut self,
        names: impl IntoIterator<Item = Hash>,
    ) -> Result<Vec<Hash>, ProdromeError>
    where
        E: Husking,
    {
        let dag = self.memory.dag();
        let husks: Vec<(Hash, HuskObject<E>)> = names
            .into_iter()
            .filter(|name| !dag.husks().contains(name))
            .filter_map(|name| {
                let husk = match dag.get(&name)? {
                    Envelope::Change(change) => HuskObject::of(change),
                    Envelope::Sealed { .. }
                    | Envelope::Woven { .. }
                    | Envelope::Genesis(_)
                    | Envelope::Snapshot(_)
                    | Envelope::Signed(_)
                    | Envelope::KeyAdded(_)
                    | Envelope::KeyRevoked(_) => None,
                }?;
                Some((name, husk))
            })
            .collect();
        let dir = self.store.objects_dir();
        let mut done = Vec::new();
        let mut failed = None;
        for (name, husk) in husks {
            let path = self.store.object_path(&name);
            match replace(&dir, &path, husk.print().as_bytes()) {
                Ok(()) => done.push((name, husk)),
                Err(why) => {
                    failed = Some(why);
                    break;
                }
            }
        }
        let synced = if done.is_empty() {
            Ok(())
        } else {
            sync_dir(&dir)
        };
        let names = done.iter().map(|(name, _)| name.clone()).collect();
        let now = SystemTime::now();
        let husked = done
            .into_iter()
            .map(|(name, husk)| {
                let seen = Seen::of(fs::metadata(self.store.object_path(&name)).ok(), now);
                (name, husk, seen)
            })
            .collect();
        self.memory.husk(husked);
        match failed {
            Some(why) => Err(why),
            None => synced.map(|()| names),
        }
    }
}

/// What a run of appends places: nothing yet, or, once the record of heads
/// is removed, the print of each change decided, in order.
enum Run {
    Nothing,
    Placing(Removal, Vec<(Hash, Vec<u8>)>),
}

/// A run of appends that stopped ([`Decision::append_all`]): the names of
/// the events decided and placed before it stopped, and why it stopped.
#[derive(Debug, Clone, PartialEq)]
pub struct Stopped {
    pub placed: Vec<Hash>,
    pub why: ProdromeError,
}

impl<E: Schema, Pol: Policy<E>> Decision<'_, E, Pol> {
    /// One object sealed here onto disk and into the memory, as a run of
    /// one takes and places it ([`Decision::take`]): content-addressed and
    /// idempotent, and nothing is read back.
    pub(super) fn write(&mut self, printed: Printed<E>) -> Result<Hash, ProdromeError> {
        let name = printed.0.name().clone();
        self.place(vec![printed])?;
        Ok(name)
    }

    /// Each object onto disk, parents first in the order given, and into
    /// the memory, as a run takes and places them. One that is refused
    /// gives the run up: nothing is placed, and the memory forgets what it
    /// admitted.
    pub(super) fn place(&mut self, taken: Vec<Printed<E>>) -> Result<(), ProdromeError> {
        let mut run = Run::Nothing;
        for printed in taken {
            if let Err(why) = self.take(&mut run, printed) {
                self.give_up(run);
                return Err(why);
            }
        }
        self.place_run(run)
    }

    /// Each of `names`, held, with its file as it is now seen.
    fn reseen(&mut self, names: impl IntoIterator<Item = Hash>) {
        let now = SystemTime::now();
        for name in names {
            let seen = Seen::of(fs::metadata(self.store.object_path(&name)).ok(), now);
            self.memory.reseen(&name, seen);
        }
    }
}
