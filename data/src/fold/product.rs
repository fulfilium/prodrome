use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::event::Hash;
use crate::fold::Frontier;
use crate::literal::ProdromeError;
use crate::schema::Schema;
use crate::stamp::Stamp;

/// An entity's registers: a product of semilattices, one for each register
/// (SPEC §6, "A schema's registers", and SPEC §6.6).
///
/// The product joins componentwise, so the order in which writes arrive
/// changes no value. A register that a write supersedes in is a [`Frontier`],
/// or a [`crate::fold::Climb`] where the register is inflationary. A register
/// that only accumulates is a [`crate::fold::GrowSet`].
pub trait Product<'a>: Default + Clone + Debug + PartialEq {
    type Schema: Schema;

    /// The registers that a write supersedes in, in the order that a reading
    /// lists them.
    ///
    /// A declared schema's product lists the registers of the declaration
    /// that its writes were made at, and an empty one lists none.
    fn registers(&self) -> Vec<<Self::Schema as Schema>::Register>;

    /// Joins `stamp` into every register that its event writes.
    ///
    /// The default calls [`Product::join_to`] for each register that
    /// [`Schema::writes`] names. A product with a component beside its named
    /// registers, such as the todo's tendings, must route the rest itself.
    fn join(&mut self, stamp: &'a Stamp<Self::Schema>) {
        for register in stamp.event.writes() {
            self.join_to(register, stamp);
        }
    }

    /// The frontier of a register a write supersedes in.
    fn frontier(&self, register: <Self::Schema as Schema>::Register)
        -> &Frontier<'a, Self::Schema>;

    /// Joins `stamp` into the register `register` alone.
    ///
    /// The fold also calls it to build each register's earliest writes (SPEC
    /// §6.4).
    fn join_to(
        &mut self,
        register: <Self::Schema as Schema>::Register,
        stamp: &'a Stamp<Self::Schema>,
    );

    /// The reading of a register, by name (design §3; law 31).
    ///
    /// A register that may go back reads its frontier
    /// ([`Frontier::reading`]). An inflationary register reads all its
    /// writes ([`crate::fold::Climb::reading`]). Only the product knows the
    /// register's [`crate::fold::RegisterType`], so a reader that holds only a
    /// register's name can read any schema.
    fn reading(&self, register: <Self::Schema as Schema>::Register)
        -> Vec<&'a Stamp<Self::Schema>>;

    /// The refusal of an append (design §3.1): where `event` writes an
    /// inflationary register, its value must stand at or above the reading of
    /// that register.
    ///
    /// A schema can also refuse a write by a rule that its order does not
    /// state, for example a value that stands only on one named value beneath
    /// it. The append asks this of the product of every write to the entity
    /// that the writer holds, and of the empty product for the first write.
    /// `verify` asks it of the product of the writes beneath each write (law
    /// 70: the append refuses exactly what `verify` finds). `verify` joins
    /// only the writes to inflationary registers, so the refusal must read only
    /// the readings of those registers. A schema with no inflationary register
    /// answers `Ok`.
    ///
    /// # Errors
    ///
    /// Returns an error where the value of a write falls below the reading
    /// that it supersedes, is incomparable with it, or does not stand on it,
    /// the empty reading included.
    fn grows(&self, event: &Self::Schema) -> Result<(), ProdromeError>;

    /// Law 32 between two readings of one entity: in every inflationary
    /// register, each value that `earlier` reads stands at or below a value
    /// that `self` reads.
    ///
    /// A register that is not inflationary may go back, so a schema with no
    /// inflationary register answers `Ok`. Where the objects of `earlier` are
    /// a subset of those of `self` and both are read under the policy where
    /// everything binds, the check passes for any objects.
    ///
    /// # Errors
    ///
    /// Returns an error that names the first register whose reading fell.
    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError>;

    /// The inflationary registers of this product, which a
    /// [`crate::fold::Climb`] holds and which the append holds to
    /// [`Product::grows`].
    ///
    /// Where the list is empty, `verify` looks for no write below its
    /// reading. A declared schema's product learns its registers from its
    /// first write.
    fn climbing(&self) -> Vec<<Self::Schema as Schema>::Register>;

    /// Every register with more than one write in its frontier, with the
    /// names of those writes.
    fn conflicts(&self) -> BTreeMap<<Self::Schema as Schema>::Register, Vec<Hash>> {
        self.registers()
            .into_iter()
            .filter(|register| self.frontier(*register).is_conflict())
            .map(|register| (register, self.frontier(register).names()))
            .collect()
    }
}
