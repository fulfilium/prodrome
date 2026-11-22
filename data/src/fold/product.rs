use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::event::Hash;
use crate::fold::Frontier;
use crate::literal::ProdromeError;
use crate::schema::Schema;
use crate::stamp::Stamp;

/// An entity's registers: a product of semilattices, one for each register,
/// joined componentwise, so the order of the writes changes no value (SPEC
/// §6.6).
pub trait Product<'a>: Default + Clone + Debug + PartialEq {
    type Schema: Schema;

    /// The registers that a write supersedes in, in the order that a reading
    /// lists them. A declared schema's product lists the registers of the
    /// declaration of its writes, and an empty one lists none.
    fn registers(&self) -> Vec<<Self::Schema as Schema>::Register>;

    /// Joins `stamp` into every register that [`Schema::writes`] names. A
    /// product with a component beside its named registers, such as the
    /// todo's tendings, must route the rest itself.
    fn join(&mut self, stamp: &'a Stamp<Self::Schema>) {
        for register in stamp.event.writes() {
            self.join_to(register, stamp);
        }
    }

    fn frontier(&self, register: <Self::Schema as Schema>::Register)
        -> &Frontier<'a, Self::Schema>;

    /// Joins `stamp` into `register` alone; the fold also builds each
    /// register's earliest writes with it (SPEC §6.4).
    fn join_to(
        &mut self,
        register: <Self::Schema as Schema>::Register,
        stamp: &'a Stamp<Self::Schema>,
    );

    /// The reading of a register, by name (law 31). Only the product knows
    /// the register's [`crate::fold::RegisterType`], so a reader that holds
    /// only a register's name can read any schema.
    fn reading(&self, register: <Self::Schema as Schema>::Register)
        -> Vec<&'a Stamp<Self::Schema>>;

    /// The refusal of an append: where `event` writes an inflationary
    /// register, its value must stand at or above the reading of that register
    /// (law 70). A schema may refuse more than its order does (SPEC §3).
    ///
    /// The refusal must read only the readings of inflationary registers,
    /// because `verify` joins only the writes to those registers. A schema
    /// with no inflationary register answers `Ok`.
    ///
    /// # Errors
    ///
    /// Returns an error where the value of a write falls below the reading
    /// that it supersedes, is incomparable with it, or does not stand on it,
    /// the empty reading included.
    fn grows(&self, event: &Self::Schema) -> Result<(), ProdromeError>;

    /// Law 32 between two readings of one entity: in every inflationary
    /// register, each value that `earlier` reads stands at or below a value
    /// that `self` reads. A schema with no inflationary register answers `Ok`.
    ///
    /// Where the objects of `earlier` are a subset of those of `self` and both
    /// are read under the policy where everything binds, the check passes for
    /// any objects, because the reading of a union is the join of the readings.
    ///
    /// # Errors
    ///
    /// Returns an error that names the first register whose reading fell.
    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError>;

    /// The inflationary registers, which the append holds to
    /// [`Product::grows`]. Where the list is empty, `verify` looks for no write
    /// below its reading. A declared schema's product learns the list from its
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
