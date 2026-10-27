//! The core tests' REVIEW schema (`core/tests/schemas/review.rs`), a schema
//! with no valuation that is not the todo's, instantiated through
//! [`crate::schema!`] exactly as a host instantiates its own, and read over a
//! small store: so the generic path runs in CI, and nothing todo-shaped in it
//! goes unnoticed.

use prodrome::change::mk_change;
use prodrome::event::{canonical_envelope, seal_hash, Envelope, Hash};
use prodrome::genesis::mk_genesis;
use serde_json::{json, Value};

use crate::Json;

// The schema's whole surface is the core tests'; this reads part of it.
#[allow(dead_code)]
#[path = "../../../core/tests/schemas/review.rs"]
mod schema;

use schema::{day, moved, opened, Field, Phase, Review, ReviewVocabulary};

/// A review names its entity `pr`; its one register is its phase, whose
/// value is the phase a move names.
impl Json for Review {
    fn key_fields(_: &ReviewVocabulary) -> Vec<&str> {
        vec!["pr"]
    }

    fn key_values(pr: &<Review as prodrome::schema::Schema>::Key) -> Vec<Value> {
        vec![json!(pr.as_ref())]
    }

    fn register(_: &ReviewVocabulary, _phase: Field) -> &str {
        "phase"
    }

    fn value(&self, _phase: Field) -> Value {
        match self {
            Review::Moved { phase, .. } => json!(phase.as_str()),
            Review::Opened { .. } => Value::Null,
        }
    }
}

crate::schema!(Reviews = Review);

/// One object as the exports are sent it.
fn sent(object: &Envelope<Review>) -> Value {
    json!({ "hash": seal_hash(object).as_str(), "text": canonical_envelope(object) })
}

/// A genesis, then on `pr-1` an opening, a move to review, and two moves
/// that each supersede it and not each other, a merge and a close: the one
/// real conflict of the review's order. On `pr-2`, an opening alone.
fn store() -> (Vec<Envelope<Review>>, Vec<Hash>) {
    let genesis = Envelope::Genesis(mk_genesis("reviews", &"0".repeat(32)).expect("a genesis"));
    let root = seal_hash(&genesis);
    let change = |deps: &[&Hash], event| {
        let deps = deps.iter().map(|dep| (*dep).clone()).collect();
        Envelope::Change(mk_change(root.clone(), deps, event).expect("a change"))
    };
    let open = change(&[], opened("pr-1", day(1), "ana", "a change"));
    let review = change(&[], moved("pr-1", day(2), "ana", Phase::Review));
    let under = seal_hash(&review);
    let merged = change(&[&under], moved("pr-1", day(3), "ana", Phase::Merged));
    let closed = change(&[&under], moved("pr-1", day(3), "bo", Phase::Closed));
    let other = change(&[], opened("pr-2", day(4), "bo", "another"));
    let objects = vec![genesis, open, review, merged, closed, other];
    let names = objects.iter().map(seal_hash).collect();
    (objects, names)
}

fn replica() -> (Reviews, Vec<Hash>) {
    let (objects, names) = store();
    let sent = Value::Array(objects.iter().map(sent).collect()).to_string();
    let replica = Reviews::new(&sent).unwrap_or_else(|_| panic!("the objects are read"));
    (replica, names)
}

fn answer(text: Result<String, wasm_bindgen::JsError>) -> Value {
    serde_json::from_str(&text.unwrap_or_else(|_| panic!("the export refused"))).expect("JSON")
}

/// `verify` at the review schema: a healthy store, every row named by its
/// review under `pr`, kinded by its constructor.
#[test]
fn a_review_store_verifies() {
    let (replica, names) = replica();
    let verified = answer(replica.verify());
    assert_eq!(verified["ok"], json!(true), "{verified}");
    assert_eq!(verified["genesis"], json!([names[0].as_str()]));
    let rows = verified["objects"].as_array().expect("rows");
    assert_eq!(rows.len(), 6);
    let merged = rows
        .iter()
        .find(|row| row["hash"] == names[3].as_str())
        .expect("the merge's row");
    assert_eq!(merged["pr"], "pr-1");
    assert_eq!(merged["kind"], "Moved");
    assert!(merged.get("todo").is_none(), "{merged}");
}

/// A refusal is a value the export throws, never a panic.
#[test]
fn a_set_that_is_not_objects_is_refused() {
    assert!(crate::Replica::<Review>::of("{}").is_err());
    let todo = r#"[{"hash": "00", "text": "Created()"}]"#;
    let replica = crate::Replica::<Review>::of(todo).expect("the shape is right");
    assert!(
        replica.read().is_err(),
        "a name that is no name reads nothing"
    );
}

