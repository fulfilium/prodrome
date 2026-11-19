//! THE REFERENCE PAYLOAD — the record shape `conformance/*.py` was taken
//! with, and an example of [`crate::payload::Payload`] written out in full.
//!
//! `Authored(todo, at, actor, kind, created, body, spec, rationale, category,
//! waiting_on, detail, source, subtodos, notes, note)`: one deployment's
//! decorations of a fulfillment curve — a todo's text in two rendering
//! languages, the message it came from, a checklist, comment runs — frozen here
//! because the vectors' bytes are frozen. Every `Authored(...)` print in
//! `conformance/*.py` parses and prints back byte for byte under this
//! payload; that is the whole of what this module is for.
//!
//! IT IS NOT THE FORMAT. A host defines its own [`Payload`] and gets its own
//! record kind; nothing in `event`, `store`, `fold`, `registers`, `view` or
//! `breaks` knows a field named here. Behind the default `reference` feature so
//! a host that wants none of it can turn it off.
//!
//! `MarkupSource` and `StringSource` are opaque text in this host's two
//! rendering languages; the database stores and prints them and never
//! interprets them.

use crate::event::{mk_record, newtype_str, Name, TodoEvent};
use crate::literal::codec::{blank_field, text_field, Constructor, Literal, Record};
use crate::literal::{Datetime, ProdromeError};
use crate::payload::Payload;
use crate::term::Term;

newtype_str! {
    /// Typst MARKUP source — the inside of a `[..]` block. A different type
    /// from [`StringSource`] because `]` is structure in one and `"` in the
    /// other, so a value of one language in a field of the other is a bug the
    /// reparse would catch late.
    #[derive(Default, Literal)]
    MarkupSource
}

newtype_str! {
    /// Typst STRING source — the inside of a `".."` literal.
    #[derive(Default, Literal)]
    StringSource
}

impl MarkupSource {
    pub fn new(text: impl Into<String>) -> MarkupSource {
        MarkupSource(text.into())
    }
}

impl StringSource {
    pub fn new(text: impl Into<String>) -> StringSource {
        StringSource(text.into())
    }
}

/// Where a [`Note`] may attach: a field of the record, or the block as a whole.
/// A closed set, so a note about `waiting_on` cannot be read as one about
/// `detail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoteSite {
    Block,
    Tail,
    Todo,
    Created,
    Category,
    WaitingOn,
    Source,
    Detail,
    Subtodos,
}

impl NoteSite {
    pub const ALL: [NoteSite; 9] = [
        NoteSite::Block,
        NoteSite::Tail,
        NoteSite::Todo,
        NoteSite::Created,
        NoteSite::Category,
        NoteSite::WaitingOn,
        NoteSite::Source,
        NoteSite::Detail,
        NoteSite::Subtodos,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            NoteSite::Block => "block",
            NoteSite::Tail => "tail",
            NoteSite::Todo => "todo",
            NoteSite::Created => "created",
            NoteSite::Category => "category",
            NoteSite::WaitingOn => "waiting_on",
            NoteSite::Source => "source",
            NoteSite::Detail => "detail",
            NoteSite::Subtodos => "subtodos",
        }
    }

    /// The parse boundary: a stored literal arrives as a bare string.
    ///
    /// # Errors
    ///
    /// The text is not the word of a site.
    pub fn parse(text: &str) -> Result<NoteSite, ProdromeError> {
        NoteSite::ALL
            .into_iter()
            .find(|site| site.as_str() == text)
            .ok_or_else(|| {
                let mut known: Vec<&str> = NoteSite::ALL.iter().map(|site| site.as_str()).collect();
                known.sort_unstable();
                ProdromeError::invalid(format!("Note.on must be one of {known:?}, got {text:?}"))
            })
    }
}

