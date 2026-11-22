//! The heads of a history: its tips and the count of its objects.

use std::collections::BTreeSet;

use crate::event::Hash;
use crate::literal::codec::{Literal, Record};
use crate::literal::{parse_literal, print_literal, ProdromeError, Table};

/// The tips of a history and the count of its objects (SPEC §3, "The record
/// of heads").
///
/// A program gets a `Heads` only from a derivation in this crate or from a
/// read of its literal ([`Heads::parse`] or [`Literal::from_value`]). It
/// prints as `Heads(count=<n>, tips=(<names>))`.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
pub struct Heads {
    count: usize,
    tips: BTreeSet<Hash>,
}

impl Heads {
    /// The heads of a history whose tips are `tips` and which holds
    /// `count` objects.
    pub(crate) fn new(tips: BTreeSet<Hash>, count: usize) -> Heads {
        Heads { tips, count }
    }

    /// The objects of the history that no object of it rests on.
    #[must_use]
    pub fn tips(&self) -> &BTreeSet<Hash> {
        &self.tips
    }

    /// The number of objects in the history.
    #[must_use]
    pub fn count(&self) -> usize {
        self.count
    }

    /// The canonical print: `Heads(count=<n>, tips=(<names>))`, with the
    /// tips in name order, and no newline after it.
    #[must_use]
    pub fn print(&self) -> String {
        print_literal(&self.to_value())
    }

    /// Reads `text` as a record of heads. Only a canonical print reads. A
    /// print ends with the closing parenthesis of its constructor, so no
    /// proper prefix of a print reads.
    ///
    /// # Errors
    ///
    /// The text does not parse as a `Heads` call, or it is not the
    /// canonical print of the value that it parses to.
    pub fn parse(text: &str) -> Result<Heads, ProdromeError> {
        let heads = Heads::from_value(&parse_literal(text, &Table(&[Heads::CONSTRUCTOR]))?)?;
        if heads.print() == text {
            Ok(heads)
        } else {
            Err(ProdromeError::invalid(
                "a record of heads is not in its canonical print",
            ))
        }
    }
}
