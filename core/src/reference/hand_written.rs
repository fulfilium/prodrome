//! The hand-written codec of the reference payload, kept beside the derived
//! one to show that the two print the same bytes and read the same values,
//! refusals and their messages included.

use proptest::prelude::*;

use super::*;
use crate::fpl::{mk_conj, mk_flat};
use crate::literal::Call;
use crate::literal::Value;
use crate::payload::{
    bool_field, datetime_field, expect_call, spec_field, string_field, string_or_empty, strings,
    tuple_field, tuple_or_empty,
};

fn text(value: impl Into<String>) -> Value {
    Value::Str(value.into())
}

fn optional_text(value: Option<&Name>) -> Value {
    Value::Str(
        value
            .map(|name| name.as_str().to_owned())
            .unwrap_or_default(),
    )
}

fn tuple_of<T>(items: &[T], each: impl Fn(&T) -> Value) -> Value {
    Value::Tuple(items.iter().map(each).collect())
}

fn field(name: &str, value: Value) -> (String, Value) {
    (name.to_owned(), value)
}

fn source_to_value(source: &Source) -> Value {
    Value::call(
        "Source",
        vec![
            field("sender", text(source.sender.as_str())),
            field("subject", text(source.subject.as_str())),
            field("date", Value::Datetime(source.date)),
            field("hash", text(source.hash.as_str())),
            field("thread_id", text(source.thread_id.as_str())),
            field("message_id", text(source.message_id.as_str())),
        ],
    )
}

fn note_to_value(note: &Note) -> Value {
    Value::call(
        "Note",
        vec![
            field("on", text(note.on.as_str())),
            field("lines", tuple_of(&note.lines, |line| text(line.clone()))),
        ],
    )
}

fn subtodo_to_value(subtodo: &SubTodo) -> Value {
    Value::call(
        "SubTodo",
        vec![
            field("body", text(subtodo.body.clone())),
            field("done", Value::Bool(subtodo.done)),
        ],
    )
}

fn source_from_value(value: &Value) -> Result<Source, ProdromeError> {
    let call = expect_call(value, "Source")?;
    Ok(mk_source(
        &string_field(call, "sender")?,
        &string_field(call, "subject")?,
        datetime_field(call, "date")?,
        &string_or_empty(call, "hash")?,
        &string_or_empty(call, "thread_id")?,
        &string_or_empty(call, "message_id")?,
    ))
}

fn note_from_value(value: &Value) -> Result<Note, ProdromeError> {
    let call = expect_call(value, "Note")?;
    mk_note(
        &string_field(call, "on")?,
        strings(tuple_field(call, "lines")?, "Note.lines")?,
    )
}

fn subtodo_from_value(value: &Value) -> Result<SubTodo, ProdromeError> {
    let call = expect_call(value, "SubTodo")?;
    mk_subtodo(&string_field(call, "body")?, bool_field(call, "done")?)
}

fn todo_fields(todo: &Todo) -> Vec<(&'static str, Value)> {
    vec![
        ("kind", text(todo.kind.as_str())),
        ("created", Value::Datetime(todo.created)),
        ("body", text(todo.body.as_str())),
        (
            "spec",
            todo.spec
                .as_ref()
                .map_or(Value::None, |spec| spec.to_value()),
        ),
        (
            "rationale",
            tuple_of(&todo.rationale, |line| text(line.clone())),
        ),
        ("category", optional_text(todo.category.as_ref())),
        ("waiting_on", text(todo.waiting_on.as_str())),
        ("detail", text(todo.detail.as_str())),
        (
            "source",
            todo.source.as_ref().map_or(Value::None, source_to_value),
        ),
        ("subtodos", tuple_of(&todo.subtodos, subtodo_to_value)),
        ("notes", tuple_of(&todo.notes, note_to_value)),
        ("note", text(todo.note.clone())),
    ]
}

