//! The register algebra (SPEC §6, "A schema's registers", and SPEC §6.6):
//! each register is a semilattice, and an entity's registers are their
//! product, which a fold joins one write at a time.

mod climb;
mod frontier;
mod order;
mod product;
mod register;
pub mod sole;

pub use climb::Climb;
pub use frontier::Frontier;
pub use order::{grows, maximal, Discrete, Inflationary, Order, Total};
pub use product::Product;
pub use register::{GrowSet, Register, RegisterType};
