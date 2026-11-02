//! The literal parser reads bytes and copies each run of a string between
//! escapes at once. These tests check that it answers exactly as the parser
//! it replaced, [`frozen::parse`], on every text: the same value for a text
//! in the grammar, and the same refusal, word for word, for any other text,
//! because `verify` quotes the refusal.

mod frozen;

use prodrome::event::{canonical_envelope, EventVocabulary, TodoEvent};
use prodrome::literal::{
    parse_literal, print_literal, Datetime, Finite, Int, Open, Table, Timedelta, Value,
};
use prodrome::reference::Todo;
use prodrome::todo::TodoVocabulary;
use proptest::prelude::*;

/// A vocabulary with field orders, so that positional arguments bind.
static VOCABULARY: Table = Table(&[
    ("Change", &["genesis", "deps", "event"]),
    ("Note", &["on", "lines"]),
    ("Pair", &["a", "b"]),
]);

/// Both parsers on `text`, under the vocabulary `vocabulary`.
fn agree(text: &str, vocabulary: &dyn prodrome::literal::Vocabulary) -> Result<(), TestCaseError> {
    prop_assert_eq!(
        parse_literal(text, vocabulary),
        frozen::parse(text, vocabulary),
        "the parsers differ on {:?}",
        text
    );
    Ok(())
}

/// A string of any characters: control characters and characters that are
/// not printable, which the printer escapes, as well as plain text.
fn a_string() -> impl Strategy<Value = String> {
    prop_oneof![
        ".{0,40}",
        prop::collection::vec(any::<char>(), 0..40).prop_map(String::from_iter),
        "[ -~]{0,2000}",
    ]
}

fn a_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::None),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::int),
        (any::<bool>(), "[0-9]{1,30}").prop_map(|(negative, digits)| Value::Int(
            Int::new(negative, &digits).expect("digits")
        )),
        any::<f64>()
            .prop_filter_map("a finite float", |x| Finite::new(x).ok())
            .prop_map(Value::Float),
        a_string().prop_map(Value::Str),
        (
            1..=9999i32,
            1..=12u32,
            1..=28u32,
            0..24u32,
            0..60u32,
            0..60u32,
            0..1_000_000u32
        )
            .prop_map(
                |(year, month, day, hour, minute, second, micro)| Value::Datetime(
                    Datetime::new(year, month, day, hour, minute, second, micro).expect("in range")
                )
            ),
        (-(86_400_000_000i128 * 999_999_999)..86_400_000_000i128 * 999_999_999)
            .prop_map(|micros| Value::Timedelta(Timedelta::from_micros(micros).expect("in range"))),
    ]
}

/// A value of the grammar, with calls in keyword form so that the [`Open`]
/// vocabulary reads them.
fn a_value() -> impl Strategy<Value = Value> {
    a_leaf().prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Tuple),
            (
                "[A-Z][A-Za-z0-9_]{0,8}",
                prop::collection::btree_map("[a-z_][a-z0-9_]{0,6}", inner, 0..6)
            )
                .prop_map(|(name, fields)| Value::call(name, fields.into_iter().collect())),
        ]
    })
}

/// Pieces of the grammar and of what lies just outside it, which a text is
/// put together from: quotes, escapes whole and cut short, line
/// continuations, comments, whitespace inside and outside ASCII, numbers
/// and exponents, names and keywords.
const PIECES: &[&str] = &[
    "'",
    "\"",
    "\\",
    "\\\\",
    "\\'",
    "\\\"",
    "\\n",
    "\\r",
    "\\t",
    "\\a",
    "\\b",
    "\\f",
    "\\v",
    "\\0",
    "\\x4",
    "\\x41",
    "\\xzz",
    "\\u00e9",
    "\\u12",
    "\\U0001F600",
    "\\U00110000",
    "\\\n",
    "\\q",
    "\\é",
    "\n",
    "\r",
    "\t",
    " ",
    "\u{b}",
    "\u{c}",
    "\u{a0}",
    "\u{2003}",
    "\u{85}",
    "\u{1c}",
    "#",
    "# a comment\n",
    "(",
    ")",
    "[",
    "]",
    ",",
    "=",
    "==",
    " = ",
    "-",
    "*",
    "**",
    "0",
    "007",
    "1",
    "12",
    "1.5",
    ".",
    ".5",
    "1.",
    "1e",
    "1e5",
    "1E-3",
    "e+",
    "None",
    "True",
    "False",
    "x",
    "_a",
    "é",
    "日本",
    "a1",
    "Change",
    "Note",
    "Pair",
    "Pair(",
    "datetime(",
    "datetime(2026, 9, 1)",
    "timedelta(",
    "timedelta(hours=3)",
    "deps=",
    "a=",
    "b =",
    "lines=",
];

/// A text put together from [`PIECES`].
fn a_text() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES), 0..24).prop_map(|pieces| pieces.concat())
}

proptest! {
    #![proptest_config(crate::common::cases::cases(2048))]

    /// The print of any value reads back as both parsers read it.
    #[test]
    fn a_print_parses_as_the_frozen_parser_parses_it(value in a_value()) {
        let text = print_literal(&value);
        agree(&text, &Open)?;
        prop_assert_eq!(parse_literal(&text, &Open), Ok(value));
    }

    /// Any text built from the grammar's pieces gets the same answer from
    /// both parsers: the same value, or the same refusal.
    #[test]
    fn a_text_parses_as_the_frozen_parser_parses_it(text in a_text()) {
        agree(&text, &VOCABULARY)?;
        agree(&text, &Open)?;
    }

    /// Any text at all gets the same answer from both parsers.
    #[test]
    fn any_text_parses_as_the_frozen_parser_parses_it(text in ".{0,200}") {
        agree(&text, &VOCABULARY)?;
    }

    /// A stored object, and the object with a cut or a splice in it, get the
    /// same answer from both parsers at the store's vocabulary.
    #[test]
    fn an_object_parses_as_the_frozen_parser_parses_it(
        object in crate::events::a_new_object(),
        at in any::<prop::sample::Index>(),
        cut in 0usize..8,
        splice in a_text(),
    ) {
        let schema = TodoVocabulary::<Todo>::default();
        let vocabulary = EventVocabulary::<TodoEvent<Todo>>::new(&schema);
        let text = canonical_envelope(&object);
        agree(&text, &vocabulary)?;
        let mut at = at.index(text.len() + 1);
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        let mut end = (at + cut).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        agree(&format!("{}{splice}{}", &text[..at], &text[end..]), &vocabulary)?;
    }
}
