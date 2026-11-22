//! A replica's frontier named by one 64-character token (SPEC §3, "A
//! frontier token"; law 47).
//!
//! A token replaces the list of the replica's tips, which can be too long
//! for one request. [`Frontiers::answer`] reads a pull's [`Since`] and
//! answers a [`Lacking`]. The host chooses how many of the lacking objects
//! to send, and [`Lacking::record`] records the frontier of what the replica
//! then holds. The library builds that set, so it is closed under parents by
//! construction.
//!
//! A token that the store of record does not hold, or one that names a tip
//! that the store no longer holds, reads as [`Reached::Unknown`], and the
//! answer then starts from nothing, so no answer leaves out an object that
//! the replica lacks.
//!
//! The host supplies the [`Records`], for example a directory or a table.
//! A [`Frontiers`] serialises its writes to one store with a lock in its own
//! process, so a host keeps one [`Frontiers`] for each place where it keeps
//! records.

#![warn(clippy::pedantic)]

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::dag::{Dag, Reach};
use crate::event::{Envelope, Hash};
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

/// What the replica holds, as its pull names it: nothing, or the frontier
/// that an earlier answer named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Since {
    /// The replica holds nothing of the store.
    Nothing,
    /// The frontier that an answer of the store of record named.
    Frontier(Token),
}

/// What a [`Since`] reaches in the store's objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reached<'d> {
    /// Every object of the store: the frontier's tips are the store's tips.
    /// The answer builds no set of them and walks none, so a replica that
    /// lacks nothing costs the store of record a read of its record alone.
    Everything,
    /// The objects that the frontier's tips rest on, the tips included,
    /// as positions in the store's linear order (SPEC law 64). The set is
    /// empty for [`Since::Nothing`].
    Known(&'d Reach<'d>),
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
    /// A pull ends only while fewer than `kept` other distinct frontiers of
    /// its store are recorded between each two of its pieces (SPEC law 47).
    /// So a host chooses `kept` above the number of frontiers that it records
    /// between two pieces of one pull.
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
    /// `since` reaches (SPEC law 47(a)), and the objects outside it in
    /// `dag`'s linear order.
    ///
    /// Where the tips held under the token are `dag`'s own tips, `since`
    /// reaches [`Reached::Everything`], and the answer builds no closure and
    /// orders no object.
    /// Where the tips held under the token are the tips that the token names,
    /// and `dag` holds every one of them, `since` reaches their closure.
    /// Otherwise `since` reaches nothing, and every object is lacking.
    /// The check against the token means that a torn or misplaced record can
    /// never widen what a token reaches.
    /// The check against `dag` is needed because a store can go back to an
    /// earlier state that holds less, for example when it is restored from a
    /// copy.
    /// A DAG keeps its linear order ([`Dag::linearise`]) and its tips
    /// ([`Dag::tips`]) once derived, so a second answer on a DAG that did not
    /// change orders no object.
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
        let tips = dag.tips();
        let held = match since {
            Since::Nothing => Held::Known(dag.reach().map_err(Unanswered::Unordered)?),
            Since::Frontier(token) => match self.records.tips(store, token)? {
                Some(named) if Token::of(&named) == *token && named == tips => Held::Everything,
                Some(named)
                    if Token::of(&named) == *token
                        && named.iter().all(|tip| dag.get(tip).is_some()) =>
                {
                    let mut reach = dag.reach().map_err(Unanswered::Unordered)?;
                    reach.close(&named);
                    Held::Known(reach)
                }
                Some(_) | None => Held::Unknown(dag.reach().map_err(Unanswered::Unordered)?),
            },
        };
        let (places, outside) = match &held {
            Held::Everything => (Vec::new(), Vec::new()),
            Held::Known(reach) | Held::Unknown(reach) => reach
                .outside()
                .filter_map(|(at, name)| Some((at, dag.objects().get_key_value(name)?)))
                .unzip(),
        };
        Ok(Lacking {
            frontiers: self,
            store,
            dag,
            tips,
            held,
            places,
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

/// What a pull's [`Since`] reaches, and what its records start from.
enum Held<'a> {
    /// Every object, as [`Reached::Everything`].
    Everything,
    /// The reach of a known frontier, as [`Reached::Known`].
    Known(Reach<'a>),
    /// Nothing, for a frontier that is not known ([`Reached::Unknown`]):
    /// the empty reach over the store's order, which the records fill.
    Unknown(Reach<'a>),
}

/// A pull read against a store's objects: what its [`Since`] reaches, and
/// what it lacks, in order.
pub struct Lacking<'a, E: Schema> {
    frontiers: &'a Frontiers,
    store: &'a str,
    dag: &'a Dag<E>,
    tips: BTreeSet<Hash>,
    held: Held<'a>,
    /// The positions in the store's linear order of the objects outside
    /// `held`, one for each object of `outside`.
    places: Vec<usize>,
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
    pub fn reached(&self) -> Reached<'_> {
        match &self.held {
            Held::Everything => Reached::Everything,
            Held::Known(reach) => Reached::Known(reach),
            Held::Unknown(_) => Reached::Unknown,
        }
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
        match &self.held {
            Held::Everything => self.everything(),
            Held::Known(reach) | Held::Unknown(reach) => {
                let mut held = reach.clone();
                for &at in self.places.iter().take(sent) {
                    held.insert(at);
                }
                self.frontier(&held)
            }
        }
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
        match &self.held {
            Held::Everything => self.everything(),
            Held::Known(reach) | Held::Unknown(reach) => {
                let mut held = reach.clone();
                let taken: Vec<Hash> = taken.into_iter().collect();
                held.close(&taken);
                self.frontier(&held)
            }
        }
    }

    /// The frontier of a replica that holds every object: the store's
    /// tips, and the count of the store's objects.
    fn everything(&self) -> Result<Recorded, Unrecorded> {
        Ok(Recorded {
            token: self.frontiers.record(self.store, &self.tips)?,
            count: self.dag.objects().len(),
        })
    }

    /// `held`, a set of the store's objects closed under parents, recorded
    /// by its tips.
    fn frontier(&self, held: &Reach<'_>) -> Result<Recorded, Unrecorded> {
        Ok(Recorded {
            token: self.frontiers.record(self.store, &held.tips())?,
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