/// The tips are the objects nothing rests on: a change rests on its
/// genesis and on the writes it supersedes, so the opening nothing
/// superseded, the merge and the close, and the other review, and never the
/// genesis. They are the genesis's heads too, the one prodrome.
#[test]
fn a_review_store_has_its_tips_and_heads() {
    let (replica, names) = replica();
    let tips = answer(replica.tips());
    let mut expected: Vec<&str> = [1, 3, 4, 5].iter().map(|i| names[*i].as_str()).collect();
    expected.sort_unstable();
    assert_eq!(tips["tips"], json!(expected));
    assert_eq!(tips["heads"], json!({ names[0].as_str(): expected }));
}

/// A replica that holds the move to review, which rests on its genesis
/// alone, lacks the rest and not the genesis; one that holds nothing lacks
/// everything, in causal order, the genesis first and the merge and the
/// close after the move they supersede.
#[test]
fn a_replica_is_sent_what_its_tips_do_not_hold() {
    let (replica, names) = replica();
    let held = json!([names[2].as_str()]).to_string();
    let since = answer(replica.since(&held));
    let sent: Vec<&str> = since["since"]
        .as_array()
        .expect("names")
        .iter()
        .map(|name| name.as_str().expect("a name"))
        .collect();
    let mut expected: Vec<&str> = [1, 3, 4, 5].iter().map(|i| names[*i].as_str()).collect();
    expected.sort_unstable();
    let mut got = sent.clone();
    got.sort_unstable();
    assert_eq!(got, expected);
    let everything = answer(replica.since("[]"));
    let order = everything["since"].as_array().expect("names");
    let at = |i: usize| order.iter().position(|name| *name == names[i].as_str());
    assert_eq!(order.len(), 6);
    assert_eq!(at(0), Some(0), "{everything}");
    assert!(at(2) < at(3) && at(2) < at(4), "{everything}");
    assert!(crate::since(&replica.0, r#"["not a name"]"#).is_err());
}

/// The phase register read: a merge and a close that each supersede the
/// move to review are both maximal, the one real conflict; under a policy
/// that does not stand behind the closer, the merge alone; before either,
/// the review. A review only opened has an unwritten phase.
#[test]
fn a_review_store_reads_its_phases() {
    let (replica, names) = replica();
    let phases = |at: Option<&str>, untrusted: &str| {
        answer(replica.readings(at.map(str::to_owned), untrusted))["readings"].clone()
    };
    let write = |i: usize, phase: &str| json!({ "hash": names[i].as_str(), "value": phase });
    let mut conflict = vec![write(3, "merged"), write(4, "closed")];
    conflict.sort_by_key(|w| w["hash"].as_str().expect("a name").to_owned());
    let row = |pr: &str, phase: Vec<Value>| json!({ "genesis": names[0].as_str(), "pr": pr, "registers": { "phase": phase } });
    assert_eq!(
        phases(None, "[]"),
        json!([row("pr-1", conflict), row("pr-2", vec![])])
    );
    assert_eq!(
        phases(None, r#"["bo"]"#)[0],
        row("pr-1", vec![write(3, "merged")])
    );
    assert_eq!(
        phases(Some("2026-10-02T12:00:00"), "[]")[0],
        row("pr-1", vec![write(2, "review")])
    );
}

/// A PAGE SEALS INTO ITS REPLICA. Events appended through the exports are
/// the objects a store holding the same objects writes, byte for byte, each
/// over the replica's own fold (the second move's dep is the first, which
/// only the page holds), and each object handed back hashes to its name;
/// every reading after them reads them, and appending one again writes
/// nothing and sends nothing. A move below the phase's reading is refused,
/// as the store refuses it.
#[test]
fn a_page_seals_into_its_replica() {
    use prodrome::event::canonical;
    use prodrome::store::{MemoryStore, Replica as _};

    let (mut replica, names) = replica();
    let (objects, _) = store();
    let store = MemoryStore::default();
    let prints: std::collections::BTreeMap<Hash, Vec<u8>> = objects
        .iter()
        .map(|object| (seal_hash(object), canonical_envelope(object).into_bytes()))
        .collect();
    store
        .receive(prints.keys().cloned().collect(), &|name| {
            prints.get(name).cloned()
        })
        .expect("receives");

    let mut sealed = Vec::new();
    for phase in [Phase::Review, Phase::Merged] {
        let event = schema::moved("pr-2", day(5), "bo", phase);
        let answer = answer(replica.append(&canonical(&event), None));
        let name = store.append(event).expect("appends");
        assert_eq!(answer["hash"], name.as_str());
        assert_eq!(answer["objects"][0]["hash"], name.as_str());
        let text = answer["objects"][0]["text"].as_str().expect("a print");
        assert_eq!(crate::name_of(text), name.as_str());
        assert_eq!(Some(text.as_bytes().to_vec()), store.print(&name));
        sealed.push((name, text.to_owned()));
    }
    let Ok(Envelope::Change(merged)) =
        prodrome::event::parse_envelope::<Review>(&Default::default(), &sealed[1].1)
    else {
        panic!("a change: {}", sealed[1].1);
    };
    assert_eq!(merged.deps, [sealed[0].0.clone()]);

    let read = answer(replica.readings(None, "[]"));
    assert_eq!(
        read["readings"][1]["registers"]["phase"],
        json!([{ "hash": sealed[1].0.as_str(), "value": "merged" }])
    );
    assert_eq!(answer(replica.verify())["ok"], json!(true));

    let again = schema::moved("pr-2", day(5), "bo", Phase::Merged);
    let genesis = Some(names[0].as_str().to_owned());
    assert_eq!(
        answer(replica.append(&canonical(&again), genesis)),
        json!({ "hash": sealed[1].0.as_str(), "objects": [] })
    );
    let back = schema::moved("pr-2", day(6), "bo", Phase::Draft);
    assert!(crate::append(&mut replica.0, &canonical(&back), None).is_err());
    assert!(store.append(back).is_err());
    assert!(
        crate::append(&mut replica.0, "Opened()", None).is_err(),
        "not an event"
    );
}

/// A DEVICE SIGNS WHAT IT WROTE. With a key registered to `ana` by the
/// root, her writes are unproven until her device signs them: `verify`
/// says so, `proven` names none, and a policy that requires signatures
/// reads every write a claim (`bo` has no key at all). Signing each of
/// ana's writes through the export seals a `Signed` into the replica and
/// hands it back to send, once; then they are proven, and the phase reads
/// her merge alone, bo's close still a claim.
#[test]
fn a_device_signs_what_it_wrote() {
    use prodrome::event::Actor;
    use prodrome::sign::{KeyAdded, Secret};

    let (device, root) = ("1".repeat(64), "9".repeat(64));
    let key = Reviews::public_key(&device).unwrap_or_else(|_| panic!("a seed"));
    let root_key = crate::public_key(&root).expect("a seed");
    let (mut objects, names) = store();
    let added = Envelope::KeyAdded(KeyAdded {
        genesis: names[0].clone(),
        actor: Actor::new("ana").expect("an actor"),
        key: prodrome::sign::PublicKey::new(key).expect("a key"),
    });
    let registered = Secret::new(&root).expect("a seed").sign(&seal_hash(&added));
    objects.extend([added, Envelope::Signed(registered)]);
    let sent = Value::Array(objects.iter().map(sent).collect()).to_string();
    let mut replica = Reviews::new(&sent).unwrap_or_else(|_| panic!("the objects are read"));
    let required = json!({ "untrusted": [], "roots": [root_key] }).to_string();
    let roots = Some(json!([root_key]).to_string());

    let verified = answer(replica.verify());
    assert_eq!(verified["ok"], json!(false), "ana's writes are unsigned");
    assert_eq!(answer(replica.proven(roots.clone()))["proven"], json!([]));
    let phases = |replica: &Reviews| answer(replica.readings(None, &required))["readings"].clone();
    assert_eq!(phases(&replica)[0]["registers"]["phase"], json!([]));

    for at in [1, 2, 3] {
        let signed = answer(replica.sign(&device, names[at].as_str()));
        let objects = signed["objects"].as_array().expect("objects");
        assert_eq!(objects.len(), 1, "the signature, to send");
        assert_eq!(objects[0]["hash"], signed["hash"]);
        let again = answer(replica.sign(&device, names[at].as_str()));
        assert_eq!(again["hash"], signed["hash"], "deterministic");
        assert_eq!(again["objects"], json!([]), "sent once");
    }
    let mut ana: Vec<&str> = [1, 2, 3].iter().map(|at| names[*at].as_str()).collect();
    ana.sort_unstable();
    let mut proven: Vec<String> =
        serde_json::from_value(answer(replica.proven(roots))["proven"].clone()).expect("names");
    proven.sort_unstable();
    assert_eq!(proven, ana);
    assert_eq!(answer(replica.verify())["ok"], json!(true));
    assert_eq!(
        phases(&replica)[0]["registers"]["phase"],
        json!([{ "hash": names[3].as_str(), "value": "merged" }])
    );
    assert!(
        crate::sign(&mut replica.0, &device, &"0".repeat(64)).is_err(),
        "not held"
    );
    assert!(crate::sign(&mut replica.0, "not a seed", names[1].as_str()).is_err());
}
