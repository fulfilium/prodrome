//! A field codec for `#[literal(with = text_field)]`: a text read back as a
//! `String` for the smart constructor (`#[literal(new = ...)]`) to check.

#![warn(clippy::pedantic)]

use std::fmt::Display;

use super::Literal;
use crate::literal::{ProdromeError, Value};

pub fn to_value<T: AsRef<str>>(value: &T) -> Value {
    Value::str(value.as_ref())
}

/// # Errors
///
/// `value` is not a string.
pub fn from_field(value: &Value, context: &dyn Display) -> Result<String, ProdromeError> {
    String::from_field(value, context)
}
