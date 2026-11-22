//! Who may change a store's `objects/` (SPEC §3, "The doors a store has").

/// Who may change a store's `objects/`, as the program that opens the store
/// declares.
///
/// It decides only what a decision reads ([`super::EventStore::decide`], and
/// every write). It changes no stored byte, so a program can open the same
/// store either way (SPEC law 86).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Doors {
    /// Other programs may change `objects/`: a `git merge`, a copy of
    /// files, a second binary of another version. A decision lists
    /// `objects/`, so it reads every door.
    #[default]
    Shared,
    /// Nothing but this library, under this store's lock, writes
    /// `objects/`. A decision trusts the mark of the last hold, as a read
    /// does, and lists only where the mark is not the one its handle keeps.
    ///
    /// The program declares this for every process that opens the
    /// directory, not only for itself. A `git merge`, a copy of files, a person, or a
    /// binary of a version before the mark of the last hold is a door, and
    /// breaks it. Where it is broken, an object that arrives by the door is
    /// not read as history by a handle that has read the store, also under
    /// that handle's own holds: a decision answers from a history that
    /// lacks the object, and the store's own hold writes a record of heads
    /// that lacks it. That lasts until a handle that lists holds the lock:
    /// a fresh handle's write, `record_heads()` or `fsck`. Meanwhile
    /// `verify` reports the record of heads as stale, and a fresh handle
    /// reads the object, so nothing is lost (SPEC law 85).
    Alone,
}
