//! The two waters of a view: the cost that starts a merge, and the cost
//! that a merge closes down to.

/// The high water and the low water of a view, in bytes, with the low
/// water at or below the high water.
///
/// An arrival that puts the view over the high water makes closings until the
/// view costs at most the low water or no closing is admissible (design §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Waters {
    high: u64,
    low: u64,
}

impl Waters {
    /// The waters `high` and `low`, or `None` when `low` is above `high`.
    #[must_use]
    pub fn new(high: u64, low: u64) -> Option<Waters> {
        (low <= high).then_some(Waters { high, low })
    }

    /// One budget as both waters: the view closes at every arrival that
    /// puts it over the budget, and only until it fits again.
    #[must_use]
    pub fn at(budget: u64) -> Waters {
        Waters {
            high: budget,
            low: budget,
        }
    }

    /// The cost above which an arrival starts a merge.
    #[must_use]
    pub fn high(&self) -> u64 {
        self.high
    }

    /// The cost that a merge closes down to.
    #[must_use]
    pub fn low(&self) -> u64 {
        self.low
    }
}
