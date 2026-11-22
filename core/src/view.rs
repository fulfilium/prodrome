//! The entry (SPEC §6.7): one entity as the folds see it at a moment, under a
//! schema with a [`Row`], a [`History`] and a [`Bind`].
//!
//! The confirmed reading, under the host's [`Policy`], gives the price and the
//! answer. The claimed reading, under [`Everything`], only shows what a writer
//! claimed and the policy did not bind ([`Entry::claim`]).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::event::{Hash, TodoEvent};
use crate::fold::{self, Product};
use crate::fpl::{self, Candidates, LinkError, Outcome};
use crate::literal::{Datetime, ProdromeError};
use crate::payload::Payload;
use crate::policy::{Everything, Policy};
use crate::registers::{self, Genesis, Node};
use crate::schema::{Bind, History, Row};
use crate::term::Term;

/// An entity's fulfillment function (SPEC §6.4) and its value at a moment.
///
/// An entity with no function has the price `Absent` (SPEC §7), which reads
/// `∅`.
#[derive(Debug, Clone, PartialEq)]
pub struct Price {
    /// The todo's fulfillment function over all of time, which is not the
    /// authored spec of the moment ([`fold::specs`]).
    ///
    /// The term is open: its `Ref`s name other todos. It is `Absent` where
    /// the todo has no spec and no checklist.
    pub spec: Term,
    /// The value of `spec`, linked against every todo's function, at the
    /// moment of the entry: a number in [0, 1], `None` for `∅`, or the reason
    /// that `spec` does not link.
    pub value: Result<Option<f64>, LinkError>,
    /// `spec`, linked. A consumer explains, samples or compiles this term, so
    /// that no reader links a second time with a different set of specs.
    pub linked: Result<fpl::Closed, LinkError>,
}

/// The reason that a reader sees something that the confirmed reading does
/// not have.
///
/// The case with no reason is [`Confidence::Confirmed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provisional {
    /// An event that claims gives a reading that the confirmed one refuses
    /// ([`Row::disputes`]). [`Entry::claim`] holds that reading.
    Claimed,
    /// The policy binds a winning write but does not confirm it
    /// ([`Policy::confirms`], [`Row::unconfirmed`]). Under the todo schema,
    /// the write is a content record, which a reader shows as source and
    /// never renders.
    Content,
    /// Both at once.
    ClaimedAndContent,
}

/// Whether the answer for this entity is the whole confirmed reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// The two readings agree and the policy confirms the winning content
    /// record.
    Confirmed,
    Provisional(Provisional),
}

impl Confidence {
    /// The confidence for a refused claim (`claimed`) and an unconfirmed
    /// content record (`content`).
    pub fn of(claimed: bool, content: bool) -> Confidence {
        match (claimed, content) {
            (false, false) => Confidence::Confirmed,
            (true, false) => Confidence::Provisional(Provisional::Claimed),
            (false, true) => Confidence::Provisional(Provisional::Content),
            (true, true) => Confidence::Provisional(Provisional::ClaimedAndContent),
        }
    }

    /// The flag that the wire carries as `view.Entry.unconfirmed`.
    pub fn is_provisional(self) -> bool {
        matches!(self, Confidence::Provisional(_))
    }
}

/// One entity, as the folds see it at a moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<E: Row> {
    pub genesis: Genesis,
    pub key: E::Key,
    /// The confirmed reading. For a todo, it is the candidate bindings, with
    /// `None` for open.
    pub reading: E::Reading,
    /// The claimed reading, where it disputes the confirmed one. A reader
    /// shows it and never applies it.
    pub claim: Option<E::Reading>,
    pub confidence: Confidence,
    /// The entity's function and its value at the moment of the entry.
    pub price: Price,
    /// The registers of this entity with more than one write in the frontier,
    /// with the names of those writes.
    pub conflicts: BTreeMap<E::Register, Vec<Hash>>,
    /// Every object whose event names this entity, in causal order.
    pub stream: Vec<Hash>,
}

impl<E: Row> Entry<E> {
    pub fn spec(&self) -> &Term {
        &self.price.spec
    }

    /// A number, `∅` (`Ok(None)`), or why the function does not link.
    pub fn value(&self) -> Result<Option<f64>, &LinkError> {
        self.price.value.as_ref().copied()
    }

    /// The function, linked against every todo's, when it links: what to
    /// explain, sample or compile.
    pub fn linked(&self) -> Option<&fpl::Closed> {
        self.price.linked.as_ref().ok()
    }

    /// Why the function has no value, where it does not link.
    pub fn unlinked(&self) -> Option<&LinkError> {
        self.price.value.as_ref().err()
    }
}

