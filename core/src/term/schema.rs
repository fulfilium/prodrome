use std::sync::OnceLock;

use crate::syntax::{
    mk_absent, mk_after, mk_conj, mk_curve, mk_decay, mk_flat, mk_gate, mk_importance, mk_least,
    mk_offset, mk_offset_by, mk_periodic, mk_piecewise, mk_recur, mk_ref, mk_ref_in, mk_shift,
    mk_within, Delta, FplError, Instant, PRIORITY_POWER,
};
use prodrome_data::literal::{Signature, Vocabulary};

use super::{CurvePoint, Term, TermF};

/// One field of a layer. Every format renders and reads a term's fields
/// through this type, so it is the one place where the fields are named.
#[derive(Debug, Clone, PartialEq)]
pub enum Slot<A> {
    Real(f64),
    At(Instant),
    Span(Delta),
    Text(String),
    /// An optional field left unset.
    Nothing,
    Child(A),
    Children(Vec<A>),
    /// A tuple of records, each a `row` constructor with its own fields.
    Rows(&'static str, Vec<Fields<A>>),
}

pub type Fields<A> = Vec<(&'static str, Slot<A>)>;

impl<A> Slot<A> {
    pub fn holds_term(&self) -> bool {
        match self {
            Slot::Child(_) | Slot::Children(_) => true,
            Slot::Rows(_, rows) => rows.iter().flatten().any(|(_, slot)| slot.holds_term()),
            _ => false,
        }
    }
}

/// The constructor's name and its fields, in declared order.
pub fn fields<A>(layer: &TermF<A>) -> (&'static str, Fields<&A>) {
    use Slot::*;
    match layer {
        TermF::Flat { value } => ("Flat", vec![("value", Real(*value))]),
        TermF::Decay {
            start,
            end,
            end_date,
            lead_up,
            start_date,
        } => (
            "Decay",
            vec![
                ("start", Real(*start)),
                ("end", Real(*end)),
                ("end_date", At(*end_date)),
                ("lead_up", Span(*lead_up)),
                ("start_date", start_date.map_or(Nothing, At)),
            ],
        ),
        TermF::Curve { points } => (
            "Curve",
            vec![(
                "points",
                Rows(
                    "CurvePoint",
                    points
                        .iter()
                        .map(|point| {
                            vec![
                                ("at", At(point.at)),
                                ("value", Real(point.value)),
                                ("label", Text(point.label.clone())),
                            ]
                        })
                        .collect(),
                ),
            )],
        ),
        TermF::Conj { terms, p } => (
            "Conj",
            vec![("terms", Children(terms.iter().collect())), ("p", Real(*p))],
        ),
        TermF::Least { terms } => ("Least", vec![("terms", Children(terms.iter().collect()))]),
        TermF::Offset { delta, term } => (
            "Offset",
            vec![("delta", Real(*delta)), ("term", Child(term))],
        ),
        TermF::Gate { gate, body } => ("Gate", vec![("gate", Child(gate)), ("body", Child(body))]),
        TermF::Shift { delta, term } => (
            "Shift",
            vec![("delta", Span(*delta)), ("term", Child(term))],
        ),
        TermF::Within { window, p, term } => (
            "Within",
            vec![
                ("window", Span(*window)),
                ("p", Real(*p)),
                ("term", Child(term)),
            ],
        ),
        TermF::Importance { w, term } => {
            ("Importance", vec![("w", Real(*w)), ("term", Child(term))])
        }
        TermF::After {
            event,
            anchor,
            term,
            pending,
            needs,
        } => (
            "After",
            vec![
                ("event", Text(event.clone())),
                ("anchor", At(*anchor)),
                ("term", Child(term)),
                ("pending", Child(pending)),
                ("needs", needs.map_or(Nothing, Span)),
            ],
        ),
        TermF::Recur {
            todo,
            anchor,
            term,
            pending,
        } => (
            "Recur",
            vec![
                ("todo", Text(todo.clone())),
                ("anchor", At(*anchor)),
                ("term", Child(term)),
                ("pending", Child(pending)),
            ],
        ),
        TermF::Periodic {
            period,
            anchor,
            term,
        } => (
            "Periodic",
            vec![
                ("period", Span(*period)),
                ("anchor", At(*anchor)),
                ("term", Child(term)),
            ],
        ),
        TermF::Piecewise(schedule) => (
            "Piecewise",
            vec![
                ("head", Child(schedule.head())),
                (
                    "pieces",
                    Rows(
                        "Piece",
                        schedule
                            .knots()
                            .iter()
                            .map(|(at, term)| vec![("at", At(*at)), ("term", Child(term))])
                            .collect(),
                    ),
                ),
            ],
        ),
        TermF::OffsetBy { delta, term } => (
            "OffsetBy",
            vec![("delta", Child(delta)), ("term", Child(term))],
        ),
        TermF::Ref {
            store: None,
            entity,
        } => ("Ref", vec![("todo", Text(entity.clone()))]),
        TermF::Ref {
            store: Some(store),
            entity,
        } => (
            "RefIn",
            vec![
                ("store", Text(store.clone())),
                ("entity", Text(entity.clone())),
            ],
        ),
        TermF::Absent => ("Absent", vec![]),
    }
}

/// A format's reader of the fields of one constructor.
pub trait FieldSource: Sized {
    fn real(&mut self, name: &'static str) -> Field<f64>;
    fn at(&mut self, name: &'static str) -> Field<Instant>;
    fn span(&mut self, name: &'static str) -> Field<Delta>;
    fn text(&mut self, name: &'static str) -> Field<String>;
    fn child(&mut self, name: &'static str) -> Field<Term>;
    fn children(&mut self, name: &'static str) -> Field<Vec<Term>>;
    fn rows<T>(
        &mut self,
        name: &'static str,
        row: &'static str,
        read: impl FnMut(&mut Self) -> Result<T, FplError>,
    ) -> Field<Vec<T>>;
}

/// One field as a format read it: absent, decoded, or refused.
pub struct Field<T> {
    name: &'static str,
    read: Result<Option<T>, FplError>,
}

impl<T> Field<T> {
    pub fn new(name: &'static str, read: Result<Option<T>, FplError>) -> Field<T> {
        Field { name, read }
    }

    pub fn and_then<U>(self, decode: impl FnOnce(T) -> Result<U, FplError>) -> Field<U> {
        Field::new(
            self.name,
            self.read.and_then(|read| read.map(decode).transpose()),
        )
    }

    fn need(self) -> Result<T, FplError> {
        self.read?
            .ok_or_else(|| FplError(format!("missing field {:?}", self.name)))
    }

    fn or(self, default: T) -> Result<T, FplError> {
        Ok(self.read?.unwrap_or(default))
    }

    fn maybe(self) -> Result<Option<T>, FplError> {
        self.read
    }
}

impl Field<String> {
    fn or_default(self) -> Result<String, FplError> {
        self.or(String::new())
    }
}

type Builder<S> = fn(&mut S) -> Result<Term, FplError>;

/// Each constructor read in declared order; the defaults are what a hand-written
/// literal may leave out.
fn builders<S: FieldSource>() -> [(&'static str, Builder<S>); 18] {
    [
        ("Flat", |s| mk_flat(s.real("value").need()?)),
        ("Decay", |s| {
            mk_decay(
                s.real("start").need()?,
                s.real("end").need()?,
                s.at("end_date").need()?,
                s.span("lead_up").or(Delta::weeks(1))?,
                s.at("start_date").maybe()?,
            )
        }),
        ("Curve", |s| {
            mk_curve(
                s.rows("points", "CurvePoint", |point| {
                    Ok(CurvePoint {
                        at: point.at("at").need()?,
                        value: point.real("value").need()?,
                        label: point.text("label").or_default()?,
                    })
                })
                .need()?,
            )
        }),
        ("Conj", |s| {
            mk_conj(s.children("terms").need()?, s.real("p").or(PRIORITY_POWER)?)
        }),
        ("Least", |s| mk_least(s.children("terms").need()?)),
        ("Offset", |s| {
            mk_offset(s.real("delta").need()?, s.child("term").need()?)
        }),
        ("Gate", |s| {
            mk_gate(s.child("gate").need()?, s.child("body").need()?)
        }),
        ("Shift", |s| {
            mk_shift(s.span("delta").need()?, s.child("term").need()?)
        }),
        ("Within", |s| {
            mk_within(
                s.span("window").need()?,
                s.real("p").need()?,
                s.child("term").need()?,
            )
        }),
        ("Importance", |s| {
            mk_importance(s.real("w").need()?, s.child("term").need()?)
        }),
        ("After", |s| {
            mk_after(
                s.text("event").or_default()?,
                s.at("anchor").need()?,
                s.child("term").need()?,
                s.child("pending").need()?,
                s.span("needs").maybe()?,
            )
        }),
        ("Recur", |s| {
            mk_recur(
                s.text("todo").or_default()?,
                s.at("anchor").need()?,
                s.child("term").need()?,
                s.child("pending").need()?,
            )
        }),
        ("Periodic", |s| {
            mk_periodic(
                s.span("period").need()?,
                s.at("anchor").need()?,
                s.child("term").need()?,
            )
        }),
        ("Piecewise", |s| {
            mk_piecewise(
                s.child("head").need()?,
                s.rows("pieces", "Piece", |piece| {
                    Ok((piece.at("at").need()?, piece.child("term").need()?))
                })
                .need()?,
            )
        }),
        ("OffsetBy", |s| {
            mk_offset_by(s.child("delta").need()?, s.child("term").need()?)
        }),
        ("Ref", |s| mk_ref(s.text("todo").or_default()?)),
        ("RefIn", |s| {
            mk_ref_in(
                s.text("store").or_default()?,
                s.text("entity").or_default()?,
            )
        }),
        ("Absent", |_| Ok(mk_absent())),
    ]
}

/// The constructor `kind`, built by its `mk_*` function from the fields that
/// `source` reads.
pub fn build(kind: &str, source: &mut impl FieldSource) -> Result<Term, FplError> {
    let (_, builder) = builders()
        .into_iter()
        .find(|(name, _)| *name == kind)
        .ok_or_else(|| FplError(format!("{kind:?} is not one of SPEC §7's terms")))?;
    builder(source)
}

/// The constructor names of SPEC §7 and their declared field order, as a
/// literal vocabulary.
pub struct Signatures(Vec<(&'static str, Vec<&'static str>)>);

impl Signatures {
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.0.iter().map(|(name, _)| *name)
    }
}

impl Vocabulary for Signatures {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        self.0
            .iter()
            .find(|(kind, _)| *kind == name)
            .map(|(_, fields)| Signature::Fields(fields))
    }
}

/// The signatures, which the first call records from the fields that each
/// builder reads.
pub fn signatures() -> &'static Signatures {
    static SIGNATURES: OnceLock<Signatures> = OnceLock::new();
    SIGNATURES.get_or_init(|| {
        let mut recorder = Recorder::default();
        for (kind, builder) in builders::<Recorder>() {
            recorder.open(kind);
            // Placeholders may be refused by `mk_*`; every field was read first.
            let _ = builder(&mut recorder);
        }
        Signatures(recorder.table)
    })
}

/// A source that answers every read with a placeholder and notes its name.
#[derive(Default)]
struct Recorder {
    table: Vec<(&'static str, Vec<&'static str>)>,
    open: usize,
}

impl Recorder {
    fn open(&mut self, kind: &'static str) {
        self.open = match self.table.iter().position(|(name, _)| *name == kind) {
            Some(at) => at,
            None => {
                self.table.push((kind, Vec::new()));
                self.table.len() - 1
            }
        };
    }

    fn note<T>(&mut self, name: &'static str, placeholder: T) -> Field<T> {
        self.table[self.open].1.push(name);
        Field::new(name, Ok(Some(placeholder)))
    }
}

impl FieldSource for Recorder {
    fn real(&mut self, name: &'static str) -> Field<f64> {
        self.note(name, 0.0)
    }

    fn at(&mut self, name: &'static str) -> Field<Instant> {
        self.note(name, Instant::default())
    }

    fn span(&mut self, name: &'static str) -> Field<Delta> {
        self.note(name, Delta::zero())
    }

    fn text(&mut self, name: &'static str) -> Field<String> {
        self.note(name, String::new())
    }

    fn child(&mut self, name: &'static str) -> Field<Term> {
        self.note(name, mk_absent())
    }

    fn children(&mut self, name: &'static str) -> Field<Vec<Term>> {
        self.note(name, Vec::new())
    }

    fn rows<T>(
        &mut self,
        name: &'static str,
        row: &'static str,
        mut read: impl FnMut(&mut Self) -> Result<T, FplError>,
    ) -> Field<Vec<T>> {
        let field = self.note(name, Vec::new());
        let parent = self.open;
        self.open(row);
        let _ = read(self);
        self.open = parent;
        field
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fpl::parse_term;

    /// `build` reads each constructor in the order `fields` writes it.
    #[test]
    fn the_signatures_are_the_fields_in_declared_order() {
        let at = "datetime(2026, 9, 1, 0, 0, 0)";
        let term = parse_term(&format!(
            "Conj(terms=(Flat(value=0.5), Decay(start=0.5, end=0.1, end_date={at}), \
             Curve(points=(CurvePoint(at={at}, value=0.2),)), Offset(delta=0.1, term=Absent()), \
             Gate(gate=Absent(), body=Ref(todo='a')), Shift(delta=timedelta(days=1), term=Absent()), \
             Within(window=timedelta(days=1), p=-1.0, term=Absent()), Importance(w=2.0, term=Absent()), \
             After(event='a', anchor={at}, term=Absent(), pending=Absent()), \
             Recur(todo='a', anchor={at}, term=Absent(), pending=Absent()), \
             Periodic(period=timedelta(days=7), anchor={at}, term=Absent()), \
             Piecewise(head=Absent(), pieces=(Piece(at={at}, term=Flat(value=0.5)),)), \
             OffsetBy(delta=Absent(), term=Absent()), Least(terms=(Absent(),)), \
             RefIn(store='todos', entity='a')), p=-4.0)"
        ))
        .expect("a term");
        let mut seen = Vec::new();
        term.cata(|layer: TermF<()>| {
            let (kind, fields) = fields(&layer);
            seen.push((
                kind,
                fields.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            ));
        });
        seen.push(("CurvePoint", vec!["at", "value", "label"]));
        seen.push(("Piece", vec!["at", "term"]));
        for (kind, names) in seen {
            assert_eq!(
                signatures().signature(kind),
                Some(Signature::Fields(&names[..])),
                "{kind}"
            );
        }
        assert_eq!(signatures().0.len(), 20);
    }
}
