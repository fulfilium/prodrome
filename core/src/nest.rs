//! Histories held in registers, and nests of them (design §6.3; SPEC §3,
//! "A history held in a register"; laws 39 and 41).
//!
//! A register holds a history by its heads ([`Heads`]), and a schema tells
//! which of its registers do ([`Nests`]). A [`Nest`] flattens into one
//! [`History`] keyed by [`Path`]. A [`Set`] is a nest keyed by genesis.

#![warn(clippy::pedantic)]

mod heads;
mod history;
mod level;
mod path;

pub use heads::{pointer, Heads, Nests};
pub use history::History;
pub use level::{Holding, Leaf, Level, Nest, Set};
pub use path::{Path, Segment};
