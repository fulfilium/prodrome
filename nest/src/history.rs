//! The nesting monad (design §6.3): a history is a finite set of values,
//! each keyed by a path.

use std::collections::BTreeSet;

use super::path::Path;

/// A history keyed by path: a finite set of `(path, value)` pairs, which
/// forms a monad, `P(Path × T)` (law 39).
///
/// [`History::join`] puts each inner value at its outer key and then its
/// inner key, `{(p·q, v)}`. Flattening only adds a prefix to a key, so it
/// renames no content-addressed object and rewrites none of its deps.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct History<T>(BTreeSet<(Path, T)>);

impl<T: Ord> Default for History<T> {
    fn default() -> Self {
        History(BTreeSet::new())
    }
}

impl<T: Ord> FromIterator<(Path, T)> for History<T> {
    fn from_iter<I: IntoIterator<Item = (Path, T)>>(entries: I) -> Self {
        History(entries.into_iter().collect())
    }
}

impl<T: Ord> IntoIterator for History<T> {
    type Item = (Path, T);
    type IntoIter = std::collections::btree_set::IntoIter<(Path, T)>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T: Ord> History<T> {
    /// `{(ε, value)}`: an event is the history of one object.
    pub fn unit(value: T) -> History<T> {
        History([(Path::root(), value)].into())
    }

    /// Each value mapped by `f`, at its key.
    pub fn fmap<U: Ord>(self, mut f: impl FnMut(T) -> U) -> History<U> {
        self.0
            .into_iter()
            .map(|(path, value)| (path, f(value)))
            .collect()
    }

    /// `join ∘ fmap f`: each value given the sub-history that `f` makes of
    /// it, flattened under that value's key.
    pub fn bind<U: Ord>(self, f: impl FnMut(T) -> History<U>) -> History<U> {
        self.fmap(f).join()
    }

    /// The union, the semilattice's join (design §1).
    #[must_use]
    pub fn union(mut self, other: History<T>) -> History<T> {
        self.0.extend(other.0);
        self
    }

    /// The values `chosen` keeps, each at its key.
    #[must_use]
    pub fn only(self, mut chosen: impl FnMut(&Path, &T) -> bool) -> History<T> {
        History(
            self.0
                .into_iter()
                .filter(|(path, value)| chosen(path, value))
                .collect(),
        )
    }

    /// The restricted join: this history plus exactly the values that
    /// `chosen` keeps of `inner`, each under `at`. At the root, `at` adds no
    /// prefix. `accept` of `prodrome-store` does the same for replicas.
    #[must_use]
    pub fn accept(
        self,
        at: &Path,
        inner: History<T>,
        chosen: impl FnMut(&Path, &T) -> bool,
    ) -> History<T> {
        History::from_iter([(Path::root(), self), (at.clone(), inner.only(chosen))]).join()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(Path, T)> {
        self.0.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The values, whatever their keys.
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.0.iter().map(|(_, value)| value)
    }
}

impl<T: Ord + Clone> History<T> {
    /// The values whose keys begin with `path`, each at what follows it.
    /// Where no key of a joined history begins another, reading the joined
    /// history at one of them reads the history held there (law 39).
    #[must_use]
    pub fn at(&self, path: &Path) -> History<T> {
        self.0
            .iter()
            .filter_map(|(key, value)| Some((key.strip(path)?, value.clone())))
            .collect()
    }
}

impl<T: Ord> History<History<T>> {
    /// `P (P e) → P e`: every inner value at its outer key and then its
    /// inner key.
    #[must_use]
    pub fn join(self) -> History<T> {
        self.0
            .into_iter()
            .flat_map(|(outer, inner)| {
                inner
                    .0
                    .into_iter()
                    .map(move |(key, value)| (outer.then(&key), value))
            })
            .collect()
    }
}
