//! The arithmetic of spans in microseconds that the evaluator does for
//! each node of a term.

use chrono::Duration;

use prodrome_term::fpl::{us_of, Delta, Instant};

/// `now` moved by `us` microseconds.
#[inline]
pub(super) fn after(now: Instant, us: i64) -> Instant {
    now.checked_add_signed(Duration::microseconds(us))
        .expect("datetime overflow")
}

/// `timedelta / timedelta`, the float ratio that CPython computes from
/// microseconds.
#[inline]
pub(super) fn ratio(a: Delta, b: Delta) -> f64 {
    us_of(a) as f64 / us_of(b) as f64
}