fn todo_from_fields(call: &Call) -> Result<Todo, ProdromeError> {
    mk_todo(
        &string_field(call, "kind")?,
        datetime_field(call, "created")?,
        &string_field(call, "body")?,
        spec_field(call, "spec")?,
        strings(tuple_or_empty(call, "rationale")?, "Authored.rationale")?,
        &string_or_empty(call, "category")?,
        &string_or_empty(call, "waiting_on")?,
        &string_or_empty(call, "detail")?,
        match call.field("source") {
            None | Some(Value::None) => None,
            Some(other) => Some(source_from_value(other)?),
        },
        tuple_or_empty(call, "subtodos")?
            .iter()
            .map(subtodo_from_value)
            .collect::<Result<Vec<_>, _>>()?,
        tuple_or_empty(call, "notes")?
            .iter()
            .map(note_from_value)
            .collect::<Result<Vec<_>, _>>()?,
        &string_or_empty(call, "note")?,
    )
}

fn actor_from_value(value: &Value) -> Result<crate::event::Actor, ProdromeError> {
    crate::event::Actor::new(String::from_value(value)?)
}

// --- the generated values -------------------------------------------------

fn a_datetime() -> impl Strategy<Value = Datetime> {
    (
        1i32..=9999,
        1u32..=12,
        1u32..=28,
        0u32..24,
        0u32..60,
        0u32..60,
        0u32..3,
    )
        .prop_map(|(y, mo, d, h, mi, s, us)| {
            Datetime::new(y, mo, d, h, mi, s, us * 250).expect("the ranges are valid")
        })
}

/// Text that is sometimes empty, blank, an identifier, a site's word or
/// none of these, so that every smart constructor both admits and refuses.
fn a_text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("  ".to_owned()),
        Just("block".to_owned()),
        Just("waiting_on".to_owned()),
        "[A-Za-z][A-Za-z0-9_]{0,6}",
        "[a-z][a-z0-9_-]{0,6}",
        any::<String>(),
    ]
}

fn a_term() -> impl Strategy<Value = Term> {
    (0.0f64..=1.0, 0.0f64..=1.0).prop_map(|(a, b)| {
        mk_conj(
            vec![mk_flat(a).expect("in range"), mk_flat(b).expect("in range")],
            -1.0,
        )
        .expect("a conjunction of two")
    })
}

fn a_source() -> impl Strategy<Value = Source> {
    (
        a_text(),
        a_text(),
        a_datetime(),
        a_text(),
        a_text(),
        a_text(),
    )
        .prop_map(|(sender, subject, date, hash, thread_id, message_id)| {
            mk_source(&sender, &subject, date, &hash, &thread_id, &message_id)
        })
}

fn a_note() -> impl Strategy<Value = Note> {
    (
        prop::sample::select(NoteSite::ALL.to_vec()),
        prop::collection::vec(a_text(), 1..3),
    )
        .prop_map(|(on, lines)| mk_note(on.as_str(), lines).expect("a site and lines"))
}

fn a_subtodo() -> impl Strategy<Value = SubTodo> {
    ("[a-z]{1,6}", any::<bool>()).prop_map(|(body, done)| mk_subtodo(body, done).expect("a body"))
}

fn a_todo() -> impl Strategy<Value = Todo> {
    (
        (
            "[A-Za-z][A-Za-z0-9_]{0,6}",
            a_datetime(),
            "[a-z]{1,6}",
            prop::option::of(a_term()),
            prop::collection::vec(a_text(), 0..3),
            prop::option::of("[A-Za-z][A-Za-z0-9_]{0,6}"),
        ),
        (
            a_text(),
            a_text(),
            prop::option::of(a_source()),
            prop::collection::vec(a_subtodo(), 0..3),
            prop::collection::vec(a_note(), 0..3),
            a_text(),
        ),
    )
        .prop_map(
            |(
                (kind, created, body, spec, rationale, category),
                (waiting_on, detail, source, subtodos, notes, note),
            )| {
                mk_todo(
                    kind,
                    created,
                    body,
                    spec,
                    rationale,
                    category.unwrap_or_default(),
                    waiting_on,
                    detail,
                    source,
                    subtodos,
                    notes,
                    note,
                )
                .expect("a valid record")
            },
        )
}

/// A value that no field of the payload expects, or one that it does.
fn a_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::None),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::int),
        a_text().prop_map(Value::Str),
        a_datetime().prop_map(Value::Datetime),
        Just(Value::Tuple(vec![])),
        prop::collection::vec(a_text().prop_map(Value::Str), 1..3).prop_map(Value::Tuple),
        a_term().prop_map(|term| term.to_value()),
        Just(Value::call("Note", vec![])),
        Just(Value::call("Elsewhere", vec![])),
    ]
}

