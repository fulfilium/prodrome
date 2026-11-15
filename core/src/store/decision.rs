//! A coordinated decision (design §6, §6.1): a non-monotone question asked
//! of the replica of record under its lock, and the record of the answer
//! written through, before anything is performed.

use std::fs::{self, File};
use std::sync::{Arc, MutexGuard};
use std::time::SystemTime;

use super::memory::{Memory, Printed, Seen, Verified};
use super::record::Record;
use super::EventStore;
use crate::dag::Dag;
use crate::event::Hash;
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
    /// is on disk when this returns.
    ///
    /// # Errors
    ///
    /// [`EventStore::append`]'s.
    pub fn append(&mut self, event: E) -> Result<Hash, ProdromeError> {
        let (name, sealed) = self.memory.change(self.store.genesis.as_ref(), event)?;
        match sealed {
            Some(sealed) => self.write(sealed),
            None => Ok(name),
        }
    }
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
        let now = SystemTime::now();
        let seen = taken
            .into_iter()
            .map(|(verified, _)| {
                let seen = Seen::of(
                    fs::metadata(self.store.object_path(verified.name())).ok(),
                    now,
                );
                (verified, seen)
            })
            .collect();
        self.memory.admit(&[], seen);
    }
}
