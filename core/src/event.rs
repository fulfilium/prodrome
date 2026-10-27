//! §4 — the events, and §3's envelopes and hashing.
//!
//! The closed kinds of §4, as Rust types. The schema-evolution rule is
//! absolute: the field set of a SHIPPED kind is frozen forever, because a new
//! field changes what the canonical printer emits and would orphan every
//! stored object from its hash. Evolution is a NEW KIND.
//!
//! SIX KINDS ARE THE DATABASE'S AND ONE IS THE HOST'S. `Created`, `Completed`,
//! `Cancelled`, `Reopened`, `Tended` and `SpecRevised` are lifecycle, care and
//! price — the semantics §5 and §6 are written about — and their fields are
//! here. The RECORD kind is a todo's CONTENT, and content is a deployment's:
//! it is `KIND(todo, at, actor, <the host's fields>)`, where a [`Payload`]
//! supplies the constructor name, the fields, their parse and their print. See
//! [`crate::payload`]; the payload the conformance vectors were taken with is
//! [`crate::reference`].
//!
//! Names that mean different things are different types even when they are all
//! strings (§1): [`Hash`], [`TodoId`], [`Actor`], [`Name`]. The `mk_*`
//! constructors are the only path from parameters to a value, and they are also
//! the PARSE boundary — a stored literal arrives as bare strings and comes out
//! of [`Envelope::from_value`] having been through them, which is why nothing
//! downstream re-checks.
//!
//! "" MEANS ABSENT on a shipped string field, on purpose and forever: absence
//! with two spellings is worse than a sentinel with one meaning. Where absence
//! is NOT a shipped string — `Sealed.prev` at genesis, `Woven.event` — it is an
//! `Option`, and the printer puts the `''`/`None` back.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use crate::change::Change;
use crate::genesis::Genesis;
use crate::literal::codec::Literal;
use crate::literal::{
    parse_literal, print_literal, Call, Datetime, ProdromeError, Signature, Table, Value,
    Vocabulary,
};
use crate::payload::{
    as_string, datetime_field, required, string_field, string_or_empty, tuple_field, Payload,
};
use crate::schema::Schema;
use crate::sign::{KeyAdded, KeyRevoked, Signed};
use crate::snapshot::Snapshot;
use crate::term::Term;

// --- the names, as types ----------------------------------------------------

