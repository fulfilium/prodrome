//! SPEC §9 law 67: the print of a string is a function of its text alone.
//! The printable set of §2 is the one of Unicode 16.0.0 in every build.
//!
//! The oracle is version 1.1.0 of the crate `unicode-general-category`,
//! which holds the General_Category data of Unicode 16.0.0. The printer of
//! release 0.9 takes its categories from that crate at a version 1.x, and
//! every published version 1.x holds this data. So when the table and the
//! oracle agree on every character, every object that release 0.9 stored
//! keeps its name.

use proptest::prelude::*;

use crate::common::vectors::vectors;
use prodrome::literal::printable::{self, is_printable};
use prodrome::literal::{parse_literal, print_literal, print_str, Open, Value};
use unicode_general_category::{get_general_category, GeneralCategory};

/// The SHA-256 of the printable set, as SPEC §2 writes it.
const DIGEST: &str = "b428bdfd5ffc52d306a2642112ab0f61bfd99eabef85e4d1a4f37ede2cc48595";

/// The number of printable characters in Unicode 16.0.0.
const PRINTABLE: usize = 154_810;

/// CPython's `str.isprintable`, over the categories of the oracle.
fn oracle(c: char) -> bool {
    c == ' '
        || !matches!(
            get_general_category(c),
            GeneralCategory::Control
                | GeneralCategory::Format
                | GeneralCategory::Surrogate
                | GeneralCategory::PrivateUse
                | GeneralCategory::Unassigned
                | GeneralCategory::LineSeparator
                | GeneralCategory::ParagraphSeparator
                | GeneralCategory::SpaceSeparator
        )
}

/// The characters outside ASCII that the print of `text` writes as
/// themselves, beside the ones that the oracle calls printable. An escape
/// is ASCII, so the characters outside ASCII in the print are the ones that
/// it writes as themselves.
fn written_and_expected(text: &str) -> (Vec<char>, Vec<char>) {
    let written = print_str(text).chars().filter(|c| !c.is_ascii()).collect();
    let expected = text
        .chars()
        .filter(|&c| !c.is_ascii() && oracle(c))
        .collect();
    (written, expected)
}

fn every_char() -> impl Iterator<Item = char> {
    (0..=0x10_FFFF).filter_map(char::from_u32)
}

/// Law 67: the printable set has the digest and the size that SPEC §2
/// states. A change to the table changes the digest, and fails here.
#[test]
fn the_printable_set_has_the_digest_of_the_spec() {
    assert_eq!(printable::UNICODE_VERSION, "16.0.0");
    assert_eq!(printable::digest().as_str(), DIGEST);
    assert_eq!(every_char().filter(|&c| is_printable(c)).count(), PRINTABLE);
}

/// Law 67: for every Unicode scalar value, the table answers what the
/// categories of Unicode 16.0.0 answer. The test names each code point
/// that differs, and what the printer writes for it.
#[test]
fn the_printable_set_is_the_one_of_unicode_16() {
    assert_eq!(unicode_general_category::UNICODE_VERSION, (16, 0, 0));
    let moved: Vec<String> = every_char()
        .filter(|&c| is_printable(c) != oracle(c))
        .map(|c| {
            format!(
                "U+{:04X}: the table says {}, Unicode 16.0.0 says {}, the print is {}",
                c as u32,
                is_printable(c),
                oracle(c),
                print_str(&c.to_string())
            )
        })
        .collect();
    assert!(
        moved.is_empty(),
        "{} code points differ:\n{}",
        moved.len(),
        moved.join("\n")
    );
}

/// Law 67: a character that Unicode 16.0.0 assigns prints as itself, and
/// a character that a later version assigns prints as an escape that reads
/// back to it.
///
/// U+1C89 is a letter (Lu) that Unicode 16.0.0 assigns. Version 0.6.0 of
/// `unicode-general-category` holds Unicode 15.0.0, where U+1C89 is
/// unassigned, so a build with that version printed `'Ᲊ'`. U+20C1 is
/// a currency sign (Sc) that Unicode 17.0.0 assigns. A build that asked
/// Unicode 17.0.0 would print it as itself.
#[test]
fn a_later_assignment_prints_as_an_escape() {
    for (text, print) in [("\u{1c89}", "'\u{1c89}'"), ("\u{20c1}", "'\\u20c1'")] {
        assert_eq!(print_str(text), print);
        let value = parse_literal(print, &Open).expect("a print parses");
        assert_eq!(value, Value::Str(text.to_owned()));
    }
}

/// Every file under `conformance/`, as a path relative to that directory.
fn vector_files() -> Vec<String> {
    let root: std::path::PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "conformance"]
        .iter()
        .collect();
    let mut found = Vec::new();
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("reads conformance/") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "py") {
                let relative = path.strip_prefix(&root).expect("under conformance/");
                found.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    found
}

/// Every string in a vector, depth first.
fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::Str(text) => out.push(text),
        Value::Tuple(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Call(call) => call
            .fields
            .iter()
            .for_each(|(_, field)| strings(field, out)),
        Value::None
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::Datetime(_)
        | Value::Timedelta(_) => {}
    }
}

/// Law 67: the printer writes every stored object and term in the
/// conformance vectors byte for byte as the vector holds it, and writes as
/// itself each character of a vector's strings that Unicode 16.0.0 calls
/// printable. The strings of today's vectors are ASCII, so the check of
/// the characters outside ASCII holds for a vector that a later change adds.
#[test]
fn every_print_in_the_vectors_is_unchanged() {
    let mut prints = 0;
    for file in vector_files() {
        let vector = vectors(&file);
        let mut found = Vec::new();
        strings(&vector, &mut found);
        for text in found {
            let (written, expected) = written_and_expected(text);
            assert_eq!(written, expected, "{file}: {text:?}");
            if let Ok(value @ Value::Call(_)) = parse_literal(text, &Open) {
                assert_eq!(print_literal(&value), text, "{file}");
                prints += 1;
            }
        }
    }
    assert!(prints > 100, "the vectors hold prints, found {prints}");
}

proptest! {
    #![proptest_config(crate::common::cases::cases(1024))]

    /// Law 67: over generated text, the print writes as itself exactly the
    /// characters outside ASCII that Unicode 16.0.0 calls printable, and
    /// the print reads back to the text.
    #[test]
    fn a_print_writes_as_itself_what_unicode_16_calls_printable(text in any::<String>()) {
        let (written, expected) = written_and_expected(&text);
        prop_assert_eq!(written, expected);
        let print = print_str(&text);
        prop_assert_eq!(parse_literal(&print, &Open).expect("a print parses"), Value::Str(text));
    }
}
