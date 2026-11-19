//! `#[derive(Literal)]`: the stored names of the reference payload, pinned
//! as data, and the forms and options of the derive over a neutral record
//! of a review.

use std::collections::BTreeSet;

use prodrome::event::{Actor, Hash};
use prodrome::literal::codec::{text_field, Constructor, Fields, Literal, Record};
use prodrome::literal::{parse_literal, print_literal, Datetime, ProdromeError, Table, Value};
use prodrome::reference::{Note, Source, SubTodo, Todo};
use proptest::prelude::*;

/// The constructors of the reference payload, each with its fields in print
/// order. The vectors in `conformance/` hold these names in their stored
/// objects. A rename of a Rust item that changes a stored name fails this
/// test; to rename the item and keep the stored form, give the old name with
/// `#[literal(rename = "...")]`.
#[test]
fn the_reference_payload_pins_its_stored_names() {
    let vocabulary: Vec<Constructor> = [
        Todo::CONSTRUCTORS,
        Source::CONSTRUCTORS,
        Note::CONSTRUCTORS,
        SubTodo::CONSTRUCTORS,
    ]
    .concat();
    let pinned: [Constructor; 4] = [
        (
            "Authored",
            &[
                "kind",
                "created",
                "body",
                "spec",
                "rationale",
                "category",
                "waiting_on",
                "detail",
                "source",
                "subtodos",
                "notes",
                "note",
            ],
        ),
        (
            "Source",
            &[
                "sender",
                "subject",
                "date",
                "hash",
                "thread_id",
                "message_id",
            ],
        ),
        ("Note", &["on", "lines"]),
        ("SubTodo", &["body", "done"]),
    ];
    assert_eq!(vocabulary, pinned);
}

// --- a neutral schema ---------------------------------------------------------

/// A label: a text leaf whose constructor refuses the empty text. Its impl
/// is written by a macro, not derived.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Tag(String);

impl TryFrom<String> for Tag {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        if text.is_empty() {
            Err("a tag is not empty".to_owned())
        } else {
            Ok(Tag(text))
        }
    }
}

prodrome::text_literal!(Tag => |tag| tag.0.clone());

impl AsRef<str> for Tag {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A count of lines that is never zero: a newtype whose read calls its
/// smart constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Literal)]
#[literal(new = Lines::new)]
struct Lines(u32);

impl Lines {
    fn new(count: u32) -> Result<Lines, ProdromeError> {
        if count == 0 {
            Err(ProdromeError::invalid("a review covers at least one line"))
        } else {
            Ok(Lines(count))
        }
    }
}

/// What a reviewer decided: a derived sum, with a variant without fields.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
enum Verdict {
    Accept,
    Revise {
        reason: String,
    },
    #[literal(rename = "Defer")]
    Postpone {
        until: Option<Hash>,
    },
}

/// A record of a review: a derived record whose fields are derived types,
/// types with a hand-written impl, and containers of both.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
#[literal(rename = "Review")]
struct Reviewed {
    reviewer: Actor,
    of: Vec<Hash>,
    tags: BTreeSet<Tag>,
    verdict: Verdict,
    lines: Lines,
    #[literal(rename = "remark")]
    comment: Option<String>,
    #[literal(default)]
    closed: bool,
    #[literal(skip)]
    seen: bool,
}

/// A hand-written record that holds a derived one, read with [`Fields`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct Thread {
    first: Reviewed,
    replies: Vec<Reviewed>,
}

const THREAD: Constructor = ("Thread", &["first", "replies"]);

impl Literal for Thread {
    fn to_value(&self) -> Value {
        prodrome::literal::codec::record(
            THREAD,
            vec![self.first.to_value(), self.replies.to_value()],
        )
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        let fields = Fields::of(value, THREAD)?;
        Ok(Thread {
            first: fields.get("first")?,
            replies: fields.get("replies")?,
        })
    }
}

/// A record whose field prints as its text and reads as a `String` that
/// the record's smart constructor checks.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
#[literal(new = Labelled::new)]
struct Labelled {
    #[literal(with = text_field)]
    label: Tag,
}

