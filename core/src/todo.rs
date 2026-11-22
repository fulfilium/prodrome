//! The todo schema, the reference implementation of [`Schema`] (SPEC §4, and
//! SPEC §6, "The todo schema's registers").
//!
//! Every legacy envelope holds an event of this schema, and every
//! conformance vector was taken with it.

use std::marker::PhantomData;

use std::collections::BTreeSet;

use crate::event::{event_from_value, Actor, Hash, TodoEvent, TodoId};
use crate::fold::{Discrete, Kind, RegisterType, Registers, Write};
use crate::fpl::{Candidates, Env, FplError, Outcome};
use crate::literal::{Datetime, ProdromeError, Signature, Table, Value, Vocabulary};
use crate::payload::{record_signature, Payload};
use crate::policy::Policy;
use crate::schema::{Bind, History, Price, Row, Schema};
use crate::term::schema::signatures;
use crate::term::Term;

/// The six todo kinds, each with its name and its declared field order.
pub const TODO_SIGNATURES: &[(&str, &[&str])] = &[
    ("Created", &["todo", "at", "actor", "text", "note"]),
    ("Completed", &["todo", "at", "actor", "note"]),
    ("Cancelled", &["todo", "at", "actor", "note"]),
    ("Reopened", &["todo", "at", "actor", "note"]),
    ("Tended", &["todo", "at", "actor", "note"]),
    ("SpecRevised", &["todo", "at", "actor", "spec", "note"]),
];

/// The constructors that the todo schema parses: the terms of SPEC §7, the
/// six kinds, and the payload's record kind with the constructors that its
/// fields nest.
///
/// The record kind has the fields `todo, at, actor` and then the payload's
/// fields. The core's names take precedence, so a payload record named
/// `Created` cannot be parsed as a record.
#[derive(Debug, Clone)]
pub struct TodoVocabulary<P> {
    /// `todo, at, actor` then the payload's fields, owned: the one signature that is
    /// not a compile-time table.
    record: Vec<&'static str>,
    payload: PhantomData<P>,
}

impl<P: Payload> Default for TodoVocabulary<P> {
    fn default() -> TodoVocabulary<P> {
        TodoVocabulary {
            record: record_signature::<P>(),
            payload: PhantomData,
        }
    }
}

impl<P: Payload> Vocabulary for TodoVocabulary<P> {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        if let Some(signature) = signatures().signature(name) {
            return Some(signature);
        }
        if let Some(signature) = Table(TODO_SIGNATURES).find(name) {
            return Some(signature);
        }
        if name == P::CONSTRUCTOR.0 {
            return Some(Signature::Fields(&self.record));
        }
        Table(P::VOCABULARY).find(name)
    }
}

impl<P: Payload> Schema for TodoEvent<P> {
    type Vocabulary = TodoVocabulary<P>;
    type Key = TodoId;
    type Register = Kind;

    fn registers(_: &TodoVocabulary<P>) -> Vec<Kind> {
        Kind::ALL.to_vec()
    }

    type Registers<'a> = Registers<'a, P>;

    fn to_value(&self) -> Value {
        TodoEvent::to_value(self)
    }

    fn from_value(_: &TodoVocabulary<P>, value: &Value) -> Result<Self, ProdromeError> {
        event_from_value(value)
    }

    fn key(&self) -> &TodoId {
        self.todo()
    }

    fn at(&self) -> Datetime {
        TodoEvent::at(self)
    }

    fn actor(&self) -> &Actor {
        TodoEvent::actor(self)
    }

    /// A lifecycle event writes the state, a repricing the spec, a record its
    /// content and, when it carries one, its spec. A tending accumulates and
    /// supersedes nothing; `Created` writes nothing.
    fn writes(&self) -> impl Iterator<Item = Kind> {
        Write::of(self).filter_map(|write| write.kind())
    }

    /// A content record binds whoever wrote it (§6.2, §6.3): writing content
    /// is what a writer the policy does not stand behind is for.
    fn asks(&self) -> bool {
        !matches!(self, TodoEvent::Authored(_))
    }
}

/// What a todo's row shows (SPEC §6.7): its candidate outcomes, with `None`
/// for open, and the sorted names of its candidate content records.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub outcome: Candidates,
    pub content: Vec<Hash>,
}

/// FPL prices a todo (§6.4, §7): a flat 1.0 while resolved, else its spec on
/// its checklist, one term per WORLD, one candidate from each register.
impl<P: Payload> Price for TodoEvent<P> {
    fn terms(registers: &Registers<'_, P>) -> Result<Vec<Term>, FplError> {
        registers.worlds(&Registers::default(), None)
    }
}

