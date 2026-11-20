//! A coordinated decision (design §6, §6.1): a non-monotone question asked
//! of the replica of record under its lock, and the record of the answer
//! written through, before anything is performed.

use std::fs::{self, File};
use std::sync::{Arc, MutexGuard};
use std::time::SystemTime;

use super::husk::{HuskObject, Husking};
use super::memory::{Memory, Printed, Seen, Verified};
use super::record::{Record, Removal};
use super::{rename_into, stage, sync_dir, EventStore};
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
    pub(super) _locked: Option<File>,
}

/// The lock is let go after the record of heads is written, since the
/// fields drop after this. A record that cannot be written is left absent,
/// which every reader reads as "derive": the objects of the hold are in the
/// store, so the hold's writes stand.
impl<E: Schema, Pol> Drop for Decision<'_, E, Pol> {
    fn drop(&mut self) {
        let _ = self.record.finish(&self.store.root, &self.memory.heads());
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

    /// One event of a run decided over the memory, and its name answered. A
    /// new change is admitted to the memory, so the next event sees it, and
    /// its print joins what `run` places. Before the first, the record of
    /// heads is removed, so that the memory is never ahead of the disk while
    /// a record is due.
    fn decide(&mut self, run: &mut Run, event: E) -> Result<Hash, ProdromeError> {
        let (name, sealed) = self.memory.change(self.store.genesis.as_ref(), event)?;
        match sealed {
            // A name the history holds, as a husk, is that object: its whole
            // is never written back (SPEC §3).
            Some(_) if self.memory.holds(&name) => {}
            // A file held outside the history under the name holds other
            // bytes: a sha256 collision, and the run stops rather than
            // clobber them.
            Some(_) if self.memory.files().contains_key(&name) => {
                return Err(ProdromeError::Store(format!(
                    "object {} exists with different bytes — sha256 collision or corruption",
                    name.as_str()
                )));
            }
            Some((verified, print)) => {
                match run {
                    Run::Nothing => {
                        let removal = self.record.remove(&self.store.root)?;
                        self.record = removal.placing();
                        *run = Run::Placing(removal, vec![(name.clone(), print)]);
                    }
                    Run::Placing(_, prints) => prints.push((name.clone(), print)),
                }
                self.memory.admit(&[], vec![(verified, None)]);
            }
            None => {}
        }
        Ok(name)
    }

    /// The prints of `run` onto disk, with one directory sync, and each
    /// file then seen in the memory. The record of heads is due after.
    fn place_run(&mut self, run: Run) -> Result<(), ProdromeError> {
        match run {
            Run::Nothing => Ok(()),
            Run::Placing(removal, prints) => {
                self.store
                    .place(prints.iter().map(|(name, print)| (name, print.as_slice())))?;
                self.reseen(prints.iter().map(|(name, _)| name));
                self.record = removal.placed();
                Ok(())
            }
        }
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
            match stage(&dir, husk.print().as_bytes()).and_then(|temp| rename_into(&temp, &path)) {
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
    /// One object onto disk, content-addressed and idempotent, and into the
    /// memory, which has looked under this lock: an object it holds is a
    /// no-op while its file is there, since the look found it holding its
    /// bytes. DIFFERENT bytes already at the name are a sha256 collision or
    /// a corrupt store, and the write stops rather than clobber them. The
    /// object is verified because it was sealed here, so nothing written is
    /// read back.
    pub(super) fn write(&mut self, (verified, text): Printed<E>) -> Result<Hash, ProdromeError> {
        let digest = verified.name().clone();
        let path = self.store.object_path(&digest);
        if self.memory.holds(&digest) && path.exists() {
            return Ok(digest);
        }
        let print = if path.exists() {
            let existing = fs::read(&path).map_err(|e| ProdromeError::io(&path, &e))?;
            if existing != text {
                return Err(ProdromeError::Store(format!(
                    "object {} exists with different bytes — sha256 collision or corruption",
                    digest.as_str()
                )));
            }
            None
        } else {
            Some(text)
        };
        self.place(vec![(verified, print)])?;
        Ok(digest)
    }

    /// Each object onto disk where it has a print to place, parents first
    /// in the order given ([`EventStore::place`]), and then every object
    /// into the memory, with its file as it is now seen. Before the first
    /// placement of the hold, the record of heads is removed, durably, so
    /// that a stop at any point leaves no record older than the objects.
    pub(super) fn place(
        &mut self,
        taken: Vec<(Verified<E>, Option<Vec<u8>>)>,
    ) -> Result<(), ProdromeError> {
        let prints: Vec<(&Hash, &[u8])> = taken
            .iter()
            .filter_map(|(verified, print)| Some((verified.name(), print.as_deref()?)))
            .collect();
        if prints.is_empty() {
            self.admit(taken);
        } else {
            let removal = self.record.remove(&self.store.root)?;
            self.record = removal.placing();
            self.store.place(prints)?;
            self.admit(taken);
            self.record = removal.placed();
        }
        Ok(())
    }

    /// Every object into the memory, with its file as it is now seen.
    fn admit(&mut self, taken: Vec<(Verified<E>, Option<Vec<u8>>)>) {
        let names: Vec<Hash> = taken
            .iter()
            .map(|(verified, _)| verified.name().clone())
            .collect();
        self.memory.admit(
            &[],
            taken
                .into_iter()
                .map(|(verified, _)| (verified, None))
                .collect(),
        );
        self.reseen(&names);
    }

    /// Each of `names`, held, with its file as it is now seen.
    fn reseen<'n>(&mut self, names: impl IntoIterator<Item = &'n Hash>) {
        let now = SystemTime::now();
        for name in names {
            let seen = Seen::of(fs::metadata(self.store.object_path(name)).ok(), now);
            self.memory.reseen(name, seen);
        }
    }
}
