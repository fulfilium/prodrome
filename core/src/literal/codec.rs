//! A codec between a Rust value and a literal of §2, for the values an
//! application's events carry.
//!
//! A type implements [`Literal`] when it has one stored form that reads back
//! exactly: `from_value(&x.to_value()) == Ok(x)`. The forms compose:
//!
//! - A record prints as a call of one [`Constructor`], with every field in
//!   the constructor's order, so its print is canonical by construction.
//!   [`record`] builds the call, and [`Fields`] reads it back by field name.
//! - A sum prints one constructor for each variant. [`Fields::among`] and
//!   [`arm`] tell a reader which variant a value is.
//! - A list prints as a tuple, and a set prints as a sorted tuple with each
//!   member once.
//! - An absent value prints as `None`.
//! - A leaf with a text form prints as a string, and its own constructor reads
//!   the string back. The macro [`text_literal!`](crate::text_literal)
//!   implements this for a type.
//! - A type with a finite set of values, each printed as a word, prints as
//!   that word. The macro [`word_literal!`](crate::word_literal) implements
//!   this for a type.
//!
//! The module depends on no schema. A schema's `to_value` and `from_value`
//! ([`crate::schema::Schema`]) can use it to print and read their events'
//! fields.

#![warn(clippy::pedantic)]

use std::collections::BTreeSet;
use std::fmt::Display;

use chrono::NaiveDate;

use super::{Call, Datetime, Int, ProdromeError, Value};

/// A constructor's name and its fields in printed order. A [`super::Table`]
/// is a list of them.
pub type Constructor = (&'static str, &'static [&'static str]);

/// A type with one stored literal form, which reads back exactly:
/// `from_value(&x.to_value()) == Ok(x)`.
pub trait Literal: Sized {
    /// The literal of `self`.
    fn to_value(&self) -> Value;

    /// Reads a value back from its literal.
    ///
    /// # Errors
    /// A [`ProdromeError`] that names the part of `value` that does not read.
    fn from_value(value: &Value) -> Result<Self, ProdromeError>;
}

/// The call of `constructor` with `values`, one value for each field, in the
/// constructor's order.
///
/// A caller passes exactly one value for each field. Debug builds check the
/// count.
#[must_use]
pub fn record(constructor: Constructor, values: Vec<Value>) -> Value {
    headed(Vec::new(), constructor, values)
}

/// The call of `body`'s constructor with the fields of `head` first, and then
/// `values`, one value for each field of `body`, in its order.
///
/// An event schema uses it when every event starts with the same fields (for
/// example the entity, the instant and the actor) and each event kind adds
/// its own. Debug builds check that `values` has one value for each field.
#[must_use]
pub fn headed(head: Vec<(&'static str, Value)>, body: Constructor, values: Vec<Value>) -> Value {
    let (name, fields) = body;
    debug_assert_eq!(fields.len(), values.len(), "{name} prints every field");
    Value::call(
        name,
        head.into_iter()
            .chain(fields.iter().copied().zip(values))
            .map(|(field, value)| (field.to_owned(), value))
            .collect(),
    )
}

/// The fields of a call of one constructor, read by name.
#[derive(Debug, Clone, Copy)]
pub struct Fields<'v> {
    call: &'v Call,
}

impl<'v> Fields<'v> {
    /// Reads `value` as a call of one of `constructors`. A reader of a sum
    /// uses it, for example of an event schema's events.
    ///
    /// # Errors
    /// The value is not a call, or it calls a constructor that is not in
    /// `constructors`.
    pub fn among(value: &'v Value, constructors: &[Constructor]) -> Result<Self, ProdromeError> {
        let name = arm(value)?;
        let constructor = constructors
            .iter()
            .find(|(named, _)| *named == name)
            .ok_or_else(|| ProdromeError::invalid(format!("{name} is not one of this schema's")))?;
        Self::of(value, *constructor)
    }

    /// Reads `value` as a call of `constructor`.
    ///
    /// # Errors
    /// The value is not a call of that constructor's name.
    pub fn of(value: &'v Value, constructor: Constructor) -> Result<Self, ProdromeError> {
        match value.as_call() {
            Some(call) if call.name == constructor.0 => Ok(Self { call }),
            _ => Err(ProdromeError::invalid(format!(
                "expected {}(...), got {value:?}",
                constructor.0
            ))),
        }
    }

    /// The call, for a reader of a field that is not a [`Literal`].
    #[must_use]
    pub fn call(&self) -> &'v Call {
        self.call
    }

    /// The constructor's name.
    #[must_use]
    pub fn name(&self) -> &'v str {
        &self.call.name
    }

    /// Reads the field `name` as a `T`. Every field is required.
    ///
    /// # Errors
    /// The field is absent, or it does not read as a `T`. The error names the
    /// field.
    pub fn get<T: Literal>(&self, name: &str) -> Result<T, ProdromeError> {
        let value = self.call.field(name).ok_or_else(|| {
            ProdromeError::invalid(format!("{}(...) has no field {name}", self.call.name))
        })?;
        T::from_value(value).map_err(|failed| {
            ProdromeError::invalid(format!("{}.{name}: {failed}", self.call.name))
        })
    }
}

