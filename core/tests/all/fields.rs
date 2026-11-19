//! Law 69: [`Fields`] reads with the reads of a derived impl, and it
//! accepts exactly the values that it accepted when it worded its own
//! refusals.
//!
//! The module `before` holds `Fields` and `no_arm` as they were before
//! `Fields` was built on `codec::read`. Each property reads one value with
//! both and compares the results: both accept the value and read the same
//! value, or both refuse it. Only the words of a refusal may differ.
//!
//! The properties compare the parts of `Fields` one by one, over every
//! field type that a reader of this repository reads with it. They also
//! compare four whole readers of this repository, written as they were,
//! over their events changed in up to four places.

use std::collections::BTreeSet;
use std::fmt::Debug;

use chrono::NaiveDate;
use prodrome::dag::Heads;
use prodrome::event::{Actor, Hash, TodoId};
use prodrome::literal::codec::{Constructor, Fields, Literal};
use prodrome::literal::{Datetime, ProdromeError, Value};
use prodrome::schema::headed::Constructors;
use prodrome::schema::Schema;
use prodrome::tags::alias::{aliased, unaliased, AliasEvent};
use prodrome::tags::membership::{added, removed, MembershipEvent};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use crate::derive::{a_change, a_datetime, a_hash, a_leaf, changed, Change};
use crate::note::{by_ada, Entry, Note};
use crate::tags::{label, note, Label, Labels, LABELS};

/// `Fields` and `no_arm` as they were, with their own words.
mod before {
    use prodrome::literal::codec::{arm, Constructor, Literal};
    use prodrome::literal::{Call, ProdromeError, Value};

    pub struct Fields<'v> {
        call: &'v Call,
    }

    impl<'v> Fields<'v> {
        pub fn among(
            value: &'v Value,
            constructors: &[Constructor],
        ) -> Result<Self, ProdromeError> {
            let name = arm(value)?;
            let constructor = constructors
                .iter()
                .find(|(named, _)| *named == name)
                .ok_or_else(|| {
                    ProdromeError::invalid(format!("{name} is not one of this schema's"))
                })?;
            Self::of(value, *constructor)
        }

        pub fn of(value: &'v Value, constructor: Constructor) -> Result<Self, ProdromeError> {
            match value.as_call() {
                Some(call) if call.name == constructor.0 => Ok(Self { call }),
                _ => Err(ProdromeError::invalid(format!(
                    "expected {}(...), got {value:?}",
                    constructor.0
                ))),
            }
        }

        pub fn name(&self) -> &'v str {
            &self.call.name
        }

        pub fn get<T: Literal>(&self, name: &str) -> Result<T, ProdromeError> {
            let value = self.call.field(name).ok_or_else(|| {
                ProdromeError::invalid(format!("{}(...) has no field {name}", self.call.name))
            })?;
            T::from_value(value).map_err(|failed| {
                ProdromeError::invalid(format!("{}.{name}: {failed}", self.call.name))
            })
        }
    }

    pub fn no_arm(what: &str, name: &str) -> ProdromeError {
        ProdromeError::invalid(format!("{name} is not {what}"))
    }
}

/// The readers of this repository that read through `Fields`, as they were.
mod readers {
    use std::collections::BTreeSet;

    use prodrome::event::Hash;
    use prodrome::literal::{ProdromeError, Value};
    use prodrome::schema::headed::{Body, Event};
    use prodrome::tags::alias::{Alias, AliasEvent};
    use prodrome::tags::membership::{added, removed, Membership, MembershipEvent};

    use super::before::{no_arm, Fields};
    use crate::note::{Entry, Note};
    use crate::tags::Labels;

    /// `dag::Heads`, as its tips and its count.
    pub fn heads(value: &Value) -> Result<(BTreeSet<Hash>, usize), ProdromeError> {
        let fields = Fields::of(value, ("Heads", &["count", "tips"]))?;
        Ok((fields.get("tips")?, fields.get("count")?))
    }

