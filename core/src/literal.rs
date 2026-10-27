//! §2 — values and the literal grammar: the value model, its canonical
//! printer, and the strict parser that is the only door into it.
//!
//! The doctrine is the reference's `literals.py`'s, unchanged: the whitelist
//! IS the grammar, and a parser that accepts more than the grammar is not
//! permissive, it is incorrect. This module is deliberately IGNORANT of what
//! the constructors MEAN — it knows names, field order and arity through a
//! [`Vocabulary`], and hands the fields to whichever layer owns the kind
//! (`event` for §4, `fpl` for §7). That is what lets `print ∘ parse` be the
//! identity on a stored object without this file knowing a Term from a Note.
//!
//! The printer reproduces CPython's `repr` for every case the store holds:
//! floats through the shortest round-tripping digits and CPython's own
//! fixed/scientific cutover, strings through `str.isprintable`'s Unicode
//! categories. Those two are the whole subtlety of §2; see `print_float` and
//! `push_str_repr`.

pub mod codec;

use std::fmt::Write as _;

use thiserror::Error;
use unicode_general_category::{get_general_category, GeneralCategory};

/// Every refusal in the core. Nothing here panics on input (§2: parsing is
/// fuzzed), so every boundary returns one of these.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProdromeError {
    /// The literal grammar refused the text — Python's `parse_literal`
    /// `ValueError`, whose message the store's `verify` quotes verbatim.
    #[error("parse_literal: {0}")]
    Parse(String),
    /// A smart constructor refused its parameters (§1: invariants live here,
    /// never at a use site).
    #[error("{0}")]
    Invalid(String),
    /// The object store refused (§3): a missing parent, a cycle, bytes that do
    /// not hash to their name.
    #[error("{0}")]
    Store(String),
    /// Filesystem trouble under a store root.
    #[error("{path}: {message}")]
    Io { path: String, message: String },
}

impl ProdromeError {
    pub fn parse(message: impl Into<String>) -> Self {
        ProdromeError::Parse(message.into())
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        ProdromeError::Invalid(message.into())
    }
}

/// An integer of the grammar. Every integer the chain stores fits an `i64`,
/// but one conformance vector is `12345678901234567890` and the print must be
/// the identity on it, so the value is held as its own canonical decimal
/// digits: sign plus a nonempty digit string with no leading zero.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Int {
    negative: bool,
    digits: String,
}

impl Int {
    /// The canonical integer named by `digits` (ASCII decimal, nonempty).
    pub fn new(negative: bool, digits: &str) -> Result<Int, ProdromeError> {
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ProdromeError::parse(format!(
                "integer must be nonempty decimal digits, got {digits:?}"
            )));
        }
        let trimmed = digits.trim_start_matches('0');
        let canonical = if trimmed.is_empty() { "0" } else { trimmed };
        Ok(Int {
            negative: negative && canonical != "0",
            digits: canonical.to_owned(),
        })
    }

    pub fn from_i64(value: i64) -> Int {
        Int {
            negative: value < 0,
            digits: value.unsigned_abs().to_string(),
        }
    }

    /// The value as an `i64`, or `None` when it does not fit one. Callers that
    /// need a number (a datetime field, a timedelta component) ask for this and
    /// refuse `None`; the printer never does, which is why the big vector
    /// round-trips.
    pub fn as_i64(&self) -> Option<i64> {
        let magnitude: i128 = self.digits.parse().ok()?;
        let signed = if self.negative { -magnitude } else { magnitude };
        i64::try_from(signed).ok()
    }

    pub fn is_negative(&self) -> bool {
        self.negative
    }

    pub fn digits(&self) -> &str {
        &self.digits
    }
}

/// A float that is storable: finite, so the printer is total (§2: `inf`/`nan`
/// are NOT storable, and a value that exists is valid).
#[derive(Debug, Clone, Copy)]
pub struct Finite(f64);

