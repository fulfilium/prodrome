//! The envelopes of SPEC §3, the names of objects, actors and todos, and
//! the hashing of objects.
//!
//! Never change the field set of a shipped kind: a new field changes the
//! canonical print, so no stored object would match its hash any more. Store
//! new data in a new kind.
//!
//! The `mk_*` constructors are the parse boundary: [`Envelope::from_value`]
//! builds every value through them, so later code does not check again.
//! On a shipped string field, `""` means absent. Where an absent value is not
//! a shipped string (`Sealed.prev` at a legacy root, `Woven.event`), the field
//! is an `Option`, and the printer writes `''` or `None` for `None`.

use sha2::{Digest, Sha256};

use crate::change::Change;
use crate::genesis::Genesis;
use crate::literal::codec::Literal;
use crate::literal::{
    parse_literal, print_literal, Call, ProdromeError, Signature, Table, Value, Vocabulary,
};
use crate::payload::{as_string, required, string_field, tuple_field};
use crate::schema::Schema;
use crate::sign::{KeyAdded, KeyRevoked, Signed};
use crate::snapshot::Snapshot;

// --- the names, as types ----------------------------------------------------

/// A string newtype with a private field, `as_str`, `into_string`,
/// `AsRef<str>` and `Display`. The crates of this workspace declare their
/// names with it.
#[doc(hidden)]
#[macro_export]
macro_rules! newtype_str {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

/// An object's name: the sha256 of its canonical print, as 64 lowercase hex
/// digits.
///
/// A name orders as its text does, and two names are equal exactly when
/// their texts are equal.
#[derive(Clone)]
pub struct Hash {
    words: [u64; 4],
    text: String,
}

impl Hash {
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn into_string(self) -> String {
        self.text
    }
}

impl PartialEq for Hash {
    fn eq(&self, other: &Self) -> bool {
        self.words == other.words
    }
}

impl Eq for Hash {}

impl PartialOrd for Hash {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hash {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.words.cmp(&other.words)
    }
}

impl std::hash::Hash for Hash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.words.hash(state);
    }
}

/// As a newtype of its text prints: `Hash("…")`.
impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Hash").field(&self.text).finish()
    }
}

