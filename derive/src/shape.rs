//! The shape of a type that derives `Literal`: what the derive reads from
//! the type, checked once. A value of [`Shape`] is a type that has a
//! literal form, so the code generation refuses nothing.

use std::collections::BTreeSet;

use proc_macro2::{Ident, Span};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{
    parse_quote, Data, DeriveInput, Error, Fields, FieldsUnnamed, LitStr, Path, Result, Type,
};

use crate::options::{Options, Place};

/// A type that derives `Literal`, with its shape.
pub struct Derived {
    pub ident: Ident,
    /// The path of the core: `::prodrome`, or the path that
    /// `#[literal(crate = path)]` gives.
    pub root: Path,
    pub shape: Shape,
}

/// The forms that a derived literal takes.
pub enum Shape {
    /// A struct with named fields, or a unit struct: one constructor.
    Record(Record),
    /// A struct with named fields, one of which is marked `sum`: one
    /// constructor for each variant of that field's sum.
    Headed(Headed),
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

/// A record whose field `sum` names its constructor. Each constructor's
/// fields are `fields`, then the fields of the variant.
pub struct Headed {
    /// The other fields, in declaration order.
    pub fields: Vec<Field>,
    pub sum: SumField,
    pub build: Build,
}

/// The field of a [`Headed`] record that names the constructor.
pub struct SumField {
    pub ident: Ident,
    pub ty: Type,
    /// The number of other fields that come before it in the declaration.
    pub position: usize,
}

/// One variant of a sum, and what it prints.
pub struct Variant {
    pub ident: Ident,
    pub arm: Arm,
}

/// What a variant of a sum prints.
pub enum Arm {
    /// A constructor of its own, with the variant's named fields.
    Own(Constructor),
    /// The call of the record of this type, the variant's one unnamed
    /// field: the record's constructor, with the record's fields.
    Holds(Type),
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
        let (root, shape) = match &input.data {
            Data::Struct(data) => match &data.fields {
                Fields::Named(_) | Fields::Unit => {
                    let (root, options) = type_options(input, Place::Record)?;
                    (root, record_shape(&ident, options, &data.fields)?)
                }
                Fields::Unnamed(fields) => {
                    let (root, options) = type_options(input, Place::Newtype)?;
                    (root, newtype_shape(&ident, options, fields)?)
                }
            },
            Data::Enum(data) => {
                let (root, _) = type_options(input, Place::Sum)?;
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
                    .map(variant)
                    .collect::<Result<Vec<_>>>()?;
                distinct_constructors(&ident, &variants)?;
                (root, Shape::Sum(variants))
            }
            Data::Union(data) => {
                return Err(Error::new(
                    data.union_token.span(),
                    format!("a union has no literal form: make `{ident}` an enum"),
                ))
            }
        };
        Ok(Derived { ident, root, shape })
    }
}

/// The options of the type `input`, which stands at `place`, and the path
/// of the core that they give.
fn type_options(input: &DeriveInput, place: Place) -> Result<(Path, Options)> {
    let mut options = Options::read(&input.attrs, place)?;
    let root = options
        .krate
        .take()
        .unwrap_or_else(|| parse_quote!(::prodrome));
    Ok((root, options))
}

/// The shape of the struct `ident` with unnamed fields: a newtype, if it
/// has one field.
fn newtype_shape(ident: &Ident, options: Options, fields: &FieldsUnnamed) -> Result<Shape> {
    match fields.unnamed.iter().collect::<Vec<_>>().as_slice() {
        [field] => {
            Options::read(&field.attrs, Place::Inner)?;
            Ok(Shape::Newtype(Newtype {
                inner: field.ty.clone(),
                build: build(options.new),
            }))
        }
        _ => Err(Error::new(
            fields.span(),
            format!(
                "a literal names each field of a constructor: give each field of \
                 `{ident}` a name, or keep one field to make `{ident}` a newtype"
            ),
        )),
    }
}

/// A variant of a sum: one with named fields or none, or one that holds a
/// record in its one unnamed field.
fn variant(variant: &syn::Variant) -> Result<Variant> {
    let options = Options::read(&variant.attrs, Place::Variant)?;
    let tag = &variant.ident;
    let arm = match &variant.fields {
        Fields::Unnamed(fields) => match fields.unnamed.iter().collect::<Vec<_>>().as_slice() {
            [field] => {
                Options::read(&field.attrs, Place::Inner)?;
                if let Some(rename) = options.rename {
                    return Err(Error::new(
                        rename.span(),
                        format!(
                            "`{tag}` holds a record, and the record's constructor is the \
                             variant's: remove `rename`, and rename the record instead"
                        ),
                    ));
                }
                Arm::Holds(field.ty.clone())
            }
            _ => {
                return Err(Error::new(
                    fields.span(),
                    format!(
                        "a variant with unnamed fields holds exactly one record: write \
                         `{tag}(Record)`, or name the fields as `{tag} {{ name: Type }}`"
                    ),
                ))
            }
        },
        Fields::Named(_) | Fields::Unit => {
            let (fields, marked) = fields_of(&variant.fields)?;
            if let Some(sum) = marked.first() {
                return Err(Error::new(
                    sum.span,
                    "a field of a variant cannot be marked `sum`: mark a field of a struct",
                ));
            }
            Arm::Own(constructor(tag, options.rename, fields)?)
        }
    };
    Ok(Variant {
        ident: tag.clone(),
        arm,
    })
}

