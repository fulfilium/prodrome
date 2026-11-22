//! The characters that a string's print writes as themselves (SPEC §2,
//! "The printable set").
//!
//! The set is a table in this crate, generated once by
//! `scripts/printable.py`, so that the print of a string does not depend on
//! the Unicode data that a build links.

mod table;

use std::fmt::Write as _;

use crate::data::event::Hash;

use table::RANGES;

/// The version of Unicode whose General_Category decides the printable set.
pub const UNICODE_VERSION: &str = "16.0.0";

/// Whether the print of a string writes `c` as itself (SPEC §2).
pub fn is_printable(c: char) -> bool {
    RANGES
        .binary_search_by(|&(first, last)| {
            if last < c {
                std::cmp::Ordering::Less
            } else if first > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// The SHA-256 of the printable set, over the text that SPEC §2, "The
/// printable set", defines: one line `FIRST..LAST` for each range.
pub fn digest() -> Hash {
    let mut text = String::new();
    for (first, last) in RANGES {
        let _ = writeln!(text, "{:04X}..{:04X}", first as u32, last as u32);
    }
    Hash::of_bytes(text.as_bytes())
}
