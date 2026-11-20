//! A schema DECLARED AS DATA through [`crate::schema!`]'s `declared` form:
//! the core tests' proposal schema (`core/tests/schemas/proposal.rs`), its
//! text given at construction, read and appended to over a small store.

use prodrome::change::mk_change;
use prodrome::declared::{Declaration, Declared};
use prodrome::event::{canonical, canonical_envelope, seal_hash, Actor, Envelope};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::schema::Schema;
use serde_json::{json, Value};

// The schema's whole surface is the core tests'; this reads part of it.
#[allow(dead_code)]
#[path = "../../../core/tests/schemas/proposal.rs"]
pub(crate) mod schema;

use schema::{Id, Proposal, Says, State, DECLARED};

crate::schema!(Proposals, declared);

fn day(day: u32) -> Datetime {
    Datetime::new(2026, 10, day, 12, 0, 0, 0).expect("a real instant")
}

fn event(says: Says, d: u32, actor: &str) -> Declared {
    let schema = Declaration::admit(DECLARED).expect("lawful");
    let proposal = Proposal {
        proposal: Id("p-1".to_owned()),
        at: day(d),
        actor: Actor::new(actor).expect("an actor"),
        says,
    };
    Declared::from_value(&schema, &proposal.to_value()).expect("a declared event")
}

fn answer(text: Result<String, wasm_bindgen::JsError>) -> Value {
    serde_json::from_str(&text.unwrap_or_else(|_| panic!("the export refused"))).expect("JSON")
}

/// A genesis, a proposal standing, then on two devices a send and a
/// dismissal (the one real conflict), and tags written as a list whose set
/// is what reads.
#[test]
fn a_declared_store_reads_and_appends_through_the_exports() {
    let genesis = Envelope::Genesis(mk_genesis("inbox", &"0".repeat(32)).expect("a genesis"));
    let root = seal_hash(&genesis);
    let change = |deps: Vec<_>, event| {
        Envelope::Change(mk_change(root.clone(), deps, event).expect("a change"))
    };
    let standing = change(vec![], event(Says::Moved(State::Standing), 1, "ana"));
    let under = seal_hash(&standing);
    let sent = change(
        vec![under.clone()],
        event(Says::Moved(State::Sent), 2, "ana"),
    );
    let dismissed = change(vec![under], event(Says::Moved(State::Dismissed), 2, "bo"));
    let tags = change(
        vec![],
        event(
            Says::Tagged(vec!["b".to_owned(), "a".to_owned(), "b".to_owned()]),
            1,
            "ana",
        ),
    );
    let objects: Vec<Value> = [&genesis, &standing, &sent, &dismissed, &tags]
        .iter()
        .map(|o| json!({ "hash": seal_hash(o).as_str(), "text": canonical_envelope(o) }))
        .collect();
    let objects = Value::Array(objects).to_string();

    let refused = crate::Replica::declared(
        &DECLARED.replace("True", "False").replacen(
            "Inclusion(), inflationary=False",
            "Total(), inflationary=False",
            1,
        ),
        &objects,
        crate::Sender::Stranger,
    );
    let Err(refusal) = refused else {
        panic!("a total order over a list is ill-typed")
    };
    assert!(refusal.contains("typed"), "{refusal}");

    let mut replica = Proposals::new(DECLARED, &objects).unwrap_or_else(|_| panic!("admitted"));
    let declaration = answer(replica.declaration());
    assert_eq!(declaration["key"], "proposal");
    assert_eq!(
        declaration["registers"],
        json!(["attempts", "state", "tags", "text"])
    );
    assert_eq!(answer(replica.verify())["ok"], json!(true));

    let read = answer(replica.readings(None, "[]"));
    let row = &read["readings"][0];
    assert_eq!(row["proposal"], "p-1");
    let mut conflict = vec![
        json!({ "hash": seal_hash(&dismissed).as_str(), "value": "dismissed" }),
        json!({ "hash": seal_hash(&sent).as_str(), "value": "sent" }),
    ];
    conflict.sort_by_key(|w| w["hash"].as_str().expect("a name").to_owned());
    assert_eq!(row["registers"]["state"], json!(conflict));
    assert_eq!(
        row["registers"]["tags"],
        json!([{ "hash": seal_hash(&tags).as_str(), "value": ["a", "b"] }])
    );
    assert_eq!(row["registers"]["attempts"], json!([]));

    // An inflationary machine whose two maximal states have no upper bound
    // keeps its conflict: a write must climb above both, and none can.
    let resent = event(Says::Moved(State::Sent), 3, "ana");
    assert!(crate::append(&mut replica.0, &canonical(&resent), None).is_err());
    // A register that is not inflationary takes any write.
    let attempt = event(Says::Attempted(2), 3, "ana");
    let appended = answer(replica.append(&canonical(&attempt), None));
    assert_eq!(appended["objects"].as_array().map(Vec::len), Some(1));
    let read = answer(replica.readings(None, "[]"));
    assert_eq!(
        read["readings"][0]["registers"]["attempts"],
        json!([{ "hash": appended["hash"], "value": 2 }])
    );
    assert_eq!(read["readings"][0]["registers"]["state"], json!(conflict));
}
