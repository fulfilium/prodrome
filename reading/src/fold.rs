//! Folds (SPEC §6): each entity's stream folded into its registers
//! ([`read`], under any schema), and the readings of a todo (SPEC §6.1 to
//! §6.5), each a projection of its [`Registers`]. [`read`] is defined in
//! `prodrome-dag`, and the register algebra, such as [`Product`] and
//! [`Frontier`], in `prodrome-data`. This module re-exports both.

use std::collections::{BTreeMap, BTreeSet};

pub use prodrome_dag::fold::*;
pub use prodrome_todo::todo::{Kind, Registers, Write};

use prodrome_dag::policy::Policy;
use prodrome_dag::registers::{Folded, Prodrome};
use prodrome_data::policy::Everything;
use prodrome_data::stamp::Stamp;
use prodrome_fpl::fpl::{self, Env, FplError, Instant};
use prodrome_term::schedule::Schedule;
use prodrome_term::term::Term;
use prodrome_todo::event::{Authored, TodoEvent, TodoId};
use prodrome_todo::payload::Payload;

use crate::schema::{Bind, History, Price, Schema};

/// An entity's registers as a schedule over time, as of `at` (`None`: ever)
/// under `policy` (SPEC §6.5, `history`).
///
/// The value is the empty product before the first write. From each instant
/// at which a write is dated, the value joins every write dated at or before
/// that instant.
#[must_use]
pub fn readings<'a, E: Schema>(
    stream: &'a [Stamp<E>],
    at: Option<Instant>,
    policy: &impl Policy<E>,
) -> Schedule<E::Registers<'a>> {
    let mut knots = BTreeMap::new();
    prodrome_dag::fold::scan(stream, at, policy, |instant, registers| {
        knots.insert(instant, registers.clone());
    });
    Schedule::of(E::Registers::default(), knots)
}

/// The environment at `at` that FPL terms read (SPEC §6.1): the candidate
/// bindings and the tendings of each entity.
pub fn env<E: Bind>(prodrome: &Prodrome<E>, at: Instant, policy: &impl Policy<E>) -> Env {
    let mut env = Env::new();
    for (key, stream) in prodrome {
        E::bind(key, &read(stream, Some(at), policy), &mut env);
    }
    env
}

/// Each todo's candidate specs at `at` (SPEC §6.2).
///
/// A todo that is missing from the map has no price, which is not a price of
/// zero.
pub fn specs<P: Payload>(
    prodrome: &Prodrome<TodoEvent<P>>,
    at: Instant,
    policy: &impl Policy<TodoEvent<P>>,
) -> BTreeMap<TodoId, Vec<Term>> {
    prodrome
        .iter()
        .map(|(todo, stream)| {
            let specs = read(stream, Some(at), policy).specs();
            (todo.clone(), specs.into_iter().cloned().collect::<Vec<_>>())
        })
        .filter(|(_, specs)| !specs.is_empty())
        .collect()
}

/// Each todo's candidate records at `at`, whoever wrote them (SPEC §6.3).
///
/// [`crate::view::entries`] marks an unconfirmed record
/// ([`crate::view::Provisional::Content`]).
pub fn content<P: Payload>(
    prodrome: &Prodrome<TodoEvent<P>>,
    at: Instant,
) -> BTreeMap<TodoId, Vec<Authored<P>>> {
    prodrome
        .iter()
        .map(|(todo, stream)| {
            let records = read(stream, Some(at), &Everything).records();
            (
                todo.clone(),
                records.into_iter().cloned().collect::<Vec<_>>(),
            )
        })
        .filter(|(_, records)| !records.is_empty())
        .collect()
}

/// Each entity's history as of `at`, as one fulfillment function (SPEC
/// §6.4).
///
/// The function has a piece at every instant at which one of the entity's
/// registers was written. The term of each piece is `Least` over the terms
/// of [`History::moment`] at that instant. The head extends to −∞ and is the
/// price of each register's earliest writes. An entity whose earliest writes
/// have no price has no function.
///
/// # Errors
///
/// Returns an error where a smart constructor refuses a piece.
pub fn flatten<E: History>(
    prodrome: &Prodrome<E>,
    at: Instant,
    policy: &impl Policy<E>,
) -> Result<BTreeMap<E::Key, Term>, FplError> {
    let mut out = BTreeMap::new();
    for (key, stream) in prodrome {
        if let Some(function) = function(stream, at, policy)? {
            out.insert(key.clone(), function);
        }
    }
    Ok(out)
}

