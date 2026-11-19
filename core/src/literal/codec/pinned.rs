//! A pin of a schema's stored names: a test that fails when a constructor
//! or a field changes its name or its place.
//!
//! The name of a constructor and the names and the order of its fields are
//! in every stored object, so a schema pins them in a test. The test holds
//! the constructors as text, one `Name(field, …)` on each line, and
//! [`assert_pinned`] compares the text with the constructors that the
//! schema declares, for example [`Literal::CONSTRUCTORS`](super::Literal).

#![warn(clippy::pedantic)]

use super::Constructor;

/// Checks that `constructors` spell `pinned`: one `Name(field, …)` on each
/// line, in order. A line with no field is `Name()`. Spaces at the start
/// and the end of a line, and empty lines, do not count.
///
/// ```
/// use prodrome::literal::codec::assert_pinned;
///
/// assert_pinned(
///     &[("Note", &["on", "lines"]), ("Seen", &[])],
///     "
///     Note(on, lines)
///     Seen()
///     ",
/// );
/// ```
///
/// # Panics
///
/// The constructors do not spell `pinned`. The message gives both texts,
/// so that a reader sees the change. A test calls this function, and the
/// panic is the test's failure.
#[track_caller]
pub fn assert_pinned(constructors: &[Constructor], pinned: &str) {
    let declared = spell(constructors);
    let pinned: Vec<&str> = pinned
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    assert!(
        declared == pinned,
        "the constructors are not the pinned ones. A changed name changes the \
         stored form; to keep the stored form, give the old name with \
         #[literal(rename = \"...\")].\npinned:\n{}\ndeclared:\n{}",
        pinned.join("\n"),
        declared.join("\n"),
    );
}

/// Each constructor as `Name(field, …)`.
fn spell(constructors: &[Constructor]) -> Vec<String> {
    constructors
        .iter()
        .map(|(name, fields)| format!("{name}({})", fields.join(", ")))
        .collect()
}
