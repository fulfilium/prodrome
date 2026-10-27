//! A READING IS TOTAL OVER WHAT A REPLICA HOLDS: a print that is not an
//! object at the schema a replica reads leaves out itself and everything
//! resting on it, every other object reads as it would had the print never
//! arrived, and every reading answers what it left out (SPEC law 44).
//!
//! The unreadable prints are the real case: histories of the core tests'
//! proposal schema, drawn by the core's own generators, read at a NEWER
//! schema whose `Tagged` has gained a field, so every `Tagged` written
//! before fails to parse, as a store migrated away from does.

use std::collections::BTreeSet;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use prodrome::dag::{Dag, Finding};
use prodrome::declared::{Declaration, Declared};
use prodrome::event::{canonical_envelope, Envelope, Hash};
use prodrome::literal::Value as Literal;
use prodrome::policy::Everything;
use prodrome::schema::Schema;
use prodrome::store::EventStore;
use proptest::prelude::*;
use serde_json::{json, Value};

use crate::declared::schema;
use crate::Replica;

#[allow(dead_code)]
#[path = "../../../core/tests/common/draw.rs"]
mod draw;

#[path = "../../../core/tests/common/cases.rs"]
mod cases;

use draw::{a_draw, history, Draw};
use schema::{a_proposal, Proposal, DECLARED};

/// The proposal schema as written.
fn written() -> &'static Declaration {
    static ADMITTED: OnceLock<Declaration> = OnceLock::new();
    ADMITTED.get_or_init(|| Declaration::admit(DECLARED).expect("the proposal schema is lawful"))
}

/// The proposal schema after a migration: `Tagged` says why, a field no
/// `Tagged` written before has.
fn migrated() -> &'static Declaration {
    static ADMITTED: OnceLock<Declaration> = OnceLock::new();
    ADMITTED.get_or_init(|| {
        let text = DECLARED.replacen(
            "Field(name='tags', type=List(item=Text())))",
            "Field(name='tags', type=List(item=Text())), Field(name='why', type=Text()))",
            1,
        );
        assert_ne!(text, DECLARED, "the migration changes the schema");
        Declaration::admit(&text).expect("the migrated schema is lawful")
    })
}

/// A draw's objects, written at the proposal schema as written.
fn objects(draw: &Draw<Proposal>) -> Vec<(Hash, Envelope<Declared>)> {
    let events: Vec<(Declared, u64)> = draw
        .events
        .iter()
        .map(|(event, bits)| {
            let event = Declared::from_value(written(), &event.to_value())
                .expect("a proposal is a declared one");
            (event, *bits)
        })
        .collect();
    history(&events)
}

/// `objects` as a page sends them, read at the migrated schema.
fn replica(objects: &[&(Hash, Envelope<Declared>)]) -> Replica<Declared> {
    let sent: Vec<Value> = objects
        .iter()
        .map(|(name, object)| json!({ "hash": name.as_str(), "text": canonical_envelope(object) }))
        .collect();
    Replica::at(migrated().clone(), &Value::Array(sent).to_string()).expect("the shape is right")
}

fn answer(text: Result<String, String>) -> Value {
    serde_json::from_str(&text.unwrap_or_else(|refusal| panic!("refused: {refusal}")))
        .expect("JSON")
}

/// An answer less what it says it left out.
fn read(mut answer: Value) -> (Value, Value) {
    let excluded = answer
        .as_object_mut()
        .and_then(|fields| fields.remove("excluded"))
        .expect("every reading answers what it left out");
    (answer, excluded)
}

fn names(value: &Value) -> BTreeSet<Hash> {
    value
        .as_array()
        .expect("names")
        .iter()
        .map(|name| Hash::new(name.as_str().expect("a name")).expect("a name"))
        .collect()
}

