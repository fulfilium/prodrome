//! The printable set of SPEC §2: the characters that a string's print
//! writes as themselves. Every other character outside ASCII prints as an
//! escape.
//!
//! The General_Category values of Unicode 16.0.0 decide the set, and the
//! set never changes. A character is printable when its category is not
//! Cc, Cf, Cs, Co, Cn, Zl, Zp or Zs, and SPACE (U+0020) is printable too.
//! These are the categories that CPython's `str.isprintable` refuses. The
//! set is a table in this crate, generated once by `scripts/printable.py`,
//! so the print of a string does not depend on the Unicode data that a
//! build links. A character that a later version of Unicode assigns is
//! unassigned (Cn) in 16.0.0, so it prints as an escape. The escape is
//! text that the parser reads back to the same character.

mod table;

use std::fmt::Write as _;

use crate::event::Hash;

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

/// The SHA-256 of the printable set, as SPEC §2 writes it: one line for
/// each range of the table, `FIRST..LAST` and a newline, with each code
/// point in uppercase hex of at least four digits. A program in another
/// language can compute the same digest from its own table, and compare.
pub fn digest() -> Hash {
    let mut text = String::new();
    for (first, last) in RANGES {
        let _ = writeln!(text, "{:04X}..{:04X}", first as u32, last as u32);
    }
    Hash::of_bytes(text.as_bytes())
}
