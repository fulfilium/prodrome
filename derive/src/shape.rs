//! The shape of a type that derives `Literal`: what the derive reads from
//! the type, checked once. A value of [`Shape`] is a type that has a
//! literal form, so the code generation refuses nothing.

use std::collections::BTreeSet;

use proc_macro2::{Ident, Span};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Error, Fields, LitStr, Path, Result, Type};

use crate::options::{Options, Place};

/// A type that derives `Literal`, with its shape.
pub struct Derived {
    pub ident: Ident,
    pub shape: Shape,
}

/// The three forms that a derived literal takes.
pub enum Shape {
    /// A struct with named fields, or a unit struct: one constructor.
    Record(Record),
    /// An enum: one constructor for each variant.
    Sum(Vec<Variant>),
    /// A struct with one unnamed field, whose literal is the field's.
    Newtype(Newtype),
}

/// A record: its constructor, and how a read builds the value.
pub struct Record {
    pub constructor: Constructor,
    pub build: Build,
}

/// One variant of a sum, and its constructor.
pub struct Variant {
    pub ident: Ident,
    pub constructor: Constructor,
}

/// A newtype: the type of its field, and how a read builds the value.
pub struct Newtype {
    pub inner: Type,
    pub build: Build,
}

/// A constructor: its stored name, and the fields of the Rust item in
/// declaration order, which is the print order of the stored ones.
pub struct Constructor {
    pub name: String,
    pub fields: Vec<Field>,
}

/// A named field of a struct or a variant.
pub struct Field {
    pub ident: Ident,
    pub ty: Type,
    pub role: Role,
}

/// Whether a field is in the literal.
pub enum Role {
    /// The literal holds the field under `name`.
    Stored(Stored),
    /// The literal does not hold the field. A read sets it to its type's
    /// `Default`.
    Skipped,
}

/// A field that the literal holds.
pub struct Stored {
    pub name: String,
    pub absent: Absent,
    pub codec: Codec,
}

/// What a read does when a call lacks a stored field.
pub enum Absent {
    /// The read refuses the call.
    Refused,
    /// The read takes the `Default` of the type that the field reads as.
    Default,
}

/// How a field prints and reads.
pub enum Codec {
    /// Through the field type's `Literal` impl.
    Literal,
    /// Through the module at the path: its `to_value` and `from_field`.
    With(Path),
}

/// How a read builds a value from the fields that it read.
pub enum Build {
    /// The read sets each field.
    Fields,
    /// The read calls the function at the path with the stored fields, in
    /// print order, and returns its result.
    New(Path),
}