#[macro_export]
macro_rules! newtype_str {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

/// Lent to [`crate::reference`], which declares two string newtypes of its
/// own. `#[allow]` because a core built without that feature has no other
/// user for it.
#[allow(unused_imports)]
pub(crate) use newtype_str;

newtype_str! {
    /// An object's name: sha256 of its canonical print, 64 lowercase hex.
    Hash
}

newtype_str! {
    /// A todo's identifier: `^[a-z0-9_-]+$`.
    TodoId
}

newtype_str! {
    /// Who wrote an event. SYNTACTIC only (`^[a-z][a-z0-9_-]*$`): WHICH actors
    /// exist, and what a host makes of each, is instance data — the engine
    /// knows the mechanism, never the roster (§5, [`crate::policy`]).
    Actor
}

newtype_str! {
    /// An identifier in a DOCUMENT's vocabulary — a payload's kind or category
    /// field: `^[A-Za-z][A-Za-z0-9_]*$`. Which values exist is the document's
    /// business, not the engine's.
    Name
}

pub(crate) fn lower_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn matches_todo(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn matches_actor(text: &str) -> bool {
    let mut bytes = text.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn matches_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

impl Hash {
    /// 64 lowercase hex, checked once so that every later reader — a parent
    /// name, a ref filename, a `prev` — is a name and not a hope.
    pub fn new(text: impl Into<String>) -> Result<Hash, ProdromeError> {
        let text = text.into();
        if lower_hex(&text, 64) {
            Ok(Hash(text))
        } else {
            Err(ProdromeError::invalid(format!(
                "object name must be 64 lowercase hex, got {text:?}"
            )))
        }
    }

    pub(crate) fn of_bytes(bytes: &[u8]) -> Hash {
        Hash(
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    }
}

impl TodoId {
    pub fn new(text: impl Into<String>) -> Result<TodoId, ProdromeError> {
        let text = text.into();
        if matches_todo(&text) {
            Ok(TodoId(text))
        } else {
            Err(ProdromeError::invalid(format!(
                "todo id must match ^[a-z0-9_-]+$ (nonempty), got {text:?}"
            )))
        }
    }
}

impl Actor {
    pub fn new(text: impl Into<String>) -> Result<Actor, ProdromeError> {
        let text = text.into();
        if matches_actor(&text) {
            Ok(Actor(text))
        } else {
            Err(ProdromeError::invalid(format!(
                "actor must match ^[a-z][a-z0-9_-]*$, got {text:?}"
            )))
        }
    }
}

impl Literal for Hash {
    fn to_value(&self) -> Value {
        Value::str(self.as_str())
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        Hash::new(String::from_value(value)?)
    }
}

impl Literal for Actor {
    fn to_value(&self) -> Value {
        Value::str(self.as_str())
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        Actor::new(String::from_value(value)?)
    }
}

impl Name {
    /// A required identifier.
    pub fn new(field: &str, text: impl Into<String>) -> Result<Name, ProdromeError> {
        let text = text.into();
        if matches_name(&text) {
            Ok(Name(text))
        } else {
            Err(ProdromeError::invalid(format!(
                "{field} must be an identifier, got {text:?}"
            )))
        }
    }

    /// An identifier that may be absent, where `""` is "not recorded" and
    /// nothing else.
    pub fn optional(field: &str, text: impl Into<String>) -> Result<Option<Name>, ProdromeError> {
        let text = text.into();
        if text.is_empty() {
            Ok(None)
        } else {
            Name::new(field, text).map(Some)
        }
    }
}

// --- the spec a repricing carries -------------------------------------------
//
// A `SpecRevised` and a record carry a §7 `Term`, and `term::Term` IS that type:
// one parser, one printer, one evaluator. The field is `term::Term` and not a
// wrapper, because a wrapper would be a second name for the same value and a
// place for a second reading to grow. §7's per-field bounds arrive with it
// (`Flat`'s 0..1, `Conj`'s `p`, `Decay`'s lead-up), so a stored spec that
// parses is a spec that evaluates.

// --- the records ------------------------------------------------------------

/// `Created(todo, at, actor, text, note)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    pub todo: TodoId,
    pub at: Datetime,
    pub actor: Actor,
    pub text: String,
    pub note: String,
}

/// The four kinds that share a shape: `Completed`, `Cancelled`, `Reopened`
/// and `Tended`, each `(todo, at, actor, note)`. They are DIFFERENT kinds — the
/// enum below keeps them apart — and one record type is what stops the four
/// from drifting apart field by field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    pub todo: TodoId,
    pub at: Datetime,
    pub actor: Actor,
    pub note: String,
}

/// `SpecRevised(todo, at, actor, spec, note)` — a new authored price, effective
/// at `at`. No env effect: the query layer picks the latest spec.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecRevised {
    pub todo: TodoId,
    pub at: Datetime,
    pub actor: Actor,
    pub spec: Term,
    pub note: String,
}

/// A todo's CONTENT, in full, as of `at`. Snapshot semantics, like a git blob:
/// an edit is a new record for the same id and the fold keeps the latest.
///
/// THE THREE FIELDS ARE THE CORE'S AND THE PAYLOAD IS THE HOST'S. Every event
/// in this database says who, about what, and when its writer thought it was;
/// what a record additionally SAYS is [`Payload`], and the core reads exactly
/// two things out of it (`spec`, `checklist_len`).
#[derive(Debug, Clone, PartialEq)]
pub struct Authored<P> {
    pub todo: TodoId,
    pub at: Datetime,
    pub actor: Actor,
    pub payload: P,
}

/// The closed kinds of §4.
#[derive(Debug, Clone, PartialEq)]
pub enum TodoEvent<P> {
    Created(Created),
    Completed(Lifecycle),
    Cancelled(Lifecycle),
    Reopened(Lifecycle),
    /// A pass at a recurring todo: care recorded, state untouched. A todo that
    /// is never done is never `Completed`, and `Recur` re-anchors to these.
    Tended(Lifecycle),
    SpecRevised(SpecRevised),
    /// Boxed: a content record carries a todo's whole content and is an order
    /// of magnitude wider than a lifecycle event, and a log is mostly
    /// lifecycle.
    Authored(Box<Authored<P>>),
}

impl<P: Payload> TodoEvent<P> {
    pub fn todo(&self) -> &TodoId {
        match self {
            TodoEvent::Created(e) => &e.todo,
            TodoEvent::Completed(e)
            | TodoEvent::Cancelled(e)
            | TodoEvent::Reopened(e)
            | TodoEvent::Tended(e) => &e.todo,
            TodoEvent::SpecRevised(e) => &e.todo,
            TodoEvent::Authored(e) => &e.todo,
        }
    }

    /// The instant the writer stamped. DATA, never an order (§1): no clock
    /// decides who wins.
    pub fn at(&self) -> Datetime {
        match self {
            TodoEvent::Created(e) => e.at,
            TodoEvent::Completed(e)
            | TodoEvent::Cancelled(e)
            | TodoEvent::Reopened(e)
            | TodoEvent::Tended(e) => e.at,
            TodoEvent::SpecRevised(e) => e.at,
            TodoEvent::Authored(e) => e.at,
        }
    }

    pub fn actor(&self) -> &Actor {
        match self {
            TodoEvent::Created(e) => &e.actor,
            TodoEvent::Completed(e)
            | TodoEvent::Cancelled(e)
            | TodoEvent::Reopened(e)
            | TodoEvent::Tended(e) => &e.actor,
            TodoEvent::SpecRevised(e) => &e.actor,
            TodoEvent::Authored(e) => &e.actor,
        }
    }

    /// The constructor name this kind prints as — the payload's own for a
    /// record.
    pub fn kind_name(&self) -> &'static str {
        match self {
            TodoEvent::Created(_) => "Created",
            TodoEvent::Completed(_) => "Completed",
            TodoEvent::Cancelled(_) => "Cancelled",
            TodoEvent::Reopened(_) => "Reopened",
            TodoEvent::Tended(_) => "Tended",
            TodoEvent::SpecRevised(_) => "SpecRevised",
            TodoEvent::Authored(_) => P::KIND,
        }
    }
}