/// The price of a reading (design §4): `Least` over the terms of its
/// candidates ([`Price::terms`]), or `None` where no candidate has a term.
///
/// Law 34: a conflict prices as `Least` over its candidates.
///
/// # Errors
///
/// Returns an error where a smart constructor refuses the term of a
/// candidate.
pub fn price<E: Price>(registers: &E::Registers<'_>) -> Result<Option<Term>, FplError> {
    least(E::terms(registers)?)
}

/// `Least` over `terms`, the meet in the fulfillment order; none for none.
fn least(terms: Vec<Term>) -> Result<Option<Term>, FplError> {
    if terms.is_empty() {
        Ok(None)
    } else {
        fpl::least_of(terms).map(Some)
    }
}

/// Each register's first writes among `written`: those no other write to it
/// precedes.
fn earliest<'a, E: Schema>(written: &[&'a Stamp<E>]) -> E::Registers<'a> {
    let mut first = E::Registers::default();
    for stamp in written {
        for register in stamp.event.writes() {
            let preceded = written
                .iter()
                .any(|other| stamp.descends(other) && other.event.writes().any(|r| r == register));
            if !preceded {
                first.join_to(register, stamp);
            }
        }
    }
    first
}

fn function<E: History>(
    stream: &[Stamp<E>],
    at: Instant,
    policy: &impl Policy<E>,
) -> Result<Option<Term>, FplError> {
    let written: Vec<&Stamp<E>> = stream
        .iter()
        .filter(|stamp| admits(stamp, Some(at), policy))
        .filter(|stamp| stamp.event.writes().next().is_some())
        .collect();
    let first = earliest(&written);
    let Some(head) = least(E::moment(&E::Registers::default(), &first, None)?)? else {
        return Ok(None);
    };
    // A piece where a register moves: a tending alone moves none.
    let moments: BTreeSet<Instant> = written
        .iter()
        .map(|stamp| fpl::instant_of(stamp.event.at()))
        .collect();
    let mut pieces = BTreeMap::new();
    for (m, now) in readings(stream, Some(at), policy).into_parts().1 {
        if moments.contains(&m) {
            let term = least(E::moment(&now, &first, Some(&head))?)?;
            pieces.insert(m, term.unwrap_or_else(|| head.clone()));
        }
    }
    Ok(Some(fpl::piecewise(Schedule::of(head, pieces))))
}

/// The functions of [`flatten`] in the form that [`fpl::link`] reads, over
/// every entity in `known` (SPEC §7.2).
///
/// A known entity with no function maps to `Absent`.
pub fn link_specs<'a, K: AsRef<str> + 'a>(
    functions: &BTreeMap<K, Term>,
    known: impl IntoIterator<Item = &'a K>,
) -> BTreeMap<String, Term> {
    let mut specs: BTreeMap<String, Term> = known
        .into_iter()
        .map(|key| (key.as_ref().to_owned(), fpl::mk_absent()))
        .collect();
    specs.extend(
        functions
            .iter()
            .map(|(key, term)| (key.as_ref().to_owned(), term.clone())),
    );
    specs
}

/// One prodrome in the form that a term in another prodrome reads (SPEC
/// §7.2): the functions of [`link_specs`] and the environment of [`fn@env`],
/// at `at` under `policy`.
///
/// # Errors
///
/// Returns an error where [`flatten`] cannot build a function.
pub fn member<E: History + Bind>(
    prodrome: &Prodrome<E>,
    at: Instant,
    policy: &impl Policy<E>,
) -> Result<fpl::Member, FplError> {
    Ok(fpl::Member {
        specs: link_specs(&flatten(prodrome, at, policy)?, prodrome.keys()),
        env: env(prodrome, at, policy),
    })
}

