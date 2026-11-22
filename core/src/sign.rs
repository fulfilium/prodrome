//! Detached signatures, keys registered by objects, and the proof that the
//! history gives (SPEC §3, and SPEC §5, "Proof: who wrote an object, read
//! from the history").

mod checks;

use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};

use crate::dag::Dag;
use crate::event::{
    field, hashes, hashes_value, lower_hex, sorted_distinct, Actor, Envelope, Hash,
};
use crate::literal::{Call, ProdromeError, Value};
use crate::payload::string_field;
use crate::registers::Genesis;
use crate::schema::Schema;

pub use checks::Checks;

crate::newtype_str! {
    /// An Ed25519 public key: its 32 bytes as 64 lowercase hex.
    PublicKey
}

crate::newtype_str! {
    /// An Ed25519 signature: its 64 bytes as 128 lowercase hex.
    Signature
}

impl PublicKey {
    /// Accepts only 64 lowercase hex that encode a point on the Ed25519
    /// curve.
    ///
    /// # Errors
    ///
    /// Any other text.
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
    /// Accepts 128 lowercase hex. This does not check that the signature
    /// verifies; [`Signed::verifies`] checks that.
    ///
    /// # Errors
    ///
    /// Any other text.
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

/// `text` as `N` bytes, where it is `2N` lowercase hex.
fn bytes<const N: usize>(text: &str) -> Option<[u8; N]> {
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

/// What a signature of `object` signs: the bytes `prodrome object `, then
/// the object's name. The prefix keeps a key that signs objects from
/// signing anything else by accident.
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
    /// Whether the signature verifies under its key over the name of its
    /// object.
    ///
    /// The check is strict: it refuses a small-order key and a non-canonical
    /// scalar, so that a forger cannot compute a second valid encoding for
    /// one key and one message from the first.
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

    /// # Errors
    ///
    /// A field missing, or refused by its type.
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

    /// # Errors
    ///
    /// A field missing, or refused by its type.
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
/// "Proof: who wrote an object, read from the history").
///
/// `deps` are sorted and distinct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRevoked {
    pub genesis: Hash,
    pub deps: Vec<Hash>,
    pub actor: Actor,
    pub key: PublicKey,
}

/// A revocation, with its deps sorted and distinct.
///
/// # Errors
///
/// A dep named twice.
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

