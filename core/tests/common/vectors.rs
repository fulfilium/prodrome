//! THE VECTOR GRAMMAR (SPEC §2): the conformance vectors, as literals.
//!
//! A vector file is ONE expression of §2's grammar — the same printer, the
//! same parser, the same float and string rules as a stored object — read
//! through the VECTOR VOCABULARY below and never through a store's. The two
//! vocabularies are disjoint and neither can widen the other: a store parses
//! against `event::EventVocabulary` (§4's kinds, §7's terms, the payload's
//! names) and refuses every name here, and a vector file parses against
//! [`VECTORS`] and refuses every name there.
//!
//! WHY. This crate's whole claim is that there is ONE reading of its data: one
//! closed grammar, one printer, one parser. Evidence for that claim written in
//! a second format was a second format to keep in step — and a JSON codec in
//! the core existed largely to serve it. So the evidence is in the crate's own
//! grammar now, read by the crate's own parser, and a vector file that does
//! not parse is a vector file that fails.
//!
//! WHAT IS A STRING AND WHAT IS A LITERAL. Every stored object, event and term
//! a vector holds stays a STRING holding its canonical print — never a nested
//! literal — because the exact bytes ARE what the vector is evidence of (§9.1).
//! Nesting them would have thrown that away and pinned this crate's parse of
//! them instead. Instants are `datetime(...)`, values are floats, and the maps
//! the JSON shapes keyed by todo id or object name become TUPLES of a named
//! constructor, because §2 has tuples and no mapping and because a pair with
//! two fields called `key` and `value` says less than [`Bound`] does.
//!
#![allow(dead_code)]

use prodrome::literal::{parse_literal, Call, Datetime, Table, Value};

/// The vector vocabulary: every constructor a `conformance/*.py` may name, in
/// declared field order. SPEC §2 lists exactly these.
///
/// One `Table`, so it is a closed whitelist like every other vocabulary in the
/// crate, and `Signature::Fields` throughout, so a field order is checked
/// rather than assumed.
pub const VECTORS: Table = Table(&[
    // --- the nine roots -----------------------------------------------------
    ("Folds", &["logs"]),
    ("Dags", &["dags"]),
    ("Fpl", &["terms"]),
    ("Axis", &["cases"]),
    ("Series", &["series"]),
    ("View", &["policy", "untrusted", "cases"]),
    ("Links", &["specs", "env", "cases"]),
    ("Recurs", &["untrusted", "events", "at", "cases"]),
    ("Changes", &["cases"]),
    ("Husks", &["cases"]),
    // --- one case of each ---------------------------------------------------
    (
        "FoldCase",
        &[
            "seed",
            "untrusted",
            "events",
            "at",
            "env",
            "specs",
            "content",
            "flatten",
            "history_at",
        ],
    ),
    (
        "DagCase",
        &[
            "seed",
            "objects",
            "tips",
            "linearisation",
            "parents",
            "verify",
            "env",
            "conflicts",
        ],
    ),
    (
        "FplCase",
        &[
            "seed",
            "term",
            "normalized",
            "env",
            "samples",
            "explain_at",
            "explain",
        ],
    ),
    // A term whose spans leave the time axis, and its readings (§7).
    ("AxisCase", &["why", "term", "samples"]),
    (
        "SeriesCase",
        &["seed", "term", "from", "to", "exact", "knots"],
    ),
    ("ViewCase", &["seed", "events", "instants"]),
    // A term, what it links to (None where refused), and its readings.
    ("LinkCase", &["term", "linked", "samples", "refused"]),
    // A term that reads the tendings, and its readings against them.
    ("RecurCase", &["term", "samples"]),
    // A store of changes: what it starts from, what is written, and what it reads.
    (
        "ChangeCase",
        &[
            "name", "dag", "base", "steps", "objects", "verify", "at", "prices",
        ],
    ),
    // A print kept under a name, and how a schema with husks (the letter
    // schema of the tests) and one without (the todo schema) read it.
    ("HuskCase", &["why", "name", "print", "letter", "todo"]),
    // --- what a case is made of ---------------------------------------------
    // One todo's outcome: §6.1's environment as a tuple instead of a map.
    ("Bound", &["todo", "kind", "at"]),
    // An open candidate of a todo whose state is in conflict.
    ("Open", &["todo"]),
    // One todo's term, as its canonical print — `specs` and `flatten`.
    ("Spec", &["todo", "term"]),
    // One todo's winning record, as its canonical print.
    ("Content", &["todo", "event"]),
    // One stored object: the name it claims and the bytes that name hashes.
    ("Object", &["name", "literal"]),
    // One object's parents, in the order §3 sorts them.
    ("Parents", &["name", "parents"]),
    // One register with more than one live write, in a DAG vector.
    ("Conflict", &["todo", "kind", "writes"]),
    // The same, inside a view row, which already knows its todo.
    ("RowConflict", &["kind", "writes"]),
    // Why a term does not link: `unknown` with the todo, or `cycle` with the
    // loop, its first todo repeated at the end.
    ("Refused", &["kind", "todos"]),
    // One write to a store, as the handle writing into `genesis` does it.
    ("Step", &["op", "genesis", "event", "answer", "writes"]),
    // One todo's state and value, in its prodrome.
    ("Price", &["genesis", "todo", "state", "value"]),
    // One `fulfillment` sample: the moment and the number.
    ("Sample", &["now", "value"]),
    // One knot of a series (§7 breakpoints).
    ("Knot", &["at", "value"]),
    // One moment a view was asked about, and the rows it answered with.
    ("Asked", &["at", "entries"]),
    // One §6.7 entry, in the fields the wire carries.
    (
        "Row",
        &[
            "todo",
            "state",
            "at",
            "claimed",
            "value",
            "unconfirmed",
            "conflicts",
            "content",
            "spec",
            "stream",
        ],
    ),
    // --- the explanation (§7): the DECORATION, not the term ------------------
    // One node of an `explain` tree: its kind tag, its fulfillment at the
    // moment its parent used it, its notes, and its children in order. The
    // term's own SHAPE is `FplCase.term`, so this holds only what explaining
    // adds.
    ("Node", &["kind", "value", "notes", "terms"]),
    // One note on a node, keyed as `fpl::Note`'s map keys are.
    ("NoteEntry", &["key", "note"]),
    // `fpl::Note`'s three arms, one constructor each.
    ("One", &["value"]),
    ("Many", &["values"]),
    ("Maps", &["maps"]),
    // One map inside a `Maps` note — a curve point, today.
    ("Fields", &["pairs"]),
    ("Pair", &["key", "value"]),
]);

