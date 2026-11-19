//! Prodrome in the browser — the same core, on the reader's machine.
//!
//! The web app's claim has always been that its numbers are the server's,
//! computed once by one evaluator (SPEC §1). That left the browser doing two
//! semantic things of its own: rehashing the chain objects, and drawing
//! straight lines between knots. This crate takes the FIRST of those out of
//! TypeScript and makes the second checkable, by compiling `prodrome-core`
//! itself to WebAssembly. The browser now runs the core; it still never has a
//! second evaluator.
//!
//! THIN ON PURPOSE. Every function here is: parse the arguments, call one
//! thing in `prodrome`, print the answer. No arithmetic, no policy, and no
//! shape that `view.py` does not already put on the wire. Anything
//! that needs a decision belongs in the core, where the conformance vectors
//! can see it.
//!
//! Strings and JSON at the boundary (see `wire.rs` for why).
//!
//! THIS IS THE REFERENCE MODULE: the todo schema at the reference payload.
//! Its store is read through `prodrome-wasm-exports`, the exports generic
//! over a schema, which it instantiates as `Todos` (`new Todos(objects)`,
//! see that crate for the methods) exactly as a host's module instantiates
//! its own schemas. Beside it are the free functions every page has called,
//! each answering byte for byte as before (`snapshots/dag-1.txt`):
//!
//! | function         | asks                                                    |
//! | ---------------- | ------------------------------------------------------- |
//! | `verify_objects` | §3: do these bytes hash to these names, form one DAG, and end at which tips |
//! | `lifecycle`      | §4: spell one lifecycle event, through its own `mk_*`    |
//! | `seal`           | §3: a legacy store's write onto these heads — the name and the bytes |
//! | `merge_object`   | §3: a legacy store's join of these heads — no event, no actor |
//! | `fold`           | §6.1–6.5: what does the chain believe at an instant      |
//! | `registers`      | §6.6: which registers have more than one live write      |
//! | `entries`        | §6.7: every todo as the folds see it, composed ONCE      |
//! | `fulfillment`    | §7: what is this term worth now                          |
//! | `explain`        | §7: what is that number made of                          |
//! | `series_knots`   | §7 knots: what is that term's curve over a window        |
//! | `next_change`    | §7: when does that term's value, as a view shows it, next change |
//! | `link`           | §7.2: every ref in a term bound to the todo it names     |
//! | `term_json`      | §2 → §7: a stored term's print, as the JSON shape above  |
//! | `printable`      | §2: which Unicode version decides how a string prints, and the digest of that set |
//! | `version`        | which core answered                                     |
//!
//! `store` is file-backed and a browser has no `events/` directory, so the
//! objects arrive over `/api/chain` instead and are read as a
//! [`prodrome::dag::Dag`]: [`verify_objects`] is its findings, asked of a
//! set in memory.
//!
//! NO CLOCK, anywhere below. §1 forbids one in the core, and a browser's clock
//! is the least trustworthy in the system; every moment is an argument — the
//! `at` a replica seals included, which is why `lifecycle` takes one rather
//! than reading one.
//!
//! ⚠️ SINCE STAGE 3 THIS CRATE ALSO WRITES, and it is worth saying why that is
//! not a widening. `seal` and `merge_object` build an ENVELOPE and hand back
//! its name and its bytes; they touch no store, because a browser has none.
//! Every rule about what an object may be is still the core's `mk_*`
//! constructors, and the box rehashes and re-verifies everything it is given
//! anyway on the way in. What the browser gains is the ability to
//! name a value the same way the box would — which is the one thing a replica
//! cannot do without §2's printer, and the one thing a second printer in
//! TypeScript would have got subtly wrong.

mod reference;
#[cfg(test)]
mod snapshot;

pub use reference::*;