impl AsRef<str> for Hash {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

newtype_str! {
    /// A todo's identifier, which matches `^[a-z0-9_-]+$`. Its literal is
    /// its text, and a read checks the text with [`TodoId::new`].
    #[derive(Literal)]
    #[literal(crate = crate, new = TodoId::new)]
    TodoId
}

newtype_str! {
    /// The writer of an event, which matches `^[a-z][a-z0-9_-]*$`. The host's
    /// policy decides which actors it trusts (SPEC §5, `prodrome::policy`).
    /// Its literal is its text, and a read checks the text with
    /// [`Actor::new`].
    #[derive(Literal)]
    #[literal(crate = crate, new = Actor::new)]
    Actor
}

newtype_str! {
    /// An identifier in a payload's vocabulary, such as a kind or a category,
    /// which matches `^[A-Za-z][A-Za-z0-9_]*$`.
    Name
}

pub(crate) fn lower_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The four big-endian words that 64 lowercase hex digits spell, or `None`
/// for any other text.
fn hex_words(text: &str) -> Option<[u64; 4]> {
    if text.len() != 64 {
        return None;
    }
    let mut words = [0_u64; 4];
    for (at, digit) in text.bytes().enumerate() {
        let value = match digit {
            b'0'..=b'9' => digit - b'0',
            b'a'..=b'f' => digit - b'a' + 10,
            _ => return None,
        };
        words[at / 16] = words[at / 16] << 4 | u64::from(value);
    }
    Some(words)
}

fn matches_todo(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn matches_actor(text: &str) -> bool {
    let mut bytes = text.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn matches_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_alphabetic() => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

impl Hash {
    /// Refuses text that is not 64 lowercase hex digits.
    pub fn new(text: impl Into<String>) -> Result<Hash, ProdromeError> {
        let text = text.into();
        match hex_words(&text) {
            Some(words) => Ok(Hash { words, text }),
            None => Err(ProdromeError::invalid(format!(
                "object name must be 64 lowercase hex, got {text:?}"
            ))),
        }
    }

    #[doc(hidden)]
    pub fn of_bytes(bytes: &[u8]) -> Hash {
        Hash::of_digest(Sha256::digest(bytes).into())
    }

    /// The name whose digest is `digest`. Every 32 bytes are a digest, so
    /// every 32 bytes are a name.
    #[doc(hidden)]
    pub fn of_digest(digest: [u8; 32]) -> Hash {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut words = [0_u64; 4];
        let mut text = String::with_capacity(64);
        for (at, byte) in digest.into_iter().enumerate() {
            words[at / 8] = words[at / 8] << 8 | u64::from(byte);
            text.push(char::from(HEX[usize::from(byte >> 4)]));
            text.push(char::from(HEX[usize::from(byte & 0xf)]));
        }
        Hash { words, text }
    }

    /// The 32 bytes of the digest that the name spells. A cache file
    /// holds a name so (`prodrome::memo::file`), and there is no file off
    /// unix.
    #[cfg(unix)]
    #[doc(hidden)]
    pub fn digest(&self) -> [u8; 32] {
        let mut digest = [0_u8; 32];
        for (at, word) in self.words.iter().enumerate() {
            digest[at * 8..at * 8 + 8].copy_from_slice(&word.to_be_bytes());
        }
        digest
    }
}

impl TodoId {
    pub fn new(text: impl Into<String>) -> Result<TodoId, ProdromeError> {
        let text = text.into();
        if matches_todo(&text) {
            Ok(TodoId(text))
        } else {
            Err(ProdromeError::invalid(format!(
                "todo id must match ^[a-z0-9_-]+$ (nonempty), got {text:?}"
            )))
        }
    }
}

impl Actor {
    pub fn new(text: impl Into<String>) -> Result<Actor, ProdromeError> {
        let text = text.into();
        if matches_actor(&text) {
            Ok(Actor(text))
        } else {
            Err(ProdromeError::invalid(format!(
                "actor must match ^[a-z][a-z0-9_-]*$, got {text:?}"
            )))
        }
    }
}

impl Literal for Hash {
    fn to_value(&self) -> Value {
        Value::str(self.as_str())
    }

    fn from_value(value: &Value) -> Result<Self, ProdromeError> {
        Hash::new(String::from_value(value)?)
    }
}

impl Name {
    /// A required identifier; `field` names the field in the error.
    pub fn new(field: &str, text: impl Into<String>) -> Result<Name, ProdromeError> {
        let text = text.into();
        if matches_name(&text) {
            Ok(Name(text))
        } else {
            Err(ProdromeError::invalid(format!(
                "{field} must be an identifier, got {text:?}"
            )))
        }
    }

    /// An identifier that may be absent: `""` gives `None`.
    pub fn optional(field: &str, text: impl Into<String>) -> Result<Option<Name>, ProdromeError> {
        let text = text.into();
        if text.is_empty() {
            Ok(None)
        } else {
            Name::new(field, text).map(Some)
        }
    }
}

/// The stored object (SPEC §3), which holds an event of the schema `E`.
/// `Sealed` and `Woven` are legacy envelopes: a store reads them, and no
/// `append` writes them.
#[derive(Debug, Clone, PartialEq)]
pub enum Envelope<E> {
    Sealed {
        prev: Option<Hash>,
        event: E,
    },
    Woven {
        parents: Vec<Hash>,
        event: Option<E>,
    },
    Genesis(Genesis),
    Change(Change<E>),
    Snapshot(Snapshot),
    /// A detached signature of another object ([`crate::sign`]).
    Signed(Signed),
    KeyAdded(KeyAdded),
    KeyRevoked(KeyRevoked),
}

impl<E> Envelope<E> {
    pub fn event(&self) -> Option<&E> {
        match self {
            Envelope::Sealed { event, .. } => Some(event),
            Envelope::Woven { event, .. } => event.as_ref(),
            Envelope::Change(change) => Some(&change.event),
            Envelope::Genesis(_)
            | Envelope::Snapshot(_)
            | Envelope::Signed(_)
            | Envelope::KeyAdded(_)
            | Envelope::KeyRevoked(_) => None,
        }
    }

    pub fn into_event(self) -> Option<E> {
        match self {
            Envelope::Sealed { event, .. } => Some(event),
            Envelope::Woven { event, .. } => event,
            Envelope::Change(change) => Some(change.event),
            Envelope::Genesis(_)
            | Envelope::Snapshot(_)
            | Envelope::Signed(_)
            | Envelope::KeyAdded(_)
            | Envelope::KeyRevoked(_) => None,
        }
    }

    /// The genesis that a change, a snapshot or a key names. A signature
    /// names none, because it belongs to the prodrome of the object it signs.
    pub fn genesis(&self) -> Option<&Hash> {
        match self {
            Envelope::Change(change) => Some(&change.genesis),
            Envelope::Snapshot(snapshot) => Some(&snapshot.genesis),
            Envelope::KeyAdded(added) => Some(&added.genesis),
            Envelope::KeyRevoked(revoked) => Some(&revoked.genesis),
            Envelope::Sealed { .. }
            | Envelope::Woven { .. }
            | Envelope::Genesis(_)
            | Envelope::Signed(_) => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Envelope::Sealed { .. } => "Sealed",
            Envelope::Woven { .. } => "Woven",
            Envelope::Genesis(_) => "Genesis",
            Envelope::Change(_) => "Change",
            Envelope::Snapshot(_) => "Snapshot",
            Envelope::Signed(_) => "Signed",
            Envelope::KeyAdded(_) => "KeyAdded",
            Envelope::KeyRevoked(_) => "KeyRevoked",
        }
    }
}

/// The names that an object rests on (SPEC §3), sorted and distinct: the
/// edges it names and the genesis it names.
///
/// This set defines the causal order that every reader uses. A change rests
/// on its genesis as on its deps, so a genesis is never a head of a prodrome
/// that holds an object begun from it.
pub fn parents_of<E>(envelope: &Envelope<E>) -> Vec<Hash> {
    rests_on(envelope).into_iter().cloned().collect()
}

/// [`parents_of`], borrowed from the object: what a walk over many objects
/// reads, so that it copies no name.
#[doc(hidden)]
pub fn rests_on<E>(envelope: &Envelope<E>) -> Vec<&Hash> {
    let mut names: Vec<&Hash> = match envelope {
        Envelope::Sealed { prev, .. } => prev.iter().collect(),
        Envelope::Woven { parents, .. } => parents.iter().collect(),
        Envelope::Genesis(_) | Envelope::KeyAdded(_) => Vec::new(),
        Envelope::Change(change) => change.deps.iter().collect(),
        Envelope::Snapshot(snapshot) => snapshot.tips.iter().chain(&snapshot.previous).collect(),
        Envelope::Signed(signed) => vec![&signed.object],
        Envelope::KeyRevoked(revoked) => revoked.deps.iter().collect(),
    };
    names.extend(envelope.genesis());
    names.sort_unstable();
    names.dedup();
    names
}

// --- smart constructors ------------------------------------------------------

pub fn mk_sealed<E: Schema>(prev: Option<Hash>, event: E) -> Envelope<E> {
    Envelope::Sealed { prev, event }
}

/// Sorted, so a set of names is one print wherever it is made; a name twice
/// is refused, since it would let two objects mean one set.
#[doc(hidden)]
pub fn sorted_distinct(
    field: &str,
    what: &str,
    mut names: Vec<Hash>,
) -> Result<Vec<Hash>, ProdromeError> {
    names.sort();
    if names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ProdromeError::invalid(format!(
            "{field} names a {what} twice: {:?}",
            names.iter().map(Hash::as_str).collect::<Vec<_>>()
        )));
    }
    Ok(names)
}

/// Refuses fewer than two parents (one parent is a `Sealed`, and none is a
/// legacy root) and a parent named twice; sorts the parents.
pub fn mk_woven<E: Schema>(
    parents: Vec<Hash>,
    event: Option<E>,
) -> Result<Envelope<E>, ProdromeError> {
    if parents.len() < 2 {
        return Err(ProdromeError::invalid(format!(
            "Woven.parents must name at least two parents (one parent is a Sealed), got {}",
            parents.len()
        )));
    }
    Ok(Envelope::Woven {
        parents: sorted_distinct("Woven.parents", "parent", parents)?,
        event,
    })
}

// --- the vocabulary ----------------------------------------------------------

/// The envelope constructors and their fields in declared order, the same
/// for every schema.
pub const ENVELOPE_SIGNATURES: &[(&str, &[&str])] = &[
    ("Sealed", &["prev", "event"]),
    ("Woven", &["parents", "event"]),
    ("Genesis", &["label", "nonce"]),
    ("Change", &["genesis", "deps", "event"]),
    ("Snapshot", &["genesis", "tips", "previous"]),
    ("Signed", &["object", "key", "signature"]),
    ("KeyAdded", &["genesis", "actor", "key"]),
    ("KeyRevoked", &["genesis", "deps", "actor", "key"]),
];

/// The whole vocabulary that a stored object may use: the envelopes, then
/// the schema's own (`prodrome::todo::TodoVocabulary` for the todo schema).
///
/// `datetime` and `timedelta` belong to the grammar and need no entry. The
/// envelopes take precedence: a schema kind with an envelope's name, such as
/// `Change`, is unreachable.
pub struct EventVocabulary<'v, E: Schema> {
    schema: &'v E::Vocabulary,
}

