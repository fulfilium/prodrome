//! Detached signatures, keys registered by objects, and the proof that the
//! history gives (SPEC §3, and SPEC §5, "Proof: who wrote an object, read
//! from the history").
//!
//! The object types, such as [`Signed`] and [`KeyAdded`], are defined in
//! `prodrome-data` and re-exported here.

mod checks;

use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::SigningKey;

pub use prodrome_data::sign::*;

use crate::dag::Dag;
use crate::registers::Genesis;
use prodrome_data::event::{Actor, Envelope, Hash};
use prodrome_data::literal::ProdromeError;
use prodrome_data::schema::Schema;

pub use checks::Checks;

/// A signing key of a device: a 32-byte seed that the host keeps where it
/// decides, and never in an object.
///
/// The `Debug` output shows only the public key.
pub struct Secret(SigningKey);

impl Secret {
    /// The key whose seed is `seed`, as 64 lowercase hex. The host must draw
    /// the seed from its own source of randomness; this crate has none.
    ///
    /// # Errors
    ///
    /// A seed that is not 64 lowercase hex.
    pub fn new(seed: &str) -> Result<Secret, ProdromeError> {
        bytes::<32>(seed)
            .map(|seed| Secret(SigningKey::from_bytes(&seed)))
            .ok_or_else(|| ProdromeError::invalid("a secret key must be 64 lowercase hex"))
    }

    #[must_use]
    pub fn public(&self) -> PublicKey {
        public_of(&self.0)
    }

    /// The signature by this key of the object named `object`.
    ///
    /// The signature proves whatever bytes hash to `object`, so the caller
    /// must have hashed the object's bytes to `object` itself. To sign an
    /// object that a DAG holds, use [`Secret::sign_whole`], which refuses a
    /// name for which the DAG holds no such bytes.
    #[must_use]
    pub fn sign(&self, object: &Hash) -> Signed {
        signed_by(&self.0, object)
    }
}

impl Secret {
    /// The signature by this key of the object `object`, which `dag` must
    /// hold whole in its history (law 80: no signature is made over a name
    /// whose whole the signer does not hold).
    ///
    /// The clear fields of a husk are not bound to its name, so a signature
    /// over a husk's name could prove another whole that its giver wrote
    /// under that name (SPEC §3, "A husk").
    ///
    /// # Errors
    ///
    /// `object` is held as a husk, or it is not in the history.
    pub fn sign_whole<E>(&self, dag: &Dag<E>, object: &Hash) -> Result<Signed, ProdromeError> {
        if dag.husks().contains(object) {
            return Err(ProdromeError::invalid(format!(
                "object {object} is held as a husk: its whole is not here to sign"
            )));
        }
        if dag.get(object).is_none() {
            return Err(ProdromeError::invalid(format!(
                "object {object} is not held"
            )));
        }
        Ok(self.sign(object))
    }
}

/// No secret is printed, even to a log.
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Secret").field(&self.public()).finish()
    }
}

/// Which `KeyAdded` and `KeyRevoked` objects a [`Proof`] counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registrar {
    /// Every key object counts. `verify` uses this, because a finding must
    /// not depend on the trust of a reader.
    Anyone,
    /// A key object counts only where a verified signature by one of these
    /// root keys covers it. The host chooses the roots (SPEC §5).
    Roots(BTreeSet<PublicKey>),
}

/// The answer of a [`Proof`] for one object (SPEC §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proved {
    /// A signature by a key that speaks for the object's actor covers its
    /// name, and the object is the bytes that hash to the name.
    Proven,
    /// Such a signature covers the name of a husk, which stands on the word
    /// of its maker (SPEC §3).
    Vouched,
    /// No such signature covers it.
    Unsigned,
}

/// The objects of a history that registered keys sign for their actors,
/// and the actors that have a registered key, each in its prodrome (SPEC
/// §5, "Proof: who wrote an object, read from the history").
///
/// Law 42: the proof depends only on the object set, never on the order in
/// which the objects arrived. Law 77: no signature proves a husk, and no
/// object from a party other than its keeper is proven or vouched for
/// unless the receiver hashed its bytes to its name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Proof {
    // Two sets of names and no third: an object in neither is unsigned.
    proven: BTreeSet<Hash>,
    vouched: BTreeSet<Hash>,
    keyed: BTreeSet<(Genesis, Actor)>,
}

