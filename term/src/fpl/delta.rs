//! Spans and their arithmetic, in the units of CPython's `timedelta`.

use chrono::Duration;

use prodrome_data::instant::Instant;

/// A signed span, printed as `timedelta(...)` (SPEC §2).
pub type Delta = Duration;

/// For the core; no promise. The span in whole microseconds.
#[doc(hidden)]
#[inline]
pub fn us_of(d: Delta) -> i64 {
    d.num_microseconds()
        .expect("timedelta out of microsecond range")
}

/// For the core; no promise. `now` moved by `us` microseconds.
#[doc(hidden)]
#[inline]
pub fn after(now: Instant, us: i64) -> Instant {
    now.checked_add_signed(Duration::microseconds(us))
        .expect("datetime overflow")
}

/// Python's `_divide_and_round`: floor division with a round-half-to-even fix-up.
fn divide_and_round(a: i64, b: i64) -> i64 {
    // Python's divmod floors toward −∞ for either sign of `b`.
    let mut q = a.div_euclid(b);
    let mut r = a.rem_euclid(b);
    if b < 0 && r != 0 {
        q += 1;
        r += b;
    }
    let r2 = r.checked_mul(2).expect("divide_and_round overflow");
    let greater_than_half = if b > 0 { r2 > b } else { r2 < b };
    if greater_than_half || (r2 == b && q.rem_euclid(2) == 1) {
        q += 1;
    }
    q
}

/// `timedelta / int` — exact microseconds, rounded half to even, as CPython.
#[inline]
pub fn div_delta(d: Delta, n: i64) -> Delta {
    Duration::microseconds(divide_and_round(us_of(d), n))
}

/// For the core; no promise. `timedelta / timedelta`, the float ratio that
/// CPython computes from microseconds.
#[doc(hidden)]
#[inline]
pub fn ratio(a: Delta, b: Delta) -> f64 {
    us_of(a) as f64 / us_of(b) as f64
}

/// `timedelta.total_seconds()`: exact microseconds over 1e6.
#[inline]
pub fn total_seconds(d: Delta) -> f64 {
    us_of(d) as f64 / 1e6
}

/// The span in hours, as a float.
pub fn hours(d: Delta) -> f64 {
    total_seconds(d) / 3600.0
}

/// `timedelta(hours=…)` for a float, with CPython's round-half-even microsecond.
pub fn delta_from_hours(hours: f64) -> Delta {
    let seconds = hours * 3600.0;
    let whole = seconds.trunc();
    let frac = seconds - whole;
    Duration::microseconds((whole as i64) * 1_000_000 + round_half_even(frac * 1e6))
}

fn round_half_even(x: f64) -> i64 {
    let away = x.round(); // half away from zero
    if (x - x.trunc()).abs() == 0.5 && away % 2.0 != 0.0 {
        (away - x.signum()) as i64
    } else {
        away as i64
    }
}