impl Finite {
    pub fn new(value: f64) -> Result<Finite, ProdromeError> {
        if value.is_finite() {
            Ok(Finite(value))
        } else {
            Err(ProdromeError::invalid(format!(
                "float must be finite (inf/nan are not storable), got {value}"
            )))
        }
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

/// Bitwise, so `-0.0` and `0.0` are different values — they print differently
/// (`'-0.0'` and `'0.0'`), and in a store whose identity IS the print, two
/// values that print differently are two values.
impl PartialEq for Finite {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Finite {}

/// A naive local datetime (§2: naive local time only — there is no tz field to
/// set, which is how "tz-aware is not storable" stops being a runtime check).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Datetime {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    microsecond: u32,
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Datetime {
    /// CPython's `datetime` bounds, checked once here so that every later
    /// reader (ordering, `isoformat`) is total.
    pub fn new(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
        microsecond: u32,
    ) -> Result<Datetime, ProdromeError> {
        if !(1..=9999).contains(&year) {
            return Err(ProdromeError::invalid(format!(
                "year {year} is out of range"
            )));
        }
        if !(1..=12).contains(&month) {
            return Err(ProdromeError::invalid(format!(
                "month must be in 1..12, got {month}"
            )));
        }
        let last = days_in_month(year, month);
        if !(1..=last).contains(&day) {
            return Err(ProdromeError::invalid(format!(
                "day is out of range for month, got {day}"
            )));
        }
        if hour > 23 {
            return Err(ProdromeError::invalid(format!(
                "hour must be in 0..23, got {hour}"
            )));
        }
        if minute > 59 {
            return Err(ProdromeError::invalid(format!(
                "minute must be in 0..59, got {minute}"
            )));
        }
        if second > 59 {
            return Err(ProdromeError::invalid(format!(
                "second must be in 0..59, got {second}"
            )));
        }
        if microsecond > 999_999 {
            return Err(ProdromeError::invalid(format!(
                "microsecond must be in 0..999999, got {microsecond}"
            )));
        }
        Ok(Datetime {
            year,
            month,
            day,
            hour,
            minute,
            second,
            microsecond,
        })
    }

    pub fn year(self) -> i32 {
        self.year
    }
    pub fn month(self) -> u32 {
        self.month
    }
    pub fn day(self) -> u32 {
        self.day
    }
    pub fn hour(self) -> u32 {
        self.hour
    }
    pub fn minute(self) -> u32 {
        self.minute
    }
    pub fn second(self) -> u32 {
        self.second
    }
    pub fn microsecond(self) -> u32 {
        self.microsecond
    }

    /// CPython's `datetime.isoformat()`: seconds always, microseconds only when
    /// nonzero. `verify`'s dating finding quotes this, so it is exact.
    pub fn isoformat(self) -> String {
        let head = format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        );
        if self.microsecond == 0 {
            head
        } else {
            format!("{head}.{:06}", self.microsecond)
        }
    }
}

/// A duration, normalised as CPython normalises one: `0 <= seconds < 86400`,
/// `0 <= microseconds < 1000000`, and the sign carried entirely by `days`.
/// Normalising in the constructor is what makes `timedelta(hours=3)` and
/// `timedelta(seconds=10800)` the same value with one print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timedelta {
    days: i64,
    seconds: u32,
    microseconds: u32,
}

const MICROS_PER_SECOND: i128 = 1_000_000;
const MICROS_PER_DAY: i128 = 86_400 * MICROS_PER_SECOND;
const MAX_DAYS: i128 = 999_999_999;

impl Timedelta {
    pub const ZERO: Timedelta = Timedelta {
        days: 0,
        seconds: 0,
        microseconds: 0,
    };

    /// From the components CPython's constructor takes, in its own order.
    pub fn new(
        days: i64,
        seconds: i64,
        microseconds: i64,
        milliseconds: i64,
        minutes: i64,
        hours: i64,
        weeks: i64,
    ) -> Result<Timedelta, ProdromeError> {
        let total = i128::from(weeks) * 7 * MICROS_PER_DAY
            + i128::from(days) * MICROS_PER_DAY
            + i128::from(hours) * 3_600 * MICROS_PER_SECOND
            + i128::from(minutes) * 60 * MICROS_PER_SECOND
            + i128::from(seconds) * MICROS_PER_SECOND
            + i128::from(milliseconds) * 1_000
            + i128::from(microseconds);
        Timedelta::from_micros(total)
    }

    pub fn from_micros(total: i128) -> Result<Timedelta, ProdromeError> {
        let days = total.div_euclid(MICROS_PER_DAY);
        let rest = total.rem_euclid(MICROS_PER_DAY);
        if !(-MAX_DAYS..=MAX_DAYS).contains(&days) {
            return Err(ProdromeError::invalid(format!(
                "timedelta of {days} days is out of range"
            )));
        }
        Ok(Timedelta {
            days: days as i64,
            seconds: (rest / MICROS_PER_SECOND) as u32,
            microseconds: (rest % MICROS_PER_SECOND) as u32,
        })
    }

    pub fn days(self) -> i64 {
        self.days
    }
    pub fn seconds(self) -> u32 {
        self.seconds
    }
    pub fn microseconds(self) -> u32 {
        self.microseconds
    }

    pub fn total_micros(self) -> i128 {
        i128::from(self.days) * MICROS_PER_DAY
            + i128::from(self.seconds) * MICROS_PER_SECOND
            + i128::from(self.microseconds)
    }
}

/// A constructor call: a name from the vocabulary and its fields in DECLARED
/// order, always keyword form. The parser resolves positional arguments into
/// this shape through the [`Vocabulary`], so a printed call is canonical
/// whatever the text said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    pub fields: Vec<(String, Value)>,
}

impl Call {
    pub fn new(name: impl Into<String>, fields: Vec<(String, Value)>) -> Call {
        Call {
            name: name.into(),
            fields,
        }
    }

    /// The field named `name`, or `None`. The event layer's `from_value` reads
    /// its record through this and applies its own defaults for what is absent.
    pub fn field(&self, name: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }
}

