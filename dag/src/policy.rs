//! Standing (SPEC §5): whether an event binds or only claims. `Standing`,
//! `Everything` and `Untrusted` are data, defined in `prodrome-data` and
//! re-exported here. The trait [`Policy`] reads a [`Dag`], so it is here,
//! with the impls for those types.

use std::collections::BTreeSet;

pub use prodrome_data::policy::*;

use crate::dag::Dag;
use crate::sign::{Proof, Proved, PublicKey, Registrar};
use prodrome_data::event::Hash;
use prodrome_data::schema::Schema;

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

impl<E: Schema> Policy<E> for Everything {
    fn standing(&self, _object: &Hash, _event: &E) -> Standing {
        Standing::Binds
    }
}

impl<E: Schema> Policy<E> for Untrusted {
    fn standing(&self, _object: &Hash, event: &E) -> Standing {
        if !event.asks() || !self.actors().contains(event.actor()) {
            Standing::Binds
        } else {
            Standing::Claims
        }
    }

    /// The roster, whatever the kind — the override the doc on
    /// [`Policy::confirms`] describes. An untrusted actor's content record
    /// binds and is still that actor's.
    fn confirms(&self, _object: &Hash, event: &E) -> bool {
        !self.actors().contains(event.actor())
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
