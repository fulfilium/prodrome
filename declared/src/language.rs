//! The schema language: the form of a declared schema, in the literal
//! grammar of SPEC §2, under a closed vocabulary of its own.

use prodrome_data::literal::{parse_literal, print_literal, Call, Table, Value};
use prodrome_data::payload::{bool_field, required, string_field, tuple_field};

use super::{Law, Refusal};

/// The schema language's constructors, name and declared field order. A
/// closed table, as every vocabulary is (§2): a form names nothing else.
const SIGNATURES: &[(&str, &[&str])] = &[
    ("Schema", &["key", "events", "registers"]),
    ("Event", &["name", "fields"]),
    ("Field", &["name", "type"]),
    ("Text", &[]),
    ("Integer", &[]),
    ("Instant", &[]),
    ("Reference", &[]),
    ("Enum", &["alternatives"]),
    ("List", &["item"]),
    ("Register", &["name", "order", "inflationary", "writes"]),
    ("Discrete", &[]),
    ("Total", &[]),
    ("Machine", &["covers"]),
    ("Inclusion", &[]),
    ("Write", &["event", "field"]),
];

/// A declared schema as written, before admission checks any law.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub key: String,
    pub events: Vec<Event>,
    pub registers: Vec<Register>,
}

/// One event constructor; its fields are in printed order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: Type,
}

/// A field's type, which constrains its literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// A string.
    Text,
    /// A signed 64-bit integer.
    Integer,
    /// A `datetime` of SPEC §2.
    Instant,
    /// An object's name (SPEC §3): 64 lowercase hex digits.
    Reference,
    /// A string that is one of the alternatives.
    Enum(Vec<String>),
    /// A tuple of values of one type.
    List(Box<Type>),
}

/// A register's order (design §3). Each order except `Machine` is a
/// partial order by construction; admission checks that a machine's
/// covering relation is acyclic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poset {
    /// `a ≤ b` if and only if `a = b`, so every distinct value is a
    /// candidate.
    Discrete,
    /// The order of the integers; the reading is the greatest value.
    Total,
    /// A finite state machine over an enum's alternatives, given by its
    /// covering relation as `(lower, upper)` pairs.
    Machine(Vec<(String, String)>),
    /// Lists read as sets, under inclusion.
    Inclusion,
}

/// A register. Where it is `inflationary`, a write must climb its order
/// (design §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    pub name: String,
    pub order: Poset,
    pub inflationary: bool,
    pub writes: Vec<Write>,
}

/// The event `event` writes its field `field` to the register.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Write {
    pub event: String,
    pub field: String,
}

fn grammar(detail: impl Into<String>) -> Refusal {
    Refusal::new(Law::Grammar, detail)
}

fn call<'v>(value: &'v Value, names: &[&str]) -> Result<&'v Call, Refusal> {
    value
        .as_call()
        .filter(|call| names.contains(&call.name.as_str()))
        .ok_or_else(|| grammar(format!("expected one of {names:?}, got {value:?}")))
}

fn text(value: &Value) -> Result<String, Refusal> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| grammar(format!("expected a string, got {value:?}")))
}

fn each<T>(
    call: &Call,
    field: &str,
    read: impl Fn(&Value) -> Result<T, Refusal>,
) -> Result<Vec<T>, Refusal> {
    let items = tuple_field(call, field).map_err(|e| grammar(e.to_string()))?;
    items.iter().map(read).collect()
}

fn name(call: &Call, field: &str) -> Result<String, Refusal> {
    string_field(call, field).map_err(|e| grammar(e.to_string()))
}

impl Form {
    /// Parses `text` in the closed vocabulary of the schema language.
    ///
    /// # Errors
    ///
    /// [`Law::Grammar`]: text that is not one `Schema(...)` of the
    /// language.
    pub fn parse(text: &str) -> Result<Form, Refusal> {
        let value = parse_literal(text, &Table(SIGNATURES)).map_err(|e| grammar(e.to_string()))?;
        let schema = call(&value, &["Schema"])?;
        Ok(Form {
            key: name(schema, "key")?,
            events: each(schema, "events", Event::from_value)?,
            registers: each(schema, "registers", Register::from_value)?,
        })
    }

    /// The canonical print, with every set sorted, so that two forms of one
    /// schema print as one text. The print keeps the order of the fields,
    /// which is not a set.
    #[must_use]
    pub fn print(&self) -> String {
        print_literal(&self.normalized().to_value())
    }

    fn normalized(&self) -> Form {
        let mut form = self.clone();
        form.events.sort_by(|a, b| a.name.cmp(&b.name));
        for event in &mut form.events {
            for field in &mut event.fields {
                field.ty.normalize();
            }
        }
        form.registers.sort_by(|a, b| a.name.cmp(&b.name));
        for register in &mut form.registers {
            register.writes.sort();
            if let Poset::Machine(covers) = &mut register.order {
                covers.sort();
            }
        }
        form
    }

