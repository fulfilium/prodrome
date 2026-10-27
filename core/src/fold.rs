//! §6 — belief at a moment: each entity's stream folded into its registers
//! ([`read`], under any schema), and the todo's readings (§6.1–6.5) each a
//! projection of its [`Registers`].
//!
//! See ../../SPEC.md §6. ORDER IS CAUSAL, TIME IS DATA (§1): a write
//! joins its register when it is dated by the moment asked and the policy
//! admits it, and what it supersedes is ancestry alone. No clock and no
//! linearisation picks a winner; a conflict is its candidates.

mod frontier;
mod order;
mod product;
mod register;
pub mod sole;
mod write;

use std::collections::{BTreeMap, BTreeSet};

pub use frontier::Frontier;
pub use order::{grows, maximal, Discrete, Inflationary, Order, Total};
pub use product::Product;
pub use register::{GrowSet, Register, RegisterType};
pub use write::{Kind, Write};

use crate::event::{Authored, TodoEvent, TodoId};
use crate::fpl::{self, Candidates, Env, FplError, Instant};
use crate::literal::ProdromeError;
use crate::payload::Payload;
use crate::policy::{Everything, Policy};
use crate::registers::{Folded, Prodrome, Stamp};
use crate::schedule::Schedule;
use crate::schema::{Bind, History, Price, Schema};
use crate::term::Term;
use crate::todo::{Content, Spec, State};

/// An entity's registers at `at` (`None`: ever) under `policy`: [`readings`]
/// at its end, the scan keeping nothing but its last value.
#[must_use]
pub fn read<'a, E: Schema>(
    stream: &'a [Stamp<E>],
    at: Option<Instant>,
    policy: &impl Policy<E>,
) -> E::Registers<'a> {
    scan(stream, at, policy, |_, _| {})
}

/// An entity's registers as a function of time, as of `at` (`None`: ever)
/// under `policy`: the empty product before anything, and from each instant
/// a write is dated, every write dated at or before it joined (§6.5's
/// `history`). The one fold over time: `read` is its value at `at`, and
/// every reading of an entity's past (§6.4's function, §6.5's environment)
/// is a function of it.
#[must_use]
pub fn readings<'a, E: Schema>(
    stream: &'a [Stamp<E>],
    at: Option<Instant>,
    policy: &impl Policy<E>,
) -> Schedule<E::Registers<'a>> {
    let mut knots = BTreeMap::new();
    scan(stream, at, policy, |instant, registers| {
        knots.insert(instant, registers.clone());
    });
    Schedule::of(E::Registers::default(), knots)
}

/// The fold: the admitted writes joined instant by instant, `each` shown
/// the registers as they stand at every instant one is dated. The product is
/// a semilattice, so the order writes join in within an instant, or across
/// the stream, changes nothing.
fn scan<'a, E: Schema>(
    stream: &'a [Stamp<E>],
    at: Option<Instant>,
    policy: &impl Policy<E>,
    mut each: impl FnMut(Instant, &E::Registers<'a>),
) -> E::Registers<'a> {
    let mut dated: BTreeMap<Instant, Vec<&'a Stamp<E>>> = BTreeMap::new();
    for stamp in stream.iter().filter(|stamp| admits(stamp, at, policy)) {
        dated
            .entry(fpl::instant_of(stamp.event.at()))
            .or_default()
            .push(stamp);
    }
    let mut registers = E::Registers::default();
    for (instant, stamps) in dated {
        for stamp in stamps {
            registers.join(stamp);
        }
        each(instant, &registers);
    }
    registers
}

/// Does `stamp` join its registers at `at` (`None`: ever) under `policy`?
/// Where it is dated by then and the policy binds it: the reference policy
/// binds an event the schema does not ask about whoever wrote it (a todo's
/// record, §6.2, §6.3), and a policy that requires signatures binds only
/// what the history proves (§5).
fn admits<E: Schema>(stamp: &Stamp<E>, at: Option<Instant>, policy: &impl Policy<E>) -> bool {
    at.is_none_or(|at| fpl::instant_of(stamp.event.at()) <= at)
        && policy.standing(&stamp.name, &stamp.event).binds()
}

