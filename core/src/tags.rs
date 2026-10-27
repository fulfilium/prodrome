//! Mergeable tag sets: a set of tags on each target that any replica can
//! change, and that every replica reads alike once it holds the same
//! objects (SPEC law 48).
//!
//! Two schemas hold the sets. A membership event puts a tag on a target or
//! takes it off. Its key is the pair of a target and a tag, so each pair
//! has one register, and the register reads the greatest value of its
//! frontier under `Off < On`. So a removal takes off the adds that its
//! writer saw, and an add that it did not see stays on: an observed-remove
//! set where an add wins. An alias event makes a tag a name of another tag,
//! or a name of its own again. A rename or a merge of two tags writes
//! aliases and no membership. [`TagSet`](set::TagSet) reads the two stores
//! together: it follows each name's alias to the tag that it stands for,
//! and reads a ring of aliases as its least name.
//!
//! A host describes its sets with one [`Tagging`]: the type of a target,
//! the type of a tag, and the names of their fields in an event's print.
//! The events print as `Added(<target>, <tag>, at, actor)`,
//! `Removed(<target>, <tag>, at, actor)`, `Aliased(<tag>, at, actor, to)`
//! and `Unaliased(<tag>, at, actor)`. The target and the tag can be one
//! type.
//!
//! Each field name must differ from the other and from `at`, `actor` and
//! `to`, since a print with a repeated field does not parse. The check runs
//! when a schema is first used with a [`Tagging`], at compile time:
//!
//! ```compile_fail
//! use prodrome::tags::{membership, Tagging};
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
//! let _ = <membership::Membership<Labels> as prodrome::schema::headed::Body>::EVENTS;
//! ```

#![warn(clippy::pedantic)]

use std::fmt::Debug;

use crate::literal::codec::Literal;

pub mod alias;
pub mod membership;
pub mod set;

/// A host's tag sets: what a target is, what a tag is, and the names of
/// their fields in an event's print.
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