/// The stored object (§3), holding an event of the schema `E`. `Sealed` and
/// `Woven` are the legacy envelopes, read forever and written by no `append`
/// (SPEC §3).
#[derive(Debug, Clone, PartialEq)]
pub enum Envelope<E> {
    Sealed {
        prev: Option<Hash>,
        event: E,
    },
    Woven {
        parents: Vec<Hash>,
        event: Option<E>,
    },
    Genesis(Genesis),
    Change(Change<E>),
    Snapshot(Snapshot),
    /// A detached signature of another object ([`crate::sign`]).
    Signed(Signed),
    KeyAdded(KeyAdded),
    KeyRevoked(KeyRevoked),
}

impl<E> Envelope<E> {
    pub fn event(&self) -> Option<&E> {
        match self {
            Envelope::Sealed { event, .. } => Some(event),
            Envelope::Woven { event, .. } => event.as_ref(),
            Envelope::Change(change) => Some(&change.event),
            Envelope::Genesis(_)
            | Envelope::Snapshot(_)
            | Envelope::Signed(_)
            | Envelope::KeyAdded(_)
            | Envelope::KeyRevoked(_) => None,
        }
    }

    pub fn into_event(self) -> Option<E> {
        match self {
            Envelope::Sealed { event, .. } => Some(event),
            Envelope::Woven { event, .. } => event,
            Envelope::Change(change) => Some(change.event),
            Envelope::Genesis(_)
            | Envelope::Snapshot(_)
            | Envelope::Signed(_)
            | Envelope::KeyAdded(_)
            | Envelope::KeyRevoked(_) => None,
        }
    }

