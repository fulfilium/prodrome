//! Caches as tabulations of pure functions (design §6.2; SPEC §6, "A cache
//! is a tabulation of a pure function").
//!
//! A reading through a cache equals the function (`memo f = f`), so a
//! program may drop any entry. A [`Holder`] keeps the pairs, and every
//! holder obeys the same laws (law 57). [`fold()`] is the memoised fold over
//! a content-addressed tree, over any holder. [`balance()`] gives a long
//! sequence a balanced shape, so that a change renames a logarithmic number
//! of nodes.
//!
//! The module depends only on the hash type and on the literal codec of the
//! crate: it uses no schema, no store and no FPL.

#![warn(clippy::pedantic)]

pub mod balance;
pub mod cache;
#[cfg(unix)]
pub mod file;
pub mod fold;
pub mod front;
pub mod holder;

pub use balance::balance;
pub use cache::{Cache, Evict, Finding, Keep, Key, Lookup, Lru};
#[cfg(unix)]
pub use file::File;
pub use fold::{fold, Algebra, Counts, Outcome, Report, Tree, Visit};
pub use front::Front;
pub use holder::{Holder, Memory};

use prodrome_data::event::Hash;

/// The content name of a node, over its own bytes and its children's names
/// in order, so that a change anywhere below renames exactly the path up to
/// the node.
///
/// The own bytes are length-prefixed and a name is fixed-width, so no two
/// nodes print alike. The same function names an algebra: its code's bytes,
/// and the names of what that code reads as its children.
pub fn name<'a>(own: &[u8], children: impl IntoIterator<Item = &'a Hash>) -> Hash {
    let mut bytes = format!("memo {}:", own.len()).into_bytes();
    bytes.extend_from_slice(own);
    for child in children {
        bytes.push(b' ');
        bytes.extend_from_slice(child.as_str().as_bytes());
    }
    Hash::of_bytes(&bytes)
}
