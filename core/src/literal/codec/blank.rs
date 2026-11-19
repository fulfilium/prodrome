//! A field codec for `#[literal(with = blank)]`: an optional text that
//! prints as `''` when it is absent, read back as a `String` for the type's
//! smart constructor (`#[literal(new = ...)]`) to check.
//!
//! A shipped string field of the core uses `''` for "absent" (SPEC §4), so
//! that absence has one spelling.

#![warn(clippy::pedantic)]

use crate::literal::Value;

/// The text of `value`, or `''` where it is absent.
///
/// The derive passes a reference to the field, so the parameter is a
/// reference to an `Option`.
#[allow(clippy::ref_option)]
pub fn to_value<T: AsRef<str>>(value: &Option<T>) -> Value {
    Value::str(value.as_ref().map_or("", AsRef::as_ref))
}

pub use super::text::from_field;