    /// The genesis a change, a snapshot or a key names. A signature names
    /// none: its prodrome is its object's.
    pub fn genesis(&self) -> Option<&Hash> {
        match self {
            Envelope::Change(change) => Some(&change.genesis),
            Envelope::Snapshot(snapshot) => Some(&snapshot.genesis),
            Envelope::KeyAdded(added) => Some(&added.genesis),
            Envelope::KeyRevoked(revoked) => Some(&revoked.genesis),
            Envelope::Sealed { .. }
            | Envelope::Woven { .. }
            | Envelope::Genesis(_)
            | Envelope::Signed(_) => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Envelope::Sealed { .. } => "Sealed",
            Envelope::Woven { .. } => "Woven",
            Envelope::Genesis(_) => "Genesis",
            Envelope::Change(_) => "Change",
            Envelope::Snapshot(_) => "Snapshot",
            Envelope::Signed(_) => "Signed",
            Envelope::KeyAdded(_) => "KeyAdded",
            Envelope::KeyRevoked(_) => "KeyRevoked",
        }
    }
}

/// What an object RESTS ON (§3), sorted and distinct: the edges it names
/// and the genesis it names. This is the causal order, the one every
/// reader of it reads (tips, the linearisation, closure, the interior, a
/// replica's walk): a change rests on its genesis as on its deps, so no
/// genesis is a head of a prodrome holding anything begun from it.
pub fn parents_of<E>(envelope: &Envelope<E>) -> Vec<Hash> {
    let edges = match envelope {
        Envelope::Sealed { prev, .. } => prev.iter().cloned().collect(),
        Envelope::Woven { parents, .. } => parents.clone(),
        Envelope::Genesis(_) | Envelope::KeyAdded(_) => Vec::new(),
        Envelope::Change(change) => change.deps.clone(),
        Envelope::Snapshot(snapshot) => snapshot
            .tips
            .iter()
            .chain(&snapshot.previous)
            .cloned()
            .collect(),
        Envelope::Signed(signed) => vec![signed.object.clone()],
        Envelope::KeyRevoked(revoked) => revoked.deps.clone(),
    };
    let parents: BTreeSet<Hash> = edges
        .into_iter()
        .chain(envelope.genesis().cloned())
        .collect();
    parents.into_iter().collect()
}

// --- smart constructors ------------------------------------------------------

pub fn mk_created<P: Payload>(
    todo: &str,
    at: Datetime,
    actor: &str,
    text: &str,
    note: &str,
) -> Result<TodoEvent<P>, ProdromeError> {
    Ok(TodoEvent::Created(Created {
        todo: TodoId::new(todo)?,
        at,
        actor: Actor::new(actor)?,
        text: text.to_owned(),
        note: note.to_owned(),
    }))
}

fn mk_lifecycle(
    todo: &str,
    at: Datetime,
    actor: &str,
    note: &str,
) -> Result<Lifecycle, ProdromeError> {
    Ok(Lifecycle {
        todo: TodoId::new(todo)?,
        at,
        actor: Actor::new(actor)?,
        note: note.to_owned(),
    })
}

pub fn mk_completed<P: Payload>(
    todo: &str,
    at: Datetime,
    actor: &str,
    note: &str,
) -> Result<TodoEvent<P>, ProdromeError> {
    Ok(TodoEvent::Completed(mk_lifecycle(todo, at, actor, note)?))
}

pub fn mk_cancelled<P: Payload>(
    todo: &str,
    at: Datetime,
    actor: &str,
    note: &str,
) -> Result<TodoEvent<P>, ProdromeError> {
    Ok(TodoEvent::Cancelled(mk_lifecycle(todo, at, actor, note)?))
}

pub fn mk_reopened<P: Payload>(
    todo: &str,
    at: Datetime,
    actor: &str,
    note: &str,
) -> Result<TodoEvent<P>, ProdromeError> {
    Ok(TodoEvent::Reopened(mk_lifecycle(todo, at, actor, note)?))
}

pub fn mk_tended<P: Payload>(
    todo: &str,
    at: Datetime,
    actor: &str,
    note: &str,
) -> Result<TodoEvent<P>, ProdromeError> {
    Ok(TodoEvent::Tended(mk_lifecycle(todo, at, actor, note)?))
}