impl<'v, E: Schema> EventVocabulary<'v, E> {
    /// The envelopes' vocabulary, then the vocabulary `schema`.
    pub fn new(schema: &'v E::Vocabulary) -> EventVocabulary<'v, E> {
        EventVocabulary { schema }
    }
}

impl<E: Schema> Vocabulary for EventVocabulary<'_, E> {
    fn signature(&self, name: &str) -> Option<Signature<'_>> {
        Table(ENVELOPE_SIGNATURES)
            .find(name)
            .or_else(|| self.schema.signature(name))
    }
}

// --- the literal round trip --------------------------------------------------

#[doc(hidden)]
pub fn field(name: &str, value: Value) -> (String, Value) {
    (name.to_owned(), value)
}

#[doc(hidden)]
pub fn hashes_value(names: &[Hash]) -> Value {
    Value::Tuple(names.iter().map(|name| Value::str(name.as_str())).collect())
}

#[doc(hidden)]
pub fn hashes(call: &Call, name: &str) -> Result<Vec<Hash>, ProdromeError> {
    let context = format!("{}.{name}", call.name);
    tuple_field(call, name)?
        .iter()
        .map(|item| Hash::new(as_string(item, &context)?))
        .collect()
}

/// A name where `''` is absent.
pub(crate) fn optional_hash(call: &Call, name: &str) -> Result<Option<Hash>, ProdromeError> {
    let text = string_field(call, name)?;
    (!text.is_empty()).then(|| Hash::new(text)).transpose()
}

