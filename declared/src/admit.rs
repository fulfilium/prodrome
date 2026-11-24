//! Admission: the laws that a declared schema must keep before a store is
//! opened at it, checked in order. A refusal names the first law that fails.

use std::collections::BTreeSet;
use std::fmt;

use prodrome_data::event::{Hash, ENVELOPE_SIGNATURES};

use super::language::{Event, Form, Poset, Type};
use super::order::{Closure, Interpreted};
use super::Slot;

/// A law of admission. The variants are in the order that admission checks
/// them, and a refusal names the first law that fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Law {
    /// The text is one `Schema(...)` of the schema language.
    Grammar,
    /// The text is its own canonical print, so the schema's name is the hash
    /// of its text, as an object's is (SPEC §3).
    Canonical,
    /// Names are unique: events (and no event has an envelope's name or the
    /// grammar's own), fields within an event, registers, an enum's
    /// alternatives, a machine's covers, and the writes into a register, one
    /// per event.
    Unique,
    /// Every event has the key field, a text.
    Key,
    /// Every event has `at`, an instant, and `actor`, a text.
    Stamp,
    /// Every register is written, by fields that exist, of one type, the
    /// type its order reads: an integer for a total order, a list for
    /// inclusion, an enum naming every state of a machine.
    Typed,
    /// A machine's covering relation is acyclic, so its transitive closure
    /// is a partial order. The other orders are partial orders by
    /// construction.
    PartialOrder,
    /// An inflationary register's order has a bottom, because its reading is
    /// a homomorphism of semilattices with units, and so the empty history
    /// must read as the least value.
    Bottom,
}

impl Law {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Law::Grammar => "grammar",
            Law::Canonical => "canonical",
            Law::Unique => "unique",
            Law::Key => "key",
            Law::Stamp => "stamp",
            Law::Typed => "typed",
            Law::PartialOrder => "partial order",
            Law::Bottom => "bottom",
        }
    }
}

impl fmt::Display for Law {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a declared schema was not admitted: the law that failed, and where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("refused under the law {law}: {detail}")]
pub struct Refusal {
    pub law: Law,
    pub detail: String,
}

impl Refusal {
    pub(crate) fn new(law: Law, detail: impl Into<String>) -> Refusal {
        Refusal {
            law,
            detail: detail.into(),
        }
    }
}

/// An admitted schema: its canonical text and name, and each event's and
/// register's shape resolved to indices, so reading an event never looks a
/// name up twice.
#[derive(Debug)]
pub(crate) struct Admitted {
    pub(crate) text: String,
    pub(crate) name: Hash,
    pub(crate) key: String,
    pub(crate) events: Vec<Shape>,
    pub(crate) registers: Vec<Reg>,
}

/// An event constructor, resolved.
#[derive(Debug)]
pub(crate) struct Shape {
    pub(crate) name: String,
    pub(crate) types: Vec<Type>,
    /// The field order, as the parse binds it.
    pub(crate) names: Vec<String>,
    pub(crate) key: usize,
    pub(crate) at: usize,
    pub(crate) actor: usize,
    /// The registers this event writes, each with the field it writes.
    pub(crate) writes: Vec<(Slot, usize)>,
}

/// A register, resolved.
#[derive(Debug)]
pub(crate) struct Reg {
    pub(crate) name: String,
    pub(crate) order: Interpreted,
    pub(crate) inflationary: bool,
}

fn refuse<T>(law: Law, detail: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal::new(law, detail))
}

/// The first name in `names` seen before, if any.
fn repeated<'n>(names: impl IntoIterator<Item = &'n str>) -> Option<&'n str> {
    let mut seen = BTreeSet::new();
    names.into_iter().find(|name| !seen.insert(*name))
}

/// The names the database's own vocabulary takes: the envelopes', looked up
/// before any schema's, and the grammar's.
fn reserved(name: &str) -> bool {
    ENVELOPE_SIGNATURES
        .iter()
        .any(|(envelope, _)| *envelope == name)
        || name == "datetime"
        || name == "timedelta"
}