impl Labelled {
    fn new(label: String) -> Result<Labelled, ProdromeError> {
        Tag::try_from(label)
            .map(|label| Labelled { label })
            .map_err(ProdromeError::invalid)
    }
}

/// The constructors that a print of a thread uses.
const VOCABULARY: Table = Table(&[
    THREAD,
    (
        "Review",
        &[
            "reviewer", "of", "tags", "verdict", "lines", "remark", "closed",
        ],
    ),
    ("Accept", &[]),
    ("Revise", &["reason"]),
    ("Defer", &["until"]),
]);

fn a_hash() -> impl Strategy<Value = Hash> {
    "[0-9a-f]{64}".prop_map(|text| Hash::new(text).expect("the pattern is a name"))
}

fn a_verdict() -> impl Strategy<Value = Verdict> {
    prop_oneof![
        Just(Verdict::Accept),
        any::<String>().prop_map(|reason| Verdict::Revise { reason }),
        prop::option::of(a_hash()).prop_map(|until| Verdict::Postpone { until }),
    ]
}

fn a_review() -> impl Strategy<Value = Reviewed> {
    (
        "[a-z][a-z0-9_-]{0,8}".prop_map(|text| Actor::new(text).expect("the pattern is an actor")),
        prop::collection::vec(a_hash(), 0..3),
        prop::collection::btree_set("[a-z]{1,6}".prop_map(Tag), 0..4),
        a_verdict(),
        (1u32.., any::<Option<String>>(), any::<bool>()),
    )
        .prop_map(
            |(reviewer, of, tags, verdict, (lines, comment, closed))| Reviewed {
                reviewer,
                of,
                tags,
                verdict,
                lines: Lines::new(lines).expect("not zero"),
                comment,
                closed,
                seen: false,
            },
        )
}

fn a_thread() -> impl Strategy<Value = Thread> {
    (a_review(), prop::collection::vec(a_review(), 0..3))
        .prop_map(|(first, replies)| Thread { first, replies })
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// A derived value reads back from its literal, and from the canonical
    /// print of its literal, inside a hand-written record and around
    /// hand-written leaves.
    #[test]
    fn a_derived_value_reads_back_from_its_literal(thread in a_thread()) {
        let value = thread.to_value();
        prop_assert_eq!(Thread::from_value(&value), Ok(thread.clone()));
        let parsed = parse_literal(&print_literal(&value), &VOCABULARY).expect("a print parses");
        prop_assert_eq!(Thread::from_value(&parsed), Ok(thread));
    }
}

fn review() -> Reviewed {
    Reviewed {
        reviewer: Actor::new("ada").expect("an actor"),
        of: vec![],
        tags: BTreeSet::from([Tag("style".to_owned())]),
        verdict: Verdict::Postpone { until: None },
        lines: Lines::new(3).expect("not zero"),
        comment: None,
        closed: true,
        seen: true,
    }
}

/// A record prints each stored field under its stored name, in
/// declaration order, and leaves a skipped field out.
#[test]
fn a_record_prints_its_stored_names_in_order() {
    assert_eq!(
        print_literal(&review().to_value()),
        "Review(reviewer='ada', of=(), tags=('style',), verdict=Defer(until=None), \
         lines=3, remark=None, closed=True)"
    );
    assert_eq!(
        Reviewed::CONSTRUCTOR,
        (
            "Review",
            &["reviewer", "of", "tags", "verdict", "lines", "remark", "closed"][..]
        )
    );
    let names: Vec<&str> = review()
        .fields()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, Reviewed::CONSTRUCTOR.1);
}

/// A read sets a skipped field to its default, and reads a missing
/// `default` field as its default.
#[test]
fn a_read_fills_a_skipped_and_a_defaulted_field() {
    let Value::Call(mut call) = review().to_value() else {
        panic!("a record prints as a call");
    };
    call.fields.retain(|(name, _)| name != "closed");
    let read = Reviewed::from_value(&Value::Call(call)).expect("a review");
    assert_eq!(
        read,
        Reviewed {
            closed: false,
            seen: false,
            ..review()
        }
    );
}