    /// The note schema of `schema/headed/example.rs`.
    pub fn note(value: &Value) -> Result<Note, ProdromeError> {
        let fields = Fields::among(value, Entry::EVENTS)?;
        let entry = match fields.name() {
            "Titled" => Entry::Titled(fields.get("title")?),
            "Viewed" => Entry::Viewed,
            other => return Err(no_arm("a note event", other)),
        };
        Event::new(
            fields.get("note")?,
            fields.get("at")?,
            fields.get("actor")?,
            entry,
        )
        .map_err(|refused| ProdromeError::invalid(refused.to_string()))
    }

    /// The membership schema of `tags`, over notes tagged with labels.
    pub fn membership(value: &Value) -> Result<MembershipEvent<Labels>, ProdromeError> {
        let fields = Fields::among(value, Membership::<Labels>::EVENTS)?;
        let write = match fields.name() {
            "Added" => added::<Labels>,
            "Removed" => removed::<Labels>,
            other => return Err(no_arm("a membership event", other)),
        };
        Ok(write(
            fields.get("note")?,
            fields.get("label")?,
            fields.get("at")?,
            fields.get("actor")?,
        ))
    }

    /// The alias schema of `tags`, over labels.
    pub fn alias(value: &Value) -> Result<AliasEvent<Labels>, ProdromeError> {
        let fields = Fields::among(value, Alias::<Labels>::EVENTS)?;
        let alias = match fields.name() {
            "Aliased" => Alias::As(fields.get("to")?),
            "Unaliased" => Alias::Itself,
            other => return Err(no_arm("an alias event", other)),
        };
        Event::new(
            fields.get("label")?,
            fields.get("at")?,
            fields.get("actor")?,
            alias,
        )
        .map_err(|refused| ProdromeError::invalid(refused.to_string()))
    }
}

/// Both reads accept and give the same value, or both refuse.
fn agree<T: PartialEq + Debug>(
    before: Result<T, ProdromeError>,
    after: Result<T, ProdromeError>,
) -> Result<(), TestCaseError> {
    match (before, after) {
        (Ok(before), Ok(after)) => prop_assert_eq!(before, after),
        (Err(_), Err(_)) => {}
        (before, after) => prop_assert!(false, "before {before:?}, after {after:?}"),
    }
    Ok(())
}

/// A refusal of the field `f` of a call of `C` names the field: as `C.f`,
/// or as the field that the call lacks.
fn names_the_field<T>(read: &Result<T, ProdromeError>) -> Result<(), TestCaseError> {
    if let Err(refused) = read {
        let message = refused.to_string();
        prop_assert!(
            message.starts_with("C.f") || message == "C(...) is missing f",
            "{message}"
        );
    }
    Ok(())
}

/// A derived sum, for a field whose type is a derived impl.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
enum Shape {
    Opened { title: String },
    Paused,
}

/// A derived record that holds a set, for a field whose type is one.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
struct Opening {
    at: Datetime,
    tips: BTreeSet<Hash>,
}

/// Compares `get` of the field `f` of `call` as each type that a reader
/// of this repository reads with `Fields`, and as the derived types above.
fn every_get_agrees(call: &Value) -> Result<(), TestCaseError> {
    let fail = |error: ProdromeError| TestCaseError::fail(error.to_string());
    let before = before::Fields::of(call, ("C", &["f"])).map_err(fail)?;
    let after = Fields::of(call, ("C", &["f"])).map_err(fail)?;
    macro_rules! each {
        ($($ty:ty),+ $(,)?) => {$(
            let read = after.get::<$ty>("f");
            names_the_field(&read)?;
            agree(before.get::<$ty>("f"), read)?;
        )+};
    }
    each!(
        String,
        bool,
        Datetime,
        NaiveDate,
        u32,
        u64,
        usize,
        Actor,
        TodoId,
        Hash,
        Label,
        crate::tags::Note,
        Option<String>,
        Option<Hash>,
        Vec<String>,
        Vec<Hash>,
        BTreeSet<Hash>,
        Shape,
        Opening,
        Option<Shape>,
        Vec<Opening>,
    );
    Ok(())
}

