//! An event of a declared schema, and an entity's registers under one: the
//! `Schema` trait family implemented by interpreting the declaration.

use prodrome_data::event::{Actor, Hash};
use prodrome_data::fold::{grows, Climb, Frontier, Product, Register};
use prodrome_data::literal::{Call, Datetime, ProdromeError, Value};
use prodrome_data::payload::required;
use prodrome_data::schema::Schema;
use prodrome_data::stamp::Stamp;

use super::language::Type;
use super::order::{Climbing, Valued};
use super::{Declaration, Slot};

/// A field's value, of one of the declared types.
///
/// The `Ord` exists only to read a list as a set; a register compares
/// values by its declared order ([`Valued`]), never by this one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Datum {
    Text(String),
    Integer(i64),
    Instant(Datetime),
    Reference(Hash),
    Alternative(String),
    List(Vec<Datum>),
}

impl Datum {
    /// `value` as a value of the type `ty`, through every smart constructor.
    fn parse(ty: &Type, value: &Value, context: &str) -> Result<Datum, ProdromeError> {
        let refused = || {
            ProdromeError::invalid(format!(
                "{context} must be a value of {ty:?}, got {value:?}"
            ))
        };
        match (ty, value) {
            (Type::Text, Value::Str(text)) => Ok(Datum::Text(text.clone())),
            (Type::Integer, Value::Int(int)) => {
                int.as_i64().map(Datum::Integer).ok_or_else(refused)
            }
            (Type::Instant, Value::Datetime(at)) => Ok(Datum::Instant(*at)),
            (Type::Reference, Value::Str(text)) => Ok(Datum::Reference(Hash::new(text.clone())?)),
            (Type::Enum(alternatives), Value::Str(text)) if alternatives.contains(text) => {
                Ok(Datum::Alternative(text.clone()))
            }
            (Type::List(item), Value::Tuple(items)) => items
                .iter()
                .map(|value| Datum::parse(item, value, context))
                .collect::<Result<_, _>>()
                .map(Datum::List),
            _ => Err(refused()),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Datum::Text(text) | Datum::Alternative(text) => Value::str(text.as_str()),
            Datum::Integer(int) => Value::int(*int),
            Datum::Instant(at) => Value::Datetime(*at),
            Datum::Reference(name) => Value::str(name.as_str()),
            Datum::List(items) => Value::Tuple(items.iter().map(Datum::to_value).collect()),
        }
    }
}

/// The entity of a declared event: the text of its key field.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(String);

impl AsRef<str> for Key {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// An event of a declared schema: one of its constructors, with every field
/// a value of its declared type.
///
/// The event never prints its declaration, so no object carries a schema.
#[derive(Debug, Clone)]
pub struct Declared {
    schema: Declaration,
    event: usize,
    key: Key,
    at: Datetime,
    actor: Actor,
    data: Vec<Datum>,
}

impl PartialEq for Declared {
    fn eq(&self, other: &Self) -> bool {
        self.schema == other.schema && self.event == other.event && self.data == other.data
    }
}

impl Declared {
    #[must_use]
    pub fn schema(&self) -> &Declaration {
        &self.schema
    }

    #[must_use]
    pub fn constructor(&self) -> &str {
        &self.schema.shape(self.event).name
    }

    /// Every field's value, in declared order.
    #[must_use]
    pub fn data(&self) -> &[Datum] {
        &self.data
    }

    /// The value that this event writes to the register `slot`, if any.
    #[must_use]
    pub fn written(&self, slot: Slot) -> Option<&Datum> {
        self.schema
            .shape(self.event)
            .writes
            .iter()
            .find(|(written, _)| *written == slot)
            .map(|(_, field)| &self.data[*field])
    }

    /// The value written to `slot` under that register's order.
    fn valued(&self, slot: Slot) -> Valued<'_> {
        let datum = self
            .written(slot)
            .expect("a write joins only the registers it writes");
        self.schema.value(slot, datum)
    }
}

impl Schema for Declared {
    type Vocabulary = Declaration;
    type Key = Key;
    type Register = Slot;
    type Registers<'a> = Registers<'a>;

    fn registers(schema: &Declaration) -> Vec<Slot> {
        schema.registers().collect()
    }

    fn to_value(&self) -> Value {
        let shape = self.schema.shape(self.event);
        Value::call(
            shape.name.as_str(),
            shape
                .names
                .iter()
                .cloned()
                .zip(self.data.iter().map(Datum::to_value))
                .collect(),
        )
    }