/// Read a vector file: the bytes, through §2's parser, against [`VECTORS`].
///
/// Panics with the parse error, which is the honest failure for a file this
/// suite exists to read: a vector that does not parse is not a vector.
pub fn vectors(name: &str) -> Value {
    let path: std::path::PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "conformance", name]
        .iter()
        .collect();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    parse_literal(&text, &VECTORS)
        .unwrap_or_else(|e| panic!("{} is not a vector literal: {e}", path.display()))
}

// --- reading one apart -------------------------------------------------------
//
// The accessors a suite reads a case with. Each panics with what it wanted,
// because a vector file that is the wrong shape is a broken vector file and
// not a case to report politely.

pub fn call<'a>(value: &'a Value, name: &str) -> &'a Call {
    match value.as_call() {
        Some(call) if call.name == name => call,
        other => panic!("expected a {name}(...), got {other:?}"),
    }
}

pub fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    value
        .as_call()
        .and_then(|call| call.field(key))
        .unwrap_or_else(|| panic!("no field {key:?} on {value:?}"))
}

pub fn items(value: &Value) -> &[Value] {
    value
        .as_tuple()
        .unwrap_or_else(|| panic!("expected a tuple, got {value:?}"))
}

/// The tuple at `key`, which is the shape every plural field has.
pub fn each<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    items(field(value, key))
}

pub fn text(value: &Value) -> &str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("expected a string, got {value:?}"))
}

/// A string field where `None` is a real answer — `Row.content`, `Row.spec`.
pub fn maybe_text(value: &Value) -> Option<&str> {
    match value {
        Value::None => None,
        other => Some(text(other)),
    }
}

pub fn number(value: &Value) -> f64 {
    match value {
        Value::Float(f) => f.get(),
        Value::Int(i) => i.as_i64().expect("an integer that fits") as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

pub fn maybe_number(value: &Value) -> Option<f64> {
    match value {
        Value::None => None,
        other => Some(number(other)),
    }
}

pub fn integer(value: &Value) -> i64 {
    match value {
        Value::Int(i) => i.as_i64().expect("an integer that fits"),
        other => panic!("expected an integer, got {other:?}"),
    }
}

pub fn boolean(value: &Value) -> bool {
    match value {
        Value::Bool(b) => *b,
        other => panic!("expected a bool, got {other:?}"),
    }
}

pub fn moment(value: &Value) -> Datetime {
    match value {
        Value::Datetime(at) => *at,
        other => panic!("expected a datetime, got {other:?}"),
    }
}

/// A field's string, in one call — `text(field(v, k))`, which every suite says
/// often enough to name.
pub fn text_at<'a>(value: &'a Value, key: &str) -> &'a str {
    text(field(value, key))
}

pub fn moment_at(value: &Value, key: &str) -> Datetime {
    moment(field(value, key))
}

pub fn strings(value: &Value, key: &str) -> Vec<String> {
    each(value, key)
        .iter()
        .map(|v| text(v).to_owned())
        .collect()
}