impl<P: Payload> Entry<TodoEvent<P>> {
    /// The confirmed candidate bindings.
    #[must_use]
    pub fn outcome(&self) -> &Candidates {
        &self.reading.outcome
    }

    /// The names of the candidate content records, sorted.
    ///
    /// The entry holds names and not records, because the host decides what a
    /// record means.
    #[must_use]
    pub fn content(&self) -> &[Hash] {
        &self.reading.content
    }

    pub fn is_open(&self) -> bool {
        self.reading.outcome == Candidates::from([None])
    }

    /// The lifecycle in the wire format: each candidate's constructor name in
    /// lower case, `"open"` for none, joined by `|` under a conflict. The
    /// string is a rendering, and [`Entry::outcome`] is the value.
    pub fn state(&self) -> String {
        lowered(&self.reading.outcome)
    }

    /// The instants of the bound candidates in the format of CPython's
    /// `isoformat(" ")`, joined by `|`, or `""` for an open todo.
    pub fn at(&self) -> String {
        let instants: Vec<String> = self
            .reading
            .outcome
            .iter()
            .flatten()
            .map(|binding| fpl::iso(binding.at()).replace('T', " "))
            .collect();
        instants.join("|")
    }

    /// The claimed state in the format of [`Entry::state`]. It is `""` where
    /// the two readings agree, and also where the claim is only that the todo
    /// is open, because the wire format requires it.
    pub fn claimed(&self) -> String {
        match &self.claim {
            Some(claim) if claim.outcome != Candidates::from([None]) => lowered(&claim.outcome),
            _ => String::new(),
        }
    }
}

/// The list order of SPEC §6.7: ascending in value, ties by (genesis, id),
/// then every row with no number, by (genesis, id).
pub fn list_order<E: Row>(a: &Entry<E>, b: &Entry<E>) -> Ordering {
    let number = |entry: &Entry<E>| entry.value().ok().flatten();
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then_with(|| (&a.genesis, &a.key).cmp(&(&b.genesis, &b.key)))
}

/// `state`'s and `claimed`'s shared spelling.
fn lowered(candidates: &Candidates) -> String {
    let names: Vec<&str> = candidates
        .iter()
        .map(|binding| match binding {
            None => "open",
            Some(Outcome::Completed(_)) => "completed",
            Some(Outcome::Cancelled(_)) => "cancelled",
        })
        .collect();
    names.join("|")
}