    fn from_value(schema: &Declaration, value: &Value) -> Result<Declared, ProdromeError> {
        let call: &Call = value
            .as_call()
            .ok_or_else(|| ProdromeError::invalid("a declared event is a constructor call"))?;
        let event = schema.event(&call.name).ok_or_else(|| {
            ProdromeError::invalid(format!("{} is not an event of this schema", call.name))
        })?;
        let shape = schema.shape(event);
        let data = shape
            .names
            .iter()
            .zip(&shape.types)
            .map(|(name, ty)| {
                Datum::parse(ty, required(call, name)?, &format!("{}.{name}", call.name))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (Datum::Text(key), Datum::Instant(at), Datum::Text(actor)) =
            (&data[shape.key], &data[shape.at], &data[shape.actor])
        else {
            unreachable!("the key and stamp laws type these fields");
        };
        Ok(Declared {
            key: Key(key.clone()),
            at: *at,
            actor: Actor::new(actor.as_str())?,
            schema: schema.clone(),
            event,
            data,
        })
    }

    fn key(&self) -> &Key {
        &self.key
    }

    fn at(&self) -> Datetime {
        self.at
    }

    fn actor(&self) -> &Actor {
        &self.actor
    }

    fn writes(&self) -> impl Iterator<Item = Slot> {
        self.schema
            .shape(self.event)
            .writes
            .iter()
            .map(|(slot, _)| *slot)
    }
}

/// An entity's registers under a declared schema, each read under its
/// declared order.
///
/// The value learns the declaration from the first write that it joins; before
/// that write, every register is unwritten.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Registers<'a> {
    held: Vec<Held<'a>>,
    unwritten: Frontier<'a, Declared>,
}

/// One declared register: a [`Frontier`] where the declaration lets it go
/// back, and a [`Climb`] where it is declared inflationary.
#[derive(Debug, Clone, PartialEq)]
enum Held<'a> {
    Frontier(Frontier<'a, Declared>),
    Climb(Climb<'a, Declared>),
}

impl Held<'_> {
    /// The register `slot` of `schema`, unwritten.
    fn of(schema: &Declaration, slot: Slot) -> Self {
        if schema.inflationary(slot) {
            Held::Climb(Climb::default())
        } else {
            Held::Frontier(Frontier::default())
        }
    }
}

/// The value of a write to the inflationary register `slot`.
fn climbing<'a>(slot: Slot) -> impl Fn(&'a Stamp<Declared>) -> Climbing<'a> {
    move |stamp| Climbing(stamp.event.valued(slot))
}

impl<'a> Registers<'a> {
    /// Hold every register of `schema` up to `slot`.
    fn hold(&mut self, schema: &Declaration, slot: Slot) {
        while self.held.len() <= slot.index() {
            let next = Slot(self.held.len());
            self.held.push(Held::of(schema, next));
        }
    }

    /// The register `slot`, where it is inflationary and held.
    fn climb(&self, slot: Slot) -> Option<&Climb<'a, Declared>> {
        match self.held.get(slot.index()) {
            Some(Held::Climb(climb)) => Some(climb),
            Some(Held::Frontier(_)) | None => None,
        }
    }
}

impl<'a> Product<'a> for Registers<'a> {
    type Schema = Declared;

    fn registers(&self) -> Vec<Slot> {
        (0..self.held.len()).map(Slot).collect()
    }

    fn join(&mut self, stamp: &'a Stamp<Declared>) {
        if let Some(last) = stamp.event.schema.registers().last() {
            self.hold(&stamp.event.schema, last);
        }
        for slot in stamp.event.writes() {
            self.join_to(slot, stamp);
        }
    }

    fn frontier(&self, slot: Slot) -> &Frontier<'a, Declared> {
        match self.held.get(slot.index()) {
            Some(Held::Frontier(frontier)) => frontier,
            Some(Held::Climb(climb)) => climb.frontier(),
            None => &self.unwritten,
        }
    }

    fn join_to(&mut self, slot: Slot, stamp: &'a Stamp<Declared>) {
        self.hold(&stamp.event.schema, slot);
        match &mut self.held[slot.index()] {
            Held::Frontier(frontier) => frontier.join(stamp),
            Held::Climb(climb) => climb.join_by(stamp, climbing(slot)),
        }
    }

    /// The register's maximal writes under its declared order, the
    /// completion every register is read by: of its frontier, or, where it
    /// is inflationary, of all its writes.
    fn reading(&self, slot: Slot) -> Vec<&'a Stamp<Declared>> {
        let valued = |stamp: &'a Stamp<Declared>| stamp.event.valued(slot);
        match self.held.get(slot.index()) {
            Some(Held::Frontier(frontier)) => frontier.read(valued),
            Some(Held::Climb(climb)) => climb.read(valued),
            None => Vec::new(),
        }
    }

    /// Where `event` writes a register declared inflationary, its value must
    /// stand at or above that register's reading: [`grows`], the refusal
    /// every inflationary register is held to.
    fn grows(&self, event: &Declared) -> Result<(), ProdromeError> {
        for slot in event.writes() {
            let Some(climb) = self.climb(slot) else {
                continue;
            };
            let held: Vec<Climbing<'_>> = climb
                .read(climbing(slot))
                .into_iter()
                .map(climbing(slot))
                .collect();
            grows(&held, &Climbing(event.valued(slot))).map_err(|why| {
                ProdromeError::invalid(format!("{}: {why}", event.schema.register_name(slot)))
            })?;
        }
        Ok(())
    }

    /// The registers declared inflationary, of the declaration that the
    /// first write joined.
    fn climbing(&self) -> Vec<Slot> {
        self.registers()
            .into_iter()
            .filter(|slot| self.climb(*slot).is_some())
            .collect()
    }

    /// Each register declared inflationary reads at or above what it read
    /// in `earlier`; an unwritten one reads nothing.
    fn climbed(&self, earlier: &Self) -> Result<(), ProdromeError> {
        let unwritten = Climb::default();
        for slot in earlier.registers() {
            if let Some(then) = earlier.climb(slot) {
                self.climb(slot)
                    .unwrap_or(&unwritten)
                    .climbed_by(then, climbing(slot))?;
            }
        }
        Ok(())
    }
}
