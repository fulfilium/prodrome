//! The doors through which prints reach a store's `objects/`, said when the
//! store is opened.

/// Who may change a store's `objects/`, as the program that opens the store
/// says. It decides one thing: what a decision reads
/// ([`super::EventStore::decide`], and every write). It changes no stored
/// byte, so a store moves between the two by being opened the other way.
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
    /// An object that arrives by a door all the same is not read as history
    /// by a handle that has read the store until a handle that lists holds
    /// the lock, and `verify` reports the record of heads as stale (SPEC
    /// law 85).
    Alone,
}
