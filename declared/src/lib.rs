//! A schema that a host declares as text at run time, and that the crate
//! admits only when its laws hold (SPEC §6, "A schema may be declared";
//! `docs/design-register-types.md` §5, §6.0).
//!
//! [`Declared`], an event of a declared schema, implements `Schema` by
//! interpreting its [`Declaration`]. A declared schema has no price (design
//! §4).
//!
//! The text form is the literal grammar of SPEC §2 under the closed
//! vocabulary [`Form`], so that objects and schemas share one parser, one
//! printer and one whitelist. Every set in a declaration prints sorted, so
//! the hash of the text is the schema's name.

mod admit;
mod event;
mod language;
mod order;

use std::fmt;
use std::sync::Arc;

pub use admit::{Law, Refusal};
pub use event::{Datum, Declared, Key, Registers};
pub use language::{Event, Field, Form, Poset, Register, Type, Write};
pub use order::Valued;

use admit::{Admitted, Reg, Shape};
use prodrome_data::event::Hash;
use prodrome_data::literal::{Signature, Vocabulary};

/// An admitted declared schema: only [`Declaration::admit`] makes one, so
/// every [`Law`] holds for it.
///
/// Two declarations are equal exactly when their names (the hashes of their
/// canonical texts) are equal.
#[derive(Clone)]
pub struct Declaration(Arc<Admitted>);

/// A register of a declared schema, by its position in the declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot(usize);

impl Slot {
    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

impl Declaration {
    /// Admits `text` as a schema.
    ///
    /// # Errors
    ///
    /// The first law, in [`Law`]'s order, that the text does not keep.
    pub fn admit(text: &str) -> Result<Declaration, Refusal> {
        admit::admit(text).map(|admitted| Declaration(Arc::new(admitted)))
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.0.text
    }

    /// The hash of the canonical text.
    #[must_use]
    pub fn name(&self) -> &Hash {
        &self.0.name
    }

    /// The field by which every event names its entity.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.0.key
    }

    /// Every register, sorted by name, which is the order that a reading
    /// lists them in.
    pub fn registers(&self) -> impl Iterator<Item = Slot> {
        (0..self.0.registers.len()).map(Slot)
    }

    #[must_use]
    pub fn register(&self, name: &str) -> Option<Slot> {
        self.0
            .registers
            .iter()
            .position(|register| register.name == name)
            .map(Slot)
    }

    /// # Panics
    ///
    /// Panics if `slot` comes from another declaration and lies past this
    /// declaration's registers.
    #[must_use]
    pub fn register_name(&self, slot: Slot) -> &str {
        &self.reg(slot).name
    }

    /// # Panics
    ///
    /// As [`Declaration::register_name`].
    #[must_use]
    pub fn inflationary(&self, slot: Slot) -> bool {
        self.reg(slot).inflationary
    }

    /// `datum` under the order of the register `slot`, as the register's
    /// reading compares it.
    ///
    /// # Panics
    ///
    /// As [`Declaration::register_name`].
    #[must_use]
    pub fn value<'d>(&'d self, slot: Slot, datum: &'d Datum) -> Valued<'d> {
        Valued {
            order: &self.reg(slot).order,
            datum,
        }
    }

    fn reg(&self, slot: Slot) -> &Reg {
        &self.0.registers[slot.0]
    }

    fn shape(&self, event: usize) -> &Shape {
        &self.0.events[event]
    }

    fn event(&self, name: &str) -> Option<usize> {
        self.0.events.iter().position(|shape| shape.name == name)
    }
}

impl PartialEq for Declaration {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.name == other.0.name
    }
}

impl Eq for Declaration {}

/// Its name, not its text: a declaration is its hash.
impl fmt::Debug for Declaration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Declaration")
            .field(&self.0.name.as_str())
            .finish()
    }
}

/// Its constructors, each with its declared field order: §2's whitelist
/// for its events.
impl Vocabulary for Declaration {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        self.event(name)
            .map(|event| Signature::Owned(&self.shape(event).names))
    }
}
