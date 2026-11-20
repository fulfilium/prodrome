//! A replica that holds a husk (SPEC §3) reads, appends and signs. The page
//! that sends a replica its objects is its keeper of record, and a write
//! receives the replica's own history as its keeper's (law 73's replicas,
//! on the path a page takes).

#[path = "../../../core/tests/schemas/letter.rs"]
#[allow(dead_code)]
mod letter;

use prodrome::event::{canonical, canonical_envelope, seal_hash, Actor, Envelope};
use prodrome::genesis::mk_genesis;
use prodrome::literal::Datetime;
use prodrome::store::{HuskObject, MemoryStore, Replica as _, Sender};
use serde_json::{json, Value};

use letter::{Id, Letter, LetterVocabulary, Part, Says};

fn wrote(minute: u32, part: Part) -> Letter {
    Letter {
        letter: Id("l0".to_owned()),
        at: Datetime::new(2026, 10, 1, 9, minute, 0, 0).expect("an instant"),
        actor: Actor::new("ada").expect("an actor"),
        says: Says::Wrote(vec![part]),
    }
}

/// A page sends a genesis and the husk of a change, and the replica reads
/// the husk under the whole's name, appends a change over it, and signs it.
#[test]
fn a_replica_that_holds_a_husk_appends_and_signs() {
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
    let mut replica = crate::Replica::<Letter>::at(LetterVocabulary, &objects).expect("the shape");
    let read = replica.read().expect("reads");
    assert!(read.dag.husks().contains(&name), "the page's husk is held");
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
    crate::sign(&mut replica, &"1".repeat(64), name.as_str()).expect("signs the husk's name");
}
