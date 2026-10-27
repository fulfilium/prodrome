//! A replica's frontier named by one 64-character token (SPEC law 47).
//!
//! A pull asks the store of record for the objects that a replica lacks.
//! The replica must say which objects it holds, and a list of its tips can
//! be too long for one request. So the store of record answers each pull
//! with a [`Token`] for the replica's frontier: the tips of what the replica
//! holds after it takes the answer. The store of record records those tips
//! under the token, and the replica names the token in its next pull.
//!
//! [`Frontiers::answer`] reads a pull's [`Since`] against the store's
//! objects and answers a [`Lacking`]: what the token reaches, and the
//! objects outside it in the store's linear order. The host chooses how
//! many of those objects to send, and [`Lacking::record`] records the
//! frontier of what the replica then holds. The library builds that set,
//! so it is closed under parents by construction.
//!
//! The store of record keeps a bounded number of frontiers for each store,
//! and lets the oldest go. A token that it does not hold, or one that
//! names a tip that the store no longer holds, reads as [`Reached::Unknown`],
//! and the answer then starts from nothing. So no answer leaves out an
//! object that the replica lacks.
//!
//! The records are kept through [`Records`], an effect that the host
//! supplies, for example a directory or a table. A [`Frontiers`] serialises
//! its writes to one store with a lock in its own process, so a host keeps
//! one [`Frontiers`] for each place where it keeps records.

#![warn(clippy::pedantic)]

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::dag::{tips_among, Dag};
use crate::event::{parents_of, Envelope, Hash};
use crate::literal::ProdromeError;
use crate::schema::Schema;

/// The name of a frontier: the sha256 of its tips, with each name on a
/// line of its own in sorted order. It is 64 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Token(Hash);

/// A text that is not 64 lowercase hex digits, given as a token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a frontier token is 64 lowercase hex digits")]
pub struct NotAToken;

impl Token {
    /// The token of the frontier whose tips are `tips`.
    #[must_use]
    pub fn of(tips: &BTreeSet<Hash>) -> Self {
        let mut lines = Vec::with_capacity(tips.len() * 65);
        for tip in tips {
            lines.extend_from_slice(tip.as_str().as_bytes());
            lines.push(b'\n');
        }
        Self(Hash::of_bytes(&lines))
    }

    /// `text` as a token.
    ///
    /// # Errors
    ///
    /// [`NotAToken`] for anything but 64 lowercase hex digits.
    pub fn parse(text: &str) -> Result<Self, NotAToken> {
        Hash::new(text).map(Self).map_err(|_| NotAToken)
    }

    /// The 64 hex digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// What a pull says that the replica holds: nothing, or the frontier that
/// an earlier answer named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Since {
    /// The replica holds nothing of the store.
    Nothing,
    /// The frontier that an answer of the store of record named.
    Frontier(Token),
}

/// What a [`Since`] reaches in the store's objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reached {
    /// The objects that the frontier's tips rest on, the tips included.
    /// The set is empty for [`Since::Nothing`].
    Known(BTreeSet<Hash>),
    /// The store of record does not hold the token, or the store no longer
    /// holds one of the frontier's tips. The answer starts from nothing.
    Unknown,
}

/// The records could not be read or written. The text is the host's.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the recorded frontiers: {0}")]
pub struct Unrecorded(pub String);

/// A pull that the store of record could not answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unanswered {
    /// The records could not be read or written.
    #[error(transparent)]
    Unrecorded(#[from] Unrecorded),
    /// The store's objects have no linear order: a parent is missing, or
    /// the objects form a cycle.
    #[error("the store's objects: {0}")]
    Unordered(ProdromeError),
}

/// The host's place for the frontiers: the tips of each frontier, by its
/// store and its token, and the tokens of each store, newest last. Each
/// method is one read or one replacement.
pub trait Records: Send + Sync {
    /// The tips recorded under `token` in `store`, or `None`.
    ///
    /// # Errors
    ///
    /// [`Unrecorded`] when the records cannot be read.
    fn tips(&self, store: &str, token: &Token) -> Result<Option<BTreeSet<Hash>>, Unrecorded>;

    /// Record `tips` under `token` in `store`.
    ///
    /// # Errors
    ///
    /// [`Unrecorded`] when the records cannot be written.
    fn keep(&self, store: &str, token: &Token, tips: &BTreeSet<Hash>) -> Result<(), Unrecorded>;

    /// Remove the tips under `token` in `store`. Removing a token that is
    /// not there succeeds.
    ///
    /// # Errors
    ///
    /// [`Unrecorded`] when the records cannot be written.
    fn let_go(&self, store: &str, token: &Token) -> Result<(), Unrecorded>;

    /// The tokens that `store` keeps, newest last. The list is empty when
    /// the store keeps none.
    ///
    /// # Errors
    ///
    /// [`Unrecorded`] when the records cannot be read.
    fn ring(&self, store: &str) -> Result<Vec<Token>, Unrecorded>;

    /// Replace the tokens that `store` keeps with `ring`.
    ///
    /// # Errors
    ///
    /// [`Unrecorded`] when the records cannot be written.
    fn keep_ring(&self, store: &str, ring: &[Token]) -> Result<(), Unrecorded>;
}

