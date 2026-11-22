//! The instant of an event: a stored [`Datetime`] as a [`chrono`] time, and
//! its ISO text (SPEC §2). The grammar's time records carry no arithmetic,
//! and `chrono`'s do.

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use prodrome_data::literal::{Datetime, ProdromeError};
use thiserror::Error;

/// A naive local time: a stored instant never carries a time zone (SPEC §2).
pub type Instant = NaiveDateTime;

/// A refusal of malformed text: an ISO instant, a term, or a smart
/// constructor's argument. Malformed input gets this refusal, never a crash
/// (SPEC §2).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct FplError(pub String);

/// A term's refusal, as the store's refusal. The two layers keep their own
/// error types because their vocabularies of failure differ: a store reports
/// a missing parent, a term an out-of-range exponent.
impl From<FplError> for ProdromeError {
    fn from(error: FplError) -> ProdromeError {
        ProdromeError::Invalid(error.0)
    }
}

/// A grammar refusal, for a caller who asked for a term.
impl From<ProdromeError> for FplError {
    fn from(error: ProdromeError) -> FplError {
        FplError(error.to_string())
    }
}

/// The grammar's `datetime` as an [`Instant`].
#[inline]
pub fn instant_of(at: Datetime) -> Instant {
    NaiveDate::from_ymd_opt(at.year(), at.month(), at.day())
        .and_then(|day| {
            day.and_hms_micro_opt(at.hour(), at.minute(), at.second(), at.microsecond())
        })
        .expect("literal::Datetime::new admits only representable instants")
}

/// The inverse of [`instant_of`]. It refuses a year outside 1..=9999, the
/// years of the grammar.
#[inline]
pub fn datetime_of(t: Instant) -> Result<Datetime, ProdromeError> {
    Datetime::new(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        t.and_utc().timestamp_subsec_micros(),
    )
}

/// The text of Python's `datetime.isoformat()`, with microseconds only when
/// they are not zero.
pub fn iso(t: Instant) -> String {
    if t.and_utc().timestamp_subsec_micros() == 0 {
        t.format("%Y-%m-%dT%H:%M:%S").to_string()
    } else {
        t.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
    }
}

/// The inverse of [`iso`]. It also reads a date alone, as midnight.
pub fn parse_iso(s: &str) -> Result<Instant, FplError> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S"))
        .or_else(|_| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(0, 0, 0).expect("midnight is a time"))
        })
        .map_err(|e| FplError(format!("not an ISO instant: {s:?} ({e})")))
}
