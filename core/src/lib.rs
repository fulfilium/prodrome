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
//! # Which crate to depend on
//!
//! This crate re-exports the twelve other crates of the library at the paths
//! of its modules, and it holds the law kit (feature `laws`). A program that
//! needs only some of the library depends on those crates and not on this
//! one. A crate's "tokens" are the size of its public API as an agent reads
//! it: `scripts/face.py --lines`, four characters to a token.
//! `docs/design-core-face.md`, section 14, gives the method and the numbers.
//!
//! | Crate | Holds | Tokens |
//! |---|---|--:|
//! | `prodrome-data` | values and their print, events, objects and their names, the traits of a schema and of a register, `#[derive(Literal)]` | 19,994 |
//! | `prodrome-memo` | caches and the cached fold | 7,296 |
//! | `prodrome-dag` | the DAG of a store's objects, its fold, policies, the proof of signatures | 14,118 |
//! | `prodrome-store` | the store on disk and in memory, replicas, doors, frontiers | 21,027 |
//! | `prodrome-declared` | schemas declared as text | 3,973 |
//! | `prodrome-tags`, `prodrome-nest`, `prodrome-digest` | tag sets; histories held in registers; a summary tree over a log | 3,979, 3,903, 4,099 |
//! | `prodrome-term` | FPL terms: build, print, parse; schedules | 6,574 |
//! | `prodrome-fpl` | the evaluator, the chain compiler, breakpoints, observation | 5,500 |
//! | `prodrome-todo` | the todo schema's events and payload, the reference payload | 4,686 |
//! | `prodrome-reading` | a todo's price and its row in a list | 3,949 |
//!
//! What to depend on, for what a program does:
//!
//! | The program | Depends on | Tokens |
//! |---|---|--:|
//! | writes events with a store and reads them under a policy | data, memo, dag, store | 62,435 |
//! | reads a todo's price and its row in a list | data, memo, dag, term, fpl, todo, reading | 62,117 |
//! | has a schema of its own and no todos: defines it, prints and parses its events | data | 19,994 |
//! | the same, and folds its events under a policy | data, memo, dag | 41,408 |
//! | the same, and keeps them in a store | data, memo, dag, store | 62,435 |
//! | adds a second backend for a replica: implements `store::Replica` | data, memo, dag, store | 62,435 |
//! | uses the whole library, or the law kit | this crate | 112,431 |
//!
//! `prodrome-dag` depends on `prodrome-memo`, so a program that reads a DAG
//! reads both. `prodrome-store` also holds the files of a store on disk.
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
//! store.append(mk_created("todo-1", at, "ada", "publish the crate", "")?)?;
//!
//! // A spec is an FPL term — what this todo is worth as a function of time.
//! let deadline = instant_of(Datetime::new(2026, 9, 15, 17, 0, 0, 0)?);
//! let spec = mk_decay(0.55, 0.05, deadline, delta_from_hours(72.0), None)?;
//! store.append(mk_spec_revised("todo-1", at, "ada", spec, "")?)?;
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
//! # Modules
//!
//! Each module, with the section of `SPEC.md` (or of a design note) for it:
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

#[cfg(feature = "laws")]
pub mod laws;

// The readings of a schema's history and their instances for the todo
// schema, from `prodrome-reading`, at the paths that a program has always
// named.
pub use prodrome_reading::{fold, schema, todo, view};

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
