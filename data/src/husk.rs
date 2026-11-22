//! A husk: an object whose sealed parts were dropped, kept under the name of the whole it was
//! (SPEC §3, "A husk"). Where no key that a holder keeps opens a sealed part, a husk reads as
//! its whole for good, because keys are only destroyed.

use crate::change::{mk_change, Change};
use crate::event::{canonical_envelope, Envelope, Hash};
use crate::literal::codec::{Constructor, Fields};
use crate::literal::{parse_literal, print_literal};
use crate::literal::{ProdromeError, Signature, Table, Value, Vocabulary};
use crate::schema::Schema;

/// A schema whose events hold sealed parts, each under a named key, which a holder may drop.
///
/// A schema that implements it must also answer [`Schema::husk`] with [`HuskObject::of_other`],
/// so that every store at the schema reads its husks.
pub trait Husking: Schema {
    /// The name of the key that a part was sealed under.
    type Seal: Ord;
    /// The schema's constructors and each sealed part's dropped form, without its sealed form.
    type Husks: Vocabulary + Default;

    /// The event with every sealed part dropped, or `None` where it holds none. The husk must keep
    /// every clear field (SPEC §3), so that a fold reads the husk where it read the event.
    fn drop_sealed(&self) -> Option<Self>;

    /// The event that a husk's `value` prints; an error where a part is sealed or it holds none.
    fn read_dropped(value: &Value) -> Result<Self, ProdromeError>;

    /// The key each part was sealed under, in order.
    fn sealed_under(&self) -> impl Iterator<Item = &Self::Seal>;

    fn holds_sealed(&self) -> bool {
        self.drop_sealed().is_some_and(|husk| husk != *self)
    }
}

/// An event with every sealed part dropped. Only [`Husk::of`] and [`Husk::read`] make one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Husk<E>(E);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a husk that does not read: {0}")]
pub struct NotAHusk(String);

impl<E: Husking> Husk<E> {
    /// `None` for an event that holds no sealed part.
    #[must_use]
    pub fn of(event: &E) -> Option<Self> {
        event.drop_sealed().map(Self)
    }

    #[must_use]
    pub fn event(&self) -> &E {
        &self.0
    }

    #[must_use]
    pub fn print(&self) -> String {
        print_literal(&self.0.to_value())
    }

    /// Refuses a print that is not the canonical print of an event with every part dropped.
    pub fn read(text: &str) -> Result<Self, NotAHusk> {
        let value = parse_literal(text, &E::Husks::default()).map_err(refused)?;
        let husk = Self::dropped(E::read_dropped(&value).map_err(refused)?)?;
        exact(&husk.print(), text.as_bytes())?;
        Ok(husk)
    }

    /// Checks that `event` holds a dropped part and no sealed part, whatever the schema's
    /// [`Husking::read_dropped`] checked.
    fn dropped(event: E) -> Result<Self, NotAHusk> {
        if event.holds_sealed() {
            return Err(NotAHusk("it holds a sealed part".to_owned()));
        }
        if event.sealed_under().next().is_none() {
            return Err(NotAHusk("it holds no dropped part".to_owned()));
        }
        Ok(Self(event))
    }

    /// Whether a key for which `live` answers true opens a part of the whole it was.
    pub fn opens(&self, live: impl Fn(&E::Seal) -> bool) -> bool {
        self.0.sealed_under().any(live)
    }
}

/// The whole's `Change` with its event husked, which a store keeps under the whole's name. Only
/// [`HuskObject::of`] and [`HuskObject::of_other`] make one.
#[derive(Debug, Clone, PartialEq)]
pub struct HuskObject<E> {
    object: Envelope<E>,
}

const CHANGE: Constructor = ("Change", &["genesis", "deps", "event"]);

struct ObjectVocabulary<V>(V);

impl<V: Vocabulary> Vocabulary for ObjectVocabulary<V> {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        Table(&[CHANGE])
            .find(name)
            .or_else(|| self.0.signature(name))
    }
}

impl<E: Husking> HuskObject<E> {
    /// `None` where the event of `change` holds no sealed part. The caller must have verified the
    /// whole against its name. Only a `Change` is husked, because no other envelope that `append`
    /// writes carries an event; a `Woven` that `merge` writes is not husked.
    #[must_use]
    pub fn of(change: &Change<E>) -> Option<Self> {
        let husk = Husk::of(&change.event)?;
        Some(Self::over(
            change.genesis.clone(),
            change.deps.clone(),
            husk,
        ))
    }