/// One todo's registers at a moment: the todo schema's product. The state,
/// spec and content are discrete; the tendings, a grow-only set, are ordered
/// by inclusion.
#[derive(Debug, Clone, PartialEq)]
pub struct Registers<'a, P: Payload> {
    pub state: Frontier<'a, TodoEvent<P>>,
    pub spec: Frontier<'a, TodoEvent<P>>,
    pub content: Frontier<'a, TodoEvent<P>>,
    pub tended: GrowSet<Instant>,
}

impl<P: Payload> Default for Registers<'_, P> {
    fn default() -> Self {
        Registers {
            state: Frontier::default(),
            spec: Frontier::default(),
            content: Frontier::default(),
            tended: GrowSet::new(),
        }
    }
}

impl<'a, P: Payload> Product<'a> for Registers<'a, P> {
    type Schema = TodoEvent<P>;

    fn registers(&self) -> Vec<Kind> {
        Kind::ALL.to_vec()
    }

    fn join(&mut self, stamp: &'a Stamp<TodoEvent<P>>) {
        for write in Write::of(&stamp.event) {
            self.route(stamp, write);
        }
    }

    fn frontier(&self, kind: Kind) -> &Frontier<'a, TodoEvent<P>> {
        match kind {
            Kind::State => &self.state,
            Kind::Spec => &self.spec,
            Kind::Content => &self.content,
        }
    }

    fn frontier_mut(&mut self, kind: Kind) -> &mut Frontier<'a, TodoEvent<P>> {
        match kind {
            Kind::State => &mut self.state,
            Kind::Spec => &mut self.spec,
            Kind::Content => &mut self.content,
        }
    }

    fn reading(&self, kind: Kind) -> Vec<&'a Stamp<TodoEvent<P>>> {
        match kind {
            Kind::State => self.state.reading::<State>(),
            Kind::Spec => self.spec.reading::<Spec>(),
            Kind::Content => self.content.reading::<Content>(),
        }
    }

    /// No todo register is inflationary: a `Reopened` after a `Completed`
    /// goes back, which is why a todo's conflicts are shown and priced.
    fn grows(&self, _event: &TodoEvent<P>) -> Result<(), ProdromeError> {
        Ok(())
    }
}

impl<'a, P: Payload> Registers<'a, P> {
    fn route(&mut self, stamp: &'a Stamp<TodoEvent<P>>, write: Write<'a, P>) {
        match write {
            Write::State(_) => self.state.join(stamp),
            Write::Spec(_) => self.spec.join(stamp),
            Write::Content(_) => self.content.join(stamp),
            Write::Tend(at) => self.tended.join(at),
        }
    }

    /// The candidate bindings, `{None}` for an open todo.
    pub fn outcomes(&self) -> Candidates {
        let mut out: Candidates = self
            .state
            .reading::<State>()
            .into_iter()
            .flat_map(|stamp| Write::of(&stamp.event))
            .filter_map(|write| match write {
                Write::State(binding) => Some(binding),
                _ => None,
            })
            .collect();
        if out.is_empty() {
            out.insert(None);
        }
        out
    }

    pub fn specs(&self) -> Vec<&'a Term> {
        self.spec
            .reading::<Spec>()
            .into_iter()
            .flat_map(|stamp| Write::of(&stamp.event))
            .filter_map(|write| match write {
                Write::Spec(spec) => Some(spec),
                _ => None,
            })
            .collect()
    }

    pub fn records(&self) -> Vec<&'a Authored<P>> {
        self.content
            .reading::<Content>()
            .into_iter()
            .flat_map(|stamp| Write::of(&stamp.event))
            .filter_map(|write| match write {
                Write::Content(record) => Some(record),
                _ => None,
            })
            .collect()
    }

    /// Record this todo in `env`: its bindings unless simply open, and its
    /// tendings.
    pub fn bind(&self, todo: &TodoId, env: &mut Env) {
        let outcomes = self.outcomes();
        if outcomes != Candidates::from([None]) {
            env.outcomes.insert(todo.as_str().to_owned(), outcomes);
        }
        if !self.tended.is_empty() {
            env.tended
                .insert(todo.as_str().to_owned(), self.tended.clone());
        }
    }

    /// The price of each world in force, one candidate from each register,
    /// falling back to `first` where a register is unwritten yet: a flat 1.0
    /// while resolved, else `checklist(spec, items)`, else `head`.
    pub(crate) fn worlds(&self, first: &Self, head: Option<&Term>) -> Result<Vec<Term>, FplError> {
        let mut specs: Vec<Option<&Term>> = or_first(self.specs(), first.specs())
            .into_iter()
            .map(Some)
            .collect();
        if specs.is_empty() {
            specs.push(None);
        }
        let mut items: Vec<usize> = or_first(self.records(), first.records())
            .into_iter()
            .map(|record| record.payload.checklist_len())
            .collect();
        if items.is_empty() {
            items.push(0);
        }
        let closed: BTreeSet<bool> = self.outcomes().iter().map(Option::is_some).collect();
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
}

