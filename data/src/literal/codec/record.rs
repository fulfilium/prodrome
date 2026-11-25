//! The reads of a call's fields that a derived [`Literal`] impl makes.
//!
//! A missing field is refused as `C(...) is missing f`, and a field that does
//! not read is refused in the context `C.f`: the messages of the core's own
//! kinds and of the reference payload (SPEC §4).

#![warn(clippy::pedantic)]

use std::fmt::{self, Display};

use super::{Constructor, Literal};
use crate::literal::{Call, ProdromeError, Value};

/// A type whose literal is a call of one constructor.
pub trait Record: Literal {
    const CONSTRUCTOR: Constructor;

    /// The values of the fields, in the order of [`Record::CONSTRUCTOR`].
    fn values(&self) -> Vec<Value>;

    /// Reads the fields of `call` by name, whatever its name, and ignores a
    /// field that the constructor does not name.
    ///
    /// # Errors
    ///
    /// A field is missing or does not read, or the smart constructor refuses.
    fn from_call(call: &Call) -> Result<Self, ProdromeError>;

    fn fields(&self) -> Vec<(&'static str, Value)> {
        Self::CONSTRUCTOR
            .1
            .iter()
            .copied()
            .zip(self.values())
            .collect()
    }
}

/// For this library's own code; no promise. The context `C.f` of a field in a refusal, formatted only on a
/// refusal.
#[doc(hidden)]
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

/// For this library's own code; no promise. How a derived field is read.
#[doc(hidden)]
pub type Reader<T> = fn(&Value, &dyn Display) -> Result<T, ProdromeError>;

/// # Errors
///
/// `call` lacks the field.
pub(crate) fn required<'a>(call: &'a Call, name: &str) -> Result<&'a Value, ProdromeError> {
    call.field(name)
        .ok_or_else(|| ProdromeError::invalid(format!("{}(...) is missing {name}", call.name)))
}

/// For this library's own code; no promise. Reads the required field `name` of `call` with
/// `reader`.
///
/// # Errors
///
/// `call` lacks the field, or `reader` refuses its value.
#[doc(hidden)]
pub fn read<T>(call: &Call, name: &str, reader: Reader<T>) -> Result<T, ProdromeError> {
    reader(required(call, name)?, &site(call, name))
}

/// For this library's own code; no promise. Reads the field `name` of `call` with `reader`, or
/// the `Default` of `T` where `call` lacks the field.
///
/// # Errors
///
/// `reader` refuses the field's value.
#[doc(hidden)]
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

/// # Errors
///
/// `value` is not a call.
pub fn call(value: &Value) -> Result<&Call, ProdromeError> {
    value.as_call().ok_or_else(|| {
        ProdromeError::invalid(format!("expected a constructor call, got {value:?}"))
    })
}

/// For this library's own code; no promise. The call that `value` holds, if it is a call of
/// `name`.
///
/// # Errors
///
/// `value` is not a call of `name`.
#[doc(hidden)]
pub fn call_of<'a>(value: &'a Value, name: &str) -> Result<&'a Call, ProdromeError> {
    match value.as_call() {
        Some(call) if call.name == name => Ok(call),
        other => Err(ProdromeError::invalid(format!(
            "expected a {name}(...), got {other:?}"
        ))),
    }
}
