//! The literal bridge: a term as one expression of the grammar of SPEC §2.
//! The grammar's time records carry no arithmetic and `chrono`'s do; they
//! meet here.

use chrono::Duration;

use prodrome_data::literal::{self, print_literal, Call, Finite, ProdromeError};

use super::{datetime_of, err, instant_of, us_of, Delta, FplError, Instant};
use crate::term::schema::{self, signatures, Field, FieldSource, Fields, Slot};
use crate::term::Term;

fn float_value(value: f64) -> literal::Value {
    literal::Value::Float(
        Finite::new(value).expect("a Term's floats are finite: every mk_* refuses the rest"),
    )
}

fn instant_value(t: Instant) -> literal::Value {
    literal::Value::Datetime(datetime_of(t).expect(
        "an instant of a term of the smart constructors is in the years 1..=9999; \
             a normal form is for evaluation and is not printed",
    ))
}

fn delta_value(d: Delta) -> literal::Value {
    literal::Value::Timedelta(
        literal::Timedelta::from_micros(us_of(d))
            .expect("a Term's spans passed admit_span, which is inside timedelta's range"),
    )
}

fn call_value(name: &str, fields: Fields<&Term>) -> literal::Value {
    literal::Value::call(
        name,
        fields
            .into_iter()
            .map(|(field, slot)| (field.to_owned(), slot_value(slot)))
            .collect(),
    )
}

fn slot_value(slot: Slot<&Term>) -> literal::Value {
    match slot {
        Slot::Real(value) => float_value(value),
        Slot::At(t) => instant_value(t),
        Slot::Span(d) => delta_value(d),
        Slot::Text(text) => literal::Value::Str(text),
        Slot::Nothing => literal::Value::None,
        Slot::Child(term) => term.to_value(),
        Slot::Children(terms) => {
            literal::Value::Tuple(terms.into_iter().map(Term::to_value).collect())
        }
        Slot::Rows(row, rows) => literal::Value::Tuple(
            rows.into_iter()
                .map(|fields| call_value(row, fields))
                .collect(),
        ),
    }
}

/// A call's fields, `None` or left out being absent.
struct CallSource<'a>(&'a Call);

impl<'a> CallSource<'a> {
    fn read<T>(
        &self,
        name: &'static str,
        decode: impl FnOnce(&'a literal::Value) -> Option<T>,
    ) -> Field<T> {
        Field::new(
            name,
            match self.0.field(name) {
                None | Some(literal::Value::None) => Ok(None),
                Some(value) => decode(value).map(Some).ok_or_else(|| {
                    FplError(format!("{}.{name} is malformed: {value:?}", self.0.name))
                }),
            },
        )
    }
}

impl FieldSource for CallSource<'_> {
    fn real(&mut self, name: &'static str) -> Field<f64> {
        self.read(name, |value| match value {
            literal::Value::Float(value) => Some(value.get()),
            literal::Value::Int(int) => int.as_i64().map(|value| value as f64),
            _ => None,
        })
    }

    fn at(&mut self, name: &'static str) -> Field<Instant> {
        self.read(name, |value| match value {
            literal::Value::Datetime(at) => Some(instant_of(*at)),
            _ => None,
        })
    }

    fn span(&mut self, name: &'static str) -> Field<Delta> {
        self.read(name, |value| match value {
            literal::Value::Timedelta(d) => i64::try_from(d.total_micros())
                .ok()
                .map(Duration::microseconds),
            _ => None,
        })
    }

    fn text(&mut self, name: &'static str) -> Field<String> {
        self.read(name, |value| value.as_str().map(str::to_owned))
    }

    fn child(&mut self, name: &'static str) -> Field<Term> {
        self.read(name, Some).and_then(Term::from_value)
    }

    fn children(&mut self, name: &'static str) -> Field<Vec<Term>> {
        self.read(name, literal::Value::as_tuple)
            .and_then(|items| items.iter().map(Term::from_value).collect())
    }

    fn rows<T>(
        &mut self,
        name: &'static str,
        row: &'static str,
        mut read: impl FnMut(&mut Self) -> Result<T, FplError>,
    ) -> Field<Vec<T>> {
        self.read(name, literal::Value::as_tuple).and_then(|items| {
            items
                .iter()
                .map(|item| match item.as_call() {
                    Some(call) if call.name == row => read(&mut CallSource(call)),
                    _ => err(format!("expected a {row}(...), got {item:?}")),
                })
                .collect()
        })
    }
}

impl Term {
    /// The term as one expression of the grammar of SPEC §2.
    pub fn to_value(&self) -> literal::Value {
        let (kind, fields) = schema::fields(self.out());
        call_value(kind, fields)
    }

    /// The term that one expression of the grammar of SPEC §2 denotes, built
    /// by the smart constructors.
    pub fn from_value(value: &literal::Value) -> Result<Term, FplError> {
        let call = value
            .as_call()
            .ok_or_else(|| FplError(format!("expected a term constructor, got {value:?}")))?;
        schema::build(&call.name, &mut CallSource(call))
    }
}

/// A term in a field of a stored type: an optional spec, for one. The FPL
/// signature table ([`schema::fields`] and [`schema::build`]) gives the
/// print and the read, so the impl is written here and not derived.
impl literal::codec::Literal for Term {
    fn to_value(&self) -> literal::Value {
        Term::to_value(self)
    }

    fn from_value(value: &literal::Value) -> Result<Term, ProdromeError> {
        Ok(Term::from_value(value)?)
    }
}

/// The canonical print of a term (SPEC §2).
pub fn print_term(term: &Term) -> String {
    print_literal(&term.to_value())
}

/// The inverse of [`print_term`], through the smart constructors.
pub fn parse_term(text: &str) -> Result<Term, FplError> {
    Term::from_value(&literal::parse_literal(text, signatures())?)
}
