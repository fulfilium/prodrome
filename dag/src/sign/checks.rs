//! A table of signature checks, kept by each `Signed` object's content
//! name (design §6.4).

use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::sign::Signed;
use prodrome_data::event::Hash;
use prodrome_data::literal::print_literal;
use prodrome_memo::{Cache, Key, Lookup, Lru};

/// A bounded table of the results of [`Signed::verifies`], keyed by the
/// content name of each `Signed` object.
///
/// The table computes that name itself, as the SHA-256 of the print, so no
/// caller can make the check of one object answer for another object. The
/// SHA-256 costs far less than the Ed25519 check that it saves.
///
/// Law 49: the table answers exactly as [`Signed::verifies`], whatever it
/// holds.
#[derive(Debug)]
pub struct Checks(Mutex<Cache<bool, Lru>>);

impl Checks {
    /// An empty table that keeps at most `capacity` checks.
    #[must_use]
    pub fn new(capacity: usize) -> Checks {
        Checks(Mutex::new(Cache::new(Lru::new(capacity))))
    }

    /// Whether `signed` verifies.
    #[must_use]
    pub fn verifies(&self, signed: &Signed) -> bool {
        let key = Key {
            function: prodrome_memo::name(b"prodrome sign::Signed::verifies", []),
            argument: Hash::of_bytes(print_literal(&signed.to_value()).as_bytes()),
        };
        if let Lookup::Hit(verifies) = self.held().get(&key) {
            return verifies;
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
