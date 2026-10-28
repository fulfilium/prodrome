//! A table of signature checks, kept by each `Signed` object's content
//! name (design §6.4).

use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::event::Hash;
use crate::literal::print_literal;
use crate::memo::{self, Cache, Key, Lookup, Lru};
use crate::sign::Signed;

/// Whether each signature verifies, as a table. [`Signed::verifies`] is a
/// function of one `Signed` object: its object's name, its key and its
/// signature. The `Signed` object's content name covers all three, so the
/// table keeps each check under that name, and a program that reads a
/// history again checks only the signatures that the table does not hold.
///
/// The table computes that name from the `Signed` object itself, the
/// sha256 of its print, and never takes a name from its caller. So no
/// caller can find a check that one object passed under another object's
/// name. The sha256 costs far less than the Ed25519 check it saves.
///
/// The table means [`Signed::verifies`] and nothing else (SPEC law 49). It
/// keeps at most its capacity of checks and drops the least recently used,
/// and a table that holds nothing answers the same as one that holds
/// everything.
#[derive(Debug)]
pub struct Checks(Mutex<Cache<bool, Lru>>);

impl Checks {
    /// An empty table that keeps at most `capacity` checks.
    #[must_use]
    pub fn new(capacity: usize) -> Checks {
        Checks(Mutex::new(Cache::new(Lru::new(capacity))))
    }

    /// Does `signed` verify?
    #[must_use]
    pub fn verifies(&self, signed: &Signed) -> bool {
        let key = Key {
            function: memo::name(b"prodrome sign::Signed::verifies", []),
            argument: Hash::of_bytes(print_literal(&signed.to_value()).as_bytes()),
        };
        if let Lookup::Hit(verifies) = self.held().get(&key) {
            return *verifies;
        }
        let verifies = signed.verifies();
        // A second value under one key is the cache's finding; this answer
        // is the check made now.
        let _finding = self.held().insert(key, verifies);
        verifies
    }

    /// The table. A program that panicked while it held the table left a
    /// whole table, since every change is one insert, so it is read.
    fn held(&self) -> MutexGuard<'_, Cache<bool, Lru>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