    /// Refuses a print that hashes to `name` (a whole) or that [`HuskObject::of_other`] refuses.
    pub fn read(name: &Hash, text: &str) -> Result<Self, NotAHusk> {
        match Hashed::of(name, text.as_bytes()) {
            Hashed::Whole(_) => Err(NotAHusk(format!("{} is its whole", name.as_str()))),
            Hashed::Other(other) => Self::of_other(&other),
        }
    }

    /// Refuses a print that is not the canonical print of a `Change` whose event is a husk (law
    /// 78): another print could hold bytes beside the husk, a sealed part among them.
    pub fn of_other(other: &Other<'_>) -> Result<Self, NotAHusk> {
        let text = std::str::from_utf8(other.bytes).map_err(refused)?;
        let value = parse_literal(text, &ObjectVocabulary(E::Husks::default())).map_err(refused)?;
        let fields = Fields::of(&value, CHANGE).map_err(refused)?;
        let deps = fields
            .get::<Vec<String>>("deps")
            .map_err(refused)?
            .into_iter()
            .map(Hash::new)
            .collect::<Result<_, _>>()
            .map_err(refused)?;
        let genesis =
            Hash::new(fields.get::<String>("genesis").map_err(refused)?).map_err(refused)?;
        let event = crate::payload::required(fields.call(), "event").map_err(refused)?;
        let husk = Husk::dropped(E::read_dropped(event).map_err(refused)?)?;
        let Change { genesis, deps, .. } = mk_change(genesis, deps, ()).map_err(refused)?;
        let husk = Self::over(genesis, deps, husk);
        exact(&husk.print(), other.bytes)?;
        Ok(husk)
    }

    fn over(genesis: Hash, deps: Vec<Hash>, husk: Husk<E>) -> Self {
        Self {
            object: Envelope::Change(Change {
                genesis,
                deps,
                event: husk.0,
            }),
        }
    }
}

impl<E> HuskObject<E> {
    /// The object that every fold reads in place of the whole.
    #[must_use]
    pub fn object(&self) -> &Envelope<E> {
        &self.object
    }

    #[must_use]
    pub fn into_object(self) -> Envelope<E> {
        self.object
    }
}

impl<E: Schema> HuskObject<E> {
    #[must_use]
    pub fn print(&self) -> String {
        canonical_envelope(&self.object)
    }
}

/// What a name holds: the whole that it is the hash of, or its husk.
#[derive(Debug, Clone, PartialEq)]
pub enum Stored<E> {
    Whole(Envelope<E>),
    Husk(HuskObject<E>),
}

impl<E> Stored<E> {
    #[must_use]
    pub fn object(&self) -> &Envelope<E> {
        match self {
            Stored::Whole(object) => object,
            Stored::Husk(husk) => husk.object(),
        }
    }

    #[must_use]
    pub fn into_object(self) -> Envelope<E> {
        match self {
            Stored::Whole(object) => object,
            Stored::Husk(husk) => husk.into_object(),
        }
    }
}

/// A print hashed once against its name; a print that is not the whole can only be a husk.
pub enum Hashed<'t> {
    Whole(&'t [u8]),
    Other(Other<'t>),
}

/// A print that does not hash to its name, with its hash. Only [`Hashed::of`] makes one.
#[derive(Clone)]
pub struct Other<'t> {
    bytes: &'t [u8],
    computed: Hash,
}

impl Other<'_> {
    #[must_use]
    pub fn computed(&self) -> &Hash {
        &self.computed
    }
}

impl<'t> Hashed<'t> {
    #[must_use]
    pub fn of(name: &Hash, bytes: &'t [u8]) -> Self {
        let computed = Hash::of_bytes(bytes);
        if computed == *name {
            Self::Whole(bytes)
        } else {
            Self::Other(Other { bytes, computed })
        }
    }
}

/// Whose word a receipt takes for a print that does not hash to its name (SPEC §3, "A husk").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sender {
    /// Only prints that hash to their names are read, and any other print is tampered.
    #[default]
    Stranger,
    /// The keeper of record: a print that does not hash to its name reads as a husk, where the
    /// schema has husks. A store reads its own files so.
    Keeper,
}

/// A print that parses to a husk and is not its canonical print is no husk.
fn exact(print: &str, bytes: &[u8]) -> Result<(), NotAHusk> {
    if print.as_bytes() == bytes {
        Ok(())
    } else {
        Err(NotAHusk(
            "it is not the canonical print of the husk it parses to".to_owned(),
        ))
    }
}

fn refused(why: impl std::fmt::Display) -> NotAHusk {
    NotAHusk(why.to_string())
}
