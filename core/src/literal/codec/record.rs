//! A type whose literal is a call of one constructor, and the reads of a
//! call's fields that a derived [`Literal`] impl makes.
//!
//! A read names the field that it refuses. A call that lacks a required
//! field is refused as `C(...) is missing f`, and a field that does not
//! read is refused with the message of the field type's
//! [`Literal::from_field`], in the context `C.f`. These are the messages
//! that the core's own kinds and the reference payload give (SPEC §4).

#![warn(clippy::pedantic)]

use std::fmt::{self, Display};

use super::{Constructor, Literal};
use crate::literal::{Call, ProdromeError, Value};

/// A type whose literal is a call of one constructor. `#[derive(Literal)]`
/// implements it for a struct with named fields.
pub trait Record: Literal {
    /// The constructor, with its fields in print order.
    const CONSTRUCTOR: Constructor;

    /// The values of the fields, in the order of
    /// [`Record::CONSTRUCTOR`].
    fn values(&self) -> Vec<Value>;

    /// Reads the fields of `call` by name, whatever the name of `call`.
    /// A field of `call` that the constructor does not name is not read.
    ///
    /// # Errors
    ///
    /// A field that is missing or does not read, or a refusal of the
    /// type's smart constructor.
    fn from_call(call: &Call) -> Result<Self, ProdromeError>;

    /// Each field's name with its value, in print order.
    fn fields(&self) -> Vec<(&'static str, Value)> {
        Self::CONSTRUCTOR
            .1
            .iter()
            .copied()
            .zip(self.values())
            .collect()
    }
}

/// The context of a field in a refusal: `C.f`, for the field `f` of a call
/// of `C`. A read formats it only when it refuses.
#[derive(Debug, Clone, Copy)]
pub struct Site<'a> {
    constructor: &'a str,
    field: &'a str,
}

impl Display for Site<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.constructor, self.field)
    }
}

/// A function that reads a field's value, given the field's context.
pub type Reader<T> = fn(&Value, &dyn Display) -> Result<T, ProdromeError>;

/// The value of the field `name` of `call`.
///
/// # Errors
///
/// `call` lacks the field.
pub fn required<'a>(call: &'a Call, name: &str) -> Result<&'a Value, ProdromeError> {
    call.field(name)
        .ok_or_else(|| ProdromeError::invalid(format!("{}(...) is missing {name}", call.name)))
}

/// Reads the field `name` of `call` with `reader`. The field is required.
///
/// # Errors
///
/// `call` lacks the field, or `reader` refuses its value.
pub fn read<T>(call: &Call, name: &str, reader: Reader<T>) -> Result<T, ProdromeError> {
    reader(required(call, name)?, &site(call, name))
}

/// Reads the field `name` of `call` with `reader`, or the `Default` of `T`
/// where `call` lacks the field.
///
/// # Errors
///
/// `reader` refuses the field's value.
pub fn read_or_default<T: Default>(
    call: &Call,
    name: &str,
    reader: Reader<T>,
) -> Result<T, ProdromeError> {
    call.field(name).map_or_else(
        || Ok(T::default()),
        |value| reader(value, &site(call, name)),
    )
}

fn site<'a>(call: &'a Call, field: &'a str) -> Site<'a> {
    Site {
        constructor: &call.name,
        field,
    }
}

/// `value` as a call of any constructor.
///
/// # Errors
///
/// `value` is not a call.
pub fn call(value: &Value) -> Result<&Call, ProdromeError> {
    value.as_call().ok_or_else(|| {
        ProdromeError::invalid(format!("expected a constructor call, got {value:?}"))
    })
}

/// `value` as a call of the constructor `name`.
///
/// # Errors
///
/// `value` is not a call of `name`.
pub fn call_of<'a>(value: &'a Value, name: &str) -> Result<&'a Call, ProdromeError> {
    match value.as_call() {
        Some(call) if call.name == name => Ok(call),
        other => Err(ProdromeError::invalid(format!(
            "expected a {name}(...), got {other:?}"
        ))),
    }
}