/// A derived read names the constructor, the field and the context of each
/// refusal, with the messages of the core's own kinds.
#[test]
fn a_derived_read_names_what_it_refuses() {
    let refusal = |value: &Value| match Reviewed::from_value(value) {
        Ok(read) => panic!("{read:?} is read"),
        Err(error) => error.to_string(),
    };
    let with = |name: &str, field: Value| {
        let Value::Call(mut call) = review().to_value() else {
            panic!("a record prints as a call");
        };
        match call.fields.iter_mut().find(|(named, _)| named == name) {
            Some((_, value)) => *value = field,
            None => panic!("no field {name}"),
        }
        Value::Call(call)
    };
    assert_eq!(refusal(&Value::int(1)), "expected a Review(...), got None");
    let mut missing = review().to_value();
    if let Value::Call(call) = &mut missing {
        call.fields.retain(|(name, _)| name != "lines");
    }
    assert_eq!(refusal(&missing), "Review(...) is missing lines");
    assert_eq!(
        refusal(&with("remark", Value::int(1))),
        "Review.remark must be a string, got Int(Int { negative: false, digits: \"1\" })"
    );
    assert_eq!(
        refusal(&with("closed", Value::None)),
        "Review.closed must be a bool, got None"
    );
    assert_eq!(
        refusal(&with("lines", Value::int(0))),
        "a review covers at least one line"
    );
    assert_eq!(
        refusal(&with("verdict", Value::call("Reject", vec![]))),
        "Reject is not one of Verdict's constructors"
    );
    assert_eq!(
        refusal(&with("verdict", Value::str("Accept"))),
        "expected a constructor call, got Str(\"Accept\")"
    );
    assert_eq!(
        refusal(&with("tags", Value::Tuple(vec![Value::str("")]))),
        "a tag is not empty"
    );
}

/// A field with its own codec prints as its text, and the record's smart
/// constructor checks the text that the read gives it.
#[test]
fn a_field_with_its_own_codec_reads_through_the_smart_constructor() {
    let labelled = Labelled {
        label: Tag("draft".to_owned()),
    };
    assert_eq!(
        print_literal(&labelled.to_value()),
        "Labelled(label='draft')"
    );
    assert_eq!(Labelled::from_value(&labelled.to_value()), Ok(labelled));
    let empty = Value::call("Labelled", vec![("label".to_owned(), Value::str(""))]);
    assert_eq!(
        Labelled::from_value(&empty),
        Err(ProdromeError::invalid("a tag is not empty"))
    );
}

// --- a record whose sum names its constructor ------------------------------

/// What a line of a log of work says: a sum whose variant names the
/// constructor of the line that holds it.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
enum Shape {
    Opened { title: String },
    Closed { why: Option<String> },
    Paused,
}

/// A line of a log of work: the fields common to every line, and the shape
/// of the line. It prints as `Opened(at, actor, rests_on, title)`,
/// `Closed(at, actor, rests_on, why)` or `Paused(at, actor, rests_on)`.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
struct Line {
    at: Datetime,
    actor: Actor,
    #[literal(sum)]
    shape: Shape,
    rests_on: Vec<Hash>,
}

/// The same codec, written by hand: a table of fields for each shape and two
/// matches, with the core's reads.
mod by_hand {
    use super::*;
    use prodrome::literal::codec::{call, headed, no_arm};
    use prodrome::payload::{as_string, datetime_field, required, string_field, tuple_field};

    pub fn to_value(line: &Line) -> Value {
        let head = vec![
            ("at", Value::Datetime(line.at)),
            ("actor", Value::str(line.actor.as_str())),
            (
                "rests_on",
                Value::Tuple(
                    line.rests_on
                        .iter()
                        .map(|hash| Value::str(hash.as_str()))
                        .collect(),
                ),
            ),
        ];
        match &line.shape {
            Shape::Opened { title } => {
                headed(head, ("Opened", &["title"]), vec![Value::str(title)])
            }
            Shape::Closed { why } => headed(
                head,
                ("Closed", &["why"]),
                vec![why.as_ref().map_or(Value::None, Value::str)],
            ),
            Shape::Paused => headed(head, ("Paused", &[]), vec![]),
        }
    }

