//! The record kind of SPEC §4 as a parameter: the fields that a host
//! attaches to a todo.
//!
//! The core owns the lifecycle kinds and the fields `todo`, `at` and `actor`
//! of a record, and a [`Payload`] owns the other fields. The core reads only
//! the spec and the checklist length of a payload (SPEC §6).
//!
//! The name, the fields and the field order of a payload's constructor are in
//! every stored print and its hash, so they never change. To store new data,
//! add a new constructor (SPEC §1). `crate::reference` is the reference
//! payload.

use std::fmt::Debug;

use crate::literal::codec::{self, Constructor, Literal, Record};
use crate::literal::{Call, Datetime, ProdromeError, Value};
use crate::term::Term;

/// The fields of a record (SPEC §4) beyond `todo`, `at` and `actor`.
///
/// A store holds one payload type, because the vocabulary that a stored
/// object is parsed against is closed (SPEC §2). So a payload is a type
/// parameter and never a `dyn`.
///
/// # The contract
///
/// - The printer emits `KIND(todo=…, at=…, actor=…, <the fields in order>)`,
///   where `KIND` and the fields are [`Record::CONSTRUCTOR`]'s, and the
///   values are [`Record::values`].
/// - [`Payload::VOCABULARY`] holds every other constructor that the fields
///   nest. None of these names may be one of the core's, because the core's
///   name wins and the payload's is then unreachable (SPEC §2).
/// - [`Record::from_call`] is the parse boundary. It must refuse what the
///   smart constructor refuses, and its error is the parse error. A derived
///   record calls the smart constructor that `#[literal(new = ...)]` names.
/// - [`Payload::spec`] and [`Payload::checklist_len`] are the only readings
///   of a payload that the core takes (SPEC §6).
///
/// The bound is `PartialEq` and not `Eq`, because a spec is a [`Term`], and
/// a term holds floats.
pub trait Payload: Record + Clone + PartialEq + Debug + Send + Sync + 'static {
    /// Every other constructor that the fields use, with its declared field
    /// order. It is empty for a payload of plain values.
    const VOCABULARY: &'static [Constructor];

    /// The spec by which the record prices its todo, if it carries one
    /// (SPEC §6.2, §6.4).
    fn spec(&self) -> Option<&Term>;

    /// The number of checklist items in the record (SPEC §6.4). A payload
    /// without a checklist returns zero, and prices by its spec alone.
    fn checklist_len(&self) -> usize;
}

/// The core's fields of the record kind (SPEC §4), in order, before the
/// payload's fields. They never change.
pub const RECORD_PREFIX: &[&str] = &["todo", "at", "actor"];

/// The declared field order of the record kind: [`RECORD_PREFIX`], then the
/// payload's fields. The parser binds a stored record against it.
pub fn record_signature<P: Payload>() -> Vec<&'static str> {
    let mut fields = RECORD_PREFIX.to_vec();
    fields.extend_from_slice(P::CONSTRUCTOR.1);
    fields
}

// --- reading a stored call --------------------------------------------------
//
// The helpers a hand-written read wants, public because a HOST may write one:
// implementing a read outside this crate should not mean reimplementing the
// refusals the core's own kinds are parsed with. Each is a read of
// `literal::codec`, which a derived `Literal` impl makes too, so a derived
// read and a hand-written one refuse with the same messages.

/// The value of the required field `name`.
pub fn required<'a>(call: &'a Call, name: &str) -> Result<&'a Value, ProdromeError> {
    codec::required(call, name)
}

pub fn as_string(value: &Value, context: &str) -> Result<String, ProdromeError> {
    String::from_field(value, &context)
}

pub fn string_field(call: &Call, name: &str) -> Result<String, ProdromeError> {
    codec::read(call, name, String::from_field)
}

/// A string field where `""` means absent. A missing field reads as `""`,
/// so a literal that omits the trailing defaults parses as the canonical
/// print of the same value.
pub fn string_or_empty(call: &Call, name: &str) -> Result<String, ProdromeError> {
    codec::read_or_default(call, name, String::from_field)
}

pub fn datetime_field(call: &Call, name: &str) -> Result<Datetime, ProdromeError> {
    codec::read(call, name, Datetime::from_field)
}

pub fn bool_field(call: &Call, name: &str) -> Result<bool, ProdromeError> {
    codec::read(call, name, bool::from_field)
}

pub fn tuple_field<'a>(call: &'a Call, name: &str) -> Result<&'a [Value], ProdromeError> {
    codec::tuple(required(call, name)?, &format_args!("{}.{name}", call.name))
}

pub fn tuple_or_empty<'a>(call: &'a Call, name: &str) -> Result<&'a [Value], ProdromeError> {
    match call.field(name) {
        None => Ok(&[]),
        Some(value) => codec::tuple(value, &format_args!("{}.{name}", call.name)),
    }
}

pub fn strings(items: &[Value], context: &str) -> Result<Vec<String>, ProdromeError> {
    items.iter().map(|item| as_string(item, context)).collect()
}

/// A field that is a term (SPEC §7) or `None`.
pub fn spec_field(call: &Call, name: &str) -> Result<Option<Term>, ProdromeError> {
    codec::read(call, name, Option::<Term>::from_field)
}

/// `value` as a call of the constructor `name`.
pub fn expect_call<'a>(value: &'a Value, name: &str) -> Result<&'a Call, ProdromeError> {
    codec::call_of(value, name)
}
