use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::event::Hash;
use crate::fold::Frontier;
use crate::literal::ProdromeError;
use crate::registers::Stamp;
use crate::schema::Schema;

/// An entity's registers: a product of semilattices, one per register, each
/// joined by the writes its schema routes to it. Joined componentwise, the
/// product is itself a semilattice, so folding a stream into it is a monoid
/// action whatever order the writes arrive in (§6.6).
///
/// The registers a write supersedes in are [`Frontier`]s, or, where the
/// register is inflationary, [`crate::fold::Climb`]s, named by
/// [`Schema::Register`]; a register that only accumulates (the todo's
/// tendings, a [`crate::fold::GrowSet`]) is a component too, and is joined by
/// union. A component reads itself under its [`crate::fold::RegisterType`].
///
/// One product is one schema's, so the schema is an associated type.
pub trait Product<'a>: Default + Clone + Debug + PartialEq {
    type Schema: Schema;

    /// The registers a write supersedes in that this product holds, in the
    /// order a reading lists them: a Rust schema's every one, a declared
    /// schema's those of the declaration its writes were made at (none,
    /// before any was joined: an empty product has nothing to read).
    fn registers(&self) -> Vec<<Self::Schema as Schema>::Register>;

    /// The route: `stamp` joined into every register its event writes. By
    /// default, [`Product::join_to`] each register that [`Schema::writes`]
    /// names; a product with a component beside its named registers (the
    /// todo's tendings) routes the rest itself.
    fn join(&mut self, stamp: &'a Stamp<Self::Schema>) {
        for register in stamp.event.writes() {
            self.join_to(register, stamp);
        }
    }

    /// The frontier of a register a write supersedes in.
    fn frontier(&self, register: <Self::Schema as Schema>::Register)
        -> &Frontier<'a, Self::Schema>;

    /// Join `stamp` into the register `register` alone, as that register
    /// joins a write: how [`Product::join`] routes a write, and how the fold
    /// builds each register's earliest writes (§6.4).
    fn join_to(
        &mut self,
        register: <Self::Schema as Schema>::Register,
        stamp: &'a Stamp<Self::Schema>,
    );

    /// A register's READING, by name (design §3): the writes whose values
    /// are maximal under its type's order, each value named by the least
    /// write carrying it. One write is a value; more is a conflict. For a
    /// register that may go back, the writes are its frontier's
    /// ([`Frontier::reading`]); for an inflationary register, they are all
    /// its writes ([`crate::fold::Climb::reading`]). Only the product knows
    /// the register's [`crate::fold::RegisterType`], so a reader that holds
    /// a register's name alone can read any schema.
    fn reading(&self, register: <Self::Schema as Schema>::Register)
        -> Vec<&'a Stamp<Self::Schema>>;

    /// The append's refusal (design §3.1): where `event` writes a register
    /// whose type is inflationary, its value must stand at or above that
    /// register's reading, which it supersedes. A schema can also refuse a
    /// write by a rule that its order does not state, for example a value
    /// that stands only on one named value beneath it. The append asks this
    /// of the product of every write to the entity that the writer holds,
    /// and of the empty product for the first write to an entity; `verify`
    /// asks it of the product of the writes beneath each write (law 70).
    /// `verify` joins only the writes to inflationary registers, so the
    /// refusal reads only the readings of those registers. A schema with no
    /// inflationary register answers `Ok`.
    ///
    /// # Errors
    ///
    /// A write whose value falls below, or beside, the reading it
    /// supersedes, or does not stand on it, the empty reading included.
    fn grows(&self, event: &Self::Schema) -> Result<(), ProdromeError>;

    /// Law 32 between two readings of one entity: in every inflationary
    /// register, each value that `earlier` reads stands at or below a value
    /// that `self` reads. A register that is not inflationary may go back,
    /// so a schema with no inflationary register answers `Ok`. For two
    /// object sets where `earlier`'s is a subset of `self`'s, read under the
    /// policy where everything binds, this holds for any objects.
    ///
    /// # Errors
    ///
    /// The first register whose reading fell.
    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError>;

    /// The inflationary registers among this product's registers: those
    /// held in a [`crate::fold::Climb`], which the append holds to
    /// [`Product::grows`]. Empty for a schema with none, and then no write
    /// can fall below its reading, so `verify` looks for none. A declared
    /// schema's product learns its registers from its first write.
    fn climbing(&self) -> Vec<<Self::Schema as Schema>::Register>;

    /// Every register with more than one write in its frontier, each named
    /// by its writes: what a human settles with the next one.
    fn conflicts(&self) -> BTreeMap<<Self::Schema as Schema>::Register, Vec<Hash>> {
        self.registers()
            .into_iter()
            .filter(|register| self.frontier(*register).is_conflict())
            .map(|register| (register, self.frontier(register).names()))
            .collect()
    }
}
