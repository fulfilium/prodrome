//! The envelopes of SPEC §3, the names of objects, actors and todos, and
//! the hashing of objects.
//!
//! Never change the field set of a shipped kind: a new field changes the
//! canonical print, so no stored object would match its hash. The `mk_*`
//! constructors are the parse boundary. On a shipped string field, `""` means
//! absent; `Sealed.prev` and `Woven.event` are `Option`s, printed `''` and
//! `None` for `None`.

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

/// For the core; no promise. Declares a string newtype with a private field.
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
/// digits. It orders and compares as its text does.
#[derive(Clone)]
pub struct Hash {
    words: [u64; 4],
    text: String,
}

// The comparisons of `Hash` are `#[inline]`, because the core calls them
// in its hottest loops from another crate, with no link-time optimisation.
impl Hash {
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn into_string(self) -> String {
        self.text
    }
}

impl PartialEq for Hash {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.words == other.words
    }
}

impl Eq for Hash {}

impl PartialOrd for Hash {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hash {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.words.cmp(&other.words)
    }
}

impl std::hash::Hash for Hash {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.words.hash(state);
    }
}

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
    /// A todo's identifier, which matches `^[a-z0-9_-]+$`; its literal is
    /// its text.
    #[derive(Literal)]
    #[literal(crate = crate, new = TodoId::new)]
    TodoId
}

newtype_str! {
    /// The writer of an event, which matches `^[a-z][a-z0-9_-]*$`; its
    /// literal is its text. The host's policy decides which actors it trusts
    /// (SPEC §5).
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

/// The four big-endian words that 64 lowercase hex digits spell.
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

    /// For the core; no promise. The name of `bytes`: their sha256.
    #[doc(hidden)]
    pub fn of_bytes(bytes: &[u8]) -> Hash {
        Hash::of_digest(Sha256::digest(bytes).into())
    }

    /// For the core; no promise. The name that `digest` spells; any 32 bytes are a name.
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

    /// For the core; no promise. The digest, as a cache file (unix only) holds it.
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
    /// `field` names the field in the error.
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

    /// `""` gives `None`.
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

    /// A signature names no genesis, because it belongs to the prodrome of
    /// the object that it signs.
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

/// The names that an object rests on, sorted and distinct: the causal order
/// that every reader uses (SPEC §3).
pub fn parents_of<E>(envelope: &Envelope<E>) -> Vec<Hash> {
    rests_on(envelope).into_iter().cloned().collect()
}

/// For the core; no promise. [`parents_of`], borrowed, so a walk copies no name.
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

/// For the core; no promise. Sorts `names` and refuses a duplicate, so a set has one print.
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

/// Refuses fewer than two parents (one is a `Sealed`, none is a legacy root)
/// and a parent named twice.
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

/// The envelope constructors and their fields in declared order.
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

/// The vocabulary that a stored object may use: the envelopes, then the
/// schema's own. A schema kind with an envelope's name is unreachable (SPEC §2).
pub struct EventVocabulary<'v, E: Schema> {
    schema: &'v E::Vocabulary,
}

impl<'v, E: Schema> EventVocabulary<'v, E> {
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

/// For the core; no promise. A keyword field of a call.
#[doc(hidden)]
pub fn field(name: &str, value: Value) -> (String, Value) {
    (name.to_owned(), value)
}

/// For the core; no promise. The tuple of `names`, as stored.
#[doc(hidden)]
pub fn hashes_value(names: &[Hash]) -> Value {
    Value::Tuple(names.iter().map(|name| Value::str(name.as_str())).collect())
}

/// For the core; no promise. Reads the tuple of names in the field `name`.
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

    /// Parses a literal through every `mk_*` rule, its event at `schema`.
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

/// A fold sorts by this print to break a tie deterministically.
pub fn canonical<E: Schema>(event: &E) -> String {
    print_literal(&event.to_value())
}

/// The name of an event apart from the object that holds it; no store
/// keeps it.
pub fn event_id<E: Schema>(event: &E) -> Hash {
    Hash::of_bytes(canonical(event).as_bytes())
}

/// The object's bytes, whose sha256 is its name.
pub fn canonical_envelope<E: Schema>(envelope: &Envelope<E>) -> String {
    print_literal(&envelope.to_value())
}

/// The name of an object at append time. Verification hashes the stored
/// bytes and never a reprint, so a change to the printer cannot report the
/// whole store as tampered.
pub fn seal_hash<E: Schema>(envelope: &Envelope<E>) -> Hash {
    Hash::of_bytes(canonical_envelope(envelope).as_bytes())
}

/// Parses one stored object through [`EventVocabulary`] and every `mk_*` rule.
pub fn parse_envelope<E: Schema>(
    schema: &E::Vocabulary,
    text: &str,
) -> Result<Envelope<E>, ProdromeError> {
    let vocabulary = EventVocabulary::<E>::new(schema);
    Envelope::from_value(schema, &parse_literal(text, &vocabulary)?)
}

/// The inverse of [`canonical`], through the vocabulary of [`parse_envelope`].
pub fn parse_event<E: Schema>(schema: &E::Vocabulary, text: &str) -> Result<E, ProdromeError> {
    let vocabulary = EventVocabulary::<E>::new(schema);
    E::from_value(schema, &parse_literal(text, &vocabulary)?)
}