fn alternatives(ty: &Type) -> Vec<&[String]> {
    match ty {
        Type::Enum(alternatives) => vec![alternatives],
        Type::List(item) => alternatives(item),
        Type::Text | Type::Integer | Type::Instant | Type::Reference => vec![],
    }
}

fn field<'f>(event: &'f Event, name: &str) -> Option<(usize, &'f Type)> {
    event
        .fields
        .iter()
        .position(|field| field.name == name)
        .map(|at| (at, &event.fields[at].ty))
}

/// Every law, in order, over `form`, whose text is `text`.
pub(crate) fn admit(text: &str) -> Result<Admitted, Refusal> {
    let form = Form::parse(text)?;
    let canonical = form.print();
    if canonical != text {
        return refuse(
            Law::Canonical,
            format!("the text does not print back the same; its canonical print is {canonical}"),
        );
    }
    unique(&form)?;
    stamped(&form)?;
    let orders = typed(&form)?;
    let registers = ordered(&form, &orders)?;
    let events = form
        .events
        .iter()
        .map(|event| Shape {
            name: event.name.clone(),
            types: event.fields.iter().map(|field| field.ty.clone()).collect(),
            names: event
                .fields
                .iter()
                .map(|field| field.name.clone())
                .collect(),
            key: field(event, &form.key).expect("the key law").0,
            at: field(event, "at").expect("the stamp law").0,
            actor: field(event, "actor").expect("the stamp law").0,
            writes: form
                .registers
                .iter()
                .enumerate()
                .flat_map(|(slot, register)| {
                    register
                        .writes
                        .iter()
                        .filter(|write| write.event == event.name)
                        .map(move |write| {
                            let at = field(event, &write.field).expect("the typing law").0;
                            (Slot(slot), at)
                        })
                })
                .collect(),
        })
        .collect();
    Ok(Admitted {
        text: text.to_owned(),
        name: Hash::of_bytes(text.as_bytes()),
        key: form.key,
        events,
        registers,
    })
}

fn unique(form: &Form) -> Result<(), Refusal> {
    if let Some(name) = form
        .events
        .iter()
        .map(|e| e.name.as_str())
        .find(|n| reserved(n))
    {
        return refuse(
            Law::Unique,
            format!("the event {name} takes a name the database's own vocabulary has"),
        );
    }
    if let Some(name) = repeated(form.events.iter().map(|e| e.name.as_str())) {
        return refuse(Law::Unique, format!("the event {name} is declared twice"));
    }
    for event in &form.events {
        if let Some(name) = repeated(event.fields.iter().map(|f| f.name.as_str())) {
            return refuse(
                Law::Unique,
                format!("the field {name} of {} is declared twice", event.name),
            );
        }
        for alternatives in event.fields.iter().flat_map(|f| alternatives(&f.ty)) {
            if let Some(name) = repeated(alternatives.iter().map(String::as_str)) {
                return refuse(
                    Law::Unique,
                    format!("the alternative {name} in {} is declared twice", event.name),
                );
            }
        }
    }
    if let Some(name) = repeated(form.registers.iter().map(|r| r.name.as_str())) {
        return refuse(
            Law::Unique,
            format!("the register {name} is declared twice"),
        );
    }
    for register in &form.registers {
        if let Some(event) = repeated(register.writes.iter().map(|w| w.event.as_str())) {
            return refuse(
                Law::Unique,
                format!("{event} writes the register {} twice", register.name),
            );
        }
        if let Poset::Machine(covers) = &register.order {
            if covers.windows(2).any(|pair| pair[0] == pair[1]) {
                return refuse(
                    Law::Unique,
                    format!(
                        "a cover of the register {} is declared twice",
                        register.name
                    ),
                );
            }
        }
    }
    Ok(())
}

