//! A page's replica and a husk (SPEC §3). From a stranger, the husk is left
//! out as any print that fails its hash. From the keeper of record, the
//! replica holds it, says so in `verify`, and appends over it, since a
//! write receives the replica's own history as its keeper's (law 73's
//! replicas, on the path a page takes). It never signs the husk's name
//! (law 80).

#[path = "../../../core/tests/schemas/letter.rs"]
#[allow(dead_code)]
mod letter;

use prodrome::event::{canonical, canonical_envelope, seal_hash, Actor, Envelope};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::store::{HuskObject, MemoryStore, Replica as _, Sender};
use serde_json::{json, Value};

use letter::{Field, Id, Letter, LetterVocabulary, Part, Says};

impl crate::Json for Letter {
    fn key_fields(_: &LetterVocabulary) -> Vec<&str> {
        vec!["letter"]
    }

    fn key_values(letter: &Id) -> Vec<Value> {
        vec![json!(letter.0)]
    }

    fn register(_: &LetterVocabulary, field: Field) -> &str {
        match field {
            Field::Body => "body",
            Field::Title => "title",
        }
    }

    fn value(&self, _: Field) -> Value {
        Value::Null
    }
}

fn wrote(minute: u32, part: Part) -> Letter {
    Letter {
        letter: Id("l0".to_owned()),
        at: Datetime::new(2026, 10, 1, 9, minute, 0, 0).expect("an instant"),
        actor: Actor::new("ada").expect("an actor"),
        says: Says::Wrote(vec![part]),
    }
}

/// A page sends a genesis and the husk of a change. As a stranger's, the
/// husk is left out. As the keeper's, the replica reads the husk under the
/// whole's name, marks its row, appends a change over it, signs the genesis
/// it holds whole, and refuses to sign the husk's name.
#[test]
fn a_replica_reads_a_keepers_husk_and_never_signs_it() {
    let genesis: Envelope<Letter> =
        Envelope::Genesis(mk_genesis("letters", &"0".repeat(32)).expect("a genesis"));
    let genesis_name = seal_hash(&genesis);
    let genesis_print = canonical_envelope(&genesis);
    let store = MemoryStore::<Letter>::default();
    store
        .receive(
            [genesis_name.clone()].into(),
            &|_| Some(genesis_print.clone().into_bytes()),
            Sender::Stranger,
        )
        .expect("begins");
    let sealed = Part::Boxed {
        key: "k0".to_owned(),
        sealed: "00ff".to_owned(),
    };
    let name = store.append(wrote(0, sealed)).expect("appends");
    let Some(Envelope::Change(change)) = store.held().expect("reads").dag.get(&name).cloned()
    else {
        panic!("a change");
    };
    let husk = HuskObject::of(&change).expect("a box to drop").print();
    let objects = json!([
        {"hash": genesis_name.as_str(), "text": genesis_print},
        {"hash": name.as_str(), "text": husk},
    ])
    .to_string();

    let stranger = crate::Replica::<Letter>::at(LetterVocabulary, &objects, Sender::Stranger)
        .expect("the shape");
    let read = stranger.read().expect("reads");
    assert!(
        read.dag.get(&name).is_none(),
        "a stranger's husk is left out"
    );
    assert!(read.dag.husks().is_empty());

    let mut replica = crate::Replica::<Letter>::at(LetterVocabulary, &objects, Sender::Keeper)
        .expect("the shape");
    let read = replica.read().expect("reads");
    assert!(
        read.dag.husks().contains(&name),
        "the keeper's husk is held"
    );
    let verified: Value =
        serde_json::from_str(&crate::verify(&replica).expect("verifies")).expect("json");
    let row = verified["objects"]
        .as_array()
        .expect("rows")
        .iter()
        .find(|row| row["hash"] == name.as_str())
        .expect("the husk's row");
    assert_eq!(row["husk"], json!(true));
    assert_eq!(row["hashOk"], json!(false));
    let appended: Value = serde_json::from_str(
        &crate::append(
            &mut replica,
            &canonical(&wrote(1, Part::Clear("next".to_owned()))),
            None,
        )
        .expect("appends over a husk"),
    )
    .expect("json");
    assert!(appended["hash"].is_string());
    let refused = crate::sign(&mut replica, &"1".repeat(64), name.as_str())
        .expect_err("a husk's name is never signed");
    assert!(refused.contains("husk"), "{refused}");
    crate::sign(&mut replica, &"1".repeat(64), genesis_name.as_str())
        .expect("signs a whole it holds");
}
