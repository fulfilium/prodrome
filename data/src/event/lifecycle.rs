//! The fields of a lifecycle event of the todo schema (SPEC §4).

use super::{Actor, TodoId};
use crate::literal::Datetime;

/// The fields of `Completed`, `Cancelled`, `Reopened` and `Tended`, each
/// `(todo, at, actor, note)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    pub todo: TodoId,
    pub at: Datetime,
    pub actor: Actor,
    pub note: String,
}