    fn to_value(&self) -> Value {
        Value::call(
            "Schema",
            vec![
                ("key".to_owned(), Value::str(&self.key)),
                (
                    "events".to_owned(),
                    Value::Tuple(self.events.iter().map(Event::to_value).collect()),
                ),
                (
                    "registers".to_owned(),
                    Value::Tuple(self.registers.iter().map(Register::to_value).collect()),
                ),
            ],
        )
    }
}

impl Event {
    fn from_value(value: &Value) -> Result<Event, Refusal> {
        let event = call(value, &["Event"])?;
        Ok(Event {
            name: name(event, "name")?,
            fields: each(event, "fields", |value| {
                let field = call(value, &["Field"])?;
                Ok(Field {
                    name: name(field, "name")?,
                    ty: Type::from_value(
                        required(field, "type").map_err(|e| grammar(e.to_string()))?,
                    )?,
                })
            })?,
        })
    }

    fn to_value(&self) -> Value {
        let fields = self
            .fields
            .iter()
            .map(|field| {
                Value::call(
                    "Field",
                    vec![
                        ("name".to_owned(), Value::str(&field.name)),
                        ("type".to_owned(), field.ty.to_value()),
                    ],
                )
            })
            .collect();
        Value::call(
            "Event",
            vec![
                ("name".to_owned(), Value::str(&self.name)),
                ("fields".to_owned(), Value::Tuple(fields)),
            ],
        )
    }
}

impl Type {
    fn from_value(value: &Value) -> Result<Type, Refusal> {
        let ty = call(
            value,
            &["Text", "Integer", "Instant", "Reference", "Enum", "List"],
        )?;
        Ok(match ty.name.as_str() {
            "Text" => Type::Text,
            "Integer" => Type::Integer,
            "Instant" => Type::Instant,
            "Reference" => Type::Reference,
            "Enum" => Type::Enum(each(ty, "alternatives", text)?),
            _ => Type::List(Box::new(Type::from_value(
                required(ty, "item").map_err(|e| grammar(e.to_string()))?,
            )?)),
        })
    }

    fn normalize(&mut self) {
        match self {
            Type::Enum(alternatives) => alternatives.sort(),
            Type::List(item) => item.normalize(),
            Type::Text | Type::Integer | Type::Instant | Type::Reference => {}
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Type::Text => Value::call("Text", vec![]),
            Type::Integer => Value::call("Integer", vec![]),
            Type::Instant => Value::call("Instant", vec![]),
            Type::Reference => Value::call("Reference", vec![]),
            Type::Enum(alternatives) => Value::call(
                "Enum",
                vec![(
                    "alternatives".to_owned(),
                    Value::Tuple(alternatives.iter().map(Value::str).collect()),
                )],
            ),
            Type::List(item) => Value::call("List", vec![("item".to_owned(), item.to_value())]),
        }
    }
}

impl Register {
    fn from_value(value: &Value) -> Result<Register, Refusal> {
        let register = call(value, &["Register"])?;
        let order = call(
            required(register, "order").map_err(|e| grammar(e.to_string()))?,
            &["Discrete", "Total", "Machine", "Inclusion"],
        )?;
        Ok(Register {
            name: name(register, "name")?,
            order: match order.name.as_str() {
                "Discrete" => Poset::Discrete,
                "Total" => Poset::Total,
                "Inclusion" => Poset::Inclusion,
                _ => Poset::Machine(each(order, "covers", |pair| match pair.as_tuple() {
                    Some([lower, upper]) => Ok((text(lower)?, text(upper)?)),
                    _ => Err(grammar(format!(
                        "a cover is a (lower, upper) pair, got {pair:?}"
                    ))),
                })?),
            },
            inflationary: bool_field(register, "inflationary")
                .map_err(|e| grammar(e.to_string()))?,
            writes: each(register, "writes", |value| {
                let write = call(value, &["Write"])?;
                Ok(Write {
                    event: name(write, "event")?,
                    field: name(write, "field")?,
                })
            })?,
        })
    }

    fn to_value(&self) -> Value {
        let order = match &self.order {
            Poset::Discrete => Value::call("Discrete", vec![]),
            Poset::Total => Value::call("Total", vec![]),
            Poset::Inclusion => Value::call("Inclusion", vec![]),
            Poset::Machine(covers) => Value::call(
                "Machine",
                vec![(
                    "covers".to_owned(),
                    Value::Tuple(
                        covers
                            .iter()
                            .map(|(lower, upper)| {
                                Value::Tuple(vec![Value::str(lower), Value::str(upper)])
                            })
                            .collect(),
                    ),
                )],
            ),
        };
        let writes = self
            .writes
            .iter()
            .map(|write| {
                Value::call(
                    "Write",
                    vec![
                        ("event".to_owned(), Value::str(&write.event)),
                        ("field".to_owned(), Value::str(&write.field)),
                    ],
                )
            })
            .collect();
        Value::call(
            "Register",
            vec![
                ("name".to_owned(), Value::str(&self.name)),
                ("order".to_owned(), order),
                ("inflationary".to_owned(), Value::Bool(self.inflationary)),
                ("writes".to_owned(), Value::Tuple(writes)),
            ],
        )
    }
}