/// A site prints as its word.
impl AsRef<str> for NoteSite {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

// THE STORED FORMS. Each type below derives `Literal`, so its constructor's
// name, its fields' names and their order are the declaration's, and the
// print and the read follow from them. A read gives each field to the
// type's smart constructor (`new`), in field order, so it refuses what the
// smart constructor refuses. A field marked `default` reads as `''`, `()`
// or `None` where a call lacks it. `core/tests/all/derive.rs` pins the
// constructors, so a rename that changes a stored name fails a test.

/// The message a todo came from. `date` is a day; the three identifiers are
/// `""` when the era that authored the todo did not record them.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
pub struct Source {
    pub sender: StringSource,
    pub subject: StringSource,
    pub date: Datetime,
    #[literal(default)]
    pub hash: StringSource,
    #[literal(default)]
    pub thread_id: StringSource,
    #[literal(default)]
    pub message_id: StringSource,
}

/// A comment run attached to one field of a record.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
#[literal(new = mk_note)]
pub struct Note {
    #[literal(with = text_field)]
    pub on: NoteSite,
    pub lines: Vec<String>,
}

/// A checklist item inside a todo: no id, no history of its own.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
#[literal(new = mk_subtodo)]
pub struct SubTodo {
    pub body: String,
    pub done: bool,
}

/// THE PAYLOAD: everything this host attaches to a todo, beyond the `todo`,
/// `at` and `actor` the core owns. Its record kind is `Authored`.
#[derive(Debug, Clone, PartialEq, Literal)]
#[literal(rename = "Authored", new = mk_todo)]
pub struct Todo {
    #[literal(with = text_field)]
    pub kind: Name,
    pub created: Datetime,
    #[literal(with = text_field)]
    pub body: MarkupSource,
    pub spec: Option<Term>,
    #[literal(default)]
    pub rationale: Vec<String>,
    #[literal(default, with = blank_field)]
    pub category: Option<Name>,
    #[literal(default, with = text_field)]
    pub waiting_on: StringSource,
    #[literal(default, with = text_field)]
    pub detail: MarkupSource,
    #[literal(default)]
    pub source: Option<Source>,
    #[literal(default)]
    pub subtodos: Vec<SubTodo>,
    #[literal(default)]
    pub notes: Vec<Note>,
    #[literal(default)]
    pub note: String,
}

// --- smart constructors ------------------------------------------------------

pub fn mk_source(
    sender: &str,
    subject: &str,
    date: Datetime,
    hash: &str,
    thread_id: &str,
    message_id: &str,
) -> Source {
    Source {
        sender: StringSource::new(sender),
        subject: StringSource::new(subject),
        date,
        hash: StringSource::new(hash),
        thread_id: StringSource::new(thread_id),
        message_id: StringSource::new(message_id),
    }
}

/// A note on the site `on`.
///
/// # Errors
///
/// `lines` is empty, or `on` is not the word of a site, in that order.
pub fn mk_note(on: impl AsRef<str>, lines: Vec<String>) -> Result<Note, ProdromeError> {
    if lines.is_empty() {
        return Err(ProdromeError::invalid(
            "Note.lines must be nonempty — an empty note is not a note",
        ));
    }
    Ok(Note {
        on: NoteSite::parse(on.as_ref())?,
        lines,
    })
}

/// A checklist item.
///
/// # Errors
///
/// `body` is blank.
pub fn mk_subtodo(body: impl Into<String>, done: bool) -> Result<SubTodo, ProdromeError> {
    let body = body.into();
    if body.trim().is_empty() {
        return Err(ProdromeError::invalid("SubTodo.body cannot be empty"));
    }
    Ok(SubTodo { body, done })
}

/// Every field of this payload, in declared order. Long by construction: the
/// shipped field set is frozen, so a builder that let one be forgotten would be
/// a way to print a different object under the same name.
///
/// # Errors
///
/// `body` is blank, `kind` is not an identifier, or `category` is neither
/// `""` nor an identifier, in that order.
#[allow(clippy::too_many_arguments)]
pub fn mk_todo(
    kind: impl Into<String>,
    created: Datetime,
    body: impl Into<String>,
    spec: Option<Term>,
    rationale: Vec<String>,
    category: impl Into<String>,
    waiting_on: impl Into<String>,
    detail: impl Into<String>,
    source: Option<Source>,
    subtodos: Vec<SubTodo>,
    notes: Vec<Note>,
    note: impl Into<String>,
) -> Result<Todo, ProdromeError> {
    let body = body.into();
    if body.trim().is_empty() {
        return Err(ProdromeError::invalid("Authored.body cannot be empty"));
    }
    Ok(Todo {
        kind: Name::new("Authored.kind", kind)?,
        created,
        body: MarkupSource::new(body),
        spec,
        rationale,
        category: Name::optional("Authored.category", category)?,
        waiting_on: StringSource::new(waiting_on),
        detail: MarkupSource::new(detail),
        source,
        subtodos,
        notes,
        note: note.into(),
    })
}

/// The whole record: the core's three fields and this payload's twelve — the
/// constructor every test and every vector in this repository is written
/// against.
#[allow(clippy::too_many_arguments)]
pub fn mk_authored(
    todo: &str,
    at: Datetime,
    actor: &str,
    kind: &str,
    created: Datetime,
    body: &str,
    spec: Option<Term>,
    rationale: Vec<String>,
    category: &str,
    waiting_on: &str,
    detail: &str,
    source: Option<Source>,
    subtodos: Vec<SubTodo>,
    notes: Vec<Note>,
    note: &str,
) -> Result<TodoEvent<Todo>, ProdromeError> {
    mk_record(
        todo,
        at,
        actor,
        mk_todo(
            kind, created, body, spec, rationale, category, waiting_on, detail, source, subtodos,
            notes, note,
        )?,
    )
}

// --- the payload -------------------------------------------------------------

impl Payload for Todo {
    const VOCABULARY: &'static [Constructor] =
        &[Source::CONSTRUCTOR, Note::CONSTRUCTOR, SubTodo::CONSTRUCTOR];

