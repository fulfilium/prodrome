//! Reads of the fields of a stored call, for a hand-written read of a
//! constructor.
//!
//! Each helper is a read of `literal::codec`, which a derived `Literal` impl
//! makes too, so a derived read and a hand-written one refuse with the same
//! messages.

use crate::literal::codec::{self, Literal};
use crate::literal::{Call, Datetime, ProdromeError, Value};

/// The value of the required field `name`.
pub fn required<'a>(call: &'a Call, name: &str) -> Result<&'a Value, ProdromeError> {
    codec::required(call, name)
}

pub fn as_string(value: &Value, context: &str) -> Result<String, ProdromeError> {
    String::from_field(value, &context)
}

pub fn string_field(call: &Call, name: &str) -> Result<String, ProdromeError> {
    codec::read(call, name, String::from_field)
}

/// A string field where `""` means absent. A missing field reads as `""`,
/// so a literal that omits the trailing defaults parses as the canonical
/// print of the same value.
pub fn string_or_empty(call: &Call, name: &str) -> Result<String, ProdromeError> {
    codec::read_or_default(call, name, String::from_field)
}

pub fn datetime_field(call: &Call, name: &str) -> Result<Datetime, ProdromeError> {
    codec::read(call, name, Datetime::from_field)
}

pub fn bool_field(call: &Call, name: &str) -> Result<bool, ProdromeError> {
    codec::read(call, name, bool::from_field)
}

pub fn tuple_field<'a>(call: &'a Call, name: &str) -> Result<&'a [Value], ProdromeError> {
    codec::tuple(required(call, name)?, &format_args!("{}.{name}", call.name))
}

pub fn tuple_or_empty<'a>(call: &'a Call, name: &str) -> Result<&'a [Value], ProdromeError> {
    match call.field(name) {
        None => Ok(&[]),
        Some(value) => codec::tuple(value, &format_args!("{}.{name}", call.name)),
    }
}

pub fn strings(items: &[Value], context: &str) -> Result<Vec<String>, ProdromeError> {
    items.iter().map(|item| as_string(item, context)).collect()
}

/// `value` as a call of the constructor `name`.
pub fn expect_call<'a>(value: &'a Value, name: &str) -> Result<&'a Call, ProdromeError> {
    codec::call_of(value, name)
}
