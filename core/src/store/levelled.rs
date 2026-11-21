//! What a handle of a store on disk has read, with the mark of the last
//! hold that its reading is level with (`root/.writes`).

use std::ops::{Deref, DerefMut};

use super::memory::Memory;
use super::writes::Mark;
use crate::schema::Schema;

/// A handle's memory and the mark that the memory is level with: the mark
/// read before the memory's last listing of `objects/`, or the mark that
/// the handle's last hold wrote. Both are behind one mutex, so a memory
/// that a panic empties keeps no mark, and the next read lists.
#[derive(Debug)]
pub(super) struct Levelled<E: Schema> {
    memory: Memory<E>,
    pub(super) mark: Option<Mark>,
}

/// Whether a hold's memory is level with `objects/` when the hold ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Level {
    /// The memory holds what the hold read and wrote.
    Level,
    /// A placement failed and the look after it failed too, so the memory
    /// can lack objects that are under their names.
    Behind,
}

impl<E: Schema> Levelled<E> {
    /// A memory that has read nothing, level with no mark.
    pub(super) fn empty() -> Levelled<E> {
        Levelled {
            memory: Memory::empty(),
            mark: None,
        }
    }
}

impl<E: Schema> Deref for Levelled<E> {
    type Target = Memory<E>;

    fn deref(&self) -> &Memory<E> {
        &self.memory
    }
}

impl<E: Schema> DerefMut for Levelled<E> {
    fn deref_mut(&mut self) -> &mut Memory<E> {
        &mut self.memory
    }
}
