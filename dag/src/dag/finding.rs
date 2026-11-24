use std::collections::BTreeSet;
use std::fmt;

use super::Heads;
use prodrome_data::event::{Actor, Hash};
use prodrome_data::literal::{Datetime, ProdromeError};

/// One result of `verify` about a store (SPEC §3, "`verify`").
#[derive(Debug, Clone, PartialEq)]
pub enum Finding {
    /// The listing of `objects/` or `quarantine/` failed.
    Io(ProdromeError),
    Unread {
        name: Hash,
        why: Unread,
    },
    /// The path, within the store, of an entry of `objects/` that is not
    /// named like an object, or of a temporary file in the root.
    Garbage(String),
    /// A `HEAD` file or a `refs/` directory, which a store from before the
    /// tips were derived holds and nothing reads.
    Leftover(&'static str),
    /// A file in `quarantine/`.
    Quarantined(String),
    /// A file of the store that is local to one machine (`.heads`,
    /// `.writes`), which the store's `.gitignore` does not name, so that
    /// git can track it.
    Unignored(&'static str),
    /// A parent that no readable object is: absent, or unread for `why`.
    Broken {
        at: Hash,
        why: Option<Unread>,
    },
    Cycle(ProdromeError),
    Dated {
        name: Hash,
        at: Datetime,
        behind: Datetime,
    },
    /// A change or a snapshot that names a genesis that this store does not
    /// hold.
    Stranger {
        object: Hash,
        genesis: Hash,
    },
    Crossing {
        object: Hash,
        parent: Hash,
    },
    /// A change with a dep that is not a write to its entity.
    Foreign {
        change: Hash,
        dep: Hash,
    },
    Redundant {
        change: Hash,
        dep: Hash,
        beneath: Hash,
    },
    Incomplete {
        snapshot: Hash,
        missing: Hash,
    },
    /// A `Signed` whose signature does not verify.
    Forged {
        signed: Hash,
    },
    /// An object whose actor has a key, and which no signature by a key of
    /// that actor proves (SPEC §5).
    Unsigned {
        object: Hash,
        actor: Actor,
    },
    /// A write to an inflationary register whose value falls below, or
    /// beside, the reading of the writes that it rests on (SPEC §6).
    ///
    /// An append refuses such a write, so only a change built by hand
    /// contains one. `why` is the refusal of the append.
    Below {
        write: Hash,
        why: ProdromeError,
    },
    /// A `.heads` file that parses and differs from the heads that the store
    /// derives (SPEC §3, "The record of heads").
    StaleHeads {
        recorded: Heads,
        derived: Heads,
    },
}

/// A name that the history of a set of prints lacks
/// ([`crate::dag::Dag::excluded`]).
///
/// `why` is `None` for a parent that no print has. `resting` holds every
/// object held that rests on the name, which the history also lacks
/// (law 40).
#[derive(Debug, Clone, PartialEq)]
pub struct Excluded {
    pub name: Hash,
    pub why: Option<Unread>,
    pub resting: BTreeSet<Hash>,
}

/// Why a named print is not an object.
#[derive(Debug, Clone, PartialEq)]
pub enum Unread {
    Io(ProdromeError),
    /// The bytes hash to this instead.
    Tampered(Hash),
    NotText,
    Unparsed(ProdromeError),
}

impl Unread {
    /// The error that a read of the object refuses with. For a tampered
    /// file, the error names `fsck` as the remedy.
    pub fn refusal(&self, name: &Hash) -> ProdromeError {
        match self {
            Unread::Io(error) | Unread::Unparsed(error) => error.clone(),
            Unread::Tampered(computed) => ProdromeError::Store(format!(
                "object {name} hashes to {computed} — tampered or corrupt: `prodrome fsck` \
                 (`EventStore::fsck`) sets it aside so the store writes again"
            )),
            Unread::NotText => ProdromeError::Store(format!("object {name} is not UTF-8")),
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Finding::Io(error) | Finding::Cycle(error) => write!(f, "{error}"),
            Finding::Unread { name, why } => match why {
                Unread::Io(_) => write!(f, "object {name} could not be read"),
                Unread::Tampered(_) => write!(
                    f,
                    "object {name} does not hash to its filename (tampered/corrupt)"
                ),
                Unread::NotText => write!(f, "object {name} failed to parse: not UTF-8"),
                Unread::Unparsed(error) => write!(f, "object {name} failed to parse: {error}"),
            },
            Finding::Garbage(file) => write!(
                f,
                "{file} is not an object: a temp an interrupted write left, or a stray (SPEC §3); \
                 delete it"
            ),
            Finding::Leftover(file) => write!(
                f,
                "{file} is left from before tips were derived (SPEC §3): nothing reads it, delete it"
            ),
            Finding::Unignored(file) => write!(
                f,
                "{file} is local to one machine, and the store's .gitignore does not name it \
                 (SPEC §3): add the line /{file}"
            ),
            Finding::Quarantined(file) => write!(
                f,
                "quarantine/{file} failed its hash and was set aside (SPEC §3): restore the \
                 object from a replica, then delete this file"
            ),
            Finding::Broken { at, why: None } => {
                write!(f, "chain broke at {at}: missing object {at}")
            }
            Finding::Broken { at, why: Some(why) } => {
                write!(f, "chain broke at {at}: {}", why.refusal(at))
            }
            Finding::Dated { name, at, behind } => write!(
                f,
                "untrusted event {name} is dated {}, behind its predecessor ({})",
                at.isoformat(),
                behind.isoformat()
            ),
            Finding::Stranger { object, genesis } => write!(
                f,
                "object {object} names genesis {genesis}, which the store does not hold as a \
                 genesis (SPEC §3)"
            ),
            Finding::Crossing { object, parent } => write!(
                f,
                "object {object} rests on {parent}, of another genesis (SPEC §3)"
            ),
            Finding::Foreign { change, dep } => write!(
                f,
                "change {change} depends on {dep}, which is not a write to its entity (SPEC §3)"
            ),
            Finding::Redundant {
                change,
                dep,
                beneath,
            } => write!(
                f,
                "change {change} depends on {dep}, which its dep {beneath} already rests on \
                 (SPEC §3)"
            ),
            Finding::Incomplete { snapshot, missing } => write!(
                f,
                "snapshot {snapshot} attests {missing}, which the store does not hold \
                 (SPEC §3)"
            ),
            Finding::Forged { signed } => write!(
                f,
                "signature {signed} does not verify under its key (SPEC §3)"
            ),
            Finding::Unsigned { object, actor } => write!(
                f,
                "object {object} is {actor}'s, who has a key, and no signature by a key of \
                 {actor}'s proves it (SPEC §5)"
            ),
            Finding::Below { write, why } => write!(
                f,
                "write {write} falls below the reading of what it rests on, which an append \
                 refuses ({why}); the reading is the greatest of all writes and does not \
                 descend (SPEC §6)"
            ),
            Finding::StaleHeads { recorded, derived } => write!(
                f,
                ".heads records (objects: {}, tips: {}), and the store derives (objects: {}, \
                 tips: {}): a program changed objects/ without the store (SPEC §3); call \
                 `record_heads`, or delete .heads",
                recorded.count(),
                recorded.tips().len(),
                derived.count(),
                derived.tips().len()
            ),
        }
    }
}
