//! Standing (SPEC §5): whether an event binds or only claims, and the two
//! policies that read nothing but the event. The trait `Policy`, which a
//! store asks, names the history of a store, so it is in `prodrome-dag`,
//! with the impls for these types.

use std::collections::BTreeSet;

use crate::event::Actor;

/// The answer of a policy for one event.
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
    #[inline]
    #[must_use]
    pub fn and(self, other: Standing) -> Standing {
        if self.binds() && other.binds() {
            Standing::Binds
        } else {
            Standing::Claims
        }
    }

    #[inline]
    #[must_use]
    pub fn binds(self) -> bool {
        matches!(self, Standing::Binds)
    }

    #[inline]
    #[must_use]
    pub fn claims(self) -> bool {
        matches!(self, Standing::Claims)
    }
}

/// The policy under which every event binds. The claimed reading of SPEC
/// §6.7 uses it (law 10). `Untrusted::none()` is equal to it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Everything;

/// The reference policy (SPEC §5, "The reference policy"): a roster of actor
/// names.
///
/// A named actor reads input that an attacker can control, so the policy
/// does not let an injected completion, tending or repricing bind.
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
    #[inline]
    pub fn actors(&self) -> &BTreeSet<Actor> {
        &self.0
    }
}
