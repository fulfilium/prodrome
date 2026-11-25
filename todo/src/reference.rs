//! The reference payload: the record shape that `conformance/*.py` was
//! taken with, and an example of a [`crate::payload::Payload`].
//!
//! Its record kind is `Authored(todo, at, actor, kind, created, body, spec,
//! rationale, category, waiting_on, detail, source, subtodos, notes, note)`.
//! Every `Authored(...)` print in `conformance/*.py` parses and prints back
//! byte for byte under this payload, so its fields never change. The default
//! feature `reference` enables the module.

use crate::event::{mk_record, Name, TodoEvent};
use crate::payload::Payload;
use prodrome_data::literal::codec::{blank_field, text_field, Constructor, Literal, Record};
use prodrome_data::literal::{Datetime, ProdromeError};
use prodrome_data::newtype_str;
use prodrome_term::term::Term;

newtype_str! {
    /// Typst markup source: the inside of a `[..]` block. The core stores and
    /// prints it, and never interprets it. It is a different type from
    /// [`StringSource`], because `]` is structure in markup and `"` is
    /// structure in a string.
    #[derive(Default, Literal)]
    #[literal(crate = prodrome_data)]
    MarkupSource
}

newtype_str! {
    /// Typst string source: the inside of a `".."` literal. The core stores
    /// and prints it, and never interprets it.
    #[derive(Default, Literal)]
    #[literal(crate = prodrome_data)]
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

/// Where a [`Note`] attaches: a field of the record, or the block as a whole.
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

    /// Reads a site from its word, which a stored literal holds as a string.
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

/// The message a todo came from. `date` is a day. The three identifiers are
/// `""` where the writer of the todo did not record them.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
#[literal(crate = prodrome_data)]
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
#[literal(crate = prodrome_data)]
#[literal(new = mk_note)]
pub struct Note {
    #[literal(with = text_field)]
    pub on: NoteSite,
    pub lines: Vec<String>,
}

/// A checklist item inside a todo: no id, no history of its own.
#[derive(Debug, Clone, PartialEq, Eq, Literal)]
#[literal(crate = prodrome_data)]
#[literal(new = mk_subtodo)]
pub struct SubTodo {
    pub body: String,
    pub done: bool,
}

/// The reference payload: the fields of a record beyond the core's `todo`,
/// `at` and `actor`. Its record kind is `Authored`.
#[derive(Debug, Clone, PartialEq, Literal)]
#[literal(crate = prodrome_data)]
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

/// Every field of this payload, in declared order. The function takes every
/// field, because a builder that let a caller forget one would print a
/// different object.
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

/// The whole record: the core's three fields, then this payload's twelve.
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
    use crate::todo::TODO_SIGNATURES;
    use prodrome_term::term::schema::signatures;

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
