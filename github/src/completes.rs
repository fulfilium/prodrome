//! A pull request's `Completes:` lines, as the completions that its merge
//! records.
//!
//! A contributor names each roadmap item that a pull request completes in
//! the pull request's description, in a line that starts with `Completes:`
//! and lists one or more item ids:
//!
//! ```text
//! Completes: mergeable-tag-sets
//! Completes: gh-12, digest-objects
//! ```
//!
//! A description with no such line completes nothing. The mirror reads the
//! lines of a pull request in two states:
//!
//! - **Not merged.** The mirror checks that the roadmap holds every item
//!   that the lines name, and writes nothing. A workflow runs this check on
//!   every change to a pull request, so a contributor sees a refusal before
//!   the merge.
//! - **Merged.** The mirror writes one `Completed` for each item, as
//!   [`MIRROR`], at the instant of the merge, with a note that names the
//!   pull request. The note holds only the number, which never changes, so a
//!   second application of the same pull request writes nothing.

use std::collections::BTreeSet;

use prodrome::event::{mk_completed, TodoEvent, TodoId};
use prodrome::literal::Datetime;
use prodrome::reference::Todo;
use serde_json::Value;

use prodrome_cli::Error;

use crate::{field, instant_of, Known, Skip, MIRROR};

/// The word that starts a completion line.
pub const WORD: &str = "Completes:";

/// A pull request, as far as a completion reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pull {
    pub number: u64,
    pub merge: Merge,
    /// The items that the description's `Completes:` lines name.
    pub completes: BTreeSet<TodoId>,
}

/// Whether a pull request has merged, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Merge {
    /// Open, or closed without a merge.
    Unmerged,
    Merged(Datetime),
}

/// The items that the `Completes:` lines of a description name. A line
/// counts when, with its surrounding whitespace removed, it starts with
/// [`WORD`]. Commas and whitespace separate the ids.
///
/// # Errors
///
/// A line that names no item, or an id that is not a valid item id.
pub fn completes_of(description: &str) -> Result<BTreeSet<TodoId>, Error> {
    let mut items = BTreeSet::new();
    for line in description.lines() {
        let Some(rest) = line.trim().strip_prefix(WORD) else {
            continue;
        };
        let ids: Vec<&str> = rest
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|id| !id.is_empty())
            .collect();
        if ids.is_empty() {
            return Err(Error::usage(format!(
                "{WORD} a completion line names no item. Write `{WORD} <item id>`, \
                 or remove the line if the pull request completes nothing"
            )));
        }
        for id in ids {
            items.insert(TodoId::new(id).map_err(|_| {
                Error::usage(format!(
                    "{WORD} {id:?} is not an item id. An item id is lowercase letters, \
                     digits, `-` and `_`, such as `digest-objects` or `gh-12`"
                ))
            })?);
        }
    }
    Ok(items)
}

/// The pull request that a payload's `pull_request` object describes: the
/// object of a `pull_request` webhook delivery, or one entry of the REST
/// list of pull requests, which has the same fields.
///
/// # Errors
///
/// A missing number, a merge instant that is not a UTC timestamp, or a
/// `Completes:` line that [`completes_of`] refuses.
pub fn pull_of(pull: &Value) -> Result<Pull, Error> {
    let number = field(pull, &["number"])
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::usage("payload: no pull_request.number"))?;
    let merge = match field(pull, &["merged_at"]) {
        Some(Value::String(at)) => Merge::Merged(instant_of(at)?),
        _ => Merge::Unmerged,
    };
    let description = field(pull, &["body"])
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(Pull {
        number,
        merge,
        completes: completes_of(description)?,
    })
}

/// The note on each completion that a pull request's merge records.
pub fn note_of(number: u64) -> String {
    format!("completed by pull request #{number}")
}

/// The completions that a pull request means against what the store holds:
/// `Err(Skip)` when it means none.
pub(crate) fn meant(
    pull: &Pull,
    known: &Known<'_>,
) -> Result<Result<Vec<TodoEvent<Todo>>, Skip>, Error> {
    let unknown: Vec<&str> = pull
        .completes
        .iter()
        .map(TodoId::as_str)
        .filter(|id| !known.has_item(id))
        .collect();
    if !unknown.is_empty() {
        return Err(Error::usage(format!(
            "{WORD} the roadmap holds no item {}. Name an item that the store on the \
             `roadmap-data` branch holds (`prodrome list` there shows the open ones), \
             or remove the line if the pull request completes nothing",
            unknown
                .iter()
                .map(|id| format!("{id:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if pull.completes.is_empty() {
        return Ok(Err(Skip::CompletesNothing));
    }
    match pull.merge {
        Merge::Unmerged => Ok(Err(Skip::Unmerged(pull.completes.clone()))),
        Merge::Merged(at) => pull
            .completes
            .iter()
            .map(|todo| {
                Ok(mk_completed(
                    todo.as_str(),
                    at,
                    MIRROR,
                    &note_of(pull.number),
                )?)
            })
            .collect::<Result<Vec<_>, Error>>()
            .map(Ok),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(description: &str) -> Vec<String> {
        completes_of(description)
            .expect("parses")
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect()
    }

    #[test]
    fn a_line_names_one_or_more_items() {
        assert_eq!(ids("Completes: digest-objects"), ["digest-objects"]);
        assert_eq!(
            ids("Text.\n\n  Completes: gh-12, digest-objects\nCompletes: gh-12"),
            ["digest-objects", "gh-12"]
        );
        assert_eq!(ids("Completes:a b"), ["a", "b"]);
    }

    #[test]
    fn a_description_without_the_word_completes_nothing() {
        assert!(ids("").is_empty());
        assert!(ids("This completes the work.\ncompletes: a\n`Completes:` b").is_empty());
    }

    #[test]
    fn a_line_with_no_item_or_a_malformed_id_is_refused() {
        for description in [
            "Completes:",
            "Completes:  , ",
            "Completes: #12",
            "Completes: Gh-12",
        ] {
            assert!(
                completes_of(description).is_err(),
                "{description:?} should be refused"
            );
        }
    }
}