/// `mine`, or `theirs` where `mine` is empty.
fn or_first<T>(mine: Vec<T>, theirs: Vec<T>) -> Vec<T> {
    if mine.is_empty() {
        theirs
    } else {
        mine
    }
}

/// §6.1 — the environment at `at`: what FPL's terms read of each entity (a
/// todo's candidate bindings and its tendings).
pub fn env<E: Bind>(prodrome: &Prodrome<E>, at: Instant, policy: &impl Policy<E>) -> Env {
    let mut env = Env::new();
    for (key, stream) in prodrome {
        E::bind(key, &read(stream, Some(at), policy), &mut env);
    }
    env
}

/// §6.2 — each todo's candidate specs at `at`. Absence is not zero: a todo
/// missing here has no price, not a price of nothing.
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

/// §6.3 — each todo's candidate records at `at`, whoever wrote them: the
/// reader marks an unconfirmed one ([`crate::view::Provisional::Content`]).
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

/// §6.4 — each entity's history as of `at`, as ONE fulfillment function: a
/// piece at every instant one of its registers was written, the term in each
/// `Least` over that [`History::moment`]'s terms. The head, extending to −∞,
/// is the price of each register's earliest writes; an entity they price
/// nothing has no function.
///
/// # Errors
///
/// A piece a smart constructor refuses.
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

/// A reading's price (design §4): `Least` over the terms its candidates
/// price as ([`Price::terms`]), so a conflict prices as its most urgent
/// candidate; none where no candidate has one.
///
/// # Errors
///
/// A candidate's term a smart constructor refuses.
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
                first.frontier_mut(register).join(stamp);
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

/// [`flatten`]'s functions as [`fpl::link`] reads them, over every entity in
/// `known`: `Absent` where a known entity has none (§7.2).
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

/// One prodrome as a term elsewhere reads it (SPEC §7.2): each entity's
/// [`flatten`] function, `Absent` for an entity with none, and the
/// environment ([`env`]) its terms read, at `at` under `policy`.
///
/// # Errors
///
/// A function [`flatten`] cannot build.
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

/// A REPLICA'S PRODROMES AS A SET (design §6.3.1, SPEC §7.2): each
/// prodrome a `Genesis` begins, a [`member`] by its genesis. The product of
/// its members' readings, each read from its own prodrome alone, so what
/// one member holds never moves another's reading (the disjointness law,
/// §3). The legacy prodrome, which no `Genesis` begins, is no member.
///
/// # Errors
///
/// A function [`flatten`] cannot build.
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
    use crate::dag::Dag;
    use crate::event::{mk_completed, mk_reopened, mk_sealed, seal_hash, Actor};
    use crate::fpl::{fulfillment, mk_flat, print_term};
    use crate::literal::Datetime;
    use crate::policy::Untrusted;
    use crate::reference::{mk_authored, mk_subtodo, Todo};
    use crate::registers::fold;

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
            .map(|i| mk_subtodo(&format!("item {i}"), false).expect("valid"))
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
            mk_completed("alpha", at(2), "bassel", "").expect("valid"),
            mk_reopened("alpha", at(4), "bassel", "").expect("valid"),
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
            authored("alpha", 1, "bassel", Some(mk_flat(0.2).expect("valid")), 0),
            mk_completed("alpha", at(3), "bassel", "").expect("valid"),
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
            "bassel",
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
            authored("alpha", 1, "bassel", None, 0),
            mk_completed("alpha", at(3), "bassel", "").expect("valid"),
        ];
        assert!(flat(&log, at(9)).is_empty());
    }
}