/// The shape of the struct `ident` with named fields: a record, or a record
/// with a sum where one field is marked `sum`.
fn record_shape(ident: &Ident, options: Options, fields: &Fields) -> Result<Shape> {
    let (fields, marked) = fields_of(fields)?;
    match marked.as_slice() {
        [] => Ok(Shape::Record(Record {
            constructor: constructor(ident, options.rename, fields)?,
            build: build(options.new),
        })),
        [sum] => {
            if let Some(rename) = options.rename {
                return Err(Error::new(
                    rename.span(),
                    format!(
                        "the variants of the field marked `sum` name the constructors of \
                         `{ident}`: remove `rename`, and rename a variant instead"
                    ),
                ));
            }
            distinct_fields(&ident.to_string(), &fields)?;
            Ok(Shape::Headed(Headed {
                fields,
                sum: SumField {
                    ident: sum.ident.clone(),
                    ty: sum.ty.clone(),
                    position: sum.position,
                },
                build: build(options.new),
            }))
        }
        [_, second, ..] => Err(Error::new(
            second.span,
            format!(
                "two fields of `{ident}` are marked `sum`, and only one variant can name a \
                 constructor: mark one field `sum`, and make the other an ordinary field"
            ),
        )),
    }
}

fn build(new: Option<Path>) -> Build {
    new.map_or(Build::Fields, Build::New)
}

/// A field marked `sum`, with where it stands.
struct Marked {
    ident: Ident,
    ty: Type,
    span: Span,
    /// The number of unmarked fields before it.
    position: usize,
}

/// The fields of an item, and the fields marked `sum`.
fn fields_of(fields: &Fields) -> Result<(Vec<Field>, Vec<Marked>)> {
    let mut unmarked = Vec::new();
    let mut marked = Vec::new();
    for field in fields {
        let ident = field
            .ident
            .clone()
            .ok_or_else(|| Error::new(field.span(), "a stored field has a name"))?;
        let options = Options::read(&field.attrs, Place::Field)?;
        if let Some(sum) = options.sum {
            if options.rename.is_some()
                || options.default.is_some()
                || options.skip.is_some()
                || options.with.is_some()
            {
                return Err(Error::new(
                    sum,
                    "the field marked `sum` is read from the constructor's name: remove \
                     `rename`, `default`, `skip` and `with` from it",
                ));
            }
            marked.push(Marked {
                ident,
                ty: field.ty.clone(),
                span: sum,
                position: unmarked.len(),
            });
            continue;
        }
        let role = match options.skip {
            Some(skip) => {
                if options.rename.is_some() || options.default.is_some() || options.with.is_some() {
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
        unmarked.push(Field {
            ident,
            ty: field.ty.clone(),
            role,
        });
    }
    Ok((unmarked, marked))
}

/// The constructor of the item `ident` with the fields `fields`, named
/// `rename` or else `ident`.
fn constructor(ident: &Ident, rename: Option<LitStr>, fields: Vec<Field>) -> Result<Constructor> {
    let name = stored_name(ident, rename, Kind::Constructor)?;
    distinct_fields(&name, &fields)?;
    Ok(Constructor { name, fields })
}

/// Refuses two fields stored under one name, in the constructor `name`.
fn distinct_fields(name: &str, fields: &[Field]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for field in fields {
        match &field.role {
            Role::Stored(stored) => {
                if !seen.insert(stored.name.as_str()) {
                    return Err(Error::new(
                        field.ident.span(),
                        format!(
                            "two fields of `{name}` are stored as `{}`: give one of them \
                             another name with #[literal(rename = \"...\")]",
                            stored.name
                        ),
                    ));
                }
            }
            Role::Skipped => {}
        }
    }
    Ok(())
}

/// Refuses two variants with constructors of their own under one name. The
/// name of a record that a variant holds is a constant of the record's
/// type, which the macro does not see: the derive writes a check that the
/// compiler evaluates for it.
fn distinct_constructors(ident: &Ident, variants: &[Variant]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for (variant, constructor) in variants.iter().filter_map(|variant| match &variant.arm {
        Arm::Own(constructor) => Some((variant, constructor)),
        Arm::Holds(_) => None,
    }) {
        if !seen.insert(constructor.name.as_str()) {
            return Err(Error::new(
                variant.ident.span(),
                format!(
                    "two constructors of `{ident}` are named `{}`: a read could not tell \
                     them apart, so give one of them another name with \
                     #[literal(rename = \"...\")]",
                    constructor.name
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