/// The objects carrying a `Tagged`: the ones the migration cannot read.
fn tagged(objects: &[(Hash, Envelope<Declared>)]) -> BTreeSet<Hash> {
    objects
        .iter()
        .filter(|(_, object)| {
            object.event().is_some_and(
                |event| matches!(event.to_value(), Literal::Call(call) if call.name == "Tagged"),
            )
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// Every reading of `replica`, each less what it left out, and that.
fn readings(replica: &Replica<Declared>) -> Vec<(Value, Value)> {
    vec![
        read(answer(crate::readings(replica, None, "[]"))),
        read(answer(crate::tips(replica))),
        read(answer(crate::since(replica, "[]"))),
        read(answer(crate::proven(replica, None))),
    ]
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

proptest! {
    #![proptest_config(cases::cases(64))]

    /// Adding unreadable prints, and what rests on them, to a held set
    /// changes no reading of the objects that do not rest on them: the
    /// whole set reads as the down-set without them, which leaves nothing
    /// out, in whatever order the prints arrive.
    #[test]
    fn an_unreadable_print_changes_no_reading_of_what_does_not_rest_on_it(
        draw in a_draw(a_proposal())
    ) {
        let objects = objects(&draw);
        let whole: Dag<Declared> = objects.iter().cloned().collect();
        let above = whole.above(tagged(&objects));
        let down: Vec<_> = objects.iter().filter(|(name, _)| !above.contains(name)).collect();
        let all: Vec<_> = objects.iter().collect();
        let (without, with) = (readings(&replica(&down)), readings(&replica(&all)));
        for ((alone, nothing), (held, _)) in without.iter().zip(&with) {
            prop_assert_eq!(nothing, &json!([]), "the down-set leaves nothing out");
            prop_assert_eq!(alone, held);
        }
        // A function of the set: sent in the other order, every answer,
        // what it left out among it, is the same.
        let reversed: Vec<_> = objects.iter().rev().collect();
        prop_assert_eq!(readings(&replica(&reversed)), with, "whatever order they arrive in");
    }

    /// What every reading says it left out names exactly the unreadable
    /// prints and the objects resting on them: each print once, with the
    /// parse's refusal, and only objects above it.
    #[test]
    fn what_is_left_out_is_exactly_the_unreadable_and_what_rests_on_them(
        draw in a_draw(a_proposal())
    ) {
        let objects = objects(&draw);
        let whole: Dag<Declared> = objects.iter().cloned().collect();
        let unreadable = tagged(&objects);
        let replica = replica(&objects.iter().collect::<Vec<_>>());
        let excluded = readings(&replica)[0].1.clone();
        let mut named = BTreeSet::new();
        let mut left_out = BTreeSet::new();
        for out in excluded.as_array().expect("a list") {
            let name = Hash::new(out["name"].as_str().expect("a name")).expect("a name");
            let why = out["why"].as_str().expect("why");
            prop_assert!(why.starts_with("failed to parse: Tagged"), "{}", why);
            let resting = names(&out["resting"]);
            let above: BTreeSet<Hash> = whole.above([name.clone()]);
            prop_assert!(resting.is_subset(&above), "only what rests on {}", name);
            prop_assert!(resting.is_disjoint(&unreadable), "an unreadable print is no object");
            left_out.extend(resting);
            named.insert(name);
        }
        prop_assert_eq!(&named, &unreadable, "each unreadable print, named");
        left_out.extend(named);
        prop_assert_eq!(left_out, whole.above(unreadable), "and everything above them");
        for (_, out) in readings(&replica) {
            prop_assert_eq!(&out, &excluded, "every reading says the same");
        }
    }

    /// A store and a page agree: for the same bytes, a store on disk at
    /// the migrated schema reports as unread exactly the prints the page
    /// leaves out, in the same words, and as broken exactly those something
    /// rests on.
    #[test]
    fn a_store_and_a_page_name_the_same_unreadable_objects(draw in a_draw(a_proposal())) {
        let objects = objects(&draw);
        let root = std::env::temp_dir().join(format!(
            "prodrome-unread-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("objects")).expect("creates objects/");
        for (name, object) in &objects {
            let file = root.join("objects").join(format!("{}.py", name.as_str()));
            fs::write(file, canonical_envelope(object)).expect("writes");
        }
        let store = EventStore::<Declared, Everything>::at(&root, Everything, migrated().clone());
        let report = store.verify();
        fs::remove_dir_all(&root).expect("removes");
        let unread: BTreeSet<String> = report
            .iter()
            .filter(|finding| matches!(finding, Finding::Unread { .. }))
            .map(ToString::to_string)
            .collect();
        let broken: BTreeSet<Hash> = report
            .iter()
            .filter_map(|finding| match finding {
                Finding::Broken { at, .. } => Some(at.clone()),
                _ => None,
            })
            .collect();
        let replica = replica(&objects.iter().collect::<Vec<_>>());
        let excluded = readings(&replica)[0].1.clone();
        let page: BTreeSet<String> = excluded
            .as_array()
            .expect("a list")
            .iter()
            .map(|out| format!(
                "object {} {}",
                out["name"].as_str().expect("a name"),
                out["why"].as_str().expect("why")
            ))
            .collect();
        prop_assert_eq!(unread, page, "one reading of unreadable");
        let rested_on: BTreeSet<Hash> = excluded
            .as_array()
            .expect("a list")
            .iter()
            .filter(|out| out["resting"].as_array().is_some_and(|resting| !resting.is_empty()))
            .map(|out| Hash::new(out["name"].as_str().expect("a name")).expect("a name"))
            .collect();
        prop_assert_eq!(broken, rested_on);
    }
}

/// After an append, what was left out is still said, and the append rests
/// on nothing left out; `verify` still reports every print.
#[test]
fn an_append_keeps_what_was_left_out() {
    use schema::{Id, Says};
    let day = |d| prodrome::literal::Datetime::new(2026, 10, d, 12, 0, 0, 0).expect("an instant");
    let proposal = |says, d| Proposal {
        proposal: Id("p-1".to_owned()),
        at: day(d),
        actor: prodrome::event::Actor::new("ana").expect("an actor"),
        says,
    };
    let draw = Draw {
        events: vec![
            (proposal(Says::Proposed("a".to_owned()), 1), 0),
            (proposal(Says::Tagged(vec!["x".to_owned()]), 2), 0b1),
            (proposal(Says::Attempted(1), 3), 0b10),
        ],
        shuffle: prodrome::laws::Shuffle::default(),
    };
    let objects = objects(&draw);
    let mut replica = replica(&objects.iter().collect::<Vec<_>>());
    let before = readings(&replica)[0].1.clone();
    assert_eq!(before.as_array().map(Vec::len), Some(1));
    assert_eq!(before[0]["name"], json!(objects[2].0.as_str()));
    assert_eq!(before[0]["resting"], json!([objects[3].0.as_str()]));
    let event = Declared::from_value(migrated(), &proposal(Says::Attempted(2), 4).to_value())
        .expect("an attempt is an event of the migrated schema too");
    let appended = answer(crate::append(
        &mut replica,
        &prodrome::event::canonical(&event),
        None,
    ));
    let (_, after) = readings(&replica)[0].clone();
    assert_eq!(after, before, "what was left out is still said");
    let row = &answer(crate::readings(&replica, None, "[]"))["readings"][0];
    assert_eq!(
        row["registers"]["attempts"],
        json!([{ "hash": appended["hash"], "value": 2 }]),
        "the append supersedes what the history holds, and nothing left out"
    );
    let verified = answer(crate::verify(&replica));
    let problems = verified["problems"].to_string();
    assert!(problems.contains(objects[2].0.as_str()), "{problems}");
    assert_eq!(
        verified["objects"].as_array().map(Vec::len),
        Some(objects.len() + 1)
    );
}
