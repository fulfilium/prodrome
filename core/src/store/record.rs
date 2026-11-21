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
//! stop between the two steps leaves no record. The final write has no sync
//! and no temp file: the store unlinks any record that is there and creates
//! the file anew, and a record that is lost, or cut short at any byte, does
//! not parse, so a reader then derives the heads.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use super::{put_durably, sync_dir};
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
    /// The record is removed, and a placement is under way or was cut
    /// short. A hold in this state at its end leaves no record.
    Placing(Removal),
    /// The record is removed, and every placement of this hold is done and
    /// in memory.
    Due(Removal),
}

/// The record removed under this hold, with the root synced. Only
/// [`Record::remove`] makes one, so no state that owes a record exists
/// without a removal before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Removal(Owes);

/// What a removal owes when its placement succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owes {
    /// The record of the hold's heads.
    Record,
    /// Nothing: an earlier placement of the hold was cut short, so its
    /// objects can be on disk and not in memory.
    Nothing,
}

impl Record {
    /// Remove the record before a placement, and sync the root, also when
    /// the file is already absent, because a host can delete it without a
    /// sync. The record is removed once for each hold of the lock.
    ///
    /// # Errors
    ///
    /// The record not removed, or the root not synced. The caller then
    /// places nothing.
    pub(super) fn remove(self, root: &Path) -> Result<Removal, ProdromeError> {
        match self {
            Record::Untouched => {
                unlink(root)?;
                sync_dir(root)?;
                Ok(Removal(Owes::Record))
            }
            Record::Due(removal) => Ok(removal),
            Record::Placing(_) => Ok(Removal(Owes::Nothing)),
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
            Record::Untouched | Record::Due(_) => write(root, heads),
            Record::Placing(_) => Ok(()),
        }
    }
}

impl Removal {
    /// The state while the placement runs.
    pub(super) fn placing(self) -> Record {
        Record::Placing(self)
    }

    /// The state after the placement succeeds.
    pub(super) fn placed(self) -> Record {
        match self.0 {
            Owes::Record => Record::Due(self),
            Owes::Nothing => Record::Placing(self),
        }
    }
}

/// The record in `root`, if it is there and parses.
pub(super) fn read(root: &Path) -> Option<Heads> {
    let text = fs::read_to_string(root.join(FILE)).ok()?;
    Heads::parse(&text).ok()
}

/// `look` run between two reads of the record in `root`, and the record if
/// both reads found the same one. A write of the store changes the record
/// (it is absent while the write runs, and its count or its tips differ
/// after), so a record that both reads found brackets a look that no write
/// of the store crossed. A change by another door in that window leaves
/// the record as it was, and a comparison then reports it, truly.
pub(super) fn bracketed<T>(root: &Path, look: impl FnOnce() -> T) -> (Option<Heads>, T) {
    let before = read(root);
    let looked = look();
    let after = read(root);
    (before.filter(|held| Some(held) == after.as_ref()), looked)
}

/// Unlink the record, if it is there.
fn unlink(root: &Path) -> Result<(), ProdromeError> {
    let path = root.join(FILE);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProdromeError::io(&path, &error)),
    }
}

/// `heads` as the record of `root`: the `.gitignore` first, durably, where
/// there is none; then any record that is there unlinked, and the file
/// created anew and written, with no sync. A record is never written over
/// another, so a power loss cannot mix the bytes of two records: the file
/// holds the old record, nothing, or a prefix of the new one.
fn write(root: &Path, heads: &Heads) -> Result<(), ProdromeError> {
    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        put_durably(&gitignore, GITIGNORE.as_bytes())?;
    }
    unlink(root)?;
    let path = root.join(FILE);
    File::options()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| file.write_all(heads.print().as_bytes()))
        .map_err(|error| ProdromeError::io(&path, &error))
}

#[cfg(test)]
mod tests {
    //! The tests of the record's own steps and bytes. The laws of the
    //! record (SPEC laws 60 to 63 and 65) run over every store of record in
    //! `core/tests/all/store_laws/`.

    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{bracketed, read, Record, FILE};
    use crate::dag::Heads;
    use crate::event::{mk_reopened, Hash, TodoEvent};
    use crate::literal::Datetime;
    use crate::policy::Untrusted;
    use crate::reference::Todo;
    use crate::store::EventStore;

    type Event = TodoEvent<Todo>;
    type Store = EventStore<Event>;

    fn at(day: u32) -> Datetime {
        Datetime::new(2026, 9, 1 + day % 28, 12, 0, 0, 0).expect("a real instant")
    }

