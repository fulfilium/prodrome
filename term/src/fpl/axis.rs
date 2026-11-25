//! The time axis: the instants that a term is read at, and motion along it
//! (SPEC §7, "The time axis").

use chrono::{NaiveDate, NaiveDateTime};

use super::{Delta, Instant};

/// The first instant of the axis: the first instant of the calendar type.
pub const FIRST: Instant = NaiveDateTime::MIN;

/// The last instant of the axis: the last whole microsecond of the calendar
/// type, `+262142-12-31T23:59:59.999999`.
pub const LAST: Instant = match NaiveDate::MAX.and_hms_micro_opt(23, 59, 59, 999_999) {
    Some(last) => last,
    None => NaiveDateTime::MAX,
};

/// For the evaluator; no promise. `at` moved `by` later, or earlier where
/// `by` is negative. A move that leaves the axis stops at the end that it
/// reaches. It is the one place where the evaluator adds a span to an
/// instant, so no sum of an instant and a span can overflow.
#[doc(hidden)]
#[inline]
#[must_use]
pub fn advance(at: Instant, by: Delta) -> Instant {
    match at.checked_add_signed(by) {
        Some(moved) => moved.min(LAST),
        None if by < Delta::zero() => FIRST,
        None => LAST,
    }
}

/// For the evaluator; no promise. `at` moved `by` earlier: [`advance`] by
/// the negated span.
#[doc(hidden)]
#[inline]
#[must_use]
pub fn retreat(at: Instant, by: Delta) -> Instant {
    match at.checked_sub_signed(by) {
        Some(moved) => moved.min(LAST),
        None if by > Delta::zero() => FIRST,
        None => LAST,
    }
}