/// A value of a field: a leaf, a tuple of leaves, or a call.
fn a_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        a_leaf(),
        Just(Value::str("2026-10-01")),
        Just(Value::str("2026-02-30")),
        Just(Value::str("red")),
        Just(Value::str("n1")),
        Just(Value::str("ada")),
        Just(Value::int(i64::from(u32::MAX) + 1)),
    ];
    leaf.prop_recursive(2, 12, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..3).prop_map(Value::Tuple),
            (
                prop::sample::select(vec!["Opened", "Paused", "Opening", "C"]),
                prop::collection::vec(
                    (
                        prop::sample::select(vec!["title", "at", "tips", "f"]),
                        inner
                    ),
                    0..3
                ),
            )
                .prop_map(|(name, fields)| Value::call(
                    name,
                    fields
                        .into_iter()
                        .map(|(field, value)| (field.to_owned(), value))
                        .collect()
                )),
        ]
    })
}

fn an_actor() -> impl Strategy<Value = Actor> {
    prop::sample::select(vec!["ada", "ben"]).prop_map(|name| Actor::new(name).expect("an actor"))
}

fn a_label() -> impl Strategy<Value = Label> {
    prop::sample::select(LABELS.to_vec()).prop_map(label)
}

/// The value of the record of heads of a history.
fn heads() -> impl Strategy<Value = Value> {
    (prop::collection::btree_set(a_hash(), 0..3), 0usize..9).prop_map(|(tips, count)| {
        Value::call(
            "Heads",
            vec![
                ("count".to_owned(), count.to_value()),
                ("tips".to_owned(), tips.to_value()),
            ],
        )
    })
}

fn a_note_event() -> impl Strategy<Value = Note> {
    (
        prop::sample::select(vec!["n1", "n2"]),
        a_datetime(),
        prop_oneof![any::<String>().prop_map(Entry::Titled), Just(Entry::Viewed)],
    )
        .prop_map(|(name, at, entry)| by_ada(name, at, entry))
}

fn a_membership() -> impl Strategy<Value = MembershipEvent<Labels>> {
    (
        0..3usize,
        a_label(),
        a_datetime(),
        an_actor(),
        any::<bool>(),
    )
        .prop_map(|(target, tag, at, actor, on)| {
            if on {
                added::<Labels>(note(target), tag, at, actor)
            } else {
                removed::<Labels>(note(target), tag, at, actor)
            }
        })
}

fn an_alias() -> impl Strategy<Value = AliasEvent<Labels>> {
    (a_label(), a_label(), a_datetime(), an_actor()).prop_map(|(tag, to, at, actor)| {
        if tag == to {
            unaliased::<Labels>(tag, at, actor)
        } else {
            aliased::<Labels>(tag, to, at, actor)
        }
        .expect("a label stands for another one, or for itself")
    })
}

/// Changes that rename a call to a name of each reader, or to another.
fn some_changes() -> impl Strategy<Value = Vec<(usize, Change)>> {
    prop::collection::vec(
        a_change(&[
            "Heads",
            "Titled",
            "Viewed",
            "Added",
            "Removed",
            "Aliased",
            "Unaliased",
            "Other",
        ]),
        0..5,
    )
}

/// The constructors that a value of [`a_value`] may call.
const TABLE: &[Constructor] = &[("Opened", &["title"]), ("C", &["f"])];

proptest! {
    #![proptest_config(crate::common::cases::cases(2048))]

    /// Law 69: `Fields::of` and `Fields::among` accept the calls that they
    /// accepted before, and `Fields::get` reads each field type as it read
    /// it before. Each refusal of a field names the field.
    #[test]
    fn fields_accepts_what_it_accepted(value in a_value(), field in prop::option::of(a_value())) {
        prop_assert_eq!(
            before::Fields::of(&value, TABLE[1]).is_ok(),
            Fields::of(&value, TABLE[1]).is_ok()
        );
        prop_assert_eq!(
            before::Fields::among(&value, TABLE).is_ok(),
            Fields::among(&value, "Table", TABLE).is_ok()
        );
        let call = Value::call(
            "C",
            field.into_iter().map(|value| ("f".to_owned(), value)).collect(),
        );
        every_get_agrees(&call)?;
    }

    /// Law 69: each reader of this repository that reads through `Fields`
    /// accepts the values that it accepted before, and reads the same
    /// value, over its values changed in up to four places.
    #[test]
    fn every_reader_accepts_what_it_accepted(
        heads in heads(),
        note in a_note_event(),
        membership in a_membership(),
        alias in an_alias(),
        changes in some_changes(),
    ) {
        let value = changed(heads, &changes);
        agree(
            readers::heads(&value),
            Heads::from_value(&value).map(|heads| (heads.tips().clone(), heads.count())),
        )?;
        let value = changed(note.to_value(), &changes);
        agree(readers::note(&value), Note::from_value(&Constructors::default(), &value))?;
        let value = changed(membership.to_value(), &changes);
        agree(
            readers::membership(&value),
            MembershipEvent::<Labels>::from_value(&Constructors::default(), &value),
        )?;
        let value = changed(alias.to_value(), &changes);
        agree(
            readers::alias(&value),
            AliasEvent::<Labels>::from_value(&Constructors::default(), &value),
        )?;
    }
}

