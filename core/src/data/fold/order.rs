use std::collections::BTreeSet;

use crate::data::literal::ProdromeError;

/// A partial order on a register's values (law 30: reflexive, antisymmetric
/// and transitive).
///
/// The order decides which concurrent values are redundant, and never which
/// write supersedes which: only ancestry decides that. The crate does not use
/// `PartialOrd`, so that a derived or lexicographic order never becomes the
/// order of a register by accident.
pub trait Order {
    fn le(&self, other: &Self) -> bool;
}

/// The default order, under which two values are equal or incomparable, so
/// every distinct value is its own candidate.
///
/// The bound is `PartialEq` and not `Eq` because an event holds floats. The
/// smart constructors keep the floats finite, so `==` is an equivalence on
/// every value that a register holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Discrete<T>(pub T);

impl<T: PartialEq> Order for Discrete<T> {
    fn le(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

/// A value read by reference is ordered as the value.
impl<T: Order + ?Sized> Order for &T {
    fn le(&self, other: &Self) -> bool {
        (**self).le(*other)
    }
}

/// A total order: a register under it reads one value, the greatest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Total<T>(pub T);

impl<T: Ord> Order for Total<T> {
    fn le(&self, other: &Self) -> bool {
        self.0 <= other.0
    }
}

/// Sets under inclusion: the grow-only set's order.
impl<T: Ord> Order for BTreeSet<T> {
    fn le(&self, other: &Self) -> bool {
        self.is_subset(other)
    }
}

/// A register type's declaration that each write is `≥` the reading that it
/// supersedes (law 32).
///
/// The crate does not trust the declaration: the append checks each write
/// with [`grows`].
pub trait Inflationary: Order {}

/// The refusal of an append for an inflationary register: `value` must stand
/// at or above every value of the reading that it supersedes.
///
/// # Errors
///
/// Returns an error where `value` does not stand at or above a superseded
/// value.
pub fn grows<'v, V: Inflationary + 'v>(
    superseded: impl IntoIterator<Item = &'v V>,
    value: &V,
) -> Result<(), ProdromeError> {
    if superseded.into_iter().all(|held| held.le(value)) {
        Ok(())
    } else {
        Err(ProdromeError::invalid(
            "a write to an inflationary register must not fall below the reading it supersedes",
        ))
    }
}

/// The items whose values are maximal, one item for each value: the first
/// item that carries it (law 31). More than one item is a conflict.
pub fn maximal<T: Copy, V: Order>(items: &[T], value: impl Fn(T) -> V) -> Vec<T> {
    let mut out: Vec<T> = Vec::with_capacity(items.len());
    for &item in items {
        let v = value(item);
        let dominated = items.iter().any(|&other| {
            let w = value(other);
            v.le(&w) && !w.le(&v)
        });
        let held = out.iter().any(|&kept| {
            let w = value(kept);
            v.le(&w) && w.le(&v)
        });
        if !dominated && !held {
            out.push(item);
        }
    }
    out
}