/// A number of frontiers to keep for each store. It suits a host that
/// records fewer than this many other distinct frontiers of a store
/// between two pieces of one pull (see [`Frontiers::over`]).
pub const KEPT: NonZeroUsize = NonZeroUsize::MIN.saturating_add(511);

/// The recorded frontiers of each store, kept in [`Records`].
pub struct Frontiers {
    records: Box<dyn Records>,
    kept: NonZeroUsize,
    locks: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
}

impl Frontiers {
    /// Frontiers kept in `records`, at most `kept` for each store, the
    /// most recently recorded.
    ///
    /// Each piece of each pull, and each post, records one frontier. A
    /// token is let go once `kept` other distinct frontiers of its store
    /// are recorded after it was last recorded, and the next piece that
    /// names it starts from nothing (SPEC law 47). So a pull ends only
    /// while fewer than `kept` other distinct frontiers are recorded
    /// between each two of its pieces, and while the store gains fewer
    /// objects between two pieces than a piece sends. A host chooses
    /// `kept` above the number of frontiers it records between two pieces
    /// of one pull.
    pub fn over(records: impl Records + 'static, kept: NonZeroUsize) -> Self {
        Self {
            records: Box::new(records),
            kept,
            locks: Mutex::default(),
        }
    }

    /// Frontiers kept in memory, for a host with no place of its own.
    #[must_use]
    pub fn remembered(kept: NonZeroUsize) -> Self {
        Self::over(Remembered::default(), kept)
    }

    /// The pull from `since` in `store`, whose objects are `dag`: what
    /// `since` reaches, and the objects outside it in `dag`'s linear order
    /// ([`Dag::linearise`]).
    ///
    /// `since` reaches the closure of its tips when the store of record
    /// holds its token, the tips it holds under the token are the tips the
    /// token names, and `dag` holds every one of those tips. The second
    /// check means that a torn or misplaced record can never widen what a
    /// token reaches. Otherwise it
    /// reaches nothing, and every object is lacking. A store can go back to
    /// an earlier state that holds less, for example when it is restored
    /// from a copy, so a frontier recorded before that can name tips that
    /// the store no longer holds.
    ///
    /// # Errors
    ///
    /// [`Unanswered`]: the records cannot be read, or `dag` has no linear
    /// order.
    pub fn answer<'a, E: Schema>(
        &'a self,
        store: &'a str,
        since: &Since,
        dag: &'a Dag<E>,
    ) -> Result<Lacking<'a, E>, Unanswered> {
        let reached = match since {
            Since::Nothing => Reached::Known(BTreeSet::new()),
            Since::Frontier(token) => match self.records.tips(store, token)? {
                Some(tips)
                    if Token::of(&tips) == *token
                        && tips.iter().all(|tip| dag.get(tip).is_some()) =>
                {
                    Reached::Known(dag.closure(tips))
                }
                Some(_) | None => Reached::Unknown,
            },
        };
        let outside = {
            let reached = match &reached {
                Reached::Known(reached) => reached,
                Reached::Unknown => &BTreeSet::new(),
            };
            let order = dag.linearise().map_err(Unanswered::Unordered)?;
            order
                .into_iter()
                .filter(|name| !reached.contains(name))
                .filter_map(|name| dag.objects().get_key_value(&name))
                .collect()
        };
        Ok(Lacking {
            frontiers: self,
            store,
            dag,
            reached,
            outside,
        })
    }

    /// Record `tips` as the newest frontier of `store`, and answer its
    /// token. The oldest frontiers past the kept number are let go.
    ///
    /// The writes are ordered so that a failure between two of them leaves
    /// only tokens that read as unknown. The tokens that are let go are
    /// removed first, then the ring is written, and the tips are written
    /// last. So a ring can list a token with no tips, which reads as
    /// unknown, and no tips are ever kept that no ring lists.
    fn record(&self, store: &str, tips: &BTreeSet<Hash>) -> Result<Token, Unrecorded> {
        let lock = self.lock_of(store);
        let _held = held(&lock);
        let token = Token::of(tips);
        let mut ring = self.records.ring(store)?;
        if ring.last() != Some(&token) {
            ring.retain(|kept| kept != &token);
            ring.push(token.clone());
            let gone = ring.len().saturating_sub(self.kept.get());
            for old in ring.drain(..gone) {
                self.records.let_go(store, &old)?;
            }
            self.records.keep_ring(store, &ring)?;
        }
        if self.records.tips(store, &token)?.is_none() {
            self.records.keep(store, &token, tips)?;
        }
        Ok(token)
    }

    fn lock_of(&self, store: &str) -> Arc<Mutex<()>> {
        Arc::clone(held(&self.locks).entry(store.to_owned()).or_default())
    }
}

/// A pull read against a store's objects: what its [`Since`] reaches, and
/// what it lacks, in order.
pub struct Lacking<'a, E: Schema> {
    frontiers: &'a Frontiers,
    store: &'a str,
    dag: &'a Dag<E>,
    reached: Reached,
    outside: Vec<(&'a Hash, &'a Envelope<E>)>,
}

