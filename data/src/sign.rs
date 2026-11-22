//! Detached signatures and the objects that register and revoke keys (SPEC
//! §3).

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};

use crate::event::{field, hashes, hashes_value, lower_hex, sorted_distinct, Actor, Hash};
use crate::literal::{Call, ProdromeError, Value};
use crate::payload::string_field;

crate::newtype_str! {
    /// An Ed25519 public key: its 32 bytes as 64 lowercase hex.
    PublicKey
}

crate::newtype_str! {
    /// An Ed25519 signature: its 64 bytes as 128 lowercase hex.
    Signature
}

impl PublicKey {
    /// Accepts only 64 lowercase hex that encode a point on the Ed25519 curve.
    pub fn new(text: impl Into<String>) -> Result<PublicKey, ProdromeError> {
        let text = text.into();
        bytes::<32>(&text)
            .filter(|bytes| VerifyingKey::from_bytes(bytes).is_ok())
            .map(|_| PublicKey(text.clone()))
            .ok_or_else(|| {
                ProdromeError::invalid(format!(
                    "a public key must be 64 lowercase hex encoding an Ed25519 point, got {text:?}"
                ))
            })
    }

    fn verifying(&self) -> VerifyingKey {
        let bytes = bytes::<32>(&self.0).expect("checked by the constructor");
        VerifyingKey::from_bytes(&bytes).expect("checked by the constructor")
    }
}

impl Signature {
    /// Accepts 128 lowercase hex, and does not check that the signature
    /// verifies ([`Signed::verifies`] does).
    pub fn new(text: impl Into<String>) -> Result<Signature, ProdromeError> {
        let text = text.into();
        if lower_hex(&text, 128) {
            Ok(Signature(text))
        } else {
            Err(ProdromeError::invalid(format!(
                "a signature must be 128 lowercase hex, got {text:?}"
            )))
        }
    }
}

/// For the core; no promise. `text` as `N` bytes, where it is `2N` lowercase hex.
#[doc(hidden)]
pub fn bytes<const N: usize>(text: &str) -> Option<[u8; N]> {
    if !lower_hex(text, 2 * N) {
        return None;
    }
    let mut out = [0; N];
    for (byte, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// The signed bytes: `prodrome object ` and the object's name. The prefix keeps
/// a key that signs objects from signing anything else by accident.
fn message(object: &Hash) -> Vec<u8> {
    let mut message = b"prodrome object ".to_vec();
    message.extend_from_slice(object.as_str().as_bytes());
    message
}

/// The detached signature by `key` of the object named `object` (SPEC §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub object: Hash,
    pub key: PublicKey,
    pub signature: Signature,
}

impl Signed {
    /// Whether the signature verifies under its key over its object's name. The
    /// check is strict (SPEC §3), so that a forger cannot compute a second valid
    /// encoding for one key and one message from the first.
    #[must_use]
    pub fn verifies(&self) -> bool {
        let signature = bytes::<64>(self.signature.as_str())
            .map(|bytes| ed25519_dalek::Signature::from_bytes(&bytes));
        signature.is_some_and(|signature| {
            self.key
                .verifying()
                .verify_strict(&message(&self.object), &signature)
                .is_ok()
        })
    }

    #[must_use]
    pub fn to_value(&self) -> Value {
        Value::call(
            "Signed",
            vec![
                field("object", Value::str(self.object.as_str())),
                field("key", Value::str(self.key.as_str())),
                field("signature", Value::str(self.signature.as_str())),
            ],
        )
    }

    pub fn from_call(call: &Call) -> Result<Signed, ProdromeError> {
        Ok(Signed {
            object: Hash::new(string_field(call, "object")?)?,
            key: PublicKey::new(string_field(call, "key")?)?,
            signature: Signature::new(string_field(call, "signature")?)?,
        })
    }
}

/// `KeyAdded(genesis, actor, key)`: in the prodrome `genesis`, `key`
/// speaks for `actor` (SPEC §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAdded {
    pub genesis: Hash,
    pub actor: Actor,
    pub key: PublicKey,
}

impl KeyAdded {
    #[must_use]
    pub fn to_value(&self) -> Value {
        Value::call(
            "KeyAdded",
            vec![
                field("genesis", Value::str(self.genesis.as_str())),
                field("actor", Value::str(self.actor.as_str())),
                field("key", Value::str(self.key.as_str())),
            ],
        )
    }

    pub fn from_call(call: &Call) -> Result<KeyAdded, ProdromeError> {
        Ok(KeyAdded {
            genesis: Hash::new(string_field(call, "genesis")?)?,
            actor: Actor::new(string_field(call, "actor")?)?,
            key: PublicKey::new(string_field(call, "key")?)?,
        })
    }
}

/// `KeyRevoked(genesis, deps, actor, key)`: `key` no longer speaks for
/// `actor`, except in the signatures that this object rests on (SPEC §5,
/// "Proof").
///
/// `deps` are sorted and distinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRevoked {
    pub genesis: Hash,
    pub deps: Vec<Hash>,
    pub actor: Actor,
    pub key: PublicKey,
}

/// A revocation with its deps sorted; refuses a dep named twice.
pub fn mk_key_revoked(
    genesis: Hash,
    deps: Vec<Hash>,
    actor: Actor,
    key: PublicKey,
) -> Result<KeyRevoked, ProdromeError> {
    Ok(KeyRevoked {
        genesis,
        deps: sorted_distinct("KeyRevoked.deps", "dep", deps)?,
        actor,
        key,
    })
}

impl KeyRevoked {
    #[must_use]
    pub fn to_value(&self) -> Value {
        Value::call(
            "KeyRevoked",
            vec![
                field("genesis", Value::str(self.genesis.as_str())),
                field("deps", hashes_value(&self.deps)),
                field("actor", Value::str(self.actor.as_str())),
                field("key", Value::str(self.key.as_str())),
            ],
        )
    }

    pub fn from_call(call: &Call) -> Result<KeyRevoked, ProdromeError> {
        mk_key_revoked(
            Hash::new(string_field(call, "genesis")?)?,
            hashes(call, "deps")?,
            Actor::new(string_field(call, "actor")?)?,
            PublicKey::new(string_field(call, "key")?)?,
        )
    }
}

/// For the core; no promise. The public key of the signing key `secret`.
#[doc(hidden)]
pub fn public_of(secret: &SigningKey) -> PublicKey {
    PublicKey(hex(secret.verifying_key().as_bytes()))
}

/// For the core; no promise. The signature by `secret` of the object `object`.
#[doc(hidden)]
pub fn signed_by(secret: &SigningKey, object: &Hash) -> Signed {
    Signed {
        object: object.clone(),
        key: public_of(secret),
        signature: Signature(hex(&secret.sign(&message(object)).to_bytes())),
    }
}