/// The key law, then the stamp law.
fn stamped(form: &Form) -> Result<(), Refusal> {
    if form.events.is_empty() {
        return refuse(Law::Key, format!("no event has the key {}", form.key));
    }
    for event in &form.events {
        match field(event, &form.key) {
            Some((_, Type::Text)) => {}
            Some(_) => {
                return refuse(
                    Law::Key,
                    format!("the key {} of {} is not a text", form.key, event.name),
                )
            }
            None => {
                return refuse(
                    Law::Key,
                    format!("{} has no key field {}", event.name, form.key),
                )
            }
        }
    }
    for event in &form.events {
        let stamped = matches!(field(event, "at"), Some((_, Type::Instant)))
            && matches!(field(event, "actor"), Some((_, Type::Text)));
        if !stamped {
            return refuse(
                Law::Stamp,
                format!(
                    "{} does not say when (at, an instant) and who (actor, a text)",
                    event.name
                ),
            );
        }
    }
    Ok(())
}

/// The typing law: each register's one type, which its order reads.
fn typed(form: &Form) -> Result<Vec<Type>, Refusal> {
    let mut types = Vec::with_capacity(form.registers.len());
    for register in &form.registers {
        let mut written: Option<&Type> = None;
        for write in &register.writes {
            let Some(event) = form.events.iter().find(|e| e.name == write.event) else {
                return refuse(
                    Law::Typed,
                    format!(
                        "the register {} is written by {}, which is no event",
                        register.name, write.event
                    ),
                );
            };
            let Some((_, ty)) = field(event, &write.field) else {
                return refuse(
                    Law::Typed,
                    format!(
                        "{} has no field {} to write {}",
                        event.name, write.field, register.name
                    ),
                );
            };
            if written.is_some_and(|held| held != ty) {
                return refuse(
                    Law::Typed,
                    format!(
                        "the register {} is written by fields of different types",
                        register.name
                    ),
                );
            }
            written = Some(ty);
        }
        let Some(ty) = written else {
            return refuse(
                Law::Typed,
                format!(
                    "no event writes the register {}, so it has no type",
                    register.name
                ),
            );
        };
        let fits = match (&register.order, ty) {
            (Poset::Discrete, _)
            | (Poset::Total, Type::Integer)
            | (Poset::Inclusion, Type::List(_)) => true,
            (Poset::Machine(covers), Type::Enum(states)) => covers
                .iter()
                .all(|(lower, upper)| states.contains(lower) && states.contains(upper)),
            _ => false,
        };
        if !fits {
            return refuse(
                Law::Typed,
                format!(
                    "the register {} holds values its order does not read",
                    register.name
                ),
            );
        }
        types.push(ty.clone());
    }
    Ok(types)
}

/// The partial-order law, then the bottom law.
fn ordered(form: &Form, types: &[Type]) -> Result<Vec<Reg>, Refusal> {
    let mut registers = Vec::with_capacity(form.registers.len());
    for (register, ty) in form.registers.iter().zip(types) {
        let order = match (&register.order, ty) {
            (Poset::Machine(covers), Type::Enum(states)) => {
                Interpreted::Machine(Closure::of(states, covers).map_err(|state| {
                    Refusal::new(
                        Law::PartialOrder,
                        format!(
                            "the machine of {} has a cycle through {state}",
                            register.name
                        ),
                    )
                })?)
            }
            (Poset::Machine(_), _) => unreachable!("the typing law"),
            (Poset::Discrete, _) => Interpreted::Discrete,
            (Poset::Total, _) => Interpreted::Total,
            (Poset::Inclusion, _) => Interpreted::Inclusion,
        };
        registers.push((register, ty, order));
    }
    registers
        .into_iter()
        .map(|(register, ty, order)| {
            let bottom = match (&order, ty) {
                (Interpreted::Inclusion, _) => true,
                (Interpreted::Machine(closure), _) => closure.has_bottom(),
                (Interpreted::Discrete, Type::Enum(states)) => states.len() == 1,
                (Interpreted::Discrete | Interpreted::Total, _) => false,
            };
            if register.inflationary && !bottom {
                return refuse(
                    Law::Bottom,
                    format!(
                        "the register {} is inflationary and its order has no bottom",
                        register.name
                    ),
                );
            }
            Ok(Reg {
                name: register.name.clone(),
                order,
                inflationary: register.inflationary,
            })
        })
        .collect()
}
