//! A codec between Rust values and the literals of SPEC §2.
//!
//! A record prints as a call of one [`Constructor`] with every field in its
//! order, so its print is canonical. A sum prints one constructor for each
//! variant, a set prints as a sorted tuple with each member once, and an
//! absent value prints as `None`. `#[derive(Literal)]` writes the impl.

#![warn(clippy::pedantic)]

use std::collections::BTreeSet;
use std::fmt::Display;

use chrono::NaiveDate;

use super::{Call, Datetime, Int, ProdromeError, Value};

pub mod blank_field;
mod pinned;
mod record;
mod sum;
pub mod text_field;

pub use pinned::assert_pinned;
pub use prodrome_derive::Literal;
pub use record::{call, call_of, read, read_or_default, required, Reader, Record, Site};
pub use sum::{distinct, headed_len, headed_names, headed_table, names_field, Sum};

/// A constructor's name and its fields in printed order.
pub type Constructor = (&'static str, &'static [&'static str]);

/// A type with one stored literal form, which reads back exactly:
/// `from_value(&x.to_value()) == Ok(x)`.
///
/// [`Literal::from_value`] and [`Literal::from_field`] must accept the same
/// values and read each as the same value; only the words of a refusal differ.
pub trait Literal: Sized {
    /// The constructors that the type declares, each with its fields in print
    /// order (none for a leaf, a container or a newtype); pin them in a test
    /// with [`assert_pinned`].
    const CONSTRUCTORS: &'static [Constructor] = &[];

    fn to_value(&self) -> Value;

    /// # Errors
    /// A refusal names the part of `value` that does not read.
    fn from_value(value: &Value) -> Result<Self, ProdromeError>;

    /// Reads the value of the field `context`, and names the context in every
    /// refusal, so that a refusal in a nested record reads as a path, such as
    /// `Thread.first: Review.remark must be a string, got …`.
    ///
    /// # Errors
    /// A refusal names the field and the part of `value` that does not read.
    fn from_field(value: &Value, context: &dyn Display) -> Result<Self, ProdromeError> {
        Self::from_value(value).map_err(|refused| refused.within(context))
    }
}

/// The call of `constructor` with exactly one value for each field, in the
/// constructor's order; debug builds check the count.
#[must_use]
pub fn record(constructor: Constructor, values: Vec<Value>) -> Value {
    headed(Vec::new(), constructor, values)
}

/// The call of `body`'s constructor with the fields of `head` first, then
/// exactly one value for each field of `body`; debug builds check the count.
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

/// The fields of a call of one constructor, read by name, for a hand-written
/// impl; it refuses a value in the same words as a derived impl.
#[derive(Debug, Clone, Copy)]
pub struct Fields<'v> {
    call: &'v Call,
}

impl<'v> Fields<'v> {
    /// Reads `value` as a call of one of `constructors`, the constructors of
    /// the sum `sum`.
    ///
    /// # Errors
    /// The value is not a call of a constructor in `constructors`.
    pub fn among(
        value: &'v Value,
        sum: &str,
        constructors: &[Constructor],
    ) -> Result<Self, ProdromeError> {
        let call = call(value)?;
        if constructors.iter().any(|(name, _)| *name == call.name) {
            Ok(Self { call })
        } else {
            Err(no_arm(sum, &call.name))
        }
    }

    /// # Errors
    /// The value is not a call of `constructor`.
    pub fn of(value: &'v Value, constructor: Constructor) -> Result<Self, ProdromeError> {
        call_of(value, constructor.0).map(|call| Self { call })
    }

    /// For a reader of a field that is not a [`Literal`].
    #[must_use]
    pub fn call(&self) -> &'v Call {
        self.call
    }

    #[must_use]
    pub fn name(&self) -> &'v str {
        &self.call.name
    }

    /// Reads the required field `name`.
    ///
    /// # Errors
    /// The field is absent or does not read; the error names the field.
    pub fn get<T: Literal>(&self, name: &str) -> Result<T, ProdromeError> {
        read(self.call, name, T::from_field)
    }
}

/// The constructor name of a call, which a reader of a sum matches on.
///
/// # Errors
/// The value is not a call.
pub fn arm(value: &Value) -> Result<&str, ProdromeError> {
    call(value).map(|call| call.name.as_str())
}

/// The refusal of a constructor `name` that is not a variant of `sum`, which
/// derived and hand-written sums share.
#[must_use]
pub fn no_arm(sum: &str, name: &str) -> ProdromeError {
    ProdromeError::invalid(format!("{name} is not one of {sum}'s constructors"))
}

/// Reads a leaf whose literal is its text, through `T`'s `TryFrom<String>`.
///
/// # Errors
/// The value is not a string, or `T` refuses it.
pub fn text<T>(value: &Value) -> Result<T, ProdromeError>
where
    T: TryFrom<String>,
    T::Error: Display,
{
    T::try_from(String::from_value(value)?)
        .map_err(|refused| ProdromeError::invalid(refused.to_string()))
}

/// Implements [`Literal`] for each type whose literal is its text, printed
/// by the given function and read by [`text()`]:
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
/// prodrome_data::text_literal!(Tag => |tag| tag.0.clone());
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

/// Reads the first value of `all` whose word under `print` is the string
/// `value`.
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

/// Implements [`Literal`] for each type whose values each print as a word,
/// read back by [`word`] from the list of all the values:
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
/// prodrome_data::word_literal!(Size: Size::ALL => |size| size.as_str());
/// ```
///
/// The list must hold every value, or a value outside it does not read back.
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

    fn from_field(value: &Value, context: &dyn Display) -> Result<Self, ProdromeError> {
        value.as_str().map(str::to_owned).ok_or_else(|| {
            ProdromeError::invalid(format!("{context} must be a string, got {value:?}"))
        })
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

    fn from_field(value: &Value, context: &dyn Display) -> Result<Self, ProdromeError> {
        match value {
            Value::Bool(flag) => Ok(*flag),
            other => Err(ProdromeError::invalid(format!(
                "{context} must be a bool, got {other:?}"
            ))),
        }
    }
}

/// Implements [`Literal`] for unsigned integers; a read refuses a negative
/// integer and one that does not fit the type.
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

    fn from_field(value: &Value, context: &dyn Display) -> Result<Self, ProdromeError> {
        match value {
            Value::Datetime(at) => Ok(*at),
            other => Err(ProdromeError::invalid(format!(
                "{context} must be a datetime, got {other:?}"
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

    fn from_field(value: &Value, context: &dyn Display) -> Result<Self, ProdromeError> {
        match value {
            Value::None => Ok(None),
            other => T::from_field(other, context).map(Some),
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

    fn from_field(value: &Value, context: &dyn Display) -> Result<Self, ProdromeError> {
        tuple(value, context)?
            .iter()
            .map(|item| T::from_field(item, context))
            .collect()
    }
}

/// The items of `value`, the value of the field `context`.
///
/// # Errors
///
/// `value` is not a tuple.
pub fn tuple<'v>(value: &'v Value, context: &dyn Display) -> Result<&'v [Value], ProdromeError> {
    value
        .as_tuple()
        .ok_or_else(|| ProdromeError::invalid(format!("{context} must be a tuple")))
}

/// A set prints as a sorted tuple with each member once; a read refuses any
/// other order, so a set has one print.
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