/// One expression of the grammar (§2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    None,
    Bool(bool),
    Int(Int),
    Float(Finite),
    Str(String),
    Datetime(Datetime),
    Timedelta(Timedelta),
    Tuple(Vec<Value>),
    Call(Call),
}

impl Value {
    pub fn int(value: i64) -> Value {
        Value::Int(Int::from_i64(value))
    }

    pub fn float(value: f64) -> Result<Value, ProdromeError> {
        Ok(Value::Float(Finite::new(value)?))
    }

    pub fn str(value: impl Into<String>) -> Value {
        Value::Str(value.into())
    }

    pub fn call(name: impl Into<String>, fields: Vec<(String, Value)>) -> Value {
        Value::Call(Call::new(name, fields))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_tuple(&self) -> Option<&[Value]> {
        match self {
            Value::Tuple(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_call(&self) -> Option<&Call> {
        match self {
            Value::Call(call) => Some(call),
            _ => None,
        }
    }
}

// --- the vocabulary: names, field order, arity ------------------------------

/// What a constructor name admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signature<'a> {
    /// The declared field order. Positional arguments bind to it in turn, a
    /// keyword must name one of these, and no field may be given twice.
    ///
    /// BORROWED FROM THE VOCABULARY, not `'static`: a vocabulary whose field
    /// order is not a compile-time table — SPEC §4's record kind, whose fields
    /// are the core's three plus the host payload's — owns its own slice and
    /// lends it. A static table lends `'static`, which coerces.
    Fields(&'a [&'a str]),
    /// The same declared field order, held as owned names: a vocabulary that
    /// arrives as data at run time (a declared schema, [`crate::declared`])
    /// lends what it owns. A field order is a field order however it is
    /// held, so the parse binds both by one function.
    Owned(&'a [String]),
    /// Any keyword field, no positional arguments — for reading a canonical
    /// print back without knowing the kind. Used by conformance tests and by
    /// tooling that inspects an object; the shipped read path never uses it,
    /// because the closed vocabulary is the point (§2).
    AnyKeywords,
}

/// The closed set of constructor names a parse admits (§2: "the names admitted
/// are the closed vocabulary of §4 and §7 plus `datetime`/`timedelta`"). The
/// trait is how `literal` stays ignorant of what those kinds mean: `event`
/// supplies §4's and §7's, and nothing else can widen it.
pub trait Vocabulary {
    fn signature(&self, name: &str) -> Option<Signature<'_>>;
}

/// A vocabulary over a static table — what `event` builds its own from.
#[derive(Debug, Clone, Copy)]
pub struct Table(pub &'static [codec::Constructor]);

impl Table {
    /// The lookup, keeping the `'static` the table's own data has — so a
    /// vocabulary composed of several tables ([`crate::event::EventVocabulary`])
    /// is not handed a signature borrowed from a temporary.
    pub fn find(&self, name: &str) -> Option<Signature<'static>> {
        self.0
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, fields)| Signature::Fields(fields))
    }
}

impl Vocabulary for Table {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        self.find(name)
    }
}

/// Admits any constructor name in keyword form. NOT a read path: it exists so
/// a canonical print can be round-tripped without the closed vocabulary
/// (conformance, tooling outside this crate), and it refuses positional arguments
/// precisely because it does not know any field order.
#[derive(Debug, Clone, Copy)]
pub struct Open;

impl Vocabulary for Open {
    fn signature(&self, _name: &str) -> Option<Signature<'_>> {
        Some(Signature::AnyKeywords)
    }
}

// --- printing ---------------------------------------------------------------

/// The canonical print (§2). Total: every `Value` that exists is printable,
/// because the constructors refused the ones that are not.
pub fn print_literal(value: &Value) -> String {
    let mut out = String::new();
    push_literal(&mut out, value);
    out
}

fn push_literal(out: &mut String, value: &Value) {
    match value {
        Value::None => out.push_str("None"),
        Value::Bool(true) => out.push_str("True"),
        Value::Bool(false) => out.push_str("False"),
        Value::Int(int) => {
            if int.negative {
                out.push('-');
            }
            out.push_str(&int.digits);
        }
        Value::Float(f) => out.push_str(&print_float(f.0)),
        Value::Str(text) => push_str_repr(out, text),
        Value::Datetime(at) => {
            let _ = write!(
                out,
                "datetime({}, {}, {}, {}, {}, {}",
                at.year, at.month, at.day, at.hour, at.minute, at.second
            );
            if at.microsecond != 0 {
                let _ = write!(out, ", {}", at.microsecond);
            }
            out.push(')');
        }
        Value::Timedelta(delta) => {
            let parts = [
                ("days", delta.days),
                ("seconds", i64::from(delta.seconds)),
                ("microseconds", i64::from(delta.microseconds)),
            ];
            out.push_str("timedelta(");
            let mut first = true;
            for (name, amount) in parts {
                if amount == 0 {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                let _ = write!(out, "{name}={amount}");
            }
            out.push(')');
        }
        Value::Tuple(items) => {
            out.push('(');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                push_literal(out, item);
            }
            if items.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }
        Value::Call(call) => {
            out.push_str(&call.name);
            out.push('(');
            for (index, (name, field)) in call.fields.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(name);
                out.push('=');
                push_literal(out, field);
            }
            out.push(')');
        }
    }
}

