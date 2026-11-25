//! The todo schema (SPEC §4, and SPEC §6, "The todo schema's registers"):
//! what a reading asks of it, its price, its binding for FPL and its row.
//! Its vocabulary, its registers and what each event writes are defined in
//! the module that this module re-exports.

use std::collections::BTreeSet;

pub use crate::todo_schema::*;

use crate::event::{Hash, TodoEvent, TodoId};
use crate::fpl::{self, Candidates, Env, FplError, Outcome};
use crate::payload::Payload;
use crate::policy::Policy;
use crate::schema::{Bind, History, Price, Row};
use crate::term::Term;

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
        worlds(registers, &Registers::default(), None)
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
        worlds(now, first, head)
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

/// The price of each world in force at `now`, one candidate from each register,
/// falling back to `first` where a register is unwritten yet: a flat 1.0
/// while resolved, else `checklist(spec, items)`, else `head`.
fn worlds<P: Payload>(
    now: &Registers<'_, P>,
    first: &Registers<'_, P>,
    head: Option<&Term>,
) -> Result<Vec<Term>, FplError> {
    let mut specs: Vec<Option<&Term>> = or_first(now.specs(), first.specs())
        .into_iter()
        .map(Some)
        .collect();
    if specs.is_empty() {
        specs.push(None);
    }
    let mut items: Vec<usize> = or_first(now.records(), first.records())
        .into_iter()
        .map(|record| record.payload.checklist_len())
        .collect();
    if items.is_empty() {
        items.push(0);
    }
    let closed: BTreeSet<bool> = now.outcomes().iter().map(Option::is_some).collect();
    let mut terms = Vec::new();
    for closed in closed {
        for spec in &specs {
            for items in &items {
                let term = if closed {
                    Some(fpl::mk_flat(1.0)?)
                } else {
                    fpl::checklist(spec.cloned(), *items)?.or_else(|| head.cloned())
                };
                terms.extend(term);
            }
        }
    }
    Ok(terms)
}

/// `mine`, or `theirs` where `mine` is empty.
fn or_first<T>(mine: Vec<T>, theirs: Vec<T>) -> Vec<T> {
    if mine.is_empty() {
        theirs
    } else {
        mine
    }
}
