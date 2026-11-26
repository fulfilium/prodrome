//! Who may change a store's `objects/` (SPEC §3, "The doors a store has").

/// Who may change a store's `objects/`, as the program that opens the store
/// declares; it decides only what a decision reads, and changes no stored
/// byte (SPEC law 86).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Doors {
    /// Other programs may change `objects/`, so a decision lists it.
    #[default]
    Shared,
    /// Nothing but this library, under this store's lock, writes
    /// `objects/`, so a decision lists only where the mark of the last hold
    /// is not the one its handle keeps.
    ///
    /// The program declares this for every process that opens the
    /// directory. A `git merge`, a copy of files, a person, or an older
    /// binary breaks the declaration. A decision then answers from a history
    /// that lacks the objects they add, until a handle that lists holds the
    /// lock (SPEC law 85).
    Alone,
}
