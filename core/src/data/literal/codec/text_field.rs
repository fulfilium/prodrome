//! A field codec for `#[literal(with = text_field)]`: a field whose literal is
//! its text, read back as a `String` for the type's smart constructor
//! (`#[literal(new = ...)]`) to check.

#![warn(clippy::pedantic)]

use std::fmt::Display;

use super::Literal;
use crate::data::literal::{ProdromeError, Value};

/// The text of `value`, as a string.
pub fn to_value<T: AsRef<str>>(value: &T) -> Value {
    Value::str(value.as_ref())
}

/// The string of `value`, for the smart constructor to check.
///
/// # Errors
///
/// `value` is not a string.
pub fn from_field(value: &Value, context: &dyn Display) -> Result<String, ProdromeError> {
    String::from_field(value, context)
}