    /// A fresh directory for one store.
    fn scratch() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "prodrome-heads-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn store(root: &PathBuf) -> Store {
        Store::new(root, Untrusted::none())
    }

    /// What a fresh handle derives: the tips and the count of the history.
    fn derived(root: &PathBuf) -> Heads {
        let fresh = store(root);
        Heads::new(
            fresh.tips().expect("derives"),
            fresh.dag().expect("reads").objects().len(),
        )
    }

    fn reopened(entity: usize, day: u32) -> Event {
        mk_reopened(&format!("todo-{entity}"), at(day), "ada", "").expect("a valid event")
    }

    fn tip(n: usize) -> Hash {
        Hash::new(format!("{n:064x}")).expect("64 lowercase hex digits are a name")
    }

    /// `verify` compares only a record that both of its reads found: a
    /// second handle that appends between the cold look and the second read
    /// leaves no stale finding, where one read after the look would report
    /// one that is not true.
    #[test]
    fn a_write_during_verify_is_not_reported_as_stale() {
        let root = scratch();
        let writer = store(&root);
        writer.init("heads").expect("begins");
        writer.append(reopened(1, 1)).expect("appends");
        let other = store(&root);
        let (recorded, looked) = bracketed(&root, || {
            let looked = writer.cold().expect("reads");
            other.append(reopened(2, 2)).expect("appends");
            looked
        });
        assert_eq!(
            recorded, None,
            "the write changed the record between the reads"
        );
        let unbracketed = read(&root).expect("recorded");
        assert_ne!(
            unbracketed,
            looked.heads(),
            "one read after the look would report it"
        );
        assert_eq!(writer.verify(), Vec::new());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A record written where an older, longer record was holds the new
    /// print and nothing of the old one, since the old file is unlinked and
    /// the new one created.
    #[test]
    fn a_shorter_record_leaves_nothing_of_a_longer_one() {
        let root = scratch();
        let writer = store(&root);
        writer.init("heads").expect("begins");
        let longer = Heads::new((1..40).map(tip).collect(), 39);
        std::fs::write(root.join(FILE), longer.print()).expect("writes");
        let heads = derived(&root);
        assert!(heads.print().len() < longer.print().len());
        Record::Untouched.finish(&root, &heads).expect("records");
        assert_eq!(
            std::fs::read_to_string(root.join(FILE)).expect("recorded"),
            heads.print()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// THE RECORD'S BYTES, a stored format: the canonical print, with no
    /// newline, as a write leaves it. No proper prefix, no text with
    /// conflict markers, no unknown field and no other print of the same
    /// value reads.
    #[test]
    fn the_record_is_its_canonical_print_and_nothing_else_reads() {
        let two = Heads::new([tip(2), tip(1)].into(), 3);
        let print = format!(
            "Heads(count=3, tips=('{}', '{}'))",
            tip(1).as_str(),
            tip(2).as_str()
        );
        assert_eq!(two.print(), print);
        assert_eq!(Heads::parse(&print), Ok(two.clone()));
        let one = Heads::new([tip(7)].into(), 1);
        assert_eq!(
            one.print(),
            format!("Heads(count=1, tips=('{}',))", tip(7).as_str())
        );
        assert_eq!(
            Heads::new(BTreeSet::new(), 0).print(),
            "Heads(count=0, tips=())"
        );
        for cut in 0..print.len() {
            assert!(
                Heads::parse(&print[..cut]).is_err(),
                "a prefix of {cut} bytes"
            );
        }
        for other in [
            format!("{print}\n"),
            format!(
                "<<<<<<< ours\n{print}\n=======\n{}\n>>>>>>> theirs",
                one.print()
            ),
            format!(
                "Heads(tips=('{}', '{}'), count=3)",
                tip(1).as_str(),
                tip(2).as_str()
            ),
            format!(
                "Heads(count=3, tips=['{}', '{}'])",
                tip(1).as_str(),
                tip(2).as_str()
            ),
            format!(
                "Heads(count=3, tips=('{}', '{}'), at=1)",
                tip(1).as_str(),
                tip(2).as_str()
            ),
            format!(
                "Heads(count=3, tips=('{}', '{}'))",
                tip(2).as_str(),
                tip(1).as_str()
            ),
        ] {
            assert!(Heads::parse(&other).is_err(), "{other}");
        }

        let root = scratch();
        let writer = store(&root);
        writer.init("heads").expect("begins");
        writer.append(reopened(1, 1)).expect("appends");
        assert_eq!(
            std::fs::read_to_string(root.join(FILE)).expect("recorded"),
            derived(&root).print()
        );
        assert_eq!(
            std::fs::read_to_string(root.join(".gitignore")).expect("written"),
            "/.heads\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