impl Derived {
    /// The shape of `input`.
    ///
    /// # Errors
    ///
    /// A type that has no literal form, or an option of `#[literal]` that
    /// does not apply. Each message says what to write instead.
    pub fn parse(input: &DeriveInput) -> Result<Derived> {
        let ident = input.ident.clone();
        if !input.generics.params.is_empty() {
            return Err(Error::new(
                input.generics.span(),
                format!(
                    "#[derive(Literal)] does not support a generic type: implement \
                     `Literal` for `{ident}` by hand, or derive it for a type without \
                     parameters"
                ),
            ));
        }
        let shape = match &input.data {
            Data::Struct(data) => match &data.fields {
                Fields::Named(_) | Fields::Unit => {
                    let options = Options::read(&input.attrs, Place::Record)?;
                    Shape::Record(Record {
                        constructor: constructor(&ident, options.rename, &data.fields)?,
                        build: build(options.new),
                    })
                }
                Fields::Unnamed(fields) => {
                    let options = Options::read(&input.attrs, Place::Newtype)?;
                    match fields.unnamed.iter().collect::<Vec<_>>().as_slice() {
                        [field] => {
                            Options::read(&field.attrs, Place::Inner)?;
                            Shape::Newtype(Newtype {
                                inner: field.ty.clone(),
                                build: build(options.new),
                            })
                        }
                        _ => {
                            return Err(Error::new(
                                fields.span(),
                                format!(
                                    "a literal names each field of a constructor: give each \
                                     field of `{ident}` a name, or keep one field to make \
                                     `{ident}` a newtype"
                                ),
                            ))
                        }
                    }
                }
            },
            Data::Enum(data) => {
                Options::read(&input.attrs, Place::Sum)?;
                if data.variants.is_empty() {
                    return Err(Error::new(
                        ident.span(),
                        format!(
                            "`{ident}` has no variant, so no value of it prints: add a variant"
                        ),
                    ));
                }
                let variants = data
                    .variants
                    .iter()
                    .map(|variant| {
                        if let Fields::Unnamed(fields) = &variant.fields {
                            return Err(Error::new(
                                fields.span(),
                                format!(
                                    "a literal names each field of a constructor: write the \
                                     variant as `{} {{ name: Type }}`",
                                    variant.ident
                                ),
                            ));
                        }
                        let options = Options::read(&variant.attrs, Place::Variant)?;
                        Ok(Variant {
                            ident: variant.ident.clone(),
                            constructor: constructor(
                                &variant.ident,
                                options.rename,
                                &variant.fields,
                            )?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                distinct_constructors(&ident, &variants)?;
                Shape::Sum(variants)
            }
            Data::Union(data) => {
                return Err(Error::new(
                    data.union_token.span(),
                    format!("a union has no literal form: make `{ident}` an enum"),
                ))
            }
        };
        Ok(Derived { ident, shape })
    }
}

fn build(new: Option<Path>) -> Build {
    new.map_or(Build::Fields, Build::New)
}

/// The constructor of the item `ident` with the fields `fields`, named
/// `rename` or else `ident`.
fn constructor(ident: &Ident, rename: Option<LitStr>, fields: &Fields) -> Result<Constructor> {
    let name = stored_name(ident, rename, Kind::Constructor)?;
    let fields = fields
        .iter()
        .map(|field| {
            let ident = field
                .ident
                .clone()
                .ok_or_else(|| Error::new(field.span(), "a stored field has a name"))?;
            let options = Options::read(&field.attrs, Place::Field)?;
            let role = match options.skip {
                Some(skip) => {
                    if options.rename.is_some()
                        || options.default.is_some()
                        || options.with.is_some()
                    {
                        return Err(Error::new(
                            skip,
                            "a skipped field is not in the literal: remove `rename`, \
                             `default` and `with`, or remove `skip`",
                        ));
                    }
                    Role::Skipped
                }
                None => Role::Stored(Stored {
                    name: stored_name(&ident, options.rename, Kind::Field)?,
                    absent: match options.default {
                        Some(_) => Absent::Default,
                        None => Absent::Refused,
                    },
                    codec: options.with.map_or(Codec::Literal, Codec::With),
                }),
            };
            Ok(Field {
                ident,
                ty: field.ty.clone(),
                role,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut seen = BTreeSet::new();
    for field in &fields {
        if let Role::Stored(stored) = &field.role {
            if !seen.insert(stored.name.as_str()) {
                return Err(Error::new(
                    field.ident.span(),
                    format!(
                        "two fields of `{name}` are stored as `{}`: give one of them another \
                         name with #[literal(rename = \"...\")]",
                        stored.name
                    ),
                ));
            }
        }
    }
    Ok(Constructor { name, fields })
}

fn distinct_constructors(ident: &Ident, variants: &[Variant]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for variant in variants {
        if !seen.insert(variant.constructor.name.as_str()) {
            return Err(Error::new(
                variant.ident.span(),
                format!(
                    "two constructors of `{ident}` are named `{}`: a read could not tell \
                     them apart, so give one of them another name with \
                     #[literal(rename = \"...\")]",
                    variant.constructor.name
                ),
            ));
        }
    }
    Ok(())
}

/// What a stored name names.
#[derive(Clone, Copy)]
enum Kind {
    Constructor,
    Field,
}

/// The stored name of `ident`: `rename` if given, or else the identifier
/// without a raw prefix. The literal grammar (SPEC §2) reads a name as
/// ASCII letters, digits and `_`, not starting with a digit. `None`,
/// `True` and `False` are constants, and `datetime` and `timedelta` are
/// the grammar's own constructors.
fn stored_name(ident: &Ident, rename: Option<LitStr>, kind: Kind) -> Result<String> {
    let (name, span) = match rename {
        Some(rename) => (rename.value(), rename.span()),
        None => (ident.unraw().to_string(), ident.span()),
    };
    let grammatical = name
        .bytes()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    let reserved: &[&str] = match kind {
        Kind::Constructor => &["None", "True", "False", "datetime", "timedelta"],
        Kind::Field => &["None", "True", "False"],
    };
    if !grammatical || reserved.contains(&name.as_str()) {
        return Err(refused_name(&name, span));
    }
    Ok(name)
}

fn refused_name(name: &str, span: Span) -> Error {
    Error::new(
        span,
        format!(
            "`{name}` cannot be a stored name: a stored name is ASCII letters, digits and \
             `_`, does not start with a digit, and is not `None`, `True`, `False`, \
             `datetime` or `timedelta`. Give another name with #[literal(rename = \"...\")]"
        ),
    )
}
