//! A sequence of integers that answers, for a range, its sum and the largest
//! sum of a prefix of it, in time logarithmic in its length, and takes a
//! change to one element in the same time: a segment tree. [`super::view`]
//! keeps one per level of the tree, over each node's gain (what its parts
//! cost less its summary), to decide in one query whether a closing lowers
//! the cost at every leftward prefix of it.

/// A range's sum and the largest sum of a prefix of it, the empty prefix
/// included, so `best` is never negative.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Sums {
    pub(super) total: i128,
    pub(super) best: i128,
}

impl Sums {
    fn one(value: i128) -> Self {
        Sums {
            total: value,
            best: value.max(0),
        }
    }

    /// The sums of `self`'s range followed by `next`'s.
    fn then(self, next: Self) -> Self {
        Sums {
            total: self.total + next.total,
            best: self.best.max(self.total + next.best),
        }
    }
}

/// The segment tree: leaf `i` of the sequence at `cells[width + i]`, and
/// each inner cell the [`Sums::then`] of its two children.
#[derive(Debug, Clone)]
pub(super) struct Prefixes {
    width: usize,
    cells: Vec<Sums>,
}

impl Prefixes {
    /// The sequence `values`, in order.
    pub(super) fn of(values: impl ExactSizeIterator<Item = i128>) -> Self {
        let width = values.len().next_power_of_two();
        let mut cells = vec![Sums::default(); 2 * width];
        for (at, value) in values.enumerate() {
            cells[width + at] = Sums::one(value);
        }
        for cell in (1..width).rev() {
            cells[cell] = cells[2 * cell].then(cells[2 * cell + 1]);
        }
        Prefixes { width, cells }
    }

    /// Element `at` made `value`.
    pub(super) fn set(&mut self, at: usize, value: i128) {
        let mut cell = self.width + at;
        self.cells[cell] = Sums::one(value);
        while cell > 1 {
            cell /= 2;
            self.cells[cell] = self.cells[2 * cell].then(self.cells[2 * cell + 1]);
        }
    }

    /// Element `at`.
    pub(super) fn get(&self, at: usize) -> i128 {
        self.cells[self.width + at].total
    }

    /// The sums of the elements `from..to`, in order.
    pub(super) fn range(&self, from: usize, to: usize) -> Sums {
        let (mut low, mut high) = (from + self.width, to + self.width);
        let (mut left, mut right) = (Sums::default(), Sums::default());
        while low < high {
            if low % 2 == 1 {
                left = left.then(self.cells[low]);
                low += 1;
            }
            if high % 2 == 1 {
                high -= 1;
                right = self.cells[high].then(right);
            }
            low /= 2;
            high /= 2;
        }
        left.then(right)
    }
}