/// Every entity that the DAG mentions, as the folds see it at `t`, per
/// genesis and by key (SPEC §6.7).
///
/// An entity whose events are all dated after `t` is still a row. The value
/// links against the functions of the entity's prodrome under the confirmed
/// environment. A qualified reference links through the set of the DAG's
/// prodromes that a `Genesis` begins (design §6.3.1).
///
/// # Errors
///
/// Returns an error where [`fold::flatten`] cannot build a function.
pub fn entries<E: Row + History + Bind>(
    nodes: &[Node<E>],
    t: Datetime,
    policy: &impl Policy<E>,
) -> Result<Vec<Entry<E>>, ProdromeError> {
    let state = registers::fold(nodes);
    let now = fpl::instant_of(t);
    let set = fold::stores(&state, now, policy)?;
    let mut out = Vec::new();
    for (genesis, prodrome) in state.prodromes() {
        let fpl::Member { specs, env } = match genesis {
            Some(genesis) => set.member(genesis).cloned().unwrap_or_default(),
            None => fold::member(prodrome, now, policy)?,
        };
        for (key, stream) in prodrome {
            let registers = fold::read(stream, Some(now), policy);
            let reading = E::reading(&registers);
            // The readings differ only where the policy refuses a write.
            let claim = (!stream
                .iter()
                .all(|s| policy.standing(&s.name, &s.event).binds()))
            .then(|| E::reading(&fold::read(stream, Some(now), &Everything)))
            .filter(|claimed| E::disputes(&reading, claimed));
            let spec = specs
                .get(key.as_ref())
                .cloned()
                .unwrap_or_else(fpl::mk_absent);
            let linked = fpl::link_in(&spec, &specs, &set);
            out.push(Entry {
                genesis: genesis.clone(),
                key: key.clone(),
                confidence: Confidence::of(claim.is_some(), E::unconfirmed(&registers, policy)),
                claim,
                reading,
                price: Price {
                    value: linked
                        .as_ref()
                        .map(|closed| fpl::fulfillment(closed, now, &env))
                        .map_err(Clone::clone),
                    linked,
                    spec,
                },
                conflicts: registers.conflicts(),
                stream: stream.iter().map(|stamp| stamp.name.clone()).collect(),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::Dag;
    use crate::event::{mk_completed, mk_sealed, seal_hash, Actor};
    use crate::fpl::mk_flat;
    use crate::policy::Untrusted;
    use crate::reference::{mk_authored, Todo};

    type Event = TodoEvent<Todo>;

    fn at(day: u32) -> Datetime {
        Datetime::new(2026, 9, day, 12, 0, 0, 0).expect("a real instant")
    }

    fn chain(events: Vec<Event>) -> Vec<Node<Event>> {
        let mut prev = None;
        let mut objects = Vec::new();
        for event in events {
            let envelope = mk_sealed(prev, event);
            let name = seal_hash(&envelope);
            prev = Some(name.clone());
            objects.push((name, envelope));
        }
        objects
            .into_iter()
            .collect::<Dag<Event>>()
            .nodes()
            .expect("a chain")
    }

    fn authored(todo: &str, day: u32, actor: &str, spec: Option<Term>) -> Event {
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
            vec![],
            vec![],
            "",
        )
        .expect("valid")
    }

    fn roster() -> Untrusted {
        Untrusted::of([Actor::new("triage").expect("valid")])
    }

    #[test]
    fn a_claiming_completion_is_a_claim_and_the_entry_is_provisional() {
        let nodes = chain(vec![
            authored("alpha", 1, "bassel", Some(mk_flat(0.25).expect("valid"))),
            mk_completed("alpha", at(3), "triage", "").expect("valid"),
        ]);
        let entries = entries(&nodes, at(9), &roster()).expect("folds");
        let [entry] = &entries[..] else {
            panic!("one todo")
        };
        assert_eq!(
            *entry.outcome(),
            Candidates::from([None]),
            "the confirmed reading refuses the claim"
        );
        assert_eq!(entry.claimed(), "completed");
        assert_eq!(
            entry.confidence,
            Confidence::Provisional(Provisional::Claimed),
            "the claim alone, since bassel wrote the content"
        );
        assert_eq!(
            entry.value(),
            Ok(Some(0.25)),
            "an open todo prices by its spec"
        );
        assert_eq!(entry.stream.len(), 2);
    }

    #[test]
    fn an_unconfirmed_record_makes_the_entry_provisional_without_a_claim() {
        let nodes = chain(vec![authored("alpha", 1, "triage", None)]);
        let entries = entries(&nodes, at(9), &roster()).expect("folds");
        let [entry] = &entries[..] else {
            panic!("one todo")
        };
        assert_eq!(entry.claim, None);
        assert_eq!(
            entry.confidence,
            Confidence::Provisional(Provisional::Content)
        );
        assert_eq!(
            entry.content(),
            [nodes[0].name().clone()].as_slice(),
            "the name of the winning record, not the record"
        );
        assert_eq!(
            fpl::print_term(entry.spec()),
            "Absent()",
            "no spec and no checklist is no function: Absent"
        );
        assert_eq!(entry.value(), Ok(None), "and Absent reads ∅, not a number");
    }

    #[test]
    fn a_todo_whose_only_event_is_dated_after_the_moment_is_still_a_row() {
        let nodes = chain(vec![authored(
            "alpha",
            9,
            "bassel",
            Some(mk_flat(0.5).expect("valid")),
        )]);
        let entries = entries(&nodes, at(1), &roster()).expect("folds");
        let [entry] = &entries[..] else {
            panic!("one todo")
        };
        assert_eq!(entry.confidence, Confidence::Confirmed);
        assert!(entry.is_open());
        assert_eq!(entry.value(), Ok(None), "the chain knows no price yet");
        assert!(entry.content().is_empty(), "and no record yet");
        assert_eq!(entry.stream.len(), 1, "the object is still its stream");
    }

    #[test]
    fn a_resolved_todo_reads_one_and_carries_its_instant() {
        let nodes = chain(vec![
            authored("alpha", 1, "bassel", Some(mk_flat(0.2).expect("valid"))),
            mk_completed("alpha", at(3), "bassel", "").expect("valid"),
        ]);
        let entries = entries(&nodes, at(9), &roster()).expect("folds");
        let [entry] = &entries[..] else {
            panic!("one todo")
        };
        assert_eq!(
            *entry.outcome(),
            Candidates::from([Some(Outcome::Completed(fpl::instant_of(at(3))))])
        );
        assert_eq!(entry.claim, None, "the two folds agree");
        assert_eq!(entry.confidence, Confidence::Confirmed);
        assert_eq!(entry.value(), Ok(Some(1.0)));
    }

    #[test]
    fn a_reference_prices_by_the_todo_it_names_and_its_lifecycle() {
        let group = fpl::mk_conj(
            vec![
                fpl::mk_ref("alpha".to_owned()).expect("valid"),
                fpl::mk_ref("beta".to_owned()).expect("valid"),
            ],
            -1.0,
        )
        .expect("valid");
        let nodes = chain(vec![
            authored("alpha", 1, "bassel", Some(mk_flat(0.25).expect("valid"))),
            authored("beta", 1, "bassel", Some(mk_flat(0.5).expect("valid"))),
            authored("group", 1, "bassel", Some(group)),
            mk_completed("beta", at(3), "bassel", "").expect("valid"),
        ]);
        let rows = entries(&nodes, at(2), &roster()).expect("folds");
        // The harmonic mean of 0.25 and 0.5 while both are open ...
        assert_eq!(rows[2].key.as_str(), "group");
        let value = |row: &Entry<Event>| row.value().expect("linked").expect("a value");
        assert!((value(&rows[2]) - 1.0 / 3.0).abs() < 1e-12);
        // ... and of 0.25 and 1.0 once beta is done.
        let rows = entries(&nodes, at(9), &roster()).expect("folds");
        assert!((value(&rows[2]) - 0.4).abs() < 1e-12);
        assert_eq!(rows[2].unlinked(), None);
    }

    #[test]
    fn a_reference_that_does_not_link_has_a_function_and_no_value() {
        let nodes = chain(vec![authored(
            "alpha",
            1,
            "bassel",
            Some(fpl::mk_ref("ghost".to_owned()).expect("valid")),
        )]);
        let rows = entries(&nodes, at(9), &roster()).expect("folds");
        assert_eq!(fpl::print_term(rows[0].spec()), "Ref(todo='ghost')");
        assert_eq!(
            rows[0].value(),
            Err(&LinkError::Unknown("ghost".to_owned()))
        );
        assert_eq!(
            rows[0].unlinked(),
            Some(&LinkError::Unknown("ghost".to_owned()))
        );
    }

    #[test]
    fn a_reference_to_a_known_todo_with_no_spec_links_to_absent() {
        let group = fpl::mk_conj(
            vec![
                fpl::mk_ref("alpha".to_owned()).expect("valid"),
                fpl::mk_ref("beta".to_owned()).expect("valid"),
                fpl::mk_ref("gamma".to_owned()).expect("valid"),
            ],
            -1.0,
        )
        .expect("valid");
        let nodes = chain(vec![
            authored("alpha", 1, "bassel", Some(mk_flat(0.25).expect("valid"))),
            // A record with no spec, and a todo only ever created.
            authored("beta", 1, "bassel", None),
            crate::event::mk_created("gamma", at(1), "bassel", "a note", "").expect("valid"),
            authored("group", 1, "bassel", Some(group)),
        ]);
        let rows = entries(&nodes, at(9), &roster()).expect("folds");
        let [alpha, beta, gamma, group] = &rows[..] else {
            panic!("four todos")
        };
        assert_eq!(alpha.value(), Ok(Some(0.25)));
        assert_eq!((beta.value(), gamma.value()), (Ok(None), Ok(None)));
        // The two absent members neither raise nor lower the mean.
        assert_eq!(group.value(), Ok(Some(0.25)));
        assert_eq!(
            group.linked().map(|closed| fpl::print_term(closed.term())),
            Some("Conj(terms=(Flat(value=0.25), Absent(), Absent()), p=-1.0)".to_owned())
        );
    }

    #[test]
    fn a_list_puts_every_row_with_no_number_after_the_priced_ones_by_id() {
        let nodes = chain(vec![
            authored("delta", 1, "bassel", Some(mk_flat(0.75).expect("valid"))),
            authored("gamma", 1, "bassel", None),
            authored("beta", 1, "bassel", Some(mk_flat(0.25).expect("valid"))),
            authored(
                "alpha",
                1,
                "bassel",
                Some(fpl::mk_ref("ghost".to_owned()).expect("valid")),
            ),
            authored("epsilon", 1, "bassel", Some(mk_flat(0.25).expect("valid"))),
        ]);
        let mut rows = entries(&nodes, at(9), &roster()).expect("folds");
        rows.sort_by(list_order);
        let order: Vec<&str> = rows.iter().map(|row| row.key.as_str()).collect();
        // Ascending in value, ties by id; then `absent` and the unlinked, by id.
        assert_eq!(order, ["beta", "epsilon", "delta", "alpha", "gamma"]);
    }

    #[test]
    fn the_confidence_of_no_asymmetry_is_confirmed() {
        assert_eq!(Confidence::of(false, false), Confidence::Confirmed);
        assert!(!Confidence::of(false, false).is_provisional());
        assert!(Confidence::of(true, true).is_provisional());
        assert_eq!(
            Confidence::of(true, true),
            Confidence::Provisional(Provisional::ClaimedAndContent)
        );
    }
}
