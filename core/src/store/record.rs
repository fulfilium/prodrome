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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use proptest::prelude::*;

    use super::{read, FILE};
    use crate::dag::{Finding, Heads};
    use crate::event::{mk_created, mk_reopened, mk_sealed, Hash, TodoEvent};
    use crate::literal::Datetime;
    use crate::policy::Untrusted;
    use crate::reference::Todo;
    use crate::store::memory::Verified;
    use crate::store::{sync, EventStore};

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

    /// One write of a script, by one of two handles of one store.
    #[derive(Debug, Clone)]
    enum Step {
        Append { by: bool, entity: usize },
        Batch { by: bool, entities: Vec<usize> },
        Receive { by: bool, entity: usize },
        Quarantine { by: bool, pick: usize },
    }

    fn a_step() -> impl Strategy<Value = Step> {
        prop_oneof![
            (any::<bool>(), 0usize..4).prop_map(|(by, entity)| Step::Append { by, entity }),
            (any::<bool>(), prop::collection::vec(0usize..4, 1..4))
                .prop_map(|(by, entities)| Step::Batch { by, entities }),
            (any::<bool>(), 0usize..4).prop_map(|(by, entity)| Step::Receive { by, entity }),
            (any::<bool>(), 0usize..64).prop_map(|(by, pick)| Step::Quarantine { by, pick }),
        ]
    }

    proptest! {
        #![proptest_config(crate::cases::cases(48))]

        /// Law 60: after every write of a script of `init`, `append`, the
        /// appends of one `Decision`, `receive` and `quarantine`, by two
        /// handles that take turns under the lock, the record on disk parses
        /// and is the derived heads, so `heads()` serves it.
        #[test]
        fn the_recorded_heads_are_the_derived_ones(
            steps in prop::collection::vec(a_step(), 1..10),
        ) {
            let root = scratch();
            let handles = [store(&root), store(&root)];
            handles[0].init("heads").expect("begins");
            let elsewhere_root = scratch();
            let elsewhere = store(&elsewhere_root);
            sync(&handles[0], &elsewhere).expect("syncs");
            let mut day = 0;
            for step in steps {
                day += 1;
                // A refused write changes nothing; the record must still hold.
                match step {
                    Step::Append { by, entity } => {
                        let _ = handles[usize::from(by)].append(reopened(entity, day));
                    }
                    Step::Batch { by, entities } => {
                        if let Ok(mut decision) = handles[usize::from(by)].decide() {
                            for (n, entity) in entities.into_iter().enumerate() {
                                let day = day + u32::try_from(n).expect("a few");
                                let _ = decision.append(reopened(entity, day));
                            }
                        }
                    }
                    Step::Receive { by, entity } => {
                        let _ = elsewhere.append(reopened(entity + 10, day));
                        let _ = sync(&elsewhere, &handles[usize::from(by)]);
                    }
                    Step::Quarantine { by, pick } => {
                        let names: Vec<Hash> =
                            derived(&root).tips().iter().cloned().collect();
                        if let Some(name) = names.get(pick % names.len().max(1)) {
                            let path = root.join("objects").join(format!("{}.py", name.as_str()));
                            std::fs::write(&path, b"damaged").expect("damages");
                        }
                        let _ = handles[usize::from(by)].fsck();
                    }
                }
                let derived = derived(&root);
                prop_assert_eq!(read(&root), Some(derived.clone()), "the record is on disk");
                prop_assert_eq!(handles[0].heads().expect("reads"), derived.clone());
                prop_assert_eq!(handles[1].heads().expect("reads"), derived);
            }
            let _ = std::fs::remove_dir_all(&root);
            let _ = std::fs::remove_dir_all(&elsewhere_root);
        }

        /// Law 61: every state that a stop of the store's own write can
        /// leave serves no record but the derived one. A killed process
        /// leaves the old record with the old objects, or no record with
        /// any prefix of the new objects placed parents first. A power loss
        /// can also lose the new record, or cut it at any byte, after every
        /// new object is placed.
        #[test]
        fn a_served_record_is_the_derived_one(
            before in 0usize..4,
            new in 1usize..4,
        ) {
            let root = scratch();
            let writer = store(&root);
            writer.init("heads").expect("begins");
            for n in 0..before {
                writer.append(reopened(n, u32::try_from(n).expect("a few"))).expect("appends");
            }
            let source_root = scratch();
            let source = store(&source_root);
            sync(&writer, &source).expect("syncs");
            let mut placed: Vec<(Hash, Vec<u8>)> = Vec::new();
            for n in 0..new {
                let name = source
                    .append(reopened(n + 10, 20 + u32::try_from(n).expect("a few")))
                    .expect("appends");
                let print = std::fs::read(source_root.join("objects").join(format!("{}.py", name.as_str())))
                    .expect("reads");
                placed.push((name, print));
            }
            prop_assert_eq!(writer.heads().expect("reads"), derived(&root), "the old record");
            let record = std::fs::read_to_string(root.join(FILE)).expect("recorded");
            std::fs::remove_file(root.join(FILE)).expect("removes");
            for (name, print) in &placed {
                std::fs::write(root.join("objects").join(format!("{}.py", name.as_str())), print)
                    .expect("places");
                prop_assert_eq!(store(&root).heads().expect("derives"), derived(&root));
            }
            let new_record = derived(&root).print();
            prop_assert_ne!(&new_record, &record);
            for cut in 0..new_record.len() {
                std::fs::write(root.join(FILE), &new_record.as_bytes()[..cut]).expect("cuts");
                prop_assert_eq!(read(&root), None, "a cut record is not served");
                prop_assert_eq!(store(&root).heads().expect("derives"), derived(&root));
            }
            std::fs::write(root.join(FILE), &new_record).expect("writes");
            prop_assert_eq!(read(&root), Some(derived(&root)));
            let _ = std::fs::remove_dir_all(&root);
            let _ = std::fs::remove_dir_all(&source_root);
        }
    }

    /// Law 61: a placement that fails after the record is removed leaves
    /// no record, also when a later write of the same hold succeeds, since
    /// the failed placement's objects can be on disk and not in memory.
    #[test]
    fn a_failed_placement_leaves_no_record() {
        let root = scratch();
        let writer = store(&root);
        let genesis = writer.init("heads").expect("begins");
        let object = |n: usize| {
            let event: Event =
                mk_created(&format!("todo-{n}"), at(1), "ada", "", "").expect("valid");
            Verified::sealed(mk_sealed(Some(genesis.clone()), event))
        };
        let (first, first_print) = object(1);
        let (second, second_print) = object(2);
        let blocked = root
            .join("objects")
            .join(format!("{}.py", second.name().as_str()));
        let mut decision = writer.decide().expect("decides");
        std::fs::create_dir(&blocked).expect("blocks the second name");
        let failed = decision.place(vec![
            (first, Some(first_print)),
            (second, Some(second_print)),
        ]);
        assert!(failed.is_err(), "the rename onto a directory fails");
        assert!(!root.join(FILE).exists(), "removed before the placement");
        decision.write(object(3)).expect("a later write succeeds");
        drop(decision);
        assert!(!root.join(FILE).exists(), "the hold records nothing");
        std::fs::remove_dir(&blocked).expect("unblocks");
        assert_eq!(writer.heads().expect("derives"), derived(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Law 62: objects that another door adds read as the older heads,
    /// until a hold of the lock, a call of `record_heads`, or a deletion
    /// of the record; after each, `heads()` is the derived heads.
    #[test]
    fn another_door_s_objects_read_as_the_older_heads() {
        let root = scratch();
        let writer = store(&root);
        writer.init("heads").expect("begins");
        let source_root = scratch();
        let source = store(&source_root);
        sync(&writer, &source).expect("syncs");
        let mut day = 0;
        let mut copy_one_in = || {
            day += 1;
            let name = source.append(reopened(1, day)).expect("appends");
            let file = format!("{}.py", name.as_str());
            std::fs::copy(
                source_root.join("objects").join(&file),
                root.join("objects").join(&file),
            )
            .expect("copies");
        };
        let ends: [&dyn Fn(); 3] = [
            &|| drop(writer.decide().expect("decides")),
            &|| drop(writer.record_heads().expect("records")),
            &|| std::fs::remove_file(root.join(FILE)).expect("deletes"),
        ];
        for end in ends {
            let older = writer.heads().expect("reads");
            copy_one_in();
            assert_eq!(writer.heads().expect("reads"), older, "the record lags");
            assert_ne!(older, derived(&root));
            end();
            assert_eq!(writer.heads().expect("reads"), derived(&root));
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&source_root);
    }

    /// Law 63: a record that names a head whose file another door removed
    /// is not served, and `verify` names the record and the derived heads.
    #[test]
    fn a_record_naming_an_absent_head_is_not_served() {
        let root = scratch();
        let writer = store(&root);
        writer.init("heads").expect("begins");
        let head = writer.append(reopened(1, 1)).expect("appends");
        let recorded = writer.heads().expect("reads");
        assert_eq!(recorded.tips(), &BTreeSet::from([head.clone()]));
        std::fs::remove_file(root.join("objects").join(format!("{}.py", head.as_str())))
            .expect("removes");
        let derived = derived(&root);
        assert_ne!(derived, recorded);
        assert_eq!(writer.heads().expect("derives"), derived);
        assert!(writer
            .verify()
            .contains(&Finding::StaleHeads { recorded, derived }));
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
