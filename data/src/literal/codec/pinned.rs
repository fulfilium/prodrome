//! A pin of a schema's stored names: a test that fails when a constructor
//! or a field changes its name or its place, because every stored object
//! holds the names and the order of the fields.

#![warn(clippy::pedantic)]

use super::Constructor;

/// Checks that `constructors` spell `pinned`: one `Name(field, …)` on each
/// line, in order, with `Name()` for no field; outer spaces and empty lines
/// do not count.
///
/// ```
/// use prodrome_data::literal::codec::assert_pinned;
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
/// The constructors do not spell `pinned`; the message gives both texts.
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