/// A todo's price is a function of time: a register unwritten yet reads as
/// first written, and a world that prices nothing there prices as the head.
impl<P: Payload> History for TodoEvent<P> {
    fn moment(
        now: &Registers<'_, P>,
        first: &Registers<'_, P>,
        head: Option<&Term>,
    ) -> Result<Vec<Term>, FplError> {
        now.worlds(first, head)
    }
}

/// A todo's row shows its candidate outcomes and records.
impl<P: Payload> Row for TodoEvent<P> {
    type Reading = Reading;

    fn reading(registers: &Registers<'_, P>) -> Reading {
        Reading {
            outcome: registers.outcomes(),
            content: registers
                .content
                .reading::<Content>()
                .iter()
                .map(|stamp| stamp.name.clone())
                .collect(),
        }
    }

    /// The candidates' kinds, `"open"` for none: the instants alone do not
    /// dispute a binding.
    fn disputes(confirmed: &Reading, claimed: &Reading) -> bool {
        kinds(&confirmed.outcome) != kinds(&claimed.outcome)
    }

    /// A candidate content record the policy does not confirm: it binds, and
    /// it is still its writer's.
    fn unconfirmed(registers: &Registers<'_, P>, policy: &impl Policy<Self>) -> bool {
        registers
            .content
            .reading::<Content>()
            .iter()
            .any(|stamp| !policy.confirms(&stamp.name, &stamp.event))
    }
}

/// A `Ref` to a todo reads its candidate bindings and its tendings.
impl<P: Payload> Bind for TodoEvent<P> {
    fn bind(todo: &TodoId, registers: &Registers<'_, P>, env: &mut Env) {
        registers.bind(todo, env);
    }
}

fn kinds(candidates: &Candidates) -> BTreeSet<&'static str> {
    candidates
        .iter()
        .map(|binding| binding.as_ref().map_or("open", Outcome::kind))
        .collect()
}

/// The discrete register of a todo's lifecycle.
pub struct State;

/// The discrete register of a todo's authored price.
pub struct Spec;

/// The discrete register of a todo's record.
pub struct Content;

/// A discrete register's value is the WHOLE EVENT that wrote it, not the
/// field it reads: two events that agree on a spec stay two candidates, as
/// they always were.
fn whole<P: Payload>(event: &TodoEvent<P>, kind: Kind) -> Option<Discrete<&TodoEvent<P>>> {
    Schema::writes(event)
        .any(|written| written == kind)
        .then_some(Discrete(event))
}

impl<P: Payload> RegisterType<TodoEvent<P>> for State {
    type Value<'e> = Discrete<&'e TodoEvent<P>>;

    fn value(event: &TodoEvent<P>) -> Option<Discrete<&TodoEvent<P>>> {
        whole(event, Kind::State)
    }
}

impl<P: Payload> RegisterType<TodoEvent<P>> for Spec {
    type Value<'e> = Discrete<&'e TodoEvent<P>>;

    fn value(event: &TodoEvent<P>) -> Option<Discrete<&TodoEvent<P>>> {
        whole(event, Kind::Spec)
    }
}

impl<P: Payload> RegisterType<TodoEvent<P>> for Content {
    type Value<'e> = Discrete<&'e TodoEvent<P>>;

    fn value(event: &TodoEvent<P>) -> Option<Discrete<&TodoEvent<P>>> {
        whole(event, Kind::Content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{mk_completed, mk_created, mk_tended};
    use crate::reference::{mk_authored, Todo};

    fn at() -> Datetime {
        Datetime::new(2026, 9, 6, 7, 3, 0, 0).expect("a real instant")
    }

    #[test]
    fn a_todo_event_writes_the_registers_its_kind_routes_it_to() {
        let created: TodoEvent<Todo> = mk_created("alpha", at(), "bassel", "", "").expect("valid");
        let done: TodoEvent<Todo> = mk_completed("alpha", at(), "bassel", "").expect("valid");
        let tended: TodoEvent<Todo> = mk_tended("alpha", at(), "bassel", "").expect("valid");
        let record: TodoEvent<Todo> = mk_authored(
            "alpha",
            at(),
            "triage",
            "todo",
            at(),
            "body",
            Some(crate::fpl::mk_flat(0.5).expect("valid")),
            vec![],
            "",
            "",
            "",
            None,
            vec![],
            vec![],
            "",
        )
        .expect("valid");
        assert_eq!(created.writes().count(), 0);
        assert_eq!(tended.writes().count(), 0, "a tending supersedes nothing");
        assert_eq!(done.writes().collect::<Vec<_>>(), [Kind::State]);
        assert_eq!(
            record.writes().collect::<Vec<_>>(),
            [Kind::Content, Kind::Spec]
        );
        assert!(done.asks() && !record.asks());
        assert_eq!(
            <TodoEvent<Todo> as Schema>::from_value(
                &TodoVocabulary::default(),
                &Schema::to_value(&record)
            ),
            Ok(record)
        );
    }
}