/// The prodromes of a replica as a set (design §6.3.1, SPEC §7.2): one
/// [`member`] for each prodrome that a `Genesis` begins, keyed by its genesis.
///
/// The function reads each member from its own prodrome alone, so the
/// objects of one member never move the reading of another (law 23, SPEC
/// §3). The legacy prodrome, which no `Genesis` begins, is not a member.
///
/// # Errors
///
/// Returns an error where [`flatten`] cannot build a function.
pub fn stores<E: History + Bind>(
    folded: &Folded<E>,
    at: Instant,
    policy: &impl Policy<E>,
) -> Result<fpl::Stores, FplError> {
    folded
        .prodromes()
        .iter()
        .filter_map(|(genesis, prodrome)| Some((genesis.clone()?, prodrome)))
        .map(|(genesis, prodrome)| Ok((genesis, member(prodrome, at, policy)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prodrome_dag::dag::Dag;
    use prodrome_dag::registers::fold;
    use prodrome_data::literal::Datetime;
    use prodrome_data::policy::Untrusted;
    use prodrome_fpl::fpl::{fulfillment, mk_flat, print_term};
    use prodrome_todo::event::{mk_completed, mk_reopened, mk_sealed, seal_hash, Actor};
    use prodrome_todo::reference::{mk_authored, mk_subtodo, Todo};

    type Event = TodoEvent<Todo>;

    /// A log as its writer's chain, folded to its one prodrome.
    fn prodrome(log: &[Event]) -> Prodrome<Event> {
        let mut prev = None;
        let mut objects = Vec::new();
        for event in log {
            let envelope = mk_sealed(prev, event.clone());
            let name = seal_hash(&envelope);
            prev = Some(name.clone());
            objects.push((name, envelope));
        }
        let dag: Dag<Event> = objects.into_iter().collect();
        fold(&dag.nodes().expect("a chain")).prodromes()[&None].clone()
    }

    fn env_at(log: &[Event], t: Datetime, policy: &impl Policy<Event>) -> Env {
        env(&prodrome(log), fpl::instant_of(t), policy)
    }

    fn flat(log: &[Event], t: Datetime) -> BTreeMap<TodoId, Term> {
        flatten(&prodrome(log), fpl::instant_of(t), &roster()).expect("folds")
    }

    fn at(day: u32) -> Datetime {
        Datetime::new(2026, 9, day, 12, 0, 0, 0).expect("a real instant")
    }

    fn authored(todo: &str, day: u32, actor: &str, spec: Option<Term>, items: usize) -> Event {
        let subtodos = (0..items)
            .map(|i| mk_subtodo(format!("item {i}"), false).expect("valid"))
            .collect();
        mk_authored(
            todo,
            at(day),
            actor,
            "todo",
            at(day),
            "body",
            spec,
            vec![],
            "",
            "",
            "",
            None,
            subtodos,
            vec![],
            "",
        )
        .expect("valid")
    }

    fn roster() -> Untrusted {
        Untrusted::of([Actor::new("triage").expect("valid")])
    }

    #[test]
    fn a_completion_binds_and_a_reopening_clears() {
        let log: Vec<Event> = vec![
            mk_completed("alpha", at(2), "ada", "").expect("valid"),
            mk_reopened("alpha", at(4), "ada", "").expect("valid"),
        ];
        let policy = roster();
        assert_eq!(env_at(&log, at(3), &policy).outcomes.len(), 1);
        assert!(env_at(&log, at(5), &policy).outcomes.is_empty());
    }

    #[test]
    fn a_claiming_completion_is_a_claim_and_not_a_binding() {
        let log: Vec<Event> = vec![mk_completed("alpha", at(2), "triage", "").expect("valid")];
        assert!(env_at(&log, at(3), &roster()).outcomes.is_empty());
        assert_eq!(env_at(&log, at(3), &Untrusted::none()).outcomes.len(), 1);
    }

    #[test]
    fn a_resolved_todo_reads_one_from_the_instant_it_was_resolved() {
        let log = vec![
            authored("alpha", 1, "ada", Some(mk_flat(0.2).expect("valid")), 0),
            mk_completed("alpha", at(3), "ada", "").expect("valid"),
        ];
        let flat = flat(&log, at(9));
        let term =
            &fpl::Closed::of(flat[&TodoId::new("alpha").expect("valid")].clone()).expect("no Ref");
        let env = env_at(&log, at(9), &roster());
        assert_eq!(fulfillment(term, fpl::instant_of(at(2)), &env), Some(0.2));
        assert_eq!(fulfillment(term, fpl::instant_of(at(4)), &env), Some(1.0));
    }

    #[test]
    fn a_checklist_is_the_parents_offset_on_its_items() {
        let log = vec![authored(
            "alpha",
            1,
            "ada",
            Some(mk_flat(0.5).expect("valid")),
            2,
        )];
        let flat = flat(&log, at(9));
        let term = &flat[&TodoId::new("alpha").expect("valid")];
        assert_eq!(
            print_term(term),
            "OffsetBy(delta=Flat(value=0.5), term=Conj(terms=(Flat(value=0.5), Flat(value=0.5)), p=-4.0))"
        );
    }

    #[test]
    fn a_todo_with_no_spec_and_no_checklist_has_no_function() {
        let log = vec![
            authored("alpha", 1, "ada", None, 0),
            mk_completed("alpha", at(3), "ada", "").expect("valid"),
        ];
        assert!(flat(&log, at(9)).is_empty());
    }
}