pub fn mk_spec_revised<P: Payload>(
    todo: &str,
    at: Datetime,
    actor: &str,
    spec: Term,
    note: &str,
) -> Result<TodoEvent<P>, ProdromeError> {
    Ok(TodoEvent::SpecRevised(SpecRevised {
        todo: TodoId::new(todo)?,
        at,
        actor: Actor::new(actor)?,
        spec,
        note: note.to_owned(),
    }))
}

/// A CONTENT RECORD: the core's three fields, and the host's payload.
///
/// The payload arrived through its own smart constructor, so this checks what
/// the core owns and nothing else — which is the whole of the seam.
pub fn mk_record<P: Payload>(
    todo: &str,
    at: Datetime,
    actor: &str,
    payload: P,
) -> Result<TodoEvent<P>, ProdromeError> {
    Ok(TodoEvent::Authored(Box::new(Authored {
        todo: TodoId::new(todo)?,
        at,
        actor: Actor::new(actor)?,
        payload,
    })))
}

pub fn mk_sealed<E: Schema>(prev: Option<Hash>, event: E) -> Envelope<E> {
    Envelope::Sealed { prev, event }
}

/// Sorted, so a set of names is one print wherever it is made; a name twice
/// is refused, since it would let two objects mean one set.
pub(crate) fn sorted_distinct(
    field: &str,
    what: &str,
    mut names: Vec<Hash>,
) -> Result<Vec<Hash>, ProdromeError> {
    names.sort();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ProdromeError::invalid(format!(
            "{field} names a {what} twice: {:?}",
            names.iter().map(Hash::as_str).collect::<Vec<_>>()
        )));
    }
    Ok(names)
}

/// Two parents at least: one is a `Sealed`, none is genesis.
pub fn mk_woven<E: Schema>(
    parents: Vec<Hash>,
    event: Option<E>,
) -> Result<Envelope<E>, ProdromeError> {
    if parents.len() < 2 {
        return Err(ProdromeError::invalid(format!(
            "Woven.parents must name at least two parents (one parent is a Sealed), got {}",
            parents.len()
        )));
    }
    Ok(Envelope::Woven {
        parents: sorted_distinct("Woven.parents", "parent", parents)?,
        event,
    })
}

// --- the vocabulary ----------------------------------------------------------

/// The envelopes' constructors, name and declared field order: the database's
/// own, whatever the schema.
pub const ENVELOPE_SIGNATURES: &[(&str, &[&str])] = &[
    ("Sealed", &["prev", "event"]),
    ("Woven", &["parents", "event"]),
    ("Genesis", &["label", "nonce"]),
    ("Change", &["genesis", "deps", "event"]),
    ("Snapshot", &["genesis", "tips", "previous"]),
    ("Signed", &["object", "key", "signature"]),
    ("KeyAdded", &["genesis", "actor", "key"]),
    ("KeyRevoked", &["genesis", "deps", "actor", "key"]),
];

/// The whole vocabulary a stored artifact may use: the envelopes, then the
/// schema's own ([`crate::todo::TodoVocabulary`] for the todo's). `datetime`
/// and `timedelta` are the grammar's own and need no entry.
///
/// STILL A WHITELIST, and the envelopes win: a schema that named a kind
/// `Change` would not shadow §3's, it would be unreachable.
pub struct EventVocabulary<'v, E: Schema> {
    schema: &'v E::Vocabulary,
}

impl<'v, E: Schema> EventVocabulary<'v, E> {
    /// The envelopes' vocabulary, then the schema `schema`'s.
    pub fn new(schema: &'v E::Vocabulary) -> EventVocabulary<'v, E> {
        EventVocabulary { schema }
    }
}

impl<E: Schema> Vocabulary for EventVocabulary<'_, E> {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        Table(ENVELOPE_SIGNATURES)
            .find(name)
            .or_else(|| self.schema.signature(name))
    }
}

// --- the literal round trip --------------------------------------------------

fn text(value: impl Into<String>) -> Value {
    Value::Str(value.into())
}

pub(crate) fn field(name: &str, value: Value) -> (String, Value) {
    (name.to_owned(), value)
}