    pub fn from_value(value: &Value) -> Result<Line, ProdromeError> {
        let call = call(value)?;
        let shape = match call.name.as_str() {
            "Opened" => Shape::Opened {
                title: string_field(call, "title")?,
            },
            "Closed" => Shape::Closed {
                why: match required(call, "why")? {
                    Value::None => None,
                    other => Some(as_string(other, &format!("{}.why", call.name))?),
                },
            },
            "Paused" => Shape::Paused,
            other => return Err(no_arm("one of Shape's constructors", other)),
        };
        Ok(Line {
            at: datetime_field(call, "at")?,
            actor: Actor::new(string_field(call, "actor")?)?,
            shape,
            rests_on: tuple_field(call, "rests_on")?
                .iter()
                .map(Hash::from_value)
                .collect::<Result<_, _>>()?,
        })
    }
}

/// Every constructor of a line, with the common fields first, as data.
#[test]
fn a_record_with_a_sum_lists_each_constructor_with_all_its_fields() {
    let pinned: [Constructor; 3] = [
        ("Opened", &["at", "actor", "rests_on", "title"]),
        ("Closed", &["at", "actor", "rests_on", "why"]),
        ("Paused", &["at", "actor", "rests_on"]),
    ];
    assert_eq!(Line::CONSTRUCTORS, pinned);
}

fn a_datetime() -> impl Strategy<Value = Datetime> {
    (
        1i32..=9999,
        1u32..=12,
        1u32..=28,
        0u32..24,
        0u32..60,
        0u32..60,
    )
        .prop_map(|(y, mo, d, h, mi, s)| Datetime::new(y, mo, d, h, mi, s, 0).expect("in range"))
}

fn a_line() -> impl Strategy<Value = Line> {
    (
        a_datetime(),
        "[a-z][a-z0-9_-]{0,8}".prop_map(|text| Actor::new(text).expect("the pattern is an actor")),
        prop_oneof![
            any::<String>().prop_map(|title| Shape::Opened { title }),
            any::<Option<String>>().prop_map(|why| Shape::Closed { why }),
            Just(Shape::Paused),
        ],
        prop::collection::vec(a_hash(), 0..3),
    )
        .prop_map(|(at, actor, shape, rests_on)| Line {
            at,
            actor,
            shape,
            rests_on,
        })
}

/// A value that a field of a line expects, or one that none expects.
fn a_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::None),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::int),
        prop_oneof![
            Just(String::new()),
            Just("Ada".to_owned()),
            "[a-z]{1,4}",
            "[0-9a-f]{64}"
        ]
        .prop_map(Value::Str),
        a_datetime().prop_map(Value::Datetime),
        Just(Value::Tuple(vec![])),
        prop::collection::vec(a_hash().prop_map(|hash| Value::str(hash.as_str())), 1..3)
            .prop_map(Value::Tuple),
        Just(Value::call("Paused", vec![])),
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
            prop::sample::select(vec!["Opened", "Closed", "Paused", "Other"])
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

proptest! {
    #![proptest_config(crate::common::cases::cases(2048))]

    /// The derived print of a record with a sum is the hand-written print,
    /// and the derived read reads it back.
    #[test]
    fn a_record_with_a_sum_prints_as_the_hand_written_codec(line in a_line()) {
        prop_assert_eq!(line.to_value(), by_hand::to_value(&line));
        prop_assert_eq!(Line::from_value(&line.to_value()), Ok(line));
    }

    /// The derived read of a line changed in up to four places is the
    /// hand-written read: the same line, or the same refusal with the same
    /// message.
    #[test]
    fn a_record_with_a_sum_reads_as_the_hand_written_codec(
        line in a_line(),
        changes in prop::collection::vec(a_change(), 0..5),
    ) {
        let mut value = line.to_value();
        for (index, change) in &changes {
            let mut index = index % parts(&value);
            apply(&mut value, &mut index, change);
        }
        prop_assert_eq!(Line::from_value(&value), by_hand::from_value(&value));
    }
}
