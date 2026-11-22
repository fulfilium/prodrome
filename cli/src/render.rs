//! What the verbs print.
//!
//! Every function here is total and returns a `String`: the binary is the only
//! thing that writes to a stream, so a test drives a verb and reads its answer.

use prodrome::event::TodoEvent;
use prodrome::event::TodoId;
use prodrome::fold::Kind;
use prodrome::fpl::print_term;
use prodrome::reference::Todo;
use prodrome::view::{list_order, Confidence, Entry, Provisional};

/// A row of the todo schema, as this CLI reads one.
type Row = Entry<TodoEvent<Todo>>;

use crate::price::iso;
use crate::Reading;

/// A price as `show` says it: the number, `absent` where the function reads
/// `∅` — absence is not a zero — or why the function does not link.
fn priced(entry: &Row) -> String {
    match entry.value() {
        Ok(Some(value)) => format!("{value:.3}"),
        Ok(None) => "absent".to_owned(),
        Err(unlinked) => format!("does not link: {unlinked}"),
    }
}

/// A price in `list`'s column, five wide: `∅` for absent, `—` for a function
/// that does not link.
fn column(entry: &Row) -> String {
    match entry.value() {
        Ok(Some(value)) => format!("{value:.3}"),
        Ok(None) => "  ∅  ".to_owned(),
        Err(_) => "  —  ".to_owned(),
    }
}

/// Why a row is not the confirmed reading, whole — empty when it is.
fn provisional(entry: &Row) -> String {
    match entry.confidence {
        Confidence::Confirmed => String::new(),
        Confidence::Provisional(reason) => {
            let claimed = format!("claimed {}", entry.claimed());
            let content = "content unconfirmed".to_owned();
            let text = match reason {
                Provisional::Claimed => claimed,
                Provisional::Content => content,
                Provisional::ClaimedAndContent => format!("{claimed}, {content}"),
            };
            format!("  [{text}]")
        }
    }
}

/// The roster a reading was taken under, as the summary line says it.
fn under(untrusted: &[String]) -> String {
    if untrusted.is_empty() {
        "standing behind every writer".to_owned()
    } else {
        format!("untrusted: {}", untrusted.join(", "))
    }
}

/// The open todos at the reading's instant, in §6.7's list order: most
/// urgent first, and the rows with no number after them.
///
/// OPEN AND ALREADY BEGUN. `view::entries` answers for every todo the DAG
/// has ever mentioned, the future's included, because that is what a §6.7 row
/// is. A list of what to do next keeps the rows that exist at `t`
/// ([`Entry::exists_at`]: an event of any kind is dated at or before `t`)
/// and whose outcome at `t` is nothing.
///
/// [`Entry::exists_at`]: prodrome::view::Entry::exists_at
pub fn list(reading: &Reading) -> String {
    let mut rows: Vec<&Row> = reading
        .entries
        .iter()
        .filter(|entry| entry.is_open() && entry.exists_at(reading.at))
        .collect();
    rows.sort_by(|a, b| list_order(a, b));

    let mut out = vec![format!(
        "{} open at {} — {}",
        rows.len(),
        iso(reading.at),
        under(&reading.untrusted)
    )];
    let width = rows
        .iter()
        .map(|entry| entry.key.as_str().len())
        .max()
        .unwrap_or(0);
    for entry in rows {
        out.push(format!(
            "{}  {:width$}  {}{}",
            column(entry),
            entry.key.as_str(),
            reading.body(&entry.key),
            provisional(entry)
        ));
    }
    out.join("\n")
}

fn field(out: &mut Vec<String>, name: &str, value: impl AsRef<str>) {
    let value = value.as_ref();
    if !value.is_empty() {
        out.push(format!("{name:<11}{value}"));
    }
}

/// One todo in full, `None` when the DAG has never mentioned it.
pub fn show(reading: &Reading, todo: &TodoId) -> Option<String> {
    let entry = reading.entries.iter().find(|entry| &entry.key == todo)?;
    let mut out = Vec::new();
    field(&mut out, "todo", entry.key.as_str());
    field(&mut out, "read at", iso(reading.at));
    field(&mut out, "state", entry.state());
    field(&mut out, "since", entry.at());
    field(&mut out, "claimed", entry.claimed());
    field(
        &mut out,
        "confidence",
        match entry.confidence {
            Confidence::Confirmed => "confirmed".to_owned(),
            Confidence::Provisional(_) => format!("provisional{}", provisional(entry).trim_start()),
        },
    );
    field(&mut out, "created", iso(entry.begun));
    field(&mut out, "price", priced(entry));
    field(&mut out, "spec", print_term(entry.spec()));
    field(&mut out, "body", reading.body(todo));
    field(&mut out, "detail", reading.detail(todo));
    field(
        &mut out,
        "content",
        entry
            .content()
            .iter()
            .map(|hash| hash.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );
    for (kind, names) in &entry.conflicts {
        field(
            &mut out,
            &format!("conflict:{}", Kind::as_str(*kind)),
            names
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    field(
        &mut out,
        "stream",
        entry
            .stream
            .iter()
            .map(|name| name.as_str())
            .collect::<Vec<_>>()
            .join("\n           "),
    );
    Some(out.join("\n"))
}