impl<E: Schema> Envelope<E> {
    pub fn to_value(&self) -> Value {
        match self {
            Envelope::Sealed { prev, event } => Value::call(
                "Sealed",
                vec![
                    field("prev", Value::str(prev.as_ref().map_or("", Hash::as_str))),
                    field("event", Schema::to_value(event)),
                ],
            ),
            Envelope::Woven { parents, event } => Value::call(
                "Woven",
                vec![
                    field("parents", hashes_value(parents)),
                    field(
                        "event",
                        event.as_ref().map_or(Value::None, Schema::to_value),
                    ),
                ],
            ),
            Envelope::Genesis(genesis) => genesis.to_value(),
            Envelope::Change(change) => change.to_value(),
            Envelope::Snapshot(snapshot) => snapshot.to_value(),
            Envelope::Signed(signed) => signed.to_value(),
            Envelope::KeyAdded(added) => added.to_value(),
            Envelope::KeyRevoked(revoked) => revoked.to_value(),
        }
    }

    /// Parses a literal into an envelope through every `mk_*` rule, and
    /// parses its event at the vocabulary `schema`.
    pub fn from_value(schema: &E::Vocabulary, value: &Value) -> Result<Envelope<E>, ProdromeError> {
        let call = value
            .as_call()
            .ok_or_else(|| ProdromeError::invalid("an object must be a Sealed or a Woven"))?;
        match call.name.as_str() {
            "Sealed" => Ok(mk_sealed(
                optional_hash(call, "prev")?,
                E::from_value(schema, required(call, "event")?)?,
            )),
            "Woven" => {
                let event = match call.field("event") {
                    None | Some(Value::None) => None,
                    Some(other) => Some(E::from_value(schema, other)?),
                };
                mk_woven(hashes(call, "parents")?, event)
            }
            "Genesis" => Ok(Envelope::Genesis(Genesis::from_call(call)?)),
            "Change" => Ok(Envelope::Change(Change::from_call(schema, call)?)),
            "Snapshot" => Ok(Envelope::Snapshot(Snapshot::from_call(call)?)),
            "Signed" => Ok(Envelope::Signed(Signed::from_call(call)?)),
            "KeyAdded" => Ok(Envelope::KeyAdded(KeyAdded::from_call(call)?)),
            "KeyRevoked" => Ok(Envelope::KeyRevoked(KeyRevoked::from_call(call)?)),
            other => Err(ProdromeError::invalid(format!(
                "object is not a chain envelope: {other}"
            ))),
        }
    }
}