/// The frontier that an answer recorded: its token, and the number of
/// objects that the replica holds of the store after it takes the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The token that the replica names in its next pull.
    pub token: Token,
    /// The number of objects in the frontier's closure.
    pub count: usize,
}

impl<E: Schema> Lacking<'_, E> {
    /// What the pull's [`Since`] reaches.
    #[must_use]
    pub fn reached(&self) -> &Reached {
        &self.reached
    }

    /// The objects that the pull lacks, each with its name, in the store's
    /// linear order. Each prefix of the list, joined to what
    /// [`Lacking::reached`] reaches, is closed under parents.
    #[must_use]
    pub fn lacking(&self) -> &[(&Hash, &Envelope<E>)] {
        &self.outside
    }

    /// Record the frontier of a piece that sends the first `sent` objects
    /// of [`Lacking::lacking`], all of them when `sent` is larger.
    ///
    /// # Errors
    ///
    /// [`Unrecorded`] when the records cannot be read or written.
    pub fn record(self, sent: usize) -> Result<Recorded, Unrecorded> {
        let mut held = self.held();
        held.extend(
            self.outside
                .iter()
                .take(sent)
                .map(|(name, _)| (*name).clone()),
        );
        self.frontier(&held)
    }

    /// Record the frontier of a post: what the pull's [`Since`] reaches,
    /// and every object of the store that the poster sent, with the objects
    /// they rest on. A name that the store does not hold is passed over.
    ///
    /// # Errors
    ///
    /// [`Unrecorded`] when the records cannot be read or written.
    pub fn record_with(
        self,
        taken: impl IntoIterator<Item = Hash>,
    ) -> Result<Recorded, Unrecorded> {
        let mut held = self.held();
        held.extend(
            self.dag.closure(
                taken
                    .into_iter()
                    .filter(|name| self.dag.get(name).is_some()),
            ),
        );
        self.frontier(&held)
    }

    fn held(&self) -> BTreeSet<Hash> {
        match &self.reached {
            Reached::Known(reached) => reached.clone(),
            Reached::Unknown => BTreeSet::new(),
        }
    }

    /// `held`, a set of the store's objects closed under parents, recorded
    /// by its tips.
    fn frontier(&self, held: &BTreeSet<Hash>) -> Result<Recorded, Unrecorded> {
        let graph: Vec<(&Hash, Vec<Hash>)> = held
            .iter()
            .filter_map(|name| self.dag.get(name).map(|object| (name, parents_of(object))))
            .collect();
        let tips = tips_among(
            graph
                .iter()
                .map(|(name, parents)| (*name, parents.as_slice())),
        );
        Ok(Recorded {
            token: self.frontiers.record(self.store, &tips)?,
            count: held.len(),
        })
    }
}

/// Records kept in memory.
#[derive(Default)]
struct Remembered {
    sets: Mutex<BTreeMap<(String, Token), BTreeSet<Hash>>>,
    rings: Mutex<BTreeMap<String, Vec<Token>>>,
}

impl Records for Remembered {
    fn tips(&self, store: &str, token: &Token) -> Result<Option<BTreeSet<Hash>>, Unrecorded> {
        Ok(held(&self.sets)
            .get(&(store.to_owned(), token.clone()))
            .cloned())
    }

    fn keep(&self, store: &str, token: &Token, tips: &BTreeSet<Hash>) -> Result<(), Unrecorded> {
        held(&self.sets).insert((store.to_owned(), token.clone()), tips.clone());
        Ok(())
    }

    fn let_go(&self, store: &str, token: &Token) -> Result<(), Unrecorded> {
        held(&self.sets).remove(&(store.to_owned(), token.clone()));
        Ok(())
    }

    fn ring(&self, store: &str) -> Result<Vec<Token>, Unrecorded> {
        Ok(held(&self.rings).get(store).cloned().unwrap_or_default())
    }

    fn keep_ring(&self, store: &str, ring: &[Token]) -> Result<(), Unrecorded> {
        held(&self.rings).insert(store.to_owned(), ring.to_vec());
        Ok(())
    }
}

/// The value behind `lock`. A writer that panicked left a whole value,
/// since every write here is one replacement, so the value is read.
fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip(n: usize) -> Hash {
        Hash::new(format!("{n:064x}")).expect("64 lowercase hex digits are a name")
    }

    /// A token is the sha256 of the sorted tips, one on each line.
    #[test]
    fn a_token_is_the_sha256_of_the_tips_one_on_each_line() {
        let tips: BTreeSet<Hash> = [tip(2), tip(1)].into();
        let lines = format!("{}\n{}\n", tip(1).as_str(), tip(2).as_str());
        assert_eq!(Token::of(&tips), Token(Hash::of_bytes(lines.as_bytes())));
        assert_eq!(Token::of(&BTreeSet::new()), Token(Hash::of_bytes(b"")));
        assert_eq!(
            Token::parse(Token::of(&tips).as_str()),
            Ok(Token::of(&tips))
        );
        assert_eq!(Token::parse("ABC"), Err(NotAToken));
    }
}