pub(crate) fn hashes_value(names: &[Hash]) -> Value {
    Value::Tuple(names.iter().map(|name| text(name.as_str())).collect())
}

pub(crate) fn hashes(call: &Call, name: &str) -> Result<Vec<Hash>, ProdromeError> {
    let context = format!("{}.{name}", call.name);
    tuple_field(call, name)?
        .iter()
        .map(|item| Hash::new(as_string(item, &context)?))
        .collect()
}

/// A name where `''` is absent.
pub(crate) fn optional_hash(call: &Call, name: &str) -> Result<Option<Hash>, ProdromeError> {
    let text = string_field(call, name)?;
    (!text.is_empty()).then(|| Hash::new(text)).transpose()
}

impl<P: Payload> TodoEvent<P> {
    /// The event as a literal — EVERY field, in declared order, keyword form,
    /// which is what makes the print total and order-stable however the value
    /// was built.
    pub fn to_value(&self) -> Value {
        match self {
            TodoEvent::Created(e) => Value::call(
                "Created",
                vec![
                    field("todo", text(e.todo.0.clone())),
                    field("at", Value::Datetime(e.at)),
                    field("actor", text(e.actor.0.clone())),
                    field("text", text(e.text.clone())),
                    field("note", text(e.note.clone())),
                ],
            ),
            TodoEvent::Completed(e)
            | TodoEvent::Cancelled(e)
            | TodoEvent::Reopened(e)
            | TodoEvent::Tended(e) => Value::call(
                self.kind_name(),
                vec![
                    field("todo", text(e.todo.0.clone())),
                    field("at", Value::Datetime(e.at)),
                    field("actor", text(e.actor.0.clone())),
                    field("note", text(e.note.clone())),
                ],
            ),
            TodoEvent::SpecRevised(e) => Value::call(
                "SpecRevised",
                vec![
                    field("todo", text(e.todo.0.clone())),
                    field("at", Value::Datetime(e.at)),
                    field("actor", text(e.actor.0.clone())),
                    field("spec", e.spec.to_value()),
                    field("note", text(e.note.clone())),
                ],
            ),
            TodoEvent::Authored(e) => e.to_value(),
        }
    }
}

impl<P: Payload> Authored<P> {
    /// A content record as a literal: the core's three fields, then the
    /// payload's own, in `P::FIELDS`'s order.
    ///
    /// Split out of [`TodoEvent::to_value`] because the CONTENT fold (§6.3)
    /// hands back records and their print is what a consumer compares — one
    /// printer, reached from either shape.
    pub fn to_value(&self) -> Value {
        let payload = self.payload.fields();
        let mut fields = Vec::with_capacity(3 + payload.len());
        fields.push(field("todo", text(self.todo.0.clone())));
        fields.push(field("at", Value::Datetime(self.at)));
        fields.push(field("actor", text(self.actor.0.clone())));
        fields.extend(payload.into_iter().map(|(name, value)| field(name, value)));
        debug_assert!(
            fields
                .iter()
                .skip(3)
                .map(|(name, _)| name.as_str())
                .eq(P::FIELDS.iter().copied()),
            "{}::fields() must answer with FIELDS, in order",
            P::KIND
        );
        Value::call(P::KIND, fields)
    }
}

impl<E: Schema> Envelope<E> {
    pub fn to_value(&self) -> Value {
        match self {
            Envelope::Sealed { prev, event } => Value::call(
                "Sealed",
                vec![
                    field("prev", text(prev.as_ref().map_or("", Hash::as_str))),
                    field("event", Schema::to_value(event)),
                ],
            ),
            Envelope::Woven { parents, event } => Value::call(
                "Woven",
                vec![
                    field("parents", hashes_value(parents)),
                    field(
                        "event",
                        event.as_ref().map_or(Value::None, Schema::to_value),
                    ),
                ],
            ),
            Envelope::Genesis(genesis) => genesis.to_value(),
            Envelope::Change(change) => change.to_value(),
            Envelope::Snapshot(snapshot) => snapshot.to_value(),
            Envelope::Signed(signed) => signed.to_value(),
            Envelope::KeyAdded(added) => added.to_value(),
            Envelope::KeyRevoked(revoked) => revoked.to_value(),
        }
    }

