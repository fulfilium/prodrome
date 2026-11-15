//! The record of a store's heads: the file `root/.heads` (SPEC §3).
//!
//! The record holds the tips of the store's history and the count of its
//! objects, as [`Heads::print`] prints them. [`super::EventStore::heads`]
//! reads it in place of a listing of `objects/`. The record is derived and
//! local to one machine: no decision reads it, and the store writes a
//! `.gitignore` that keeps it out of version control.
//!
//! The store removes the record, durably, before it changes `objects/`, and
//! writes it again when the holder of the lock lets the lock go. So a record
//! that a reader finds is never older than a change of the store's own: a
//! stop between the two steps leaves no record. The final write has no sync,
//! because a record that is lost, or cut short at any byte, does not parse,
//! and a reader then derives the heads.

use std::fs;
use std::path::Path;

use super::{put_durably, rename_into, staged, sync_dir};
use crate::dag::Heads;
use crate::literal::ProdromeError;

/// The record's file name in the store's root.
pub(super) const FILE: &str = ".heads";

/// What the store writes as its own `.gitignore`, where there is none.
const GITIGNORE: &str = "/.heads\n";

/// What a holder of the store's lock did to the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Record {
    /// Nothing was placed under this hold, and the record is as it was.
    Untouched,
    /// The record is removed, durably, and a placement can be cut short. A
    /// hold in this state at its end leaves no record.
    Removed,
    /// The record is removed, and every placement of this hold is done and
    /// in memory.
    Due,
}

impl Record {
    /// The state before a placement: the record removed, and the root
    /// synced, also when the file is already absent, because a host can
    /// delete it without a sync. Removed once for each hold of the lock.
    ///
    /// # Errors
    ///
    /// The record not removed, or the root not synced. The caller then
    /// places nothing.
    pub(super) fn remove(self, root: &Path) -> Result<Record, ProdromeError> {
        match self {
            Record::Untouched => {
                let path = root.join(FILE);
                match fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(ProdromeError::io(&path, &error)),
                }
                sync_dir(root)?;
                Ok(Record::Removed)
            }
            Record::Removed | Record::Due => Ok(Record::Removed),
        }
    }

    /// The state after a placement that succeeds. A hold whose earlier
    /// placement failed stays [`Record::Removed`], because the objects of
    /// the failed placement can be on disk and not in memory.
    pub(super) fn placed(self) -> Record {
        match self {
            Record::Untouched | Record::Due => Record::Due,
            Record::Removed => Record::Removed,
        }
    }

    /// Let the lock go: record `heads` where this hold owes it. An
    /// untouched record is rewritten only when it is not `heads`, which
    /// ends the lag of a record that another door made stale.
    ///
    /// # Errors
    ///
    /// The record or the `.gitignore` not written. The record is then
    /// absent, or the earlier record.
    pub(super) fn finish(self, root: &Path, heads: &Heads) -> Result<(), ProdromeError> {
        match self {
            Record::Untouched if read(root).as_ref() == Some(heads) => Ok(()),
            Record::Untouched | Record::Due => write(root, heads),
            Record::Removed => Ok(()),
        }
    }
}

/// The record in `root`, if it is there and parses.
pub(super) fn read(root: &Path) -> Option<Heads> {
    let text = fs::read_to_string(root.join(FILE)).ok()?;
    Heads::parse(&text).ok()
}

/// `heads` as the record of `root`: the `.gitignore` first, durably, where
/// there is none; then the record, staged and renamed in, with no sync.
fn write(root: &Path, heads: &Heads) -> Result<(), ProdromeError> {
    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        put_durably(&gitignore, GITIGNORE.as_bytes())?;
    }
    let (temp, _) = staged(root, heads.print().as_bytes())?;
    rename_into(&temp, &root.join(FILE))
}