/// CPython's `repr(float)`, written out rather than borrowed from `{}`.
///
/// Rust's `{}` and `{:e}` both give the SHORTEST round-tripping digits — the
/// same digits CPython gets from David Gay's `dtoa` in mode 0 — but neither
/// lays them out the way CPython does. So the digits come from `{:e}` and the
/// layout is `format_float_short`'s: with `decpt` the position of the decimal
/// point relative to the first significant digit, scientific form is used iff
/// `decpt <= -4 || decpt > 16`, and a fixed form with no point gets `.0`
/// appended (CPython's `Py_DTSF_ADD_DOT_0`). That cutover is what makes
/// `1e+16` scientific and `1000000000000000.0` fixed.
pub fn print_float(value: f64) -> String {
    debug_assert!(value.is_finite(), "Finite refuses non-finite floats");
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("Rust's LowerExp always emits an exponent");
    let exponent: i32 = exponent
        .parse()
        .expect("Rust's LowerExp emits a decimal exponent");
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // `decpt`: the number of digits before the point. Gay's convention, and the
    // one `format_float_short` branches on.
    let decpt = exponent + 1;
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if decpt <= -4 || decpt > 16 {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let _ = write!(
            out,
            "e{}{:02}",
            if decpt - 1 < 0 { '-' } else { '+' },
            (decpt - 1).abs()
        );
    } else if decpt <= 0 {
        out.push_str("0.");
        for _ in 0..-decpt {
            out.push('0');
        }
        out.push_str(digits);
    } else if (decpt as usize) >= digits.len() {
        out.push_str(digits);
        for _ in 0..(decpt as usize - digits.len()) {
            out.push('0');
        }
        out.push_str(".0");
    } else {
        out.push_str(&digits[..decpt as usize]);
        out.push('.');
        out.push_str(&digits[decpt as usize..]);
    }
    out
}

/// CPython's `str.isprintable`: everything except the categories Cc, Cf, Cs,
/// Co, Cn, Zl, Zp and Zs — with SPACE the one exception, printable despite
/// being Zs. (`Cs`, surrogates, cannot occur in a Rust `str` at all; the arm is
/// kept because the rule is the rule.)
pub fn is_printable(c: char) -> bool {
    if c == ' ' {
        return true;
    }
    !matches!(
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

/// CPython's `repr(str)`, as a string — the one string printer in the crate.
pub fn print_str(text: &str) -> String {
    let mut out = String::new();
    push_str_repr(&mut out, text);
    out
}

/// CPython's `repr(str)`: single quotes unless the text holds a `'` and no `"`.
fn push_str_repr(out: &mut String, text: &str) {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            _ if c == quote => {
                out.push('\\');
                out.push(c);
            }
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            _ if (c as u32) < 0x7f => out.push(c),
            _ if is_printable(c) => out.push(c),
            _ => {
                let point = c as u32;
                if point < 0x100 {
                    let _ = write!(out, "\\x{point:02x}");
                } else if point < 0x10000 {
                    let _ = write!(out, "\\u{point:04x}");
                } else {
                    let _ = write!(out, "\\U{point:08x}");
                }
            }
        }
    }
    out.push(quote);
}

// --- parsing ----------------------------------------------------------------

/// How deep a literal may nest before the parser refuses. CPython answers a
/// deeper one with `RecursionError`, which `parse_literal` turns into a
/// `ValueError`; a recursive-descent parser in Rust would answer with a stack
/// overflow, which is an abort and not a refusal — so the limit is the type of
/// the guarantee, not a tuning knob.
const MAX_DEPTH: usize = 128;

