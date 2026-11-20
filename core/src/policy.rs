//! §5 — STANDING: does this event bind, or does it only claim?
//!
//! See ../../SPEC.md. Every fold in [`crate::fold`], the register action in
//! [`crate::registers`], the composition in [`crate::view`] and §3's `verify`
//! ask exactly this one question about an event, and a [`Policy`] — the HOST's
//! — is what answers it.
//!
//! THE DATABASE DOES NOT DECIDE WHOM TO BELIEVE. Which writers a deployment
//! stands behind is not a fact about a DAG of events, any more than what a
//! record MEANS is (§4, and the same move: 0.2 made the record the host's,
//! 0.3 makes the standing the host's). The core carried one deployment's
//! answer — a set of untrusted actor names, plus a kind-dependent rule — and
//! that answer is now [`Untrusted`], the REFERENCE policy: still here, still
//! what `conformance/` was taken under, and no longer something the folds
//! know.
//!
//! WHAT STAYS IN THE CORE is the two-reading fold: the CONFIRMED environment
//! and the CLAIMED one, side by side in [`crate::view::entries`]. A store that
//! held only the filtered reading could not show a claim at all, and showing
//! one is the whole point of storing it. [`Everything`] is the policy the
//! claimed reading is taken under.
//!
//! WHO WROTE AN OBJECT IS A READING TOO. An event's `actor` is a field its
//! writer chose; a history can prove it (§3's signatures, [`Proof`]). A
//! policy may ask for that proof, and [`Proven`] is the one combinator that
//! does, over any policy: an object binds under it where the history proves
//! its actor AND the policy it wraps binds it. So standing is a function of
//! the object and of a reading of the history it is in, and the history's
//! only through that reading, which is a function of its object set (law
//! 41): never of the log's order or the moment.

use std::collections::BTreeSet;

use crate::dag::Dag;
use crate::event::{Actor, Hash};
use crate::schema::Schema;
use crate::sign::{Proof, PublicKey, Registrar};

/// What a [`Policy`] says about one event: the two readings, as a sum.
///
/// A `bool` would have been the same two inhabitants with none of the meaning:
/// every call site here reads "does this write" and "is this a claim" out of
/// the same value, and the two are not negations of each other by luck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Standing {
    /// The folds take it: it writes its registers, and what it says is what
    /// the store believes.
    Binds,
    /// The folds refuse it. It is stored and it is SHOWN — as a claim, beside
    /// the answer that stands — and it changes no confirmed reading (§9.11).
    Claims,
}

impl Standing {
    /// Both bind: the meet of two answers, `Claims` the bottom.
    #[must_use]
    pub fn and(self, other: Standing) -> Standing {
        if self.binds() && other.binds() {
            Standing::Binds
        } else {
            Standing::Claims
        }
    }

    #[must_use]
    pub fn binds(self) -> bool {
        matches!(self, Standing::Binds)
    }

    #[must_use]
    pub fn claims(self) -> bool {
        matches!(self, Standing::Claims)
    }
}

/// A deployment's answer to §5, as a type.
///
/// One required method: the standing of an OBJECT, named, carrying its
/// event, which is a function of that object and of what the policy has read
/// of the history ([`Policy::at`]) — not of the log's order, not of the
/// position, not of the moment. §9.12 is that fact as a law: because standing
/// selects objects rather than positions, folding under a policy is folding
/// the sub-log of its binding events, whatever order the log arrives in.
///
/// Asked of EVERY object that carries an event. The schema says which events
/// are about standing ([`Schema::asks`]); a policy that holds writers to a
/// roster, as [`Untrusted`] does, binds every other one whoever wrote it, and
/// a policy that asks who wrote it, as [`Proven`] does, asks of every one.
///
/// NOT SEALED. A host implements this; it is the seam the decision leaves the
/// core through. [`Untrusted`] is the reference implementation and
/// [`Everything`] the trivial one.
///
/// Generic over the schema, so a host whose events say something about their
/// own standing can read it. A policy that does not care — the two here do
/// not — implements it for every schema in one blanket impl.
pub trait Policy<E: Schema> {
    /// Does the object named `object`, carrying `event`, write, or does it
    /// only claim?
    fn standing(&self, object: &Hash, event: &E) -> Standing;

    /// Is this event the host's OWN word?
    ///
    /// Two readers ask, and neither is a fold: [`crate::view::entries`] marks
    /// a row whose winning content record is not the host's own
    /// ([`crate::view::Provisional::Content`]), and §3's `verify` holds such
    /// an event's `at` to the DAG's order — a writer whose stamp the host
    /// forces cannot legitimately be dated behind what it was written on top
    /// of, where a human backfilling 2025 completions today legitimately can.
    ///
    /// THE DEFAULT IS §5 EXACTLY: a claiming event is not the host's word, a
    /// binding one is. A policy that folds a writer's events while still
    /// marking them as that writer's overrides it — the reference policy does,
    /// because a content record from an untrusted actor binds (writing content
    /// is what such a writer is for) and is still shown as its author's.
    ///
    /// `standing(o, e) == Claims` implies `!confirms(o, e)`; every
    /// implementation here keeps that, and §9's laws are stated over
    /// policies that do.
    fn confirms(&self, object: &Hash, event: &E) -> bool {
        self.standing(object, event).binds()
    }

