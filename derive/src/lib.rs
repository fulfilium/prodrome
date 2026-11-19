//! `#[derive(Literal)]`: the stored form of a type, written from the type.
//!
//! A type that a store holds needs a `Literal` impl
//! (`prodrome::literal::codec::Literal`): its constructors with their field
//! names, a print and a read. This derive writes all three from the type
//! declaration. The core re-exports it, so a program writes
//! `#[derive(prodrome::literal::codec::Literal)]`.
//!
//! See [`macro@Literal`] for the forms, the options and the rule for
//! stored names.

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]

mod expand;
mod options;
mod shape;

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

/// Writes `Literal` for a record, a sum or a newtype.
///
/// # The forms
///
/// - **A record**, a struct with named fields, is one constructor. Its
///   literal is a call `Name(field=value, ...)` with every stored field in
///   declaration order. The derive also writes `Record`, which gives the
///   constructor as data (`Record::CONSTRUCTOR`), the field values, and a
///   read of a call whatever its name (`Record::from_call`). A unit struct
///   is a constructor with no fields.
/// - **A sum**, an enum, has one constructor for each variant. A variant
///   has named fields or none. A read refuses a call of a name that no
///   variant has.
/// - **A newtype**, a struct with one unnamed field, prints and reads as
///   its field.
///
/// Each field prints and reads through its type's `Literal` impl, whether
/// that impl is derived or written by hand. A derived type is a `Literal`
/// like any other, so a hand-written impl can hold it too.
///
/// # Stored names
///
/// The name of a constructor and the names of its fields are in every
/// stored object, and the hash of an object is the hash of its print. A
/// changed name is therefore a changed stored form, and objects of the old
/// form no longer read.
///
/// The rule: a stored name is the Rust identifier (without `r#`), unless
/// `#[literal(rename = "...")]` gives one. To rename the Rust item and keep
/// the stored form, give the old name with `rename`. Every derived type
/// gives its constructors as data in `Literal::CONSTRUCTORS`, each with its
/// stored fields in print order. A schema pins the constructors of all its
/// types in one test, so that a rename that changes a stored name fails the
/// test. The reference schema of `prodrome-core` does this in
/// `core/tests/all/derive.rs`.
///
/// # Options
///
/// - `#[literal(rename = "Name")]` on a struct, a variant or a named field:
///   the stored name.
/// - `#[literal(default)]` on a field: a call that lacks the field reads as
///   the `Default` of the type that the field reads as. The print always
///   holds the field.
/// - `#[literal(skip)]` on a field: the literal does not hold the field, and
///   a read sets it to its type's `Default`.
/// - `#[literal(with = path)]` on a field: the field prints with
///   `path::to_value(&field) -> Value` and reads with
///   `path::from_field(&Value, &dyn Display) -> Result<T, ProdromeError>`.
///   `prodrome::literal::codec::text_field` and
///   `prodrome::literal::codec::blank_field`
///   are two such modules.
/// - `#[literal(new = path)]` on a struct: a read calls
///   `path(field, ...) -> Result<Self, ProdromeError>` with the stored fields
///   in print order, so that a smart constructor checks the value. With
///   `new`, a field may read as another type than its own: the type that
///   its `with` module returns.
///
/// # Refusals
///
/// The derive refuses a type that has no literal form, with a message that
/// says what to write: a generic type, a struct with two or more unnamed
/// fields, a variant with unnamed fields, an enum with no variants, a union,
/// two constructors of a sum with one name, two fields of a constructor
/// with one name, and a stored name that the literal grammar cannot read.
///
/// ```compile_fail
/// #[derive(prodrome::literal::codec::Literal)]
/// enum Shape {
///     Circle(u32),
/// }
/// ```
///
/// ```compile_fail
/// #[derive(prodrome::literal::codec::Literal)]
/// struct Wrapped<T> {
///     value: T,
/// }
/// ```
///
/// ```compile_fail
/// #[derive(prodrome::literal::codec::Literal)]
/// enum Verdict {
///     Accept,
///     #[literal(rename = "Accept")]
///     Approve,
/// }
/// ```
///
/// # A record
///
/// ```
/// use prodrome::literal::codec::{Literal, Record};
/// use prodrome::literal::{print_literal, Value};
///
/// /// A comment on a review, by `ada`.
/// #[derive(Debug, PartialEq, Literal)]
/// #[literal(rename = "Comment")]
/// struct Remark {
///     author: String,
///     lines: Vec<String>,
///     #[literal(default)]
///     resolved: bool,
/// }
///
/// let remark = Remark {
///     author: "ada".to_owned(),
///     lines: vec!["one".to_owned()],
///     resolved: false,
/// };
/// assert_eq!(
///     print_literal(&remark.to_value()),
///     "Comment(author='ada', lines=('one',), resolved=False)"
/// );
/// assert_eq!(Remark::CONSTRUCTOR, ("Comment", &["author", "lines", "resolved"][..]));
/// assert_eq!(Remark::from_value(&remark.to_value()), Ok(remark));
///
/// // A call that lacks `resolved` reads it as `false`.
/// let short = Value::call(
///     "Comment",
///     vec![
///         ("author".to_owned(), Value::str("ada")),
///         ("lines".to_owned(), Value::Tuple(vec![])),
///     ],
/// );
/// assert!(!Remark::from_value(&short).expect("a comment").resolved);
/// ```
///
/// # A sum
///
/// ```
/// use prodrome::literal::codec::Literal;
/// use prodrome::literal::print_literal;
///
/// /// What a reviewer decided.
/// #[derive(Debug, PartialEq, Literal)]
/// enum Verdict {
///     Accept,
///     Revise { reason: String },
/// }
///
/// let verdict = Verdict::Revise { reason: "typo".to_owned() };
/// assert_eq!(print_literal(&verdict.to_value()), "Revise(reason='typo')");
/// assert_eq!(print_literal(&Verdict::Accept.to_value()), "Accept()");
/// assert_eq!(
///     Verdict::CONSTRUCTORS,
///     &[("Accept", &[][..]), ("Revise", &["reason"][..])]
/// );
/// assert_eq!(Verdict::from_value(&verdict.to_value()), Ok(verdict));
/// ```
#[proc_macro_derive(Literal, attributes(literal))]
pub fn derive_literal(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    shape::Derived::parse(&input)
        .map_or_else(syn::Error::into_compile_error, |derived| {
            expand::expand(&derived)
        })
        .into()
}

#[cfg(test)]
mod tests;
