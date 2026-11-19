//! The actors' keys, and what `verify` must find of the signatures in a
//! replica: the script's own account of every key and signature it wrote,
//! read with sets of names and never with the DAG's order.

use std::collections::{BTreeMap, BTreeSet};

use super::step::Who;
use crate::dag::Finding;
use crate::event::{Envelope, Hash};
use crate::schema::Schema;
use crate::sign::Secret;

/// The key of `who` at generation `generation`, from a seed that names
/// both.
pub(super) fn secret(who: Who, generation: u8) -> Secret {
    Secret::new(&format!("{:02x}{:02x}", who.index() + 1, generation).repeat(16))
        .expect("four hex digits repeated sixteen times are a seed")
}

/// The root, whose signature registers and revokes every key.
pub(super) fn root() -> Secret {
    Secret::new(&"9".repeat(64)).expect("64 hex digits are a seed")
}

/// A key of an actor: whose it is, and its generation.
type Owned = (Who, u8);

/// A signature the script wrote: the object it signs, the key, and whether
/// it verifies.
struct Signature {
    object: Hash,
    key: Owned,
    verifies: bool,
}

/// Every key and signature the script wrote, by object name.
#[derive(Default)]
pub(super) struct Keys {
    /// Each actor's current key generation, once it has a key.
    current: BTreeMap<Who, u8>,
    /// Each `KeyAdded`: the key it registers.
    added: BTreeMap<Hash, Owned>,
    /// Each `KeyRevoked`: the key it revokes, and every object its writer
    /// held, which is what its deps rest on.
    revoked: BTreeMap<Hash, (Owned, BTreeSet<Hash>)>,
    /// Each `Signed`.
    signatures: BTreeMap<Hash, Signature>,
}

impl Keys {
    /// The current key of `who`, if it has one.
    pub(super) fn current(&self, who: Who) -> Option<(u8, Secret)> {
        self.current
            .get(&who)
            .map(|generation| (*generation, secret(who, *generation)))
    }

    /// The generation the next key of `who` takes.
    pub(super) fn next(&self, who: Who) -> u8 {
        self.current
            .get(&who)
            .map_or(0, |generation| generation.wrapping_add(1))
    }

    /// The `KeyAdded` named `name` registers generation `generation` of
    /// `who`'s keys, which becomes its current key.
    pub(super) fn added(&mut self, name: Hash, who: Who, generation: u8) {
        self.added.insert(name, (who, generation));
        self.current.insert(who, generation);
    }

    /// The `KeyRevoked` named `name` revokes generation `generation` of
    /// `who`'s keys, and its writer held `held`.
    pub(super) fn revoked(&mut self, name: Hash, who: Who, generation: u8, held: BTreeSet<Hash>) {
        self.revoked.insert(name, ((who, generation), held));
    }

    /// The `Signed` named `name` signs `object` with generation
    /// `generation` of `who`'s keys, and verifies or not.
    pub(super) fn signed(
        &mut self,
        name: Hash,
        object: Hash,
        who: Who,
        generation: u8,
        verifies: bool,
    ) {
        self.signatures.insert(
            name,
            Signature {
                object,
                key: (who, generation),
                verifies,
            },
        );
    }

    /// What `verify` must find of the signatures among `objects`, a
    /// replica's history (SPEC §3, §5): each signature that does not
    /// verify, and each object whose actor has a key there that no
    /// signature there proves. A signature proves its object when it
    /// verifies, its key is registered there to the object's actor, and
    /// each revocation of that key there was written by a replica that held
    /// the signature. Root signatures are no actor's and prove nothing.
    pub(super) fn findings<E: Schema>(
        &self,
        objects: &BTreeMap<Hash, Envelope<E>>,
    ) -> Vec<Finding> {
        let held = |name: &Hash| objects.contains_key(name);
        let registered: BTreeSet<Owned> = self
            .added
            .iter()
            .filter(|(name, _)| held(name))
            .map(|(_, key)| *key)
            .collect();
        let proves = |signature: &Hash, object: &Hash, who: Who| {
            self.signatures.get(signature).is_some_and(|signed| {
                signed.object == *object
                    && signed.verifies
                    && signed.key.0 == who
                    && registered.contains(&signed.key)
                    && self
                        .revoked
                        .iter()
                        .filter(|(name, (key, _))| held(name) && *key == signed.key)
                        .all(|(_, (_, kept))| kept.contains(signature))
            })
        };
        let mut findings = Vec::new();
        for (name, object) in objects {
            if self
                .signatures
                .get(name)
                .is_some_and(|signed| !signed.verifies)
            {
                findings.push(Finding::Forged {
                    signed: name.clone(),
                });
            }
            let Some(event) = object.event() else {
                continue;
            };
            let Some(who) = Who::ALL
                .into_iter()
                .find(|who| who.actor() == *event.actor())
            else {
                continue;
            };
            let keyed = registered.iter().any(|(owner, _)| *owner == who);
            let vouched = self
                .signatures
                .keys()
                .any(|signature| held(signature) && proves(signature, name, who));
            if keyed && !vouched {
                findings.push(Finding::Unsigned {
                    object: name.clone(),
                    actor: event.actor().clone(),
                });
            }
        }
        findings
    }
}
