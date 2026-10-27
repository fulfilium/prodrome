use std::collections::BTreeSet;

use crate::literal::ProdromeError;

/// A partial order on a register's values: reflexive, antisymmetric and
/// transitive (§9). It decides which concurrent values are redundant, never
/// which write supersedes which; ancestry alone does that. Its own trait and
/// not `PartialOrd`, so a type's derived or lexicographic order never becomes
/// a register's by accident.
pub trait Order {
    fn le(&self, other: &Self) -> bool;
}

/// Equal or incomparable: the default, under which every distinct value is
/// its own candidate. `PartialEq` and not `Eq`, since an event holds floats;
/// the smart constructors keep them finite, so `==` is an equivalence on
/// every value a register holds.
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

/// A total order: a register under it always reads one value, the greatest.
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

/// A register type's declaration that a write only grows it: its value is
/// `≥` the reading it supersedes, so the reading of a union of histories is
/// the join of their readings. Not trusted: the append path checks it with
/// [`grows`].
pub trait Inflationary: Order {}

/// The append path's refusal for an inflationary register: `value` must
/// stand at or above every value of the reading it supersedes.
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

/// The free completion of the order: the items whose values are maximal,
/// one per value, the first that carries it. One is a value; more is a
/// conflict. Under [`Discrete`] it is every distinct value.
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