/// The constructor name of a call. A reader of a sum matches on it.
///
/// # Errors
/// The value is not a call.
pub fn arm(value: &Value) -> Result<&str, ProdromeError> {
    value
        .as_call()
        .map(|call| call.name.as_str())
        .ok_or_else(|| {
            ProdromeError::invalid(format!("expected a constructor call, got {value:?}"))
        })
}

/// The error for a constructor `name` that is not a variant of the sum
/// `what`.
#[must_use]
pub fn no_arm(what: &str, name: &str) -> ProdromeError {
    ProdromeError::invalid(format!("{name} is not {what}"))
}

/// Reads a leaf whose literal is its text: `value` must be a string, and
/// `T`'s `TryFrom<String>` reads it.
///
/// # Errors
/// The value is not a string, or `T` refuses the string.
pub fn text<T>(value: &Value) -> Result<T, ProdromeError>
where
    T: TryFrom<String>,
    T::Error: Display,
{
    T::try_from(String::from_value(value)?)
        .map_err(|refused| ProdromeError::invalid(refused.to_string()))
}

/// Implements [`Literal`] for each type whose literal is its text. The type
/// prints with the given function and reads back with
/// [`text`], through its `TryFrom<String>`:
///
/// ```
/// struct Tag(String);
///
/// impl TryFrom<String> for Tag {
///     type Error = String;
///     fn try_from(text: String) -> Result<Self, String> {
///         if text.is_empty() { Err("a tag is not empty".into()) } else { Ok(Tag(text)) }
///     }
/// }
///
/// prodrome::text_literal!(Tag => |tag| tag.0.clone());
/// ```
#[macro_export]
macro_rules! text_literal {
    ($($ty:ty => $print:expr),+ $(,)?) => {$(
        impl $crate::literal::codec::Literal for $ty {
            fn to_value(&self) -> $crate::literal::Value {
                let print: fn(&$ty) -> ::std::string::String = $print;
                $crate::literal::Value::str(print(self))
            }

            fn from_value(
                value: &$crate::literal::Value,
            ) -> ::std::result::Result<Self, $crate::literal::ProdromeError> {
                $crate::literal::codec::text(value)
            }
        }
    )+};
}

/// Reads a value of a type with a finite set of values, each of which
/// prints as a word: `value` must be a string, and the read returns the
/// first of `all` whose word `print` gives is that string.
///
/// # Errors
/// The value is not a string, or no value of `all` prints as it.
pub fn word<T>(
    value: &Value,
    all: impl IntoIterator<Item = T>,
    print: fn(&T) -> &str,
) -> Result<T, ProdromeError> {
    let text = value
        .as_str()
        .ok_or_else(|| ProdromeError::invalid(format!("expected a string, got {value:?}")))?;
    let mut words = Vec::new();
    for candidate in all {
        if print(&candidate) == text {
            return Ok(candidate);
        }
        words.push(print(&candidate).to_owned());
    }
    Err(ProdromeError::invalid(format!(
        "{text:?} is not one of {words:?}"
    )))
}