    /// A literal in, an envelope out, through every `mk_*` rule, its event
    /// parsed at the schema `schema`.
    pub fn from_value(schema: &E::Vocabulary, value: &Value) -> Result<Envelope<E>, ProdromeError> {
        let call = value
            .as_call()
            .ok_or_else(|| ProdromeError::invalid("an object must be a Sealed or a Woven"))?;
        match call.name.as_str() {
            "Sealed" => Ok(mk_sealed(
                optional_hash(call, "prev")?,
                E::from_value(schema, required(call, "event")?)?,
            )),
            "Woven" => {
                let event = match call.field("event") {
                    None | Some(Value::None) => None,
                    Some(other) => Some(E::from_value(schema, other)?),
                };
                mk_woven(hashes(call, "parents")?, event)
            }
            "Genesis" => Ok(Envelope::Genesis(Genesis::from_call(call)?)),
            "Change" => Ok(Envelope::Change(Change::from_call(schema, call)?)),
            "Snapshot" => Ok(Envelope::Snapshot(Snapshot::from_call(call)?)),
            "Signed" => Ok(Envelope::Signed(Signed::from_call(call)?)),
            "KeyAdded" => Ok(Envelope::KeyAdded(KeyAdded::from_call(call)?)),
            "KeyRevoked" => Ok(Envelope::KeyRevoked(KeyRevoked::from_call(call)?)),
            other => Err(ProdromeError::invalid(format!(
                "object is not a chain envelope: {other}"
            ))),
        }
    }
}

pub(crate) fn event_from_value<P: Payload>(value: &Value) -> Result<TodoEvent<P>, ProdromeError> {
    let call = value.as_call().ok_or_else(|| {
        ProdromeError::invalid(format!(
            "an event must be a constructor call, got {value:?}"
        ))
    })?;
    let todo = || string_field(call, "todo");
    let at = || datetime_field(call, "at");
    let actor = || string_field(call, "actor");
    match call.name.as_str() {
        "Created" => mk_created(
            &todo()?,
            at()?,
            &actor()?,
            &string_or_empty(call, "text")?,
            &string_or_empty(call, "note")?,
        ),
        "Completed" => mk_completed(&todo()?, at()?, &actor()?, &string_or_empty(call, "note")?),
        "Cancelled" => mk_cancelled(&todo()?, at()?, &actor()?, &string_or_empty(call, "note")?),
        "Reopened" => mk_reopened(&todo()?, at()?, &actor()?, &string_or_empty(call, "note")?),
        "Tended" => mk_tended(&todo()?, at()?, &actor()?, &string_or_empty(call, "note")?),
        "SpecRevised" => mk_spec_revised(
            &todo()?,
            at()?,
            &actor()?,
            Term::from_value(required(call, "spec")?)?,
            &string_or_empty(call, "note")?,
        ),
        // THE PAYLOAD'S KIND IS TRIED LAST, so a payload cannot name its record
        // after one of §4's own and take its place.
        name if name == P::KIND => mk_record(&todo()?, at()?, &actor()?, P::from_fields(call)?),
        other => Err(ProdromeError::invalid(format!(
            "{other} is not one of SPEC §4's kinds"
        ))),
    }
}

// --- hashing and printing -----------------------------------------------------

/// `print_literal` of an event — what every fold that needs a deterministic
/// tiebreak sorts by.
pub fn canonical<E: Schema>(event: &E) -> String {
    print_literal(&event.to_value())
}

/// An event's name apart from where it was written; never stored.
pub fn event_id<E: Schema>(event: &E) -> Hash {
    Hash::of_bytes(canonical(event).as_bytes())
}

/// The canonical print of an envelope. This is the object's BYTES: the name is
/// the sha256 of exactly this, and the file holds exactly this.
pub fn canonical_envelope<E: Schema>(envelope: &Envelope<E>) -> String {
    print_literal(&envelope.to_value())
}