    /// # Errors
    ///
    /// A field missing, or refused by its type.
    pub fn from_call(call: &Call) -> Result<KeyRevoked, ProdromeError> {
        mk_key_revoked(
            Hash::new(string_field(call, "genesis")?)?,
            hashes(call, "deps")?,
            Actor::new(string_field(call, "actor")?)?,
            PublicKey::new(string_field(call, "key")?)?,
        )
    }
}

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
        PublicKey(hex(self.0.verifying_key().as_bytes()))
    }

    /// The signature by this key of the object named `object`.
    ///
    /// The signature proves whatever bytes hash to `object`, so the caller
    /// must have hashed the object's bytes to `object` itself. To sign an
    /// object that a DAG holds, use [`Secret::sign_whole`], which refuses a
    /// name for which the DAG holds no such bytes.
    #[must_use]
    pub fn sign(&self, object: &Hash) -> Signed {
        Signed {
            object: object.clone(),
            key: self.public(),
            signature: Signature(hex(&self.0.sign(&message(object)).to_bytes())),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{parse_envelope, seal_hash, Envelope, TodoEvent};
    use crate::literal::print_literal;
    use crate::reference::Todo;

    type Object = Envelope<TodoEvent<Todo>>;

    /// RFC 8032 §7.1, TEST 1: the seed, and the key it derives.
    const SEED: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    fn name(text: &str) -> Hash {
        Hash::of_bytes(text.as_bytes())
    }

    fn round_trips(object: &Object) {
        let print = print_literal(&object.to_value());
        let back: Object =
            parse_envelope(&crate::todo::TodoVocabulary::default(), &print).expect("parses");
        assert_eq!(&back, object);
        assert_eq!(seal_hash(&back), Hash::of_bytes(print.as_bytes()));
    }

    #[test]
    fn a_seed_derives_rfc_8032s_key() {
        assert_eq!(Secret::new(SEED).expect("a seed").public().as_str(), PUBLIC);
    }

    #[test]
    fn each_object_prints_and_parses_back() {
        let secret = Secret::new(SEED).expect("a seed");
        let key = secret.public();
        let actor = Actor::new("bassel").expect("an actor");
        round_trips(&Envelope::Signed(secret.sign(&name("a change"))));
        round_trips(&Envelope::KeyAdded(KeyAdded {
            genesis: name("a genesis"),
            actor: actor.clone(),
            key: key.clone(),
        }));
        round_trips(&Envelope::KeyRevoked(
            mk_key_revoked(name("a genesis"), vec![name("b"), name("a")], actor, key)
                .expect("distinct deps"),
        ));
    }

    #[test]
    fn a_signature_verifies_over_its_object_and_nothing_else() {
        let secret = Secret::new(SEED).expect("a seed");
        let signed = secret.sign(&name("a change"));
        assert!(signed.verifies());
        assert_eq!(
            signed,
            secret.sign(&name("a change")),
            "signing is deterministic"
        );
        let moved = Signed {
            object: name("another change"),
            ..signed.clone()
        };
        assert!(
            !moved.verifies(),
            "a signature does not move to another object"
        );
        let other = Secret::new(&"1".repeat(64)).expect("a seed").public();
        assert!(!Signed {
            key: other,
            ..signed.clone()
        }
        .verifies());
        let mut flipped = signed.signature.as_str().to_owned();
        flipped.replace_range(0..1, if flipped.starts_with('0') { "1" } else { "0" });
        let flipped = Signed {
            signature: Signature::new(flipped).expect("hex"),
            ..signed
        };
        assert!(!flipped.verifies());
    }

    #[test]
    fn a_key_is_a_point_and_a_signature_is_hex() {
        assert!(PublicKey::new(PUBLIC).is_ok());
        assert!(PublicKey::new(PUBLIC.to_uppercase()).is_err());
        assert!(PublicKey::new(&PUBLIC[2..]).is_err());
        // The y-coordinate 2 is on no point of the curve.
        let off = format!("02{}", "0".repeat(62));
        assert!(PublicKey::new(off).is_err());
        assert!(Signature::new("0".repeat(128)).is_ok());
        assert!(Signature::new("0".repeat(127)).is_err());
        assert!(Secret::new("not hex").is_err());
    }

    #[test]
    fn a_revocation_names_each_dep_once() {
        let key = PublicKey::new(PUBLIC).expect("a key");
        let actor = Actor::new("bassel").expect("an actor");
        assert!(mk_key_revoked(name("g"), vec![name("a"), name("a")], actor, key).is_err());
    }

    /// A history built by hand: one prodrome, its objects by name.
    struct History {
        genesis: Hash,
        objects: Vec<(Hash, Object)>,
    }

    impl History {
        fn new() -> History {
            let genesis = Envelope::Genesis(
                crate::genesis::mk_genesis("signed", &"ab".repeat(16)).expect("a genesis"),
            );
            let name = seal_hash(&genesis);
            History {
                genesis: name.clone(),
                objects: vec![(name, genesis)],
            }
        }

        fn put(&mut self, object: Object) -> Hash {
            let name = seal_hash(&object);
            self.objects.push((name.clone(), object));
            name
        }

        fn write(&mut self, actor: &str, todo: &str) -> Hash {
            let at = crate::literal::Datetime::new(2026, 10, 3, 9, 0, 0, 0).expect("an instant");
            let event = crate::event::mk_completed(todo, at, actor, "").expect("an event");
            let change =
                crate::change::mk_change(self.genesis.clone(), vec![], event).expect("a change");
            self.put(Envelope::Change(change))
        }

        fn add(&mut self, actor: &str, key: &Secret) -> Hash {
            self.put(Envelope::KeyAdded(KeyAdded {
                genesis: self.genesis.clone(),
                actor: Actor::new(actor).expect("an actor"),
                key: key.public(),
            }))
        }

        fn revoke(&mut self, actor: &str, key: &Secret, deps: Vec<Hash>) -> Hash {
            let revoked = mk_key_revoked(
                self.genesis.clone(),
                deps,
                Actor::new(actor).expect("an actor"),
                key.public(),
            )
            .expect("a revocation");
            self.put(Envelope::KeyRevoked(revoked))
        }

        fn sign(&mut self, key: &Secret, object: &Hash) -> Hash {
            self.put(Envelope::Signed(key.sign(object)))
        }

        fn proof(&self, registrar: &Registrar) -> Proof {
            Proof::of(&self.dag(), registrar)
        }

        fn dag(&self) -> Dag<TodoEvent<Todo>> {
            self.objects.iter().cloned().collect()
        }
    }

    fn secret(byte: char) -> Secret {
        Secret::new(&byte.to_string().repeat(64)).expect("a seed")
    }

    #[test]
    fn a_registered_key_proves_what_it_signs_for_its_actor_only() {
        let (phone, other) = (secret('1'), secret('2'));
        let mut history = History::new();
        let mine = history.write("bassel", "a");
        let theirs = history.write("triage", "b");
        let unsigned = history.write("bassel", "c");
        history.add("bassel", &phone);
        history.sign(&phone, &mine);
        history.sign(&phone, &theirs);
        history.sign(&other, &unsigned);
        let proof = history.proof(&Registrar::Anyone);
        assert!(proof.proves(&mine));
        assert!(
            !proof.proves(&theirs),
            "the key is bassel's, the object triage's"
        );
        assert!(
            !proof.proves(&unsigned),
            "a key registered to nobody proves nothing"
        );
        let genesis = Some(history.genesis.clone());
        assert!(proof.keyed(&genesis, &Actor::new("bassel").expect("an actor")));
        assert!(!proof.keyed(&genesis, &Actor::new("triage").expect("an actor")));
    }

    #[test]
    fn a_forged_signature_proves_nothing() {
        let phone = secret('1');
        let mut history = History::new();
        let mine = history.write("bassel", "a");
        let other = history.write("bassel", "b");
        history.add("bassel", &phone);
        history.put(Envelope::Signed(Signed {
            object: mine.clone(),
            ..phone.sign(&other)
        }));
        assert!(!history.proof(&Registrar::Anyone).proves(&mine));
        let findings = history.dag().verify(&crate::policy::Everything);
        assert!(findings
            .iter()
            .any(|finding| matches!(finding, crate::dag::Finding::Forged { .. })));
        assert!(findings.iter().any(|finding| matches!(
            finding,
            crate::dag::Finding::Unsigned { object, .. } if *object == mine
        )));
    }

    #[test]
    fn a_revocation_keeps_exactly_what_it_rests_on() {
        let phone = secret('1');
        let mut history = History::new();
        history.add("bassel", &phone);
        let (kept, dropped) = (history.write("bassel", "a"), history.write("bassel", "b"));
        let kept_signature = history.sign(&phone, &kept);
        history.sign(&phone, &dropped);
        history.revoke("bassel", &phone, vec![kept_signature]);
        let later = history.write("bassel", "c");
        history.sign(&phone, &later);
        let proof = history.proof(&Registrar::Anyone);
        assert!(proof.proves(&kept));
        assert!(!proof.proves(&dropped), "signed, and not stood behind");
        assert!(
            !proof.proves(&later),
            "signed beside or after the revocation"
        );
        history.revoke("bassel", &phone, vec![]);
        assert!(
            !history.proof(&Registrar::Anyone).proves(&kept),
            "every revocation must keep it"
        );
    }

    #[test]
    fn under_roots_a_key_counts_where_a_root_signed_it() {
        let (root, phone) = (secret('9'), secret('1'));
        let mut history = History::new();
        let mine = history.write("bassel", "a");
        history.sign(&phone, &mine);
        let added = history.add("bassel", &phone);
        let roots = Registrar::Roots([root.public()].into());
        assert!(history.proof(&Registrar::Anyone).proves(&mine));
        assert!(
            !history.proof(&roots).proves(&mine),
            "no root registered it"
        );
        history.sign(&phone, &added);
        assert!(
            !history.proof(&roots).proves(&mine),
            "a key cannot register itself"
        );
        history.sign(&root, &added);
        assert!(history.proof(&roots).proves(&mine));
        let revoked = history.revoke("bassel", &phone, vec![]);
        assert!(
            history.proof(&roots).proves(&mine),
            "a revocation no root signed is not one"
        );
        history.sign(&root, &revoked);
        assert!(!history.proof(&roots).proves(&mine));
    }

    #[test]
    fn a_secret_is_never_printed() {
        let printed = format!("{:?}", Secret::new(SEED).expect("a seed"));
        assert!(!printed.contains(SEED) && printed.contains(PUBLIC));
    }
}