impl Proof {
    /// The proof of the objects of `dag`, with key objects counted as
    /// `registrar` decides.
    #[must_use]
    pub fn of<E: Schema>(dag: &Dag<E>, registrar: &Registrar) -> Proof {
        Proof::read(dag, registrar, Signed::verifies)
    }

    /// [`Proof::of`], with each signature checked through `checks`, so that
    /// a second read checks only the signatures that `checks` does not hold.
    /// Law 49: the answer equals that of [`Proof::of`].
    #[must_use]
    pub fn through<E: Schema>(dag: &Dag<E>, registrar: &Registrar, checks: &Checks) -> Proof {
        Proof::read(dag, registrar, |signed| checks.verifies(signed))
    }

    fn read<E: Schema>(
        dag: &Dag<E>,
        registrar: &Registrar,
        verifies: impl Fn(&Signed) -> bool,
    ) -> Proof {
        let mut signatures: BTreeMap<&Hash, Vec<(&Hash, &PublicKey)>> = BTreeMap::new();
        for (name, object) in dag.objects() {
            if let Envelope::Signed(signed) = object {
                if verifies(signed) {
                    signatures
                        .entry(&signed.object)
                        .or_default()
                        .push((name, &signed.key));
                }
            }
        }
        let counts = |name: &Hash| match registrar {
            Registrar::Anyone => true,
            Registrar::Roots(roots) => signatures
                .get(name)
                .is_some_and(|by| by.iter().any(|(_, key)| roots.contains(*key))),
        };
        let mut keys: BTreeSet<(Genesis, &Actor, &PublicKey)> = BTreeSet::new();
        let mut revoked: BTreeMap<(Genesis, &Actor, &PublicKey), Vec<BTreeSet<Hash>>> =
            BTreeMap::new();
        for (name, object) in dag.objects().iter().filter(|(name, _)| counts(name)) {
            match object {
                Envelope::KeyAdded(added) => {
                    keys.insert((dag.prodrome(name, object), &added.actor, &added.key));
                }
                Envelope::KeyRevoked(revocation) => revoked
                    .entry((
                        dag.prodrome(name, object),
                        &revocation.actor,
                        &revocation.key,
                    ))
                    .or_default()
                    .push(dag.closure(revocation.deps.iter().cloned())),
                _ => {}
            }
        }
        let (vouched, proven): (BTreeSet<Hash>, BTreeSet<Hash>) = dag
            .objects()
            .iter()
            .filter(|(name, object)| {
                let Some(event) = object.event() else {
                    return false;
                };
                let genesis = dag.prodrome(name, object);
                signatures.get(name).is_some_and(|by| {
                    by.iter().any(|(signature, key)| {
                        let whose = (genesis.clone(), event.actor(), *key);
                        keys.contains(&whose)
                            && revoked.get(&whose).is_none_or(|kept| {
                                kept.iter().all(|past| past.contains(*signature))
                            })
                    })
                })
            })
            .map(|(name, _)| name.clone())
            .partition(|name| dag.husks().contains(name));
        let keyed = keys
            .into_iter()
            .map(|(genesis, actor, _)| (genesis, actor.clone()))
            .collect();
        Proof {
            proven,
            vouched,
            keyed,
        }
    }

    /// Whether `object` is proven, vouched for as a husk, or unsigned.
    #[must_use]
    pub fn standing(&self, object: &Hash) -> Proved {
        if self.proven.contains(object) {
            Proved::Proven
        } else if self.vouched.contains(object) {
            Proved::Vouched
        } else {
            Proved::Unsigned
        }
    }

    /// Whether [`Proof::standing`] is [`Proved::Proven`]; never true for a
    /// husk.
    #[must_use]
    pub fn proves(&self, object: &Hash) -> bool {
        self.standing(object) == Proved::Proven
    }

    /// Whether `actor` has a registered key, revoked or not, in the prodrome
    /// `genesis`. If it has, its objects must be signed, and `verify`
    /// reports an unsigned one.
    #[must_use]
    pub fn keyed(&self, genesis: &Genesis, actor: &Actor) -> bool {
        self.keyed.contains(&(genesis.clone(), actor.clone()))
    }
}
