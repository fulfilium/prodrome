//! A field codec for `#[literal(with = blank_field)]`: an optional text that
//! prints as `''` when absent (SPEC §4), read back as a `String` for the
//! smart constructor (`#[literal(new = ...)]`) to check.

#![warn(clippy::pedantic)]

use crate::literal::Value;

/// The text of `value`, or `''`; it takes `&Option` because the derive passes
/// a reference to the field.
#[allow(clippy::ref_option)]
pub fn to_value<T: AsRef<str>>(value: &Option<T>) -> Value {
    Value::str(value.as_ref().map_or("", AsRef::as_ref))
}

pub use super::text_field::from_field;
