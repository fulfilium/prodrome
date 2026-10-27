//! The codec of `literal::codec`: every value of a type that implements
//! `Literal` reads back from its literal, and from the canonical print of that
//! literal. The tests use a neutral record of a review, with a sum, a set, a
//! list, an absent value and a text leaf among its fields.

use std::collections::BTreeSet;

use chrono::NaiveDate;
use prodrome::event::{Actor, Hash};
use prodrome::literal::codec::{arm, headed, no_arm, record, Constructor, Fields, Literal};
use prodrome::literal::{parse_literal, print_literal, Datetime, ProdromeError, Table, Value};
use proptest::prelude::*;

/// A label of a review: a text leaf whose constructor refuses the empty text.
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

/// How much of the objects a review covers: a leaf with one word for each
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    Skim,
    Full,
}

impl Depth {
    const ALL: [Depth; 2] = [Depth::Skim, Depth::Full];

    fn word(self) -> &'static str {
        match self {
            Depth::Skim => "skim",
            Depth::Full => "full",
        }
    }
}

prodrome::word_literal!(Depth: Depth::ALL => |depth| depth.word());

/// What a reviewer decided: a sum with one constructor for each variant.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Accept,
    Revise { reason: String },
}

const ACCEPT: Constructor = ("Accept", &[]);
const REVISE: Constructor = ("Revise", &["reason"]);

impl Literal for Verdict {
    fn to_value(&self) -> Value {
        match self {
            Verdict::Accept => record(ACCEPT, vec![]),
            Verdict::Revise { reason } => record(REVISE, vec![reason.to_value()]),
        }
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        let fields = Fields::among(value, &[ACCEPT, REVISE])?;
        match fields.name() {
            "Accept" => Ok(Verdict::Accept),
            "Revise" => Ok(Verdict::Revise {
                reason: fields.get("reason")?,
            }),
            other => Err(no_arm("a verdict", other)),
        }
    }
}

/// A review of some objects: a record with a field of each form.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Review {
    reviewer: Actor,
    at: Datetime,
    due: NaiveDate,
    of: Vec<Hash>,
    tags: BTreeSet<Tag>,
    depth: Depth,
    note: Option<String>,
    verdict: Verdict,
    closed: bool,
    score: u32,
    words: u64,
    lines: usize,
}

const REVIEW: Constructor = (
    "Review",
    &[
        "reviewer", "at", "due", "of", "tags", "depth", "note", "verdict", "closed", "score",
        "words", "lines",
    ],
);

/// The constructors that a print of a review uses.
const VOCABULARY: Table = Table(&[REVIEW, ACCEPT, REVISE]);

impl Literal for Review {
    fn to_value(&self) -> Value {
        record(
            REVIEW,
            vec![
                self.reviewer.to_value(),
                self.at.to_value(),
                self.due.to_value(),
                self.of.to_value(),
                self.tags.to_value(),
                self.depth.to_value(),
                self.note.to_value(),
                self.verdict.to_value(),
                self.closed.to_value(),
                self.score.to_value(),
                self.words.to_value(),
                self.lines.to_value(),
            ],
        )
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        let fields = Fields::of(value, REVIEW)?;
        Ok(Review {
            reviewer: fields.get("reviewer")?,
            at: fields.get("at")?,
            due: fields.get("due")?,
            of: fields.get("of")?,
            tags: fields.get("tags")?,
            depth: fields.get("depth")?,
            note: fields.get("note")?,
            verdict: fields.get("verdict")?,
            closed: fields.get("closed")?,
            score: fields.get("score")?,
            words: fields.get("words")?,
            lines: fields.get("lines")?,
        })
    }
}

fn a_datetime() -> impl Strategy<Value = Datetime> {
    (
        1i32..=9999,
        1u32..=12,
        1u32..=28,
        0u32..24,
        0u32..60,
        0u32..60,
        0u32..1_000_000,
    )
        .prop_map(|(y, mo, d, h, mi, s, us)| {
            Datetime::new(y, mo, d, h, mi, s, us).expect("the ranges are valid")
        })
}

fn a_verdict() -> impl Strategy<Value = Verdict> {
    prop_oneof![
        Just(Verdict::Accept),
        any::<String>().prop_map(|reason| Verdict::Revise { reason }),
    ]
}

fn a_review() -> impl Strategy<Value = Review> {
    (
        "[a-z][a-z0-9_-]{0,8}".prop_map(|text| Actor::new(text).expect("the pattern is an actor")),
        a_datetime(),
        (1i32..=9999, 1u32..=366)
            .prop_filter_map("a day of the year", |(y, d)| NaiveDate::from_yo_opt(y, d)),
        prop::collection::vec(
            "[0-9a-f]{64}".prop_map(|text| Hash::new(text).expect("the pattern is a name")),
            0..3,
        ),
        prop::collection::btree_set("[a-z]{1,6}".prop_map(Tag), 0..4),
        prop::sample::select(Depth::ALL.to_vec()),
        any::<Option<String>>(),
        a_verdict(),
        (any::<bool>(), any::<u32>(), any::<u64>(), any::<usize>()),
    )
        .prop_map(
            |(reviewer, at, due, of, tags, depth, note, verdict, (closed, score, words, lines))| {
                Review {
                    reviewer,
                    at,
                    due,
                    of,
                    tags,
                    depth,
                    note,
                    verdict,
                    closed,
                    score,
                    words,
                    lines,
                }
            },
        )
}

proptest! {
    #![proptest_config(crate::common::cases::cases(256))]

    /// A review reads back from its literal, and from the canonical print of
    /// its literal, parsed with the constructors that the print uses.
    #[test]
    fn a_value_reads_back_from_its_literal(review in a_review()) {
        let value = review.to_value();
        prop_assert_eq!(Review::from_value(&value), Ok(review.clone()));
        let parsed = parse_literal(&print_literal(&value), &VOCABULARY).expect("a print parses");
        prop_assert_eq!(Review::from_value(&parsed), Ok(review));
    }
}

/// `headed` puts the head's fields before the body's, in that order.
#[test]
fn a_head_comes_before_the_body() {
    let value = headed(
        vec![("about", Value::str("x"))],
        REVISE,
        vec![Value::str("why")],
    );
    let call = value.as_call().expect("a call");
    let names: Vec<&str> = call.fields.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["about", "reason"]);
    assert_eq!(arm(&value), Ok("Revise"));
}

/// A read refuses every value that is not the print of a value of its type.
#[test]
fn a_read_refuses_what_no_value_prints() {
    let set = |items: &[i64]| Value::Tuple(items.iter().copied().map(Value::int).collect());
    assert!(BTreeSet::<u32>::from_value(&set(&[2, 1])).is_err());
    assert!(BTreeSet::<u32>::from_value(&set(&[1, 1])).is_err());
    assert!(u32::from_value(&Value::int(i64::from(u32::MAX) + 1)).is_err());
    assert!(u32::from_value(&Value::int(-1)).is_err());
    assert!(String::from_value(&Value::int(1)).is_err());
    assert!(Tag::from_value(&Value::str("")).is_err());
    assert!(Depth::from_value(&Value::str("deep")).is_err());
    assert!(NaiveDate::from_value(&Value::str("2026-13-01")).is_err());
    assert!(Verdict::from_value(&record(("Reject", &[]), vec![])).is_err());
    assert!(arm(&Value::int(1)).is_err());

    let accepted = Verdict::Accept.to_value();
    assert!(Fields::of(&accepted, REVISE).is_err());
    let fields = Fields::of(&accepted, ACCEPT).expect("an accept");
    assert!(fields.get::<String>("reason").is_err());
}