/// The hash of the canonical print — used at APPEND time to name the object, at
/// which moment it is by construction also the hash of the stored bytes.
/// Verification hashes the STORED BYTES instead (`store::EventStore::load`),
/// never a reprint: git hashes bytes and not semantics, so that a printer
/// change cannot false-alarm the whole store as tampered.
pub fn seal_hash<E: Schema>(envelope: &Envelope<E>) -> Hash {
    Hash::of_bytes(canonical_envelope(envelope).as_bytes())
}

/// Read one stored object's text into an envelope, through the closed
/// vocabulary — the envelopes and the schema `schema`'s — and every `mk_*`
/// rule.
pub fn parse_envelope<E: Schema>(
    schema: &E::Vocabulary,
    text: &str,
) -> Result<Envelope<E>, ProdromeError> {
    let vocabulary = EventVocabulary::<E>::new(schema);
    Envelope::from_value(schema, &parse_literal(text, &vocabulary)?)
}

/// Read ONE event's canonical print back — [`canonical`]'s inverse, through the
/// same closed vocabulary. An envelope is the stored object and `parse_envelope`
/// is how a store reads one; this is for the callers that hold a bare event
/// print instead (a fold's input, a binding's argument), and it exists here
/// rather than at those call sites because the vocabulary and the smart
/// constructors are this module's, not theirs.
pub fn parse_event<E: Schema>(schema: &E::Vocabulary, text: &str) -> Result<E, ProdromeError> {
    let vocabulary = EventVocabulary::<E>::new(schema);
    E::from_value(schema, &parse_literal(text, &vocabulary)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::Todo;

    type Event = TodoEvent<Todo>;

    fn at() -> Datetime {
        Datetime::new(2026, 9, 6, 7, 3, 0, 0).expect("a real instant")
    }

    #[test]
    fn genesis_has_no_parents_and_prints_an_empty_prev() {
        let event: Event = mk_created("alpha", at(), "bassel", "", "").expect("valid");
        let genesis = mk_sealed(None, event);
        assert!(parents_of(&genesis).is_empty());
        assert_eq!(
            canonical_envelope(&genesis),
            "Sealed(prev='', event=Created(todo='alpha', at=datetime(2026, 9, 6, 7, 3, 0), actor='bassel', text='', note=''))"
        );
    }

    #[test]
    fn the_constructors_are_the_only_door() {
        assert!(mk_created::<Todo>("Alpha", at(), "bassel", "", "").is_err());
        assert!(mk_created::<Todo>("alpha", at(), "Bassel", "", "").is_err());
        assert!(mk_created::<Todo>("", at(), "bassel", "", "").is_err());
        assert!(Hash::new("nothex").is_err());
    }

    #[test]
    fn a_woven_sorts_its_parents_and_refuses_fewer_than_two() {
        let a = Hash::new("a".repeat(64)).expect("hex");
        let b = Hash::new("b".repeat(64)).expect("hex");
        assert!(mk_woven::<TodoEvent<Todo>>(vec![a.clone()], None).is_err());
        assert!(mk_woven::<TodoEvent<Todo>>(vec![a.clone(), a.clone()], None).is_err());
        let woven = mk_woven::<TodoEvent<Todo>>(vec![b.clone(), a.clone()], None)
            .expect("two distinct parents");
        assert_eq!(parents_of(&woven), vec![a, b]);
        assert!(woven.event().is_none());
    }

    #[test]
    fn a_spec_must_be_one_of_the_terms() {
        let schema = crate::todo::TodoVocabulary::default();
        let vocabulary = EventVocabulary::<TodoEvent<Todo>>::new(&schema);
        let flat = parse_literal("Flat(value=0.5)", &vocabulary).expect("parses");
        assert!(Term::from_value(&flat).is_ok());
        let not_a_term =
            parse_literal("Note(on='block', lines=('x',))", &vocabulary).expect("parses");
        assert!(Term::from_value(&not_a_term).is_err());
        assert!(Term::from_value(&Value::None).is_err());
        // And §7's BOUNDS travel with the kind: a term that parses is a term
        // that evaluates.
        let out_of_range = parse_literal("Flat(value=1.5)", &vocabulary).expect("parses");
        assert!(Term::from_value(&out_of_range).is_err());
    }
}