// --- hashing and printing -----------------------------------------------------

/// The canonical print of an event; a fold sorts by it to break a tie
/// deterministically.
pub fn canonical<E: Schema>(event: &E) -> String {
    print_literal(&event.to_value())
}

/// The name of an event apart from the object that holds it; no store
/// keeps it.
pub fn event_id<E: Schema>(event: &E) -> Hash {
    Hash::of_bytes(canonical(event).as_bytes())
}

/// The canonical print of an envelope: the object's bytes, which its file
/// holds and whose sha256 is its name.
pub fn canonical_envelope<E: Schema>(envelope: &Envelope<E>) -> String {
    print_literal(&envelope.to_value())
}

/// The sha256 of the canonical print, which names an object at append time.
///
/// Verification hashes the stored bytes instead (`store::EventStore::load`)
/// and never a reprint, so that a change to the printer cannot report the
/// whole store as tampered.
pub fn seal_hash<E: Schema>(envelope: &Envelope<E>) -> Hash {
    Hash::of_bytes(canonical_envelope(envelope).as_bytes())
}

/// Parses the text of one stored object through the closed vocabulary (the
/// envelopes, then `schema`) and every `mk_*` rule.
pub fn parse_envelope<E: Schema>(
    schema: &E::Vocabulary,
    text: &str,
) -> Result<Envelope<E>, ProdromeError> {
    let vocabulary = EventVocabulary::<E>::new(schema);
    Envelope::from_value(schema, &parse_literal(text, &vocabulary)?)
}

/// Parses one event's canonical print, the inverse of [`canonical`], through
/// the same closed vocabulary as [`parse_envelope`]. Use it for a bare event
/// print, such as the input of a fold.
pub fn parse_event<E: Schema>(schema: &E::Vocabulary, text: &str) -> Result<E, ProdromeError> {
    let vocabulary = EventVocabulary::<E>::new(schema);
    E::from_value(schema, &parse_literal(text, &vocabulary)?)
}