/// The refusals of `Fields`, in the words of a derived read.
#[test]
fn fields_refuses_in_the_words_of_a_derived_read() {
    let refusal = |result: Result<String, ProdromeError>| match result {
        Ok(read) => panic!("{read:?} is read"),
        Err(error) => error.to_string(),
    };
    let call = Value::call("C", vec![("f".to_owned(), Value::int(1))]);
    let fields = Fields::of(&call, ("C", &["f", "g"])).expect("a call of C");
    assert_eq!(refusal(fields.get("g")), "C(...) is missing g");
    assert_eq!(
        refusal(fields.get("f")),
        "C.f must be a string, got Int(Int { negative: false, digits: \"1\" })"
    );
    assert_eq!(
        fields
            .get::<u32>("f")
            .map_err(|refused| refused.to_string()),
        Ok(1)
    );
    let text = Value::call("C", vec![("f".to_owned(), Value::str("one"))]);
    let fields = Fields::of(&text, ("C", &["f"])).expect("a call of C");
    assert_eq!(
        fields
            .get::<u32>("f")
            .map_err(|refused| refused.to_string()),
        Err("C.f: expected a natural number of at most 4294967295, got Str(\"one\")".to_owned())
    );
    assert_eq!(
        fields
            .get::<Actor>("f")
            .map_err(|refused| refused.to_string()),
        Ok(Actor::new("one").expect("an actor"))
    );
    assert_eq!(
        refusal(Fields::of(&call, ("D", &[])).map(|_| String::new())),
        "expected a D(...), got Some(Call { name: \"C\", fields: [(\"f\", Int(Int { \
         negative: false, digits: \"1\" }))] })"
    );
    assert_eq!(
        refusal(Fields::of(&Value::None, ("D", &[])).map(|_| String::new())),
        "expected a D(...), got None"
    );
    assert_eq!(
        refusal(Fields::among(&call, "Verdict", &[("Accept", &[])]).map(|_| String::new())),
        "C is not one of Verdict's constructors"
    );
    assert_eq!(
        refusal(Fields::among(&Value::None, "Verdict", &[]).map(|_| String::new())),
        "expected a constructor call, got None"
    );
}

/// A refusal of an unknown event names the body's type, as a derived sum
/// names its own.
#[test]
fn an_unknown_event_names_the_body() {
    let other = Value::call("Other", vec![]);
    assert_eq!(
        Note::from_value(&Constructors::default(), &other).map(|_| ()),
        Err(ProdromeError::invalid(
            "Other is not one of Entry's constructors"
        ))
    );
    assert_eq!(
        MembershipEvent::<Labels>::from_value(&Constructors::default(), &other).map(|_| ()),
        Err(ProdromeError::invalid(
            "Other is not one of Membership's constructors"
        ))
    );
}

/// A refusal led by a context keeps its kind.
#[test]
fn a_refusal_within_a_field_keeps_its_kind() {
    assert_eq!(
        ProdromeError::parse("x").within(&"C.f"),
        ProdromeError::parse("C.f: x")
    );
    assert_eq!(
        ProdromeError::invalid("x").within(&"C.f"),
        ProdromeError::invalid("C.f: x")
    );
    assert_eq!(
        ProdromeError::Store("x".to_owned()).within(&"C.f"),
        ProdromeError::Store("C.f: x".to_owned())
    );
}
