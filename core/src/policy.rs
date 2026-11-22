//! Standing (SPEC §5): whether an event binds or only claims.

use std::collections::BTreeSet;

use crate::dag::Dag;
use crate::event::{Actor, Hash};
use crate::schema::Schema;
use crate::sign::{Proof, Proved, PublicKey, Registrar};

/// The answer of a [`Policy`] for one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Standing {
    /// The folds apply the event to its registers.
    Binds,
    /// The store keeps and shows the event as a claim, and the event changes
    /// no confirmed reading (law 11).
    Claims,
}

impl Standing {
    /// `Binds` only where both bind.
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

/// A deployment's answer to SPEC §5. A host implements it.
///
/// The standing of an object must depend only on the object and on what the
/// policy read of the history ([`Policy::at`]), never on the order of the log
/// or on the time, because law 12 depends on it.
pub trait Policy<E: Schema> {
    /// Whether the object `object`, which carries `event`, binds or claims.
    fn standing(&self, object: &Hash, event: &E) -> Standing;

    /// Whether the event is the host's own word (SPEC §5). A policy overrides
    /// the default to fold a writer's events and still mark them as that
    /// writer's. An implementation must keep `standing(o, e) == Claims`
    /// implies `!confirms(o, e)`, because the laws of SPEC §9 assume it.
    fn confirms(&self, object: &Hash, event: &E) -> bool {
        self.standing(object, event).binds()
    }

    /// This policy at the history `history`. `None` means that the policy
    /// reads nothing of a history, so it is the same at every history.
    fn at(&self, history: &Dag<E>) -> Option<Self>
    where
        Self: Sized,
    {
        let _ = history;
        None
    }
}

/// The policy under which every event binds. The claimed reading of SPEC
/// §6.7 uses it (law 10). `Untrusted::none()` is equal to it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Everything;

impl<E: Schema> Policy<E> for Everything {
    fn standing(&self, _object: &Hash, _event: &E) -> Standing {
        Standing::Binds
    }
}

/// The reference policy (SPEC §5, "The reference policy"): a roster of actor
/// names.
///
/// A named actor reads input that an attacker can control, so the policy
/// does not let an injected completion, tending or repricing bind. The policy
/// is not behind the `reference` feature, because it needs no payload and the
/// conformance suites read the vectors' `untrusted` fields through it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Untrusted(BTreeSet<Actor>);

impl Untrusted {
    /// The empty roster, under which every event binds.
    pub fn none() -> Untrusted {
        Untrusted(BTreeSet::new())
    }

    pub fn of(actors: impl IntoIterator<Item = Actor>) -> Untrusted {
        Untrusted(actors.into_iter().collect())
    }

    /// The roster, for a reader that shows the policy.
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

/// The policy that requires signatures (SPEC §5, "The policy that requires
/// signatures"), over a policy `P`.
///
/// An object binds only where a signature by a key registered to its actor
/// proves it, or, for a husk, the proof vouches for it, and `P` binds it. A
/// `Proven` is made at one history; ask it [`Policy::at`] to read another
/// history.
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

    /// How the proof makes `object` stand: a proven object and a husk the
    /// proof vouches for bind, and an unsigned one claims.
    fn proved(&self, object: &Hash) -> Standing {
        match self.proof.standing(object) {
            Proved::Proven | Proved::Vouched => Standing::Binds,
            Proved::Unsigned => Standing::Claims,
        }
    }
}

/// A husk (SPEC §3) binds where the proof VOUCHES for it, as an object
/// binds where the proof proves it: the host took the husk from its own
/// store or from its keeper of record, whose word it takes, and the maker
/// verified the signature over the whole. So husking a store changes no
/// reading under this policy (law 71).
impl<E: Schema, P: Policy<E> + Clone> Policy<E> for Proven<P> {
    fn standing(&self, object: &Hash, event: &E) -> Standing {
        self.proved(object).and(self.policy.standing(object, event))
    }

    fn confirms(&self, object: &Hash, event: &E) -> bool {
        self.proved(object) == Standing::Binds && self.policy.confirms(object, event)
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
