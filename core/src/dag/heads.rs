//! The heads of a history: its tips and the count of its objects.

use std::collections::BTreeSet;

use crate::event::Hash;
use crate::literal::codec::{record, Constructor, Fields, Literal};
use crate::literal::{parse_literal, print_literal, ProdromeError, Table, Value};

/// The tips of a history and the count of its objects. A store records
/// its heads in a file (SPEC §3), and a replica answers them from what it
/// holds.
///
/// The fields are private. A program gets a `Heads` from a derivation in
/// this crate or from [`Heads::parse`], so a `Heads` value is always one
/// of the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heads {
    tips: BTreeSet<Hash>,
    count: usize,
}

/// The constructor of a record of heads, with its fields in printed order.
const HEADS: Constructor = ("Heads", &["count", "tips"]);

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

    /// `text` read as a record of heads. Only the canonical print of a
    /// value reads. The print ends with the closing parenthesis of its
    /// constructor, so no proper prefix of a print reads.
    ///
    /// # Errors
    ///
    /// The text does not parse as a `Heads` call, or it is not the
    /// canonical print of the value that it parses to.
    pub fn parse(text: &str) -> Result<Heads, ProdromeError> {
        let heads = Heads::from_value(&parse_literal(text, &Table(&[HEADS]))?)?;
        if heads.print() == text {
            Ok(heads)
        } else {
            Err(ProdromeError::invalid(
                "a record of heads is not in its canonical print",
            ))
        }
    }
}

impl Literal for Heads {
    fn to_value(&self) -> Value {
        record(HEADS, vec![self.count.to_value(), self.tips.to_value()])
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        let fields = Fields::of(value, HEADS)?;
        Ok(Heads {
            tips: fields.get("tips")?,
            count: fields.get("count")?,
        })
    }
}
