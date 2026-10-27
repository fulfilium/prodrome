//! The tag sets' two schemas (`prodrome::tags`) cross this boundary as any
//! schema does. Here and not in a host, which could not implement this
//! crate's trait for the core's types.
//!
//! A membership row names its entity by both fields of its key, the
//! target's and the tag's, each under the field name its tagging gives. Its
//! one register is `presence`, whose value is `"on"` or `"off"`. An alias row
//! names its entity by the tag. Its one register is `alias`, whose value is
//! the tag the name stands for, or `null` for a name of its own.

use prodrome::fold::sole::Only;
use prodrome::literal::codec::Literal as _;
use prodrome::tags::alias::{Alias, AliasEvent, AliasVocabulary};
use prodrome::tags::membership::{MembershipEvent, MembershipVocabulary, Pair, Presence};
use prodrome::tags::Tagging;
use serde_json::{json, Value};

use crate::wire::json_literal;
use crate::Json;

impl<S: Tagging> Json for MembershipEvent<S> {
    fn key_fields(_: &MembershipVocabulary<S>) -> Vec<&str> {
        vec![S::TARGET, S::TAG]
    }

    fn key_values(pair: &Pair<S>) -> Vec<Value> {
        vec![
            json_literal(&pair.target.to_value()),
            json_literal(&pair.tag.to_value()),
        ]
    }

    fn register(_: &MembershipVocabulary<S>, Only: Only) -> &str {
        "presence"
    }

    fn value(&self, Only: Only) -> Value {
        match self.body().presence() {
            Presence::On => json!("on"),
            Presence::Off => json!("off"),
        }
    }
}

impl<S: Tagging> Json for AliasEvent<S> {
    fn key_fields(_: &AliasVocabulary<S>) -> Vec<&str> {
        vec![S::TAG]
    }

    fn key_values(tag: &S::Tag) -> Vec<Value> {
        vec![json_literal(&tag.to_value())]
    }

    fn register(_: &AliasVocabulary<S>, Only: Only) -> &str {
        "alias"
    }

    fn value(&self, Only: Only) -> Value {
        match self.body() {
            Alias::Itself => Value::Null,
            Alias::As(to) => json_literal(&to.to_value()),
        }
    }
}

#[cfg(test)]
mod tests {
    use prodrome::change::mk_change;
    use prodrome::event::{canonical_envelope, seal_hash, Actor, Envelope, Hash};
    use prodrome::genesis::mk_genesis;
    use prodrome::literal::Datetime;
    use prodrome::tags::membership;
    use prodrome::tags::Tagging;
    use serde_json::{json, Value};

    /// A review's name.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Pr(String);

    /// A label put on a review.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Label(String);

    impl From<String> for Pr {
        fn from(text: String) -> Self {
            Pr(text)
        }
    }

    impl From<String> for Label {
        fn from(text: String) -> Self {
            Label(text)
        }
    }

    prodrome::text_literal!(Pr => |pr| pr.0.clone(), Label => |label| label.0.clone());

    /// Reviews tagged with labels: a schema whose key is a pair.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Labels;

    impl Tagging for Labels {
        type Target = Pr;
        type Tag = Label;
        const TARGET: &'static str = "pr";
        const TAG: &'static str = "label";
    }

    type Labelled = membership::MembershipEvent<Labels>;

    crate::schema!(ReviewLabels = Labelled);

    fn day(day: u32) -> Datetime {
        Datetime::new(2026, 10, day, 9, 0, 0, 0).expect("an instant")
    }

    fn ada() -> Actor {
        Actor::new("ada").expect("an actor")
    }

    fn sent(object: &Envelope<Labelled>) -> Value {
        json!({ "hash": seal_hash(object).as_str(), "text": canonical_envelope(object) })
    }

