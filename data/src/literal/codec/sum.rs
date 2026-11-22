//! A sum whose variant names the constructor of a record that holds it.
//!
//! A record that marks one field `#[literal(sum)]` prints as the constructor
//! of that field's variant, with the record's other fields first:
//! `Opened(at=…, actor=…, title=…)`. The `const` functions exist for the
//! derive, which builds the record's table of constructors at compile time.

#![warn(clippy::pedantic)]

use super::{Constructor, Literal};
use crate::literal::{Call, ProdromeError, Value};

/// An enum whose variants are constructors.
pub trait Sum: Literal {
    /// The constructor of the variant of `self`, and the values of the
    /// variant's fields, in print order.
    fn arm(&self) -> (Constructor, Vec<Value>);

    /// Reads the variant named by `call`, and ignores a field that the
    /// variant does not name.
    ///
    /// # Errors
    ///
    /// No variant has the name of `call`, or a field is missing or does not
    /// read.
    fn from_arm(call: &Call) -> Result<Self, ProdromeError>;
}

/// For the derive: the number of names in the table of `head` and `arms`.
#[must_use]
pub const fn headed_len(head: &[&str], arms: &[Constructor]) -> usize {
    let mut total = 0;
    let mut arm = 0;
    while arm < arms.len() {
        total += head.len() + arms[arm].1.len();
        arm += 1;
    }
    total
}

/// For the derive: for each arm in order, `head` and then the arm's fields.
#[must_use]
pub const fn headed_names<const TOTAL: usize>(
    head: &[&'static str],
    arms: &[Constructor],
) -> [&'static str; TOTAL] {
    let mut names = [""; TOTAL];
    let mut at = 0;
    let mut arm = 0;
    while arm < arms.len() {
        let mut field = 0;
        while field < head.len() {
            names[at] = head[field];
            at += 1;
            field += 1;
        }
        let own = arms[arm].1;
        let mut field = 0;
        while field < own.len() {
            names[at] = own[field];
            at += 1;
            field += 1;
        }
        arm += 1;
    }
    names
}

/// For the derive: each arm's name with its slice of `names`
/// ([`headed_names`]).
#[must_use]
pub const fn headed_table<const ARMS: usize>(
    names: &'static [&'static str],
    head: usize,
    arms: &[Constructor],
) -> [Constructor; ARMS] {
    let mut table: [Constructor; ARMS] = [("", &[]); ARMS];
    let mut rest = names;
    let mut arm = 0;
    while arm < ARMS {
        let (fields, after) = rest.split_at(head + arms[arm].1.len());
        table[arm] = (arms[arm].0, fields);
        rest = after;
        arm += 1;
    }
    table
}

/// Whether a constructor of `arms` has a field named `name`.
#[must_use]
pub const fn names_field(arms: &[Constructor], name: &str) -> bool {
    let mut arm = 0;
    while arm < arms.len() {
        let fields = arms[arm].1;
        let mut field = 0;
        while field < fields.len() {
            if same(fields[field], name) {
                return true;
            }
            field += 1;
        }
        arm += 1;
    }
    false
}

/// Whether the constructors of `table` have distinct names; the derive
/// checks a table with it at compile time.
#[must_use]
pub const fn distinct(table: &[Constructor]) -> bool {
    let mut one = 0;
    while one < table.len() {
        let mut other = one + 1;
        while other < table.len() {
            if same(table[one].0, table[other].0) {
                return false;
            }
            other += 1;
        }
        one += 1;
    }
    true
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut at = 0;
    while at < a.len() {
        if a[at] != b[at] {
            return false;
        }
        at += 1;
    }
    true
}