/// A change to one part of a value.
#[derive(Debug, Clone)]
enum Change {
    /// The part's call loses its field at the index.
    Drop(usize),
    /// The part becomes the leaf.
    Replace(Value),
    /// The part's call takes the name.
    Rename(&'static str),
}

fn a_change() -> impl Strategy<Value = (usize, Change)> {
    (
        any::<usize>(),
        prop_oneof![
            any::<usize>().prop_map(Change::Drop),
            a_leaf().prop_map(Change::Replace),
            prop::sample::select(vec!["Source", "Note", "SubTodo", "Authored", "Other"])
                .prop_map(Change::Rename),
        ],
    )
}

fn parts(value: &Value) -> usize {
    1 + match value {
        Value::Tuple(items) => items.iter().map(parts).sum(),
        Value::Call(call) => call.fields.iter().map(|(_, value)| parts(value)).sum(),
        _ => 0,
    }
}

/// Applies `change` to the part at `index`, in preorder.
fn apply(value: &mut Value, index: &mut usize, change: &Change) {
    if *index == 0 {
        match (change, &mut *value) {
            (Change::Replace(leaf), _) => *value = leaf.clone(),
            (Change::Drop(at), Value::Call(call)) if !call.fields.is_empty() => {
                let at = at % call.fields.len();
                call.fields.remove(at);
            }
            (Change::Rename(name), Value::Call(call)) => call.name = (*name).to_owned(),
            _ => {}
        }
        *index = usize::MAX;
        return;
    }
    *index -= 1;
    match value {
        Value::Tuple(items) => items.iter_mut().for_each(|item| apply(item, index, change)),
        Value::Call(call) => call
            .fields
            .iter_mut()
            .for_each(|(_, value)| apply(value, index, change)),
        _ => {}
    }
}

fn each_part<'v>(value: &'v Value, out: &mut Vec<&'v Value>) {
    out.push(value);
    match value {
        Value::Tuple(items) => items.iter().for_each(|item| each_part(item, out)),
        Value::Call(call) => call
            .fields
            .iter()
            .for_each(|(_, value)| each_part(value, out)),
        _ => {}
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PRODROME_MAX_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(4096)
    ))]

    /// The derived print of a record is the hand-written print, byte for
    /// byte, and so is the print of each record it nests.
    #[test]
    fn the_derived_print_is_the_hand_written_print(todo in a_todo()) {
        prop_assert_eq!(todo.fields(), todo_fields(&todo));
        for note in &todo.notes {
            prop_assert_eq!(note.to_value(), note_to_value(note));
        }
        for subtodo in &todo.subtodos {
            prop_assert_eq!(subtodo.to_value(), subtodo_to_value(subtodo));
        }
        if let Some(source) = &todo.source {
            prop_assert_eq!(source.to_value(), source_to_value(source));
        }
    }

    /// The derived read of any value, changed from a valid record in up to
    /// four places, is the hand-written read: the same record, or the same
    /// refusal with the same message. Each part of the value is read as a
    /// `Source`, a `Note`, a `SubTodo` and an `Actor` too.
    #[test]
    fn the_derived_read_is_the_hand_written_read(
        todo in a_todo(),
        changes in prop::collection::vec(a_change(), 0..5),
    ) {
        let mut value = Value::call(
            "Authored",
            todo.fields().into_iter().map(|(name, value)| (name.to_owned(), value)).collect(),
        );
        for (index, change) in &changes {
            let mut index = index % parts(&value);
            apply(&mut value, &mut index, change);
        }
        // The event layer reads a record's fields only from a call named
        // `Authored`, so the two reads are compared there. The hand-written
        // read names `Authored` in one refusal whatever the call's name.
        if let Some(call) = value.as_call().filter(|call| call.name == "Authored") {
            prop_assert_eq!(Todo::from_call(call), todo_from_fields(call));
        }
        let mut all = Vec::new();
        each_part(&value, &mut all);
        for part in all {
            prop_assert_eq!(Source::from_value(part), source_from_value(part));
            prop_assert_eq!(Note::from_value(part), note_from_value(part));
            prop_assert_eq!(SubTodo::from_value(part), subtodo_from_value(part));
            prop_assert_eq!(crate::event::Actor::from_value(part), actor_from_value(part));
        }
    }
}