    fn spec(&self) -> Option<&Term> {
        self.spec.as_ref()
    }

    fn checklist_len(&self) -> usize {
        self.subtodos.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ENVELOPE_SIGNATURES;
    use crate::payload::record_signature;
    use crate::term::schema::signatures;
    use crate::todo::TODO_SIGNATURES;

    #[test]
    fn the_smart_constructors_are_the_only_door() {
        assert!(mk_note("nowhere", vec!["x".into()]).is_err());
        assert!(mk_note("block", vec![]).is_err());
        assert!(mk_subtodo("  ", false).is_err());
        let at = Datetime::new(2026, 9, 6, 7, 3, 0, 0).expect("a real instant");
        assert!(mk_todo(
            "todo",
            at,
            "   ",
            None,
            vec![],
            "",
            "",
            "",
            None,
            vec![],
            vec![],
            ""
        )
        .is_err());
    }

    /// The stored shape: `Authored(todo, at, actor, <the twelve>)`, and the
    /// print is in exactly that order.
    #[test]
    fn the_record_signature_is_the_cores_three_then_this_payloads_twelve() {
        assert_eq!(
            record_signature::<Todo>(),
            vec![
                "todo",
                "at",
                "actor",
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
            ]
        );
        let at = Datetime::new(2026, 9, 6, 7, 3, 0, 0).expect("a real instant");
        let todo = mk_todo(
            "todo",
            at,
            "body",
            None,
            vec![],
            "",
            "",
            "",
            None,
            vec![],
            vec![],
            "",
        )
        .expect("valid");
        let named: Vec<&str> = todo.fields().into_iter().map(|(name, _)| name).collect();
        assert_eq!(
            named,
            Todo::CONSTRUCTOR.1,
            "fields() answers with FIELDS, in order"
        );
    }

    /// §2's whitelist is a UNION, so a payload that reused one of the core's
    /// names would be unreachable rather than in force. This payload does not.
    #[test]
    fn the_payloads_names_do_not_collide_with_the_cores() {
        let core: Vec<&str> = signatures()
            .names()
            .chain(
                ENVELOPE_SIGNATURES
                    .iter()
                    .chain(TODO_SIGNATURES)
                    .map(|(name, _)| *name),
            )
            .collect();
        for (name, _) in Todo::VOCABULARY {
            assert!(!core.contains(name), "{name} is one of the core's names");
        }
        assert!(!core.contains(&Todo::CONSTRUCTOR.0));
    }
}