/// Parse exactly the grammar of §2 against `vocabulary`, and nothing else.
pub fn parse_literal(text: &str, vocabulary: &dyn Vocabulary) -> Result<Value, ProdromeError> {
    let mut parser = Parser {
        input: text,
        pos: 0,
        vocabulary,
    };
    parser.skip_trivia();
    if parser.at_end() {
        return Err(ProdromeError::parse(
            "empty artifact, expected one expression",
        ));
    }
    let value = parser.expression(0)?;
    parser.skip_trivia();
    if !parser.at_end() {
        return Err(ProdromeError::parse(format!(
            "artifact must be a single expression, trailing text at byte {}",
            parser.pos
        )));
    }
    Ok(value)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    vocabulary: &'a dyn Vocabulary,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Whitespace and `#` comments, which CPython's tokenizer drops before the
    /// AST ever sees them — so a stored object may carry a comment line above
    /// its expression and still parse.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.pos += c.len_utf8();
                }
                Some('#') => {
                    while let Some(c) = self.peek() {
                        self.pos += c.len_utf8();
                        if c == '\n' {
                            break;
                        }
                    }
                }
                _ => return,
            }
        }
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: char) -> Result<(), ProdromeError> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(ProdromeError::parse(format!(
                "expected {c:?} at byte {}, got {:?}",
                self.pos,
                self.peek()
            )))
        }
    }

    fn expression(&mut self, depth: usize) -> Result<Value, ProdromeError> {
        if depth > MAX_DEPTH {
            return Err(ProdromeError::parse("literal nests too deeply"));
        }
        self.skip_trivia();
        match self.peek() {
            None => Err(ProdromeError::parse(
                "expected an expression, got end of text",
            )),
            Some('-') => {
                self.bump();
                self.skip_trivia();
                // CPython's walker admits USub over an int/float CONSTANT and
                // nothing else: `-Flat(...)` is an operator, not a literal.
                match self.number()? {
                    Value::Int(int) => Ok(Value::Int(Int::new(!int.negative, &int.digits)?)),
                    Value::Float(f) => Ok(Value::Float(Finite::new(-f.0)?)),
                    other => Err(ProdromeError::parse(format!(
                        "unary minus applies to a number, got {other:?}"
                    ))),
                }
            }
            Some('\'') | Some('"') => self.string(),
            Some('(') => self.parenthesised(depth),
            Some('[') => self.bracketed(depth),
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == '_' => self.name_or_call(depth),
            Some(c) => Err(ProdromeError::parse(format!(
                "disallowed syntax at byte {}: {c:?}",
                self.pos
            ))),
        }
    }

    /// `(a, b)` is a tuple, `(a,)` a one-tuple, `()` the empty one — and `(a)`
    /// is just `a`, exactly as CPython parses it.
    fn parenthesised(&mut self, depth: usize) -> Result<Value, ProdromeError> {
        self.expect('(')?;
        self.skip_trivia();
        if self.eat(')') {
            return Ok(Value::Tuple(Vec::new()));
        }
        let first = self.expression(depth + 1)?;
        self.skip_trivia();
        if self.eat(')') {
            return Ok(first);
        }
        let mut items = vec![first];
        loop {
            self.expect(',')?;
            self.skip_trivia();
            if self.eat(')') {
                break;
            }
            items.push(self.expression(depth + 1)?);
            self.skip_trivia();
            if self.eat(')') {
                break;
            }
        }
        Ok(Value::Tuple(items))
    }

    /// CPython's walker maps a list display onto a tuple, so the grammar admits
    /// one and the print never produces one.
    fn bracketed(&mut self, depth: usize) -> Result<Value, ProdromeError> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_trivia();
        if self.eat(']') {
            return Ok(Value::Tuple(items));
        }
        loop {
            items.push(self.expression(depth + 1)?);
            self.skip_trivia();
            if self.eat(']') {
                break;
            }
            self.expect(',')?;
            self.skip_trivia();
            if self.eat(']') {
                break;
            }
        }
        Ok(Value::Tuple(items))
    }

    fn number(&mut self) -> Result<Value, ProdromeError> {
        let start = self.pos;
        let mut is_float = false;
        while let Some(c) = self.peek() {
            match c {
                '0'..='9' => {
                    self.bump();
                }
                '.' => {
                    is_float = true;
                    self.bump();
                }
                'e' | 'E' => {
                    // An exponent, but only when a sign or digit follows: `1e`
                    // is not a number and neither is the `e` of a name.
                    let mut lookahead = self.rest().chars();
                    lookahead.next();
                    match lookahead.next() {
                        Some(next) if next.is_ascii_digit() || next == '+' || next == '-' => {
                            is_float = true;
                            self.bump();
                            self.bump();
                        }
                        _ => break,
                    }
                }
                _ => break,
            }
        }
        let text = &self.input[start..self.pos];
        if text.is_empty() || text == "." {
            return Err(ProdromeError::parse(format!(
                "expected a number at byte {start}"
            )));
        }
        if is_float {
            let parsed: f64 = text
                .parse()
                .map_err(|_| ProdromeError::parse(format!("malformed float {text:?}")))?;
            Ok(Value::Float(Finite::new(parsed)?))
        } else {
            Ok(Value::Int(Int::new(false, text)?))
        }
    }

    fn string(&mut self) -> Result<Value, ProdromeError> {
        let quote = self.bump().expect("caller peeked a quote");
        let mut out = String::new();
        loop {
            let c = self
                .bump()
                .ok_or_else(|| ProdromeError::parse("unterminated string literal"))?;
            if c == quote {
                return Ok(Value::Str(out));
            }
            if c == '\n' {
                return Err(ProdromeError::parse(
                    "newline in a single-quoted string literal",
                ));
            }
            if c != '\\' {
                out.push(c);
                continue;
            }
            let escape = self
                .bump()
                .ok_or_else(|| ProdromeError::parse("string ends in a backslash"))?;
            match escape {
                '\\' => out.push('\\'),
                '\'' => out.push('\''),
                '"' => out.push('"'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'a' => out.push('\u{7}'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'v' => out.push('\u{b}'),
                '0' => out.push('\0'),
                '\n' => {}
                'x' => out.push(self.escape_hex(2)?),
                'u' => out.push(self.escape_hex(4)?),
                'U' => out.push(self.escape_hex(8)?),
                other => {
                    return Err(ProdromeError::parse(format!(
                        "unknown string escape \\{other}"
                    )));
                }
            }
        }
    }

    fn escape_hex(&mut self, width: usize) -> Result<char, ProdromeError> {
        let start = self.pos;
        for _ in 0..width {
            match self.peek() {
                Some(c) if c.is_ascii_hexdigit() => {
                    self.bump();
                }
                _ => return Err(ProdromeError::parse("truncated \\x/\\u/\\U escape")),
            }
        }
        let point = u32::from_str_radix(&self.input[start..self.pos], 16)
            .map_err(|_| ProdromeError::parse("malformed \\x/\\u/\\U escape"))?;
        char::from_u32(point)
            .ok_or_else(|| ProdromeError::parse(format!("\\U{point:08x} is not a character")))
    }

    fn name_or_call(&mut self, depth: usize) -> Result<Value, ProdromeError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                self.bump();
            } else {
                break;
            }
        }
        let name = &self.input[start..self.pos];
        match name {
            "None" => return Ok(Value::None),
            "True" => return Ok(Value::Bool(true)),
            "False" => return Ok(Value::Bool(false)),
            _ => {}
        }
        self.skip_trivia();
        if self.peek() != Some('(') {
            return Err(ProdromeError::parse(format!(
                "disallowed bare name {name:?}"
            )));
        }
        let name = name.to_owned();
        let (positional, keywords) = self.arguments(depth)?;
        self.build_call(&name, positional, keywords)
    }

    #[allow(clippy::type_complexity)]
    fn arguments(
        &mut self,
        depth: usize,
    ) -> Result<(Vec<Value>, Vec<(String, Value)>), ProdromeError> {
        self.expect('(')?;
        let mut positional = Vec::new();
        let mut keywords: Vec<(String, Value)> = Vec::new();
        self.skip_trivia();
        if self.eat(')') {
            return Ok((positional, keywords));
        }
        loop {
            self.skip_trivia();
            if self.peek() == Some('*') {
                return Err(ProdromeError::parse(
                    "*args/**kwargs are not allowed in a call",
                ));
            }
            let keyword = self.keyword_name();
            match keyword {
                Some(key) => {
                    if keywords.iter().any(|(seen, _)| *seen == key) {
                        return Err(ProdromeError::parse(format!("field {key:?} given twice")));
                    }
                    let value = self.expression(depth + 1)?;
                    keywords.push((key, value));
                }
                None => {
                    if !keywords.is_empty() {
                        return Err(ProdromeError::parse("positional argument after a keyword"));
                    }
                    positional.push(self.expression(depth + 1)?);
                }
            }
            self.skip_trivia();
            if self.eat(')') {
                return Ok((positional, keywords));
            }
            self.expect(',')?;
            self.skip_trivia();
            if self.eat(')') {
                return Ok((positional, keywords));
            }
        }
    }

    /// `name=` at the cursor, consumed; nothing consumed otherwise. `==` is not
    /// a keyword and not in the grammar either way.
    fn keyword_name(&mut self) -> Option<String> {
        let start = self.pos;
        let mut end = self.pos;
        let mut chars = self.rest().char_indices();
        match chars.next() {
            Some((_, c)) if c.is_ascii_alphabetic() || c == '_' => end += c.len_utf8(),
            _ => return None,
        }
        for (_, c) in chars {
            if c.is_ascii_alphanumeric() || c == '_' {
                end += c.len_utf8();
            } else {
                break;
            }
        }
        let after = &self.input[end..];
        let trimmed = after.trim_start();
        let gap = after.len() - trimmed.len();
        if !trimmed.starts_with('=') || trimmed.starts_with("==") {
            return None;
        }
        let name = self.input[start..end].to_owned();
        self.pos = end + gap + 1;
        Some(name)
    }

    fn build_call(
        &self,
        name: &str,
        positional: Vec<Value>,
        keywords: Vec<(String, Value)>,
    ) -> Result<Value, ProdromeError> {
        match name {
            "datetime" => return build_datetime(positional, keywords),
            "timedelta" => return build_timedelta(positional, keywords),
            _ => {}
        }
        let signature = self
            .vocabulary
            .signature(name)
            .ok_or_else(|| ProdromeError::parse(format!("unknown constructor {name:?}")))?;
        let fields = match signature {
            Signature::AnyKeywords => {
                if !positional.is_empty() {
                    return Err(ProdromeError::parse(format!(
                        "{name}(...) takes keyword fields only in this vocabulary"
                    )));
                }
                keywords
            }
            Signature::Fields(order) => bind(name, order, positional, keywords)?,
            Signature::Owned(order) => bind(name, order, positional, keywords)?,
        };
        Ok(Value::Call(Call::new(name, fields)))
    }
}