    /// A genesis; on `pr-1`, the label `bug` put on and taken off; and on
    /// `pr-1` and `pr-2`, the label `docs` put on.
    fn store() -> (Vec<Envelope<Labelled>>, Vec<Hash>) {
        let genesis = Envelope::Genesis(mk_genesis("labels", &"0".repeat(32)).expect("a genesis"));
        let root = seal_hash(&genesis);
        let change = |deps: &[&Hash], event| {
            let deps = deps.iter().map(|dep| (*dep).clone()).collect();
            Envelope::Change(mk_change(root.clone(), deps, event).expect("a change"))
        };
        let pr = |name: &str| Pr(name.to_owned());
        let label = |name: &str| Label(name.to_owned());
        let bug = change(
            &[],
            membership::added(pr("pr-1"), label("bug"), day(1), ada()),
        );
        let fixed = change(
            &[&seal_hash(&bug)],
            membership::removed(pr("pr-1"), label("bug"), day(2), ada()),
        );
        let docs = change(
            &[],
            membership::added(pr("pr-1"), label("docs"), day(3), ada()),
        );
        let other = change(
            &[],
            membership::added(pr("pr-2"), label("docs"), day(4), ada()),
        );
        let objects = vec![genesis, bug, fixed, docs, other];
        let names = objects.iter().map(seal_hash).collect();
        (objects, names)
    }

    fn replica() -> (ReviewLabels, Vec<Hash>) {
        let (objects, names) = store();
        let sent = Value::Array(objects.iter().map(sent).collect()).to_string();
        let replica = ReviewLabels::new(&sent).unwrap_or_else(|_| panic!("the objects are read"));
        (replica, names)
    }

    fn answer(text: Result<String, wasm_bindgen::JsError>) -> Value {
        serde_json::from_str(&text.unwrap_or_else(|_| panic!("the export refused"))).expect("JSON")
    }

    /// `verify` at a pair-keyed schema: every row of an event names its
    /// entity by both key fields, and the genesis's row has both empty.
    #[test]
    fn verify_names_each_row_by_both_fields_of_its_key() {
        let (replica, names) = replica();
        let verified = answer(replica.verify());
        assert_eq!(verified["ok"], json!(true), "{verified}");
        let rows = verified["objects"].as_array().expect("rows");
        let row = |name: &Hash| {
            rows.iter()
                .find(|row| row["hash"] == json!(name.as_str()))
                .expect("a row")
        };
        assert_eq!(row(&names[0])["pr"], json!(""));
        assert_eq!(row(&names[0])["label"], json!(""));
        assert_eq!(row(&names[2])["pr"], json!("pr-1"));
        assert_eq!(row(&names[2])["label"], json!("bug"));
        assert_eq!(row(&names[2])["kind"], json!("Removed"));
        assert_eq!(row(&names[4])["pr"], json!("pr-2"));
        assert_eq!(row(&names[4])["label"], json!("docs"));
    }

    /// `readings` at a pair-keyed schema: one row for each pair, named by
    /// both fields, whose `presence` reads its frontier's value.
    #[test]
    fn readings_name_each_pair_and_read_its_presence() {
        let (replica, names) = replica();
        let readings = answer(replica.readings(None, "[]"));
        let rows = readings["readings"].as_array().expect("rows");
        let pairs: Vec<(Value, Value, Value)> = rows
            .iter()
            .map(|row| {
                (
                    row["pr"].clone(),
                    row["label"].clone(),
                    row["registers"]["presence"].clone(),
                )
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                (
                    json!("pr-1"),
                    json!("bug"),
                    json!([{ "hash": names[2].as_str(), "value": "off" }])
                ),
                (
                    json!("pr-1"),
                    json!("docs"),
                    json!([{ "hash": names[3].as_str(), "value": "on" }])
                ),
                (
                    json!("pr-2"),
                    json!("docs"),
                    json!([{ "hash": names[4].as_str(), "value": "on" }])
                ),
            ]
        );
    }

    /// `append` at a pair-keyed schema seals an event over its pair's
    /// heads, and `since` answers what a replica holding the genesis
    /// lacks.
    #[test]
    fn append_and_since_run_at_a_pair_keyed_schema() {
        let (mut replica, names) = replica();
        let event = prodrome::event::canonical(&membership::added::<Labels>(
            Pr("pr-2".to_owned()),
            Label("bug".to_owned()),
            day(5),
            ada(),
        ));
        let appended = answer(replica.append(&event, None));
        assert_eq!(appended["objects"].as_array().map(Vec::len), Some(1));
        let since = answer(replica.since(&json!([names[0].as_str()]).to_string()));
        assert_eq!(since["since"].as_array().map(Vec::len), Some(5), "{since}");
    }
}
