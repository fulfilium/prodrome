//! Prodrome — a temporal, content-addressed event database with
//! fulfillment-priority semantics.
//!
//! A store is a directory of plain files: one file per object, holding that
//! object's canonical print, named by the SHA-256 of those bytes. Objects name
//! their parents, so the history is a DAG; order is causal and the `at` a
//! writer stamped is data, read by the folds and never by the merge. A fold
//! turns the DAG into what is believed at an instant; a term of FPL turns a
//! todo into what it is worth as a function of time.
//!
//! The specification is `../SPEC.md` — it is the contract, and this crate is
//! the reference implementation of it. The vectors in `../conformance/` are
//! frozen evidence that an independent implementation once answered the same:
//! prints, hashes, linearisations, frontiers and verify findings exactly,
//! fulfillments to 1e-9. Types first (§1): every value is a closed algebraic
//! type, invariants live in smart constructors, and a record that exists is
//! valid.
//!
//! # Quick start
//!
//! ```
//! use prodrome::event::{mk_created, mk_spec_revised, TodoEvent, TodoId};
//! use prodrome::fold::{env, flatten, link_specs};
//! use prodrome::fpl::{delta_from_hours, fulfillment, instant_of, link, mk_decay};
//! use prodrome::literal::Datetime;
//! use prodrome::policy::Untrusted;
//! use prodrome::reference::Todo;
//! use prodrome::registers::fold;
//! use prodrome::store::EventStore;
//! use prodrome::view::entries;
//!
//! # fn main() -> Result<(), prodrome::literal::ProdromeError> {
//! # let dir = std::env::temp_dir().join("prodrome-quick-start");
//! # let _ = std::fs::remove_dir_all(&dir);
//! // A store is a directory. Its type parameter is its SCHEMA: the events it
//! // holds and what each writes (`schema::Schema`). `TodoEvent<P>` is the
//! // todo schema, whose record kind carries the HOST's fields as a
//! // `payload::Payload`; `reference::Todo` is the payload this repository's
//! // vectors were taken with. The argument is the deployment's STANDING
//! // policy (§5) — which events bind and which only claim. `Untrusted` is
//! // the reference policy: a roster of actor names whose lifecycle events are
//! // claims.
//! let store = EventStore::<TodoEvent<Todo>>::new(&dir, Untrusted::none());
//!
//! // A prodrome begins with a genesis, and every write names it.
//! store.init("quick start")?;
//!
//! // An event carries the instant its writer stamped on it. The store has no
//! // clock: `at` is data, and nothing here reads the machine's.
//! let at = Datetime::new(2026, 9, 8, 9, 0, 0, 0)?;
//! store.append(mk_created("todo-1", at, "bassel", "publish the crate", "")?)?;
//!
//! // A spec is an FPL term — what this todo is worth as a function of time.
//! let deadline = instant_of(Datetime::new(2026, 9, 15, 17, 0, 0, 0)?);
//! let spec = mk_decay(0.55, 0.05, deadline, delta_from_hours(72.0), None)?;
//! store.append(mk_spec_revised("todo-1", at, "bassel", spec, "")?)?;
//!
//! // Read the DAG back: every object rehashed on the way in, once; the store
//! // holds what it has read, and the next read reads only what is new.
//! let dag = store.dag()?;
//! assert_eq!(dag.objects().len(), 3);
//! assert!(store.verify().is_empty(), "no finding against this store");
//!
//! // Fold at an instant, and price what the fold believes. The fold keeps each
//! // prodrome's todos apart; this store is one.
//! let now = Datetime::new(2026, 9, 14, 9, 0, 0, 0)?;
//! let policy = store.policy();
//! let nodes = dag.nodes()?;
//! let state = fold(&nodes);
//! let prodrome = state.prodromes().values().next().expect("one prodrome");
//! let env = env(prodrome, instant_of(now), policy);
//! let functions = flatten(prodrome, instant_of(now), policy)?;
//! let todo = TodoId::new("todo-1")?;
//! // A spec may name other todos with `Ref`, so evaluation takes a CLOSED term:
//! // `link` binds every reference to that todo's own function, and to
//! // `Absent` for a todo the store knows that has none.
//! let closed = link(&functions[&todo], &link_specs(&functions, [&todo]))?;
//! // A value is `Option<f64>`: `None` is `∅`, the reading of `Absent`.
//! let value = fulfillment(&closed, instant_of(now), &env);
//! assert!(value.is_some_and(|value| (0.0..=1.0).contains(&value)));
//!
//! // Or the whole composition at once: one row per todo the chain mentions,
//! // each with its outcome, its function, its price and its conflicts (§6.7).
//! let rows = entries(&nodes, now, policy)?;
//! assert_eq!(rows.len(), 1);
//! assert_eq!(rows[0].state(), "open");
//! assert_eq!(rows[0].value(), Ok(value));
//! # let _ = std::fs::remove_dir_all(&dir);
//! # Ok(())
//! # }
//! ```
//!
//! Layout, one module per layer of the spec:
//! - `literal`  — §2: the grammar, its printer (Python `repr` rules) and parser
//! - `schema`   — §4–§6: a store's events, their entities and registers
//! - `declared` — design §5, §6.0: a schema as data, admitted when its laws hold
//! - `todo`     — the todo schema, the reference one: §4's kinds, §6's registers
//! - `payload`  — §4: the record kind's fields, as a type parameter
//! - `event`    — §4: the event kinds, envelopes, hashing (§3)
//! - `genesis`, `change`, `snapshot` — §3: a prodrome, a change, an attestation
//! - `sign`     — §3, §5: detached signatures, keys as objects, and their proof
//! - `dag`      — §3: the DAG as a value: tips, closure, linearisation, verify
//! - `store`    — §3: the files around a `Dag` (the lock, placement, quarantine),
//!   and replicas: one in memory, an overlay, `sync` and a `Decision`
//! - `memo`     — design §6.2: caches as tabulations of pure functions
//! - `digest`   — design §6.5: a summary tree over a log, and its budgeted view
//! - `nest`     — design §6.3: histories held in registers, and their join
//! - `term`     — §7: the functor `TermF`, its fixed point `Term`, normal form
//! - `fpl`      — §7: smart constructors, evaluation, explain (Cofree), link
//! - `chain`    — §7.1: the chain compiler, `After` erased against a snapshot
//! - `policy`   — §5: `Standing`, the `Policy` trait, the reference policy, and
//!   `Proven`, the policy that requires signatures
//! - `registers`— §6.6: the DAG's structure and each todo's stream, a monoid action
//! - `schedule` — a step function of time, and its algebra: what a
//!   `Piecewise`, a register's history and an observed value each are
//! - `fold`     — §6.1–6.5: the registers, and every reading a projection of them
//! - `breaks`   — §7 breakpoints and series knots
//! - `observe`  — §7: a view's precision, and when the value it shows next changes
//! - `view`     — §6.7: the entry, the composition of the folds above
//! - `reference`— the payload the vectors were taken with (feature `reference`)

pub mod breaks;
/// §7.1 — the chain compiler: `After` erased against a snapshot, so a chain
/// composes into one evaluable term instead of a lookup per node per sample.
pub mod chain;
pub mod change;
pub mod dag;
pub mod declared;
pub mod digest;
pub mod event;
pub mod fold;
pub mod fpl;
pub mod genesis;
pub mod literal;
pub mod memo;
pub mod nest;
pub mod observe;
pub mod payload;
pub mod policy;
/// The reference payload — the record shape `conformance/*.py` was taken
/// with. A host defines its own; this one is behind a default feature so a
/// host that wants none of it can turn it off.
#[cfg(feature = "reference")]
pub mod reference;
pub mod registers;
pub mod schedule;
pub mod schema;
pub mod sign;
pub mod snapshot;
pub mod store;
pub mod tags;
pub mod term;
pub mod todo;
pub mod topo;
pub mod view;

// The property tests' case counts, one rule with the integration tests'.
#[cfg(test)]
#[path = "../tests/common/cases.rs"]
mod cases;