/// Positional arguments onto declared field names, keywords checked against
/// them, the result in DECLARED order — which is what makes the print
/// canonical however the text was written.
fn bind(
    name: &str,
    order: &[impl AsRef<str>],
    positional: Vec<Value>,
    keywords: Vec<(String, Value)>,
) -> Result<Vec<(String, Value)>, ProdromeError> {
    if positional.len() > order.len() {
        return Err(ProdromeError::parse(format!(
            "bad call to {name}(...): takes at most {} arguments, got {}",
            order.len(),
            positional.len()
        )));
    }
    let taken = positional.len();
    let mut bound: Vec<(String, Value)> = order
        .iter()
        .take(taken)
        .map(|field| field.as_ref().to_owned())
        .zip(positional)
        .collect();
    for (key, value) in keywords {
        let index = order
            .iter()
            .position(|field| field.as_ref() == key)
            .ok_or_else(|| {
                ProdromeError::parse(format!("bad call to {name}(...): unexpected field {key:?}"))
            })?;
        // A keyword that names a field a positional argument already filled is
        // CPython's "got multiple values for argument".
        if index < taken {
            return Err(ProdromeError::parse(format!(
                "bad call to {name}(...): field {key:?} given twice"
            )));
        }
        bound.push((key, value));
    }
    let mut ordered: Vec<(String, Value)> = Vec::with_capacity(bound.len());
    for field in order {
        if let Some(index) = bound.iter().position(|(key, _)| key == field.as_ref()) {
            ordered.push(bound.remove(index));
        }
    }
    Ok(ordered)
}

