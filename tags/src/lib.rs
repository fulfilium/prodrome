//! Mergeable tag sets: a set of tags on each target that any replica can
//! change, and that every replica reads alike once it holds the same
//! objects.
//!
//! SPEC law 48: a tag set is an observed-remove set where an add wins, read
//! through its aliases. The law also gives the print of each event. A
//! membership event ([`membership`]) puts a tag on a target or takes it off.
//! An alias event ([`alias`]) makes a tag a name of another tag. A rename or
//! a merge of two tags writes aliases and no membership.
//! [`TagSet`](set::TagSet) reads the two stores together. A host describes
//! its sets with one [`Tagging`]. The target and the tag can be one type.
//! `prodrome-core` re-exports this crate as `prodrome::tags`.
//!
//! The two field names must differ from each other and from `at`, `actor`
//! and `to`, because a print with a repeated field does not parse. The check
//! runs at compile time, when a schema is first used with a [`Tagging`]:
//!
//! ```compile_fail
//! use prodrome_tags::{membership, Tagging};
//!
//! #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
//! struct Labels;
//!
//! impl Tagging for Labels {
//!     type Target = String;
//!     type Tag = String;
//!     const TARGET: &'static str = "label";
//!     const TAG: &'static str = "label";
//! }
//!
//! let _ = <membership::Membership<Labels> as prodrome_data::schema::headed::Body>::EVENTS;
//! ```

#![warn(clippy::pedantic)]

use std::fmt::Debug;

use prodrome_data::literal::codec::Literal;

pub mod alias;
pub mod membership;
pub mod set;

/// A host's tag sets: the type of a target, the type of a tag, and the
/// names of their fields in an event's print.
pub trait Tagging: Clone + Debug + Ord + Send + Sync + 'static {
    /// What a tag is put on.
    type Target: Literal + Clone + Ord + Debug + Send + Sync + 'static;
    /// A tag.
    type Tag: Literal + Clone + Ord + Debug + Send + Sync + 'static;
    /// The name of the target's field.
    const TARGET: &'static str;
    /// The name of the tag's field.
    const TAG: &'static str;
}

/// The field names of `S`, checked: the two differ, and neither is `at`,
/// `actor` or `to`. Evaluating it fails compilation for a [`Tagging`]
/// whose names would print an event that does not parse.
pub(crate) struct Names<S>(std::marker::PhantomData<S>);

impl<S: Tagging> Names<S> {
    pub(crate) const CHECKED: () = assert!(
        !same(S::TARGET, S::TAG) && !reserved(S::TARGET) && !reserved(S::TAG),
        "a tagging's two field names differ, and neither is `at`, `actor` or `to`"
    );
}

const fn reserved(name: &str) -> bool {
    same(name, "at") || same(name, "actor") || same(name, "to")
}

const fn same(one: &str, other: &str) -> bool {
    let (one, other) = (one.as_bytes(), other.as_bytes());
    if one.len() != other.len() {
        return false;
    }
    let mut i = 0;
    while i < one.len() {
        if one[i] != other[i] {
            return false;
        }
        i += 1;
    }
    true
}
