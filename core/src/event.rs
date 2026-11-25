//! The events of the todo schema (SPEC §4), and the envelopes, names and
//! hashing of SPEC §3, which `prodrome-data` defines and this module
//! re-exports.
//!
//! Never change the field set of a shipped kind: a new field changes the
//! canonical print, so no stored object would match its hash any more. Store
//! new data in a new kind. The record kind carries the host's [`Payload`]
//! (see [`crate::payload`]); the conformance vectors were taken with the
//! payload `crate::reference`.

pub use prodrome_data::event::*;

use crate::payload::{datetime_field, required, string_field, string_or_empty, Payload};
use crate::term::Term;
use prodrome_data::literal::codec::headed;
use prodrome_data::literal::{Datetime, ProdromeError, Value};

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

/// `SpecRevised(todo, at, actor, spec, note)`: a new authored price, effective
/// at `at`. It does not change the environment; the query layer picks the
/// latest spec.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecRevised {
    pub todo: TodoId,
    pub at: Datetime,
    pub actor: Actor,
    pub spec: Term,
    pub note: String,
}

/// A content record: a todo's whole content as of `at`.
///
/// An edit is a new record for the same id, and the fold keeps the latest.
/// The host defines the [`Payload`], and the core reads only its `spec` and
/// its `checklist_len` (SPEC §4).
#[derive(Debug, Clone, PartialEq)]
pub struct Authored<P> {
    pub todo: TodoId,
    pub at: Datetime,
    pub actor: Actor,
    pub payload: P,
}

/// The closed kinds of the todo schema (SPEC §4).
#[derive(Debug, Clone, PartialEq)]
pub enum TodoEvent<P> {
    Created(Created),
    Completed(Lifecycle),
    Cancelled(Lifecycle),
    Reopened(Lifecycle),
    /// A pass at a recurring todo: it records care and changes no state.
    /// `Recur` re-anchors to these events (SPEC §7.3).
    Tended(Lifecycle),
    SpecRevised(SpecRevised),
    /// Boxed, because a content record is an order of magnitude wider than a
    /// lifecycle event, and a log holds mostly lifecycle events.
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

    /// The instant that the writer stamped: data, never an order, so no clock
    /// decides which event wins (SPEC §1).
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

    /// The constructor name of the kind; for a record, the payload's
    /// constructor name.
    pub fn kind_name(&self) -> &'static str {
        match self {
            TodoEvent::Created(_) => "Created",
            TodoEvent::Completed(_) => "Completed",
            TodoEvent::Cancelled(_) => "Cancelled",
            TodoEvent::Reopened(_) => "Reopened",
            TodoEvent::Tended(_) => "Tended",
            TodoEvent::SpecRevised(_) => "SpecRevised",
            TodoEvent::Authored(_) => P::CONSTRUCTOR.0,
        }
    }
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

/// A content record: the core's three fields and the host's payload.
///
/// This function checks only the three fields; the caller must build
/// `payload` through the payload's own smart constructor.
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

// --- the literal round trip --------------------------------------------------

impl<P: Payload> TodoEvent<P> {
    /// The event as a literal with every field, in declared order and in
    /// keyword form, so the print does not depend on how the value was built.
    pub fn to_value(&self) -> Value {
        match self {
            TodoEvent::Created(e) => Value::call(
                "Created",
                vec![
                    field("todo", Value::str(e.todo.as_str())),
                    field("at", Value::Datetime(e.at)),
                    field("actor", Value::str(e.actor.as_str())),
                    field("text", Value::str(e.text.clone())),
                    field("note", Value::str(e.note.clone())),
                ],
            ),
            TodoEvent::Completed(e)
            | TodoEvent::Cancelled(e)
            | TodoEvent::Reopened(e)
            | TodoEvent::Tended(e) => Value::call(
                self.kind_name(),
                vec![
                    field("todo", Value::str(e.todo.as_str())),
                    field("at", Value::Datetime(e.at)),
                    field("actor", Value::str(e.actor.as_str())),
                    field("note", Value::str(e.note.clone())),
                ],
            ),
            TodoEvent::SpecRevised(e) => Value::call(
                "SpecRevised",
                vec![
                    field("todo", Value::str(e.todo.as_str())),
                    field("at", Value::Datetime(e.at)),
                    field("actor", Value::str(e.actor.as_str())),
                    field("spec", e.spec.to_value()),
                    field("note", Value::str(e.note.clone())),
                ],
            ),
            TodoEvent::Authored(e) => e.to_value(),
        }
    }
}

impl<P: Payload> Authored<P> {
    /// A content record as a literal: the core's three fields, then the
    /// payload's fields in the order of its constructor.
    ///
    /// The content fold (SPEC §6.3) returns records, and a consumer compares
    /// their prints, so [`TodoEvent::to_value`] prints a record through this
    /// function too.
    pub fn to_value(&self) -> Value {
        headed(
            vec![
                ("todo", Value::str(self.todo.as_str())),
                ("at", Value::Datetime(self.at)),
                ("actor", Value::str(self.actor.as_str())),
            ],
            P::CONSTRUCTOR,
            self.payload.values(),
        )
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
        name if name == P::CONSTRUCTOR.0 => {
            mk_record(&todo()?, at()?, &actor()?, P::from_call(call)?)
        }
        other => Err(ProdromeError::invalid(format!(
            "{other} is not one of SPEC §4's kinds"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::Todo;
    use prodrome_data::literal::parse_literal;

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