fn integer_argument(name: &str, field: &str, value: &Value) -> Result<i64, ProdromeError> {
    match value {
        Value::Int(int) => int.as_i64().ok_or_else(|| {
            ProdromeError::parse(format!("bad call to {name}(...): {field} is out of range"))
        }),
        other => Err(ProdromeError::parse(format!(
            "bad call to {name}(...): {field} must be an integer, got {other:?}"
        ))),
    }
}

fn build_datetime(
    positional: Vec<Value>,
    keywords: Vec<(String, Value)>,
) -> Result<Value, ProdromeError> {
    const ORDER: &[&str] = &[
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "microsecond",
    ];
    let fields = bind("datetime", ORDER, positional, keywords)?;
    let mut parts = [0i64; 7];
    for (key, value) in &fields {
        let index = ORDER
            .iter()
            .position(|field| field == key)
            .expect("bind checked the name");
        parts[index] = integer_argument("datetime", key, value)?;
    }
    if fields.len() < 3 {
        return Err(ProdromeError::parse(
            "bad call to datetime(...): year, month and day are required",
        ));
    }
    let field = |index: usize| -> Result<u32, ProdromeError> {
        u32::try_from(parts[index]).map_err(|_| {
            ProdromeError::parse(format!(
                "bad call to datetime(...): {} is out of range",
                ORDER[index]
            ))
        })
    };
    let year = i32::try_from(parts[0])
        .map_err(|_| ProdromeError::parse("bad call to datetime(...): year is out of range"))?;
    Ok(Value::Datetime(Datetime::new(
        year,
        field(1)?,
        field(2)?,
        field(3)?,
        field(4)?,
        field(5)?,
        field(6)?,
    )?))
}