    /// This policy at the history `history`, what it reads of a history read
    /// of this one; `None` where it reads nothing of a history and so is
    /// itself at every one, as both policies here are. [`Proven`] reads its
    /// [`Proof`]. A reader of a history asks the policy at that history,
    /// and the store asks its own at every `verify`.
    fn at(&self, history: &Dag<E>) -> Option<Self>
    where
        Self: Sized,
    {
        let _ = history;
        None
    }
}

/// The policy under which every event binds.
///
/// §6.7's CLAIMED reading is taken under it — "what would every writer's
/// events bind" — and §9.10 is the law that says the two readings are then the
/// same reading. `Untrusted::none()` is equal to it and says the same thing
/// about a deployment with an empty roster.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Everything;

impl<E: Schema> Policy<E> for Everything {
    fn standing(&self, _object: &Hash, _event: &E) -> Standing {
        Standing::Binds
    }
}

/// THE REFERENCE POLICY: a set of actor names, and the rule the conformance
/// vectors were taken under.
///
/// Over any schema, the same rule: a named actor's events the schema
/// [`Schema::asks`] about CLAIM, and its others bind. Under the todo schema,
/// a named actor's LIFECYCLE, `Tended` and `SpecRevised` events CLAIM —
/// stored, shown, never folded — because such a writer reads
/// attacker-controlled input and an injected completion, tending or repricing
/// is the threat the roster exists for. Its
/// CONTENT records BIND: writing content is what the writer is for, and a fold
/// that hid its writes would not be containment, it would be an outage that
/// reports success. They are still not the host's own word, so
/// [`Policy::confirms`] refuses them and the reader marks the row.
///
/// It lives in the core, not behind the `reference` feature that gates the
/// reference PAYLOAD: it is a policy over any schema's events, it needs
/// nothing from any payload, and the core's conformance suites — which read
/// `conformance/`'s `untrusted` fields through it — would have to be
/// feature-gated with it for no gain. The `reference` feature answers "which
/// record shape"; this answers "whose word", and one store can want the second
/// without the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Untrusted(BTreeSet<Actor>);

impl Untrusted {
    /// A deployment that stands behind every writer, said out loud.
    pub fn none() -> Untrusted {
        Untrusted(BTreeSet::new())
    }

    pub fn of(actors: impl IntoIterator<Item = Actor>) -> Untrusted {
        Untrusted(actors.into_iter().collect())
    }

    /// The roster. A reader that wants to SHOW the policy asks; nothing in the
    /// core reads an actor name to decide anything.
    pub fn actors(&self) -> &BTreeSet<Actor> {
        &self.0
    }
}

impl<E: Schema> Policy<E> for Untrusted {
    fn standing(&self, _object: &Hash, event: &E) -> Standing {
        if !event.asks() || !self.0.contains(event.actor()) {
            Standing::Binds
        } else {
            Standing::Claims
        }
    }

    /// The roster, whatever the kind — the override the doc on
    /// [`Policy::confirms`] describes. An untrusted actor's content record
    /// binds and is still that actor's.
    fn confirms(&self, _object: &Hash, event: &E) -> bool {
        !self.0.contains(event.actor())
    }
}

/// THE POLICY THAT REQUIRES SIGNATURES, over any policy `P`: an object by
/// actor A binds only where a signature by a key registered to A proves it
/// (SPEC §5's proof), and then as `P` says. A key counts where one of the
/// ROOT keys signed its `KeyAdded`, and a revocation where a root signed its
/// `KeyRevoked`; the roots are the host's, as `P` is.
///
/// The meet of two policies, so it composes: `Proven<Untrusted>` holds a
/// roster's writers to the roster and every writer to its keys. An object
/// it does not prove CLAIMS, whatever its kind, a content record included:
/// a record whose author is unproven is not its author's word. It is still
/// stored and shown, as every claim is, and the claimed reading folds it.
///
/// A POLICY AT A HISTORY: it is made at one, and read only there; a reader
/// of another history asks it [`Policy::at`] that one, as the store's
/// `verify` does of what it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proven<P> {
    policy: P,
    roots: BTreeSet<PublicKey>,
    proof: Proof,
}

