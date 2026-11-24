//! Keys, signatures and their proof (SPEC §3, §5).

use prodrome::dag::Dag;
use prodrome::event::{parse_envelope, seal_hash, Envelope, TodoEvent};
use prodrome::event::{Actor, Hash};
use prodrome::literal::print_literal;
use prodrome::reference::Todo;
use prodrome::sign::*;

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
        parse_envelope(&prodrome::todo::TodoVocabulary::default(), &print).expect("parses");
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
            prodrome::genesis::mk_genesis("signed", &"ab".repeat(16)).expect("a genesis"),
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
        let at = prodrome::literal::Datetime::new(2026, 10, 3, 9, 0, 0, 0).expect("an instant");
        let event = prodrome::event::mk_completed(todo, at, actor, "").expect("an event");
        let change =
            prodrome::change::mk_change(self.genesis.clone(), vec![], event).expect("a change");
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
    let findings = history.dag().verify(&prodrome::policy::Everything);
    assert!(findings
        .iter()
        .any(|finding| matches!(finding, prodrome::dag::Finding::Forged { .. })));
    assert!(findings.iter().any(|finding| matches!(
        finding,
        prodrome::dag::Finding::Unsigned { object, .. } if *object == mine
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