fn build_timedelta(
    positional: Vec<Value>,
    keywords: Vec<(String, Value)>,
) -> Result<Value, ProdromeError> {
    const ORDER: &[&str] = &[
        "days",
        "seconds",
        "microseconds",
        "milliseconds",
        "minutes",
        "hours",
        "weeks",
    ];
    let fields = bind("timedelta", ORDER, positional, keywords)?;
    let mut parts = [0i64; 7];
    for (key, value) in &fields {
        let index = ORDER
            .iter()
            .position(|field| field == key)
            .expect("bind checked the name");
        parts[index] = integer_argument("timedelta", key, value)?;
    }
    Ok(Value::Timedelta(Timedelta::new(
        parts[0], parts[1], parts[2], parts[3], parts[4], parts[5], parts[6],
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(text: &str) -> String {
        print_literal(&parse_literal(text, &Open).expect("parses"))
    }

    /// A field order held as owned names binds exactly as the same order in
    /// a static table: positional, keyword and refused alike.
    #[test]
    fn an_owned_field_order_binds_as_a_static_one() {
        struct Owned(Vec<String>);
        impl Vocabulary for Owned {
            fn signature(&self, name: &str) -> Option<Signature<'_>> {
                (name == "Moved").then_some(Signature::Owned(&self.0))
            }
        }
        let owned = Owned(vec!["pr".to_owned(), "phase".to_owned()]);
        let table = Table(&[("Moved", &["pr", "phase"])]);
        for text in [
            "Moved('a', 'b')",
            "Moved(phase='b', pr='a')",
            "Moved('a', phase='b')",
            "Moved('a', pr='b')",
            "Moved(x=1)",
            "Moved(1, 2, 3)",
        ] {
            assert_eq!(
                parse_literal(text, &owned),
                parse_literal(text, &table),
                "{text}"
            );
        }
    }

    #[test]
    fn floats_print_as_cpython_does() {
        for text in [
            "1.0",
            "-0.0",
            "0.1",
            "1e-05",
            "1e+16",
            "123456789012345.6",
            "0.30000000000000004",
            "2.5e-07",
            "1e+22",
            "-1.5",
            "0.0001",
            "1000000000000000.0",
        ] {
            assert_eq!(round_trip(text), text, "float {text}");
        }
    }

    #[test]
    fn strings_quote_and_escape_as_cpython_does() {
        for text in [
            r"'plain'",
            r#""it's""#,
            r#"'say "hi"'"#,
            r#"'both \' and "'"#,
            r"'tab\there'",
            r"'new\nline'",
            r"'back\\slash'",
            r"'\x00\x07\x1f'",
            // A zero-width space: Cf, so CPython escapes it rather than
            // emitting a character nobody can see.
            "'\\u200b zero width'",
            "'é ü ✓ 日本'",
            "'emoji 🙂'",
        ] {
            assert_eq!(round_trip(text), text, "string {text}");
        }
    }

    #[test]
    fn datetimes_and_timedeltas_normalise() {
        assert_eq!(
            round_trip("datetime(2026, 1, 1, 0, 0, 0)"),
            "datetime(2026, 1, 1, 0, 0, 0)"
        );
        assert_eq!(
            round_trip("datetime(2026, 9, 6, 7, 3, 0, 250)"),
            "datetime(2026, 9, 6, 7, 3, 0, 250)"
        );
        assert_eq!(round_trip("timedelta(hours=3)"), "timedelta(seconds=10800)");
        assert_eq!(round_trip("timedelta()"), "timedelta()");
        assert_eq!(round_trip("timedelta(days=-1)"), "timedelta(days=-1)");
        assert_eq!(
            round_trip("timedelta(seconds=-1)"),
            "timedelta(days=-1, seconds=86399)"
        );
    }

    #[test]
    fn tuples_keep_cpythons_one_element_comma() {
        assert_eq!(round_trip("()"), "()");
        assert_eq!(round_trip("(1,)"), "(1,)");
        assert_eq!(round_trip("(1, 'a')"), "(1, 'a')");
        assert_eq!(round_trip("((1, 2), (3,))"), "((1, 2), (3,))");
        // A parenthesised expression is not a tuple, exactly as in CPython.
        assert_eq!(round_trip("(1)"), "1");
        // A list display reads as a tuple and prints as one.
        assert_eq!(round_trip("[1, 2]"), "(1, 2)");
    }

    #[test]
    fn big_integers_survive_the_round_trip() {
        assert_eq!(round_trip("12345678901234567890"), "12345678901234567890");
        assert_eq!(round_trip("-7"), "-7");
        assert_eq!(round_trip("0"), "0");
        assert_eq!(round_trip("-0"), "0");
    }

    #[test]
    fn comments_and_surrounding_whitespace_are_trivia() {
        assert_eq!(round_trip("# a note\n\n  None  \n"), "None");
    }

    #[test]
    fn positional_arguments_bind_to_the_declared_order() {
        static TABLE: Table = Table(&[("Pair", &["left", "right"])]);
        let value = parse_literal("Pair(1, right=2)", &TABLE).expect("parses");
        assert_eq!(print_literal(&value), "Pair(left=1, right=2)");
        // Out-of-order keywords still print in declared order.
        let value = parse_literal("Pair(right=2, left=1)", &TABLE).expect("parses");
        assert_eq!(print_literal(&value), "Pair(left=1, right=2)");
    }

    #[test]
    fn the_vocabulary_is_the_grammar() {
        static TABLE: Table = Table(&[("Pair", &["left", "right"])]);
        for text in [
            "Nope(1)",
            "Pair(1, 2, 3)",
            "Pair(nope=1)",
            "Pair(1, left=2)",
            "Pair(*x)",
            "x",
            "1 + 1",
            "Pair(1).left",
            "{1: 2}",
            "lambda: 1",
            "[x for x in y]",
            "",
            "None None",
            "b'bytes'",
            "1j",
            // The vectors `tests/test_literals.py`'s adversarial suite drew,
            // kept when that suite was deleted with the Python (stage 6). They
            // are refused here BY CONSTRUCTION rather than by a rule — this is
            // a closed-vocabulary grammar and not a walker over somebody
            // else's AST, so there is no `__import__` to reach and no
            // attribute to walk. That is a stronger property than the Python
            // had; it is pinned anyway, because a reader deciding whether the
            // parser is a security boundary should be able to see the attacks
            // it was written against rather than infer them from a design.
            "__import__('os').system('id')",
            "Pair(1, 2).__class__",
            "Pair(1, 2).__class__.__bases__",
            "f'{1}'",
            "Pair(**{'left': 1})",
            "(x := 1)",
            "Pair(1, 2); Pair(3, 4)",
            "open('/etc/passwd')",
            "eval('1')",
            "Pair(1, 2) if True else None",
        ] {
            assert!(parse_literal(text, &TABLE).is_err(), "must refuse {text:?}");
        }
    }
}
