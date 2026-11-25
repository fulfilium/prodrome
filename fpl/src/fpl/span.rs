//! The arithmetic of spans that the evaluator does for each node of a term.

use chrono::Duration;

use prodrome_term::fpl::{advance, delta_of_us, div_delta, us_of, Delta, Instant};

use super::WITHIN_SAMPLES;

/// `timedelta / timedelta`, the float ratio that CPython computes from
/// microseconds. `b` is not zero. A span of under 292,000 years takes the
/// 64-bit arm, which is faster, and any other span takes the 128-bit arm.
/// Both arms give the same value.
#[inline]
pub(super) fn ratio(a: Delta, b: Delta) -> f64 {
    match (a.num_microseconds(), b.num_microseconds()) {
        (Some(a), Some(b)) => a as f64 / b as f64,
        _ => us_of(a) as f64 / us_of(b) as f64,
    }
}

/// The remainder of `a` by `b` in whole microseconds, Euclidean, so never
/// negative. `b` is not zero. The two arms are the same as for [`ratio`].
#[inline]
pub(super) fn rem_euclid(a: Delta, b: Delta) -> Delta {
    match (a.num_microseconds(), b.num_microseconds()) {
        (Some(a), Some(b)) => Duration::microseconds(a.rem_euclid(b)),
        _ => delta_of_us(us_of(a).rem_euclid(us_of(b))),
    }
}

/// The 65 samples of a `Within` over `window`: `now` moved by `i` steps for
/// `i` in `0..=64`, each step a 64th of the window rounded to the
/// microsecond. A sample that would leave the time axis stops at its end.
#[inline]
pub(super) fn within_instants(now: Instant, window: Delta) -> impl Iterator<Item = Instant> {
    let step = us_of(div_delta(window, WITHIN_SAMPLES));
    (0..=WITHIN_SAMPLES).map(move |i| advance(now, delta_of_us(step * i128::from(i))))
}
