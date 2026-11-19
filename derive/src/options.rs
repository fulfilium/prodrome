//! The options that `#[literal(...)]` gives one item, read once, with the
//! place of the item deciding which options apply.

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{Attribute, Error, LitStr, Path, Result};

/// Where an attribute `#[literal(...)]` stands. Each place admits its own
/// options.
#[derive(Clone, Copy)]
pub enum Place {
    /// A struct with named fields, or a unit struct.
    Record,
    /// A struct with one unnamed field.
    Newtype,
    /// An enum.
    Sum,
    /// A variant of an enum.
    Variant,
    /// A named field of a struct or of a variant.
    Field,
    /// The one field of a newtype.
    Inner,
}

/// The name of one option.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Rename,
    New,
    Default,
    Skip,
    With,
}

impl Key {
    const ALL: [Key; 5] = [Key::Rename, Key::New, Key::Default, Key::Skip, Key::With];

    fn word(self) -> &'static str {
        match self {
            Key::Rename => "rename",
            Key::New => "new",
            Key::Default => "default",
            Key::Skip => "skip",
            Key::With => "with",
        }
    }

    /// The places where the option applies, in words, for a refusal.
    fn applies_to(self) -> &'static str {
        match self {
            Key::Rename => "a struct with named fields, a variant, or a named field",
            Key::New => "a struct",
            Key::Default | Key::Skip | Key::With => "a named field",
        }
    }
}

impl Place {
    /// The options that apply at this place.
    fn admits(self) -> &'static [Key] {
        match self {
            Place::Record => &[Key::Rename, Key::New],
            Place::Newtype => &[Key::New],
            Place::Sum | Place::Inner => &[],
            Place::Variant => &[Key::Rename],
            Place::Field => &[Key::Rename, Key::Default, Key::Skip, Key::With],
        }
    }

    fn words(self) -> &'static str {
        match self {
            Place::Record => "a struct with named fields",
            Place::Newtype => "a newtype",
            Place::Sum => "an enum",
            Place::Variant => "a variant",
            Place::Field => "a named field",
            Place::Inner => "the field of a newtype",
        }
    }
}

/// The options of one item. Each option holds the span that gave it, so
/// that a refusal points at the option.
#[derive(Default)]
pub struct Options {
    pub rename: Option<LitStr>,
    pub new: Option<Path>,
    pub default: Option<Span>,
    pub skip: Option<Span>,
    pub with: Option<Path>,
}

impl Options {
    /// The options that the attributes `attrs` give an item at `place`.
    ///
    /// # Errors
    ///
    /// An option that is unknown, given twice, or does not apply at
    /// `place`.
    pub fn read(attrs: &[Attribute], place: Place) -> Result<Options> {
        let mut options = Options::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("literal")) {
            attr.parse_nested_meta(|meta| {
                let key = Key::ALL
                    .into_iter()
                    .find(|key| meta.path.is_ident(key.word()))
                    .ok_or_else(|| {
                        meta.error(
                            "unknown option of #[literal]: write one of `rename = \"...\"`, \
                             `new = path`, `default`, `skip` or `with = path`",
                        )
                    })?;
                if !place.admits().contains(&key) {
                    return Err(meta.error(format!(
                        "`{}` does not apply to {}; it applies to {}",
                        key.word(),
                        place.words(),
                        key.applies_to()
                    )));
                }
                let span = meta.path.span();
                let twice = || Error::new(span, format!("`{}` is given twice", key.word()));
                match key {
                    Key::Rename => set(&mut options.rename, meta.value()?.parse()?, twice),
                    Key::New => set(&mut options.new, meta.value()?.parse()?, twice),
                    Key::With => set(&mut options.with, meta.value()?.parse()?, twice),
                    Key::Default => set(&mut options.default, span, twice),
                    Key::Skip => set(&mut options.skip, span, twice),
                }
            })?;
        }
        Ok(options)
    }
}

fn set<T>(slot: &mut Option<T>, value: T, twice: impl FnOnce() -> Error) -> Result<()> {
    if slot.is_some() {
        return Err(twice());
    }
    *slot = Some(value);
    Ok(())
}