/// Implements [`Literal`] for each type with a finite set of values, each
/// of which prints as a word. The type prints with the given function, and
/// [`word`] reads it back from the list of all its values:
///
/// ```
/// #[derive(Clone, Copy)]
/// enum Size { Small, Large }
///
/// impl Size {
///     const ALL: [Size; 2] = [Size::Small, Size::Large];
///
///     fn as_str(self) -> &'static str {
///         match self { Size::Small => "small", Size::Large => "large" }
///     }
/// }
///
/// prodrome::word_literal!(Size: Size::ALL => |size| size.as_str());
/// ```
///
/// A value that is not in the list prints a word that does not read back,
/// so the list must hold every value of the type.
#[macro_export]
macro_rules! word_literal {
    ($($ty:ty: $all:expr => $print:expr),+ $(,)?) => {$(
        impl $crate::literal::codec::Literal for $ty {
            fn to_value(&self) -> $crate::literal::Value {
                let print: fn(&$ty) -> &str = $print;
                $crate::literal::Value::str(print(self))
            }

            fn from_value(
                value: &$crate::literal::Value,
            ) -> ::std::result::Result<Self, $crate::literal::ProdromeError> {
                $crate::literal::codec::word(value, $all, $print)
            }
        }
    )+};
}

impl Literal for String {
    fn to_value(&self) -> Value {
        Value::str(self.as_str())
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| ProdromeError::invalid(format!("expected a string, got {value:?}")))
    }
}

impl Literal for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        match value {
            Value::Bool(flag) => Ok(*flag),
            other => Err(ProdromeError::invalid(format!(
                "expected a bool, got {other:?}"
            ))),
        }
    }
}

/// Implements [`Literal`] for unsigned integer types. A literal integer has
/// any number of digits, so each value prints as itself. A read refuses a
/// negative integer and one that does not fit the type.
macro_rules! natural {
    ($($ty:ty),+) => {$(
        impl Literal for $ty {
            fn to_value(&self) -> Value {
                // An unsigned integer prints as decimal digits with no sign
                // and no leading zero, which is the canonical form `Int` holds.
                Value::Int(Int { negative: false, digits: self.to_string() })
            }

            fn from_value(value: &Value) -> Result<Self, ProdromeError> {
                match value {
                    Value::Int(int) if !int.is_negative() => int.digits().parse().ok(),
                    _ => None,
                }
                .ok_or_else(|| {
                    ProdromeError::invalid(format!(
                        "expected a natural number of at most {}, got {value:?}",
                        <$ty>::MAX
                    ))
                })
            }
        }
    )+};
}
natural!(u32, u64, usize);

impl Literal for Datetime {
    fn to_value(&self) -> Value {
        Value::Datetime(*self)
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        match value {
            Value::Datetime(at) => Ok(*at),
            other => Err(ProdromeError::invalid(format!(
                "expected a datetime, got {other:?}"
            ))),
        }
    }
}

/// A day prints as its ISO 8601 text, `YYYY-MM-DD`.
impl Literal for NaiveDate {
    fn to_value(&self) -> Value {
        Value::str(self.format("%Y-%m-%d").to_string())
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        let text = String::from_value(value)?;
        NaiveDate::parse_from_str(&text, "%Y-%m-%d")
            .map_err(|failed| ProdromeError::invalid(format!("{text:?} is no day: {failed}")))
    }
}

impl<T: Literal> Literal for Option<T> {
    fn to_value(&self) -> Value {
        self.as_ref().map_or(Value::None, Literal::to_value)
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        match value {
            Value::None => Ok(None),
            other => T::from_value(other).map(Some),
        }
    }
}

impl<T: Literal> Literal for Vec<T> {
    fn to_value(&self) -> Value {
        Value::Tuple(self.iter().map(Literal::to_value).collect())
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        value
            .as_tuple()
            .ok_or_else(|| ProdromeError::invalid(format!("expected a tuple, got {value:?}")))?
            .iter()
            .map(T::from_value)
            .collect()
    }
}

/// A set prints as a tuple, sorted, with each member once. A read refuses
/// any other order and a repeated member, so a set has one print.
impl<T: Literal + Ord> Literal for BTreeSet<T> {
    fn to_value(&self) -> Value {
        Value::Tuple(self.iter().map(Literal::to_value).collect())
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        let listed = Vec::<T>::from_value(value)?;
        if listed.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ProdromeError::invalid(
                "a set is printed sorted and each member once",
            ));
        }
        Ok(listed.into_iter().collect())
    }
}