impl<P> Proven<P> {
    /// `policy`, with every object held to its actor's keys, registered by
    /// `roots`, at the history `history`.
    #[must_use]
    pub fn new<E: Schema>(
        policy: P,
        roots: impl IntoIterator<Item = PublicKey>,
        history: &Dag<E>,
    ) -> Proven<P> {
        let roots: BTreeSet<PublicKey> = roots.into_iter().collect();
        Proven {
            policy,
            proof: Proof::of(history, &Registrar::Roots(roots.clone())),
            roots,
        }
    }

    #[must_use]
    pub fn policy(&self) -> &P {
        &self.policy
    }

    #[must_use]
    pub fn roots(&self) -> &BTreeSet<PublicKey> {
        &self.roots
    }

    /// What it has read of the history it is at.
    #[must_use]
    pub fn proof(&self) -> &Proof {
        &self.proof
    }

    /// Does the proof prove `object`, or vouch for it as a husk?
    fn signed(&self, object: &Hash) -> bool {
        self.proof.proves(object) || self.proof.vouches(object)
    }
}

/// A husk (SPEC §3) binds where the proof VOUCHES for it, as an object
/// binds where the proof proves it: the host took the husk from its own
/// store or from its keeper of record, whose word it takes, and the maker
/// verified the signature over the whole. So husking a store changes no
/// reading under this policy (law 71).
impl<E: Schema, P: Policy<E> + Clone> Policy<E> for Proven<P> {
    fn standing(&self, object: &Hash, event: &E) -> Standing {
        let proven = if self.signed(object) {
            Standing::Binds
        } else {
            Standing::Claims
        };
        proven.and(self.policy.standing(object, event))
    }

    fn confirms(&self, object: &Hash, event: &E) -> bool {
        self.signed(object) && self.policy.confirms(object, event)
    }

    fn at(&self, history: &Dag<E>) -> Option<Self> {
        Some(Proven::new(
            at(&self.policy, history).into_owned(),
            self.roots.iter().cloned(),
            history,
        ))
    }
}

/// `policy` at the history `history` ([`Policy::at`]): itself, borrowed,
/// where it reads nothing of a history.
fn at<'p, E: Schema, P: Policy<E> + Clone>(
    policy: &'p P,
    history: &Dag<E>,
) -> std::borrow::Cow<'p, P> {
    policy
        .at(history)
        .map_or(std::borrow::Cow::Borrowed(policy), std::borrow::Cow::Owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{mk_completed, mk_created, TodoEvent};
    use crate::literal::Datetime;
    use crate::reference::{mk_authored, Todo};

    type Event = TodoEvent<Todo>;

    fn at() -> Datetime {
        Datetime::new(2026, 9, 6, 7, 3, 0, 0).expect("a real instant")
    }

    fn record(actor: &str) -> Event {
        mk_authored(
            "alpha",
            at(),
            actor,
            "todo",
            at(),
            "body",
            None,
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

    /// Neither policy here reads the object's name.
    fn object() -> Hash {
        Hash::new("0".repeat(64)).expect("a name")
    }

    fn roster() -> Untrusted {
        Untrusted::of([Actor::new("triage").expect("valid")])
    }

    #[test]
    fn a_named_actors_lifecycle_event_claims_and_its_record_binds() {
        let policy = roster();
        let claimed: Event = mk_completed("alpha", at(), "triage", "").expect("valid");
        assert_eq!(policy.standing(&object(), &claimed), Standing::Claims);
        assert_eq!(
            policy.standing(&object(), &record("triage")),
            Standing::Binds
        );
        assert_eq!(
            policy.standing(
                &object(),
                &mk_completed::<Todo>("alpha", at(), "bassel", "").expect("valid")
            ),
            Standing::Binds
        );
    }

    #[test]
    fn a_record_that_binds_can_still_not_be_the_hosts_own_word() {
        let policy = roster();
        assert!(policy.standing(&object(), &record("triage")).binds());
        assert!(
            !policy.confirms(&object(), &record("triage")),
            "it binds, and it is still the agent's"
        );
        assert!(policy.confirms(&object(), &record("bassel")));
    }

    #[test]
    fn an_empty_roster_is_the_policy_that_binds_everything() {
        let none = Untrusted::none();
        for event in [
            record("triage"),
            mk_completed("alpha", at(), "triage", "").expect("valid"),
            mk_created("alpha", at(), "triage", "", "").expect("valid"),
        ] {
            assert_eq!(
                none.standing(&object(), &event),
                Everything.standing(&object(), &event)
            );
            assert_eq!(
                none.confirms(&object(), &event),
                Everything.confirms(&object(), &event)
            );
        }
    }

    #[test]
    fn a_claiming_event_is_never_the_hosts_own_word() {
        let policy = roster();
        let claimed: Event = mk_completed("alpha", at(), "triage", "").expect("valid");
        assert!(
            policy.standing(&object(), &claimed).claims() && !policy.confirms(&object(), &claimed)
        );
    }
}
