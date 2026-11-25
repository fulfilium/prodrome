//! The arithmetic of spans that the evaluator does for each node of a term.

use prodrome_term::fpl::{advance, delta_of_us, div_delta, us_of, Delta, Instant};

use super::WITHIN_SAMPLES;

/// `timedelta / timedelta`, the float ratio that CPython computes from
/// microseconds.
#[inline]
pub(super) fn ratio(a: Delta, b: Delta) -> f64 {
    us_of(a) as f64 / us_of(b) as f64
}

/// The 65 samples of a `Within` over `window`: `now` moved by `i` steps for
/// `i` in `0..=64`, each step a 64th of the window rounded to the
/// microsecond. A sample that would leave the time axis stops at its end.
#[inline]
pub(super) fn within_instants(now: Instant, window: Delta) -> impl Iterator<Item = Instant> {
    let step = us_of(div_delta(window, WITHIN_SAMPLES));
    (0..=WITHIN_SAMPLES).map(move |i| advance(now, delta_of_us(step * i128::from(i))))
}
