//! Spans and their arithmetic, in the units of CPython's `timedelta`.

use chrono::Duration;

use super::{err, FplError};

/// A signed span, printed as `timedelta(...)` (SPEC §2).
pub type Delta = Duration;

/// The span, if it is a whole number of microseconds that fits in 64 bits
/// (about ±292,000 years). The literal reader and every smart constructor
/// of a term accept a span only under this rule, so a term never holds a
/// span that the grammar cannot print (SPEC §2).
///
/// # Errors
///
/// It refuses a span of more than `i64::MAX` microseconds in either
/// direction.
pub fn admit_span(name: &str, span: Delta) -> Result<Delta, FplError> {
    match span.num_microseconds() {
        Some(_) => Ok(span),
        None => err(format!("{name} must fit in 64 bits of microseconds")),
    }
}

/// For the evaluator; no promise. The span in whole microseconds. A span
/// holds at most `i64::MAX` milliseconds, so the count always fits in
/// 128 bits.
#[doc(hidden)]
#[inline]
pub fn us_of(d: Delta) -> i128 {
    i128::from(d.num_seconds()) * 1_000_000 + i128::from(d.subsec_nanos()) / 1_000
}

/// For the evaluator; no promise. The span of `us` microseconds, or the
/// longest span of the same sign where `us` is longer than a span holds.
#[doc(hidden)]
#[inline]
pub fn delta_of_us(us: i128) -> Delta {
    let seconds =
        i64::try_from(us.div_euclid(1_000_000)).unwrap_or(if us < 0 { i64::MIN } else { i64::MAX });
    let nanos = u32::try_from(us.rem_euclid(1_000_000) * 1_000).unwrap_or(0);
    Duration::new(seconds, nanos).unwrap_or(if us < 0 { Duration::MIN } else { Duration::MAX })
}

/// Python's `_divide_and_round`: floor division with a round-half-to-even fix-up.
#[inline]
fn divide_and_round(a: i128, b: i128) -> i128 {
    // Python's divmod floors toward −∞ for either sign of `b`.
    let mut q = a.div_euclid(b);
    let mut r = a.rem_euclid(b);
    if b < 0 && r != 0 {
        q += 1;
        r += b;
    }
    let r2 = r * 2;
    let greater_than_half = if b > 0 { r2 > b } else { r2 < b };
    if greater_than_half || (r2 == b && q.rem_euclid(2) == 1) {
        q += 1;
    }
    q
}

/// `timedelta / int` — exact microseconds, rounded half to even, as CPython.
/// `n` is not zero.
#[inline]
pub fn div_delta(d: Delta, n: i64) -> Delta {
    delta_of_us(divide_and_round(us_of(d), i128::from(n)))
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
/// A float whose span is longer than a span holds gives the longest span of
/// its sign.
pub fn delta_from_hours(hours: f64) -> Delta {
    let seconds = hours * 3600.0;
    let whole = seconds.trunc();
    let frac = seconds - whole;
    delta_of_us((whole as i128).saturating_mul(1_000_000) + i128::from(round_half_even(frac * 1e6)))
}

fn round_half_even(x: f64) -> i64 {
    let away = x.round(); // half away from zero
    if (x - x.trunc()).abs() == 0.5 && away % 2.0 != 0.0 {
        (away - x.signum()) as i64
    } else {
        away as i64
    }
}
