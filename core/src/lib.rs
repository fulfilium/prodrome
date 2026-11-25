//! Prodrome: a temporal, content-addressed event database with
//! fulfillment-priority semantics.
//!
//! This crate is the reference implementation of `SPEC.md`. A store is a
//! directory with one file for each object, which holds the canonical print
//! of the object and is named by the SHA-256 of those bytes. The writer of
//! an event sets its `at`; the folds read it, and the merge never does. The
//! vectors in `conformance/` fix prints, hashes, linearisations, frontiers
//! and `verify` findings exactly, and fulfillments to within 1e-9.
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
//! The crate `prodrome-data` holds the data of a store: `literal`, `topo`,
//! `genesis` and `change`, and the parts of `event`, `payload`, `schema`,
//! `snapshot`, `sign`, `policy` and `fold` that build, print, parse and hash
//! objects. This crate re-exports each of them at the path below. A program
//! that only builds and reads events can depend on `prodrome-data` alone.
//! An example in the documentation of a module of `prodrome-data` works
//! under this crate with `prodrome` in place of `prodrome_data`.
//!
//! The crate `prodrome-memo` holds `memo`, and `prodrome-declared` holds
//! `declared`. The crate `prodrome-dag` holds `dag` and `registers`, the
//! rest of `policy` and `sign`, and the reading of an entity's registers,
//! `fold::read`. A program that reads an entity's registers under a policy
//! can depend on `prodrome-data`, `prodrome-memo` and `prodrome-dag` alone.
//!
//! The crate `prodrome-store` holds `store` and the rest of `snapshot`, on
//! `prodrome-data` and `prodrome-dag`. A program that opens a store,
//! appends to it and reads it under a policy can depend on these three
//! crates alone. The crates `prodrome-nest`, `prodrome-tags` and
//! `prodrome-digest` hold `nest`, `tags` and `digest`. This crate
//! re-exports each module of these crates at the same path.
//!
//! The crate `prodrome-term` holds `term`, `schedule`, and the parts of
//! `fpl` that build, print and parse a term, on `prodrome-data`. The crate
//! `prodrome-fpl` holds the rest of `fpl` (the evaluator), `chain`,
//! `breaks` and `observe`, on `prodrome-term`. The crate `prodrome-todo`
//! holds `event`, `payload` and `reference`, and the todo schema's
//! vocabulary and registers, which `todo` and `fold` re-export, on
//! `prodrome-term`. A program that writes a todo's events can depend on
//! `prodrome-data`, `prodrome-term` and `prodrome-todo` alone.
//!
//! Modules, with the section of `SPEC.md` (or of a design note) for each:
//! - `literal`: values and the literal grammar (§2)
//! - `schema`: the events, entities and registers of a store (§4 to §6)
//! - `declared`: a schema as data (design §5, §6.0)
//! - `todo`: the reference todo schema (§4, §6)
//! - `payload`: the fields of the record kind (§4)
//! - `event`: event kinds, envelopes and hashing (§3, §4)
//! - `genesis`, `change`, `snapshot`: objects of a prodrome (§3)
//! - `sign`: signatures, keys and their proof (§3, §5)
//! - `dag`: the DAG as a value (§3)
//! - `store`: the files around a DAG, and replicas (§3)
//! - `topo`: the one causal order (§3)
//! - `memo`: caches (design §6.2)
//! - `digest`: a summary tree over a log (design §6.5)
//! - `nest`: histories held in registers (design §6.3)
//! - `term`: the term functor and normal form (§7)
//! - `fpl`: construction, evaluation, explanation and linking of terms (§7)
//! - `chain`: the chain compiler (§7.1)
//! - `policy`: standing and policies (§5)
//! - `registers`: the register action over the DAG (§6.6)
//! - `schedule`: step functions of time (design §6.2)
//! - `fold`: the folds (§6.1 to §6.5)
//! - `breaks`: breakpoints and series knots (§7)
//! - `observe`: precision of a view, and its next change (§7)
//! - `view`: the entry (§6.7)
//! - `tags`: mergeable tag sets (law 48)
//! - `reference`: the payload of the vectors (feature `reference`)
//! - `laws`: laws as functions over a program's schemas (§9, feature `laws`)

pub mod fold;
#[cfg(feature = "laws")]
pub mod laws;
pub mod schema;
pub mod todo;
pub mod view;

// The terms of FPL, from `prodrome-term`, and the evaluator, from
// `prodrome-fpl`, at the paths that a program has always named. `fpl`
// re-exports the constructors, the print and the parse of a term with the
// evaluator.
pub use prodrome_fpl::{breaks, chain, fpl, observe};
pub use prodrome_term::{schedule, term};

// The events and the payload of the todo schema, from `prodrome-todo`, at
// the paths that a program has always named. `todo` and `fold` re-export
// its vocabulary and its registers with the readings of a todo.
#[cfg(feature = "reference")]
pub use prodrome_todo::reference;
pub use prodrome_todo::{event, payload};

// The modules of `prodrome-data` that the core does not extend, at the
// paths that a program has always named.
pub use prodrome_data::{change, genesis, literal, text_literal, topo, word_literal};
// Schemas declared as text, from `prodrome-declared`, at the path that a
// program has always named.
pub use prodrome_declared as declared;
// The caches, from `prodrome-memo`, at the path that a program has always
// named.
pub use prodrome_memo as memo;

// The readings, from `prodrome-dag`, at the paths that a program has always
// named. `fold` re-exports the readings of `prodrome-dag` with the readings
// of a todo.
pub use prodrome_dag::{dag, policy, registers, sign};

// The store and the attestation of a snapshot, from `prodrome-store`; the
// histories held in registers, from `prodrome-nest`; the tag sets, from
// `prodrome-tags`; and the summary tree, from `prodrome-digest`. Each is at
// the path that a program has always named.
pub use prodrome_digest as digest;
pub use prodrome_nest as nest;
pub use prodrome_store::{snapshot, store};
pub use prodrome_tags as tags;
