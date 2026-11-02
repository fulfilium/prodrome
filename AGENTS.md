# Instructions for coding agents

You are a coding agent that works in this repository. Follow these
instructions for every change. If an instruction conflicts with a request,
ask the person who made the request before you continue.

## Before you push

Run these commands, and push only when all of them pass:

```console
$ nix develop -c cargo fmt --all --check
$ nix develop -c cargo clippy --locked --workspace --all-targets -- -D warnings
$ PRODROME_MAX_CASES=32 nix develop -c cargo test --locked --workspace
```

If you change the wasm, the viewer, `typst/` or `flake.nix`, also run
`nix flake check`.

If your change alters what a program can observe (a value, a stored byte,
or a result of `verify`), change `SPEC.md` in the same pull request. Add a
law in SPEC section 9 with a property test that checks it, or add a vector
in `conformance/`. Add an entry to `CHANGELOG.md`.

## Never do these things

- Never skip, weaken, disable or delete a test to make the CI pass. Fix the
  code, or report the failure.
- Never regenerate a vector in `conformance/` or `wasm/conformance/` to make
  a test pass. These files are fixed.
- Never change the fields of a stored constructor that a release contains.
  To store new data, add a new constructor.
- Never edit or delete an object in a store. On the `roadmap-data` branch,
  add objects only through the `prodrome` CLI.
- Never write private data in this repository. The repository is public.
  Do not name a real person, account, deployment, place or private project.
  In examples and tests, use neutral names, such as the actor `ada`.
- Never put two unrelated changes in one pull request.

## Style rules

Each rule has an example from the code. Where today's code does not follow
a rule yet, the rule lists the exception. Do not add new exceptions. When
you change code that is listed as an exception, fix it if you can.

### 1. Make invalid states unrepresentable

Use a sum type for a value that has several states. Do not use a `bool` or
an `Option` to encode a state. For example, `dag::finding::Unread` gives
each reason that a file is not an object its own variant:

```rust
pub enum Unread {
    Io(ProdromeError),
    Tampered(Hash), // the hash of the bytes
    NotText,
    Unparsed(ProdromeError),
}
```

Use a newtype only where code checks or constrains the value. `TodoId`,
`Hash`, `Actor` and `Name` in `core/src/event.rs` are newtypes with a
private field, because each one has its own rule for valid text.

**Exceptions.** `observe::NextChange` encodes three states with
`at: Option<Instant>` and `exact: bool`. `github::Delivery` has the field
`pull_request: bool`. The fields `start` and `end` of `TermF::Decay` are
plain `f64` values, although they must be in `[0, 1]`.

### 2. Parse, don't validate

Check input once, at the edge of the program. Return a type that keeps the
result of the check, so that later code does not check again:

```rust
pub fn new(text: impl Into<String>) -> Result<TodoId, ProdromeError> // in impl TodoId
```

The smart constructors in `core/src/fpl.rs`, such as `mk_decay`, follow the
same rule for terms. `literal::Finite::new` follows it for numbers.

**Exceptions.** `Term::new` is public, and the fields of `TermF` are
public, so code can build a term without the `mk_*` checks.
`literal::print_float` takes an `f64` instead of a `Finite`.
`declared::admit` checks the key and stamp laws, but it does not return a
type that records the result.

### 3. Write total functions

Do not call `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!` or
`unimplemented!` for a state that you think is impossible. Change the types
so that the state cannot occur, or return a `Result`.

**Exception.** The production code has no `unwrap`, `panic!`, `todo!` or
`unimplemented!`. It has about 45 calls to `expect` and 5 calls to
`unreachable!`, mostly in `fpl.rs`, `chain.rs`, `literal.rs` and
`declared/`. Each call has a message that names the invariant, for example:

```rust
// `mk_shift` refuses nothing, so the `expect` is a proof and not a hope.
fn mk_shift(delta: Delta, term: Term) -> Term {
    fpl::mk_shift(delta, term).expect("Shift admits every span")
}
```

Do not add a new call. If you must keep one, write the invariant in its
message.

### 4. Match exhaustively

Do not write a `_ =>` arm in a match on one of this workspace's own enums.
Then a new variant forces every match to handle it. A `_ =>` arm is
acceptable for a foreign type, such as `char`, `&str` or an integer.

**Exception.** About 30 matches on our own enums have a `_ =>` arm. Some
are single-variant projections, such as a function that returns `Some` for
one variant of `Value` and `None` for the rest. Some are off-diagonal pairs
in a match on a pair of enums, as in `declared/order.rs`. Others give a
default behaviour, for example in `breaks.rs`, `chain.rs`,
`term/normalize.rs`, and the matches on `Envelope` in `dag.rs`, `sign.rs`
and `store/memory.rs`. Replace the default-behaviour arms when you change
these functions.

### 5. Defunctionalize

If a set of behaviours is closed, write it as an enum, and interpret the
enum in one function. Do not use closures or trait objects for a closed
set. A term is data, not a closure:

```rust
/// The fixed point `Term ≅ TermF Term`, a deep embedding and never a closure.
pub struct Term(Box<TermF<Term>>);
```

`fpl::fulfillment` interprets a `Term`, and `chain::compile` rewrites one.
Use a trait object only for a set that is open to other programs, such as
`literal::Vocabulary` and `nest::Level`, or to pass an effect in, such as
the `print_of` callback of the store traits.

### 6. Keep the core pure, and put effects at the edge

Do not make a value that the core computes depend on the clock. An
event's `at` is a field that its writer sets. Only `cli/` reads the system
clock to stamp events, in `cli/src/price.rs`.

Make the state between two steps an explicit value, and write a run as a
fold over the steps. For example, `fold::read` computes an entity's
registers as the last value of a `scan` over its event stream:

```rust
pub fn read<'a, E: Schema>(stream: &'a [Stamp<E>], at: Option<Instant>, policy: &impl Policy<E>)
    -> E::Registers<'a> { scan(stream, at, policy, |_, _| {}) }
```

**Exception.** `core/src/store.rs` does file input and output, and it calls
`SystemTime::now()` to decide when to look at a file again. The time does
not affect any value that a read returns. The store also draws random bits
for the names of temporary files and for a genesis nonce. `spread::spread`
runs a map over a vector on several threads, for the store's reads and
`Dag::from_prints`; its answer is the map's, whatever the number of threads.

### 7. Write generic functions, and one combinator with laws

If the body of a function does not depend on a type, make the function
generic over that type. Write one combinator with stated laws, and use it
everywhere. Do not write copies. Examples:

- `TermF::traverse`, with `Term::cata` and `Term::try_cata` built on it, in
  `core/src/term/functor.rs`;
- `topo::linear`, the crate's one causal order, which `dag.rs` and
  `digest.rs` use;
- `memo::fold`, for every cached fold.

**Exception.** `dag.rs` has its own Kahn walk to admit objects whose
parents are missing.

### 8. Laws are tests

State each law in `SPEC.md` section 9. Check each law with a test, and name
the law in the test's doc comment. Use `proptest` for a law over generated
inputs, and a vector in `conformance/` for a fixed input:

```rust
/// Law 26: commutative, associative, idempotent, `Least([t]) = t`, and
/// `Absent` is the identity.
#[test]
fn least_is_a_semilattice(a in a_term(), b in a_term(), c in a_term(), env in an_env()) {
```

Configure each property test through `core/tests/common/cases.rs`, so that
`PRODROME_MAX_CASES` applies to it.

**Exception.** The doc comments of the tests do not cite laws 14, 15, 16,
22, 30 to 33, 35, 38 and 39 by number, although tests check them. Add the
number when you change one of these tests.

### 9. Put one concept in each file

Name each file for its concept, and put the main type first.
`core/src/topo.rs`, `core/src/fold/frontier.rs` and
`core/src/dag/finding.rs` are good examples.

**Exception.** Some large files contain several concepts:
`core/src/fpl.rs` (about 1460 lines), `wasm/exports/src/lib.rs`,
`core/src/literal.rs`, `core/src/store.rs` and `core/src/event.rs`. If you
add a new concept to one of these files, put it in a new file instead.

### 10. Do not program defensively

Do not add a check for a state that the types already exclude. If a check
seems necessary, change the type instead.

**Exception.** `literal::print_float` calls `debug_assert!` to check that
its `f64` is finite. A `Finite` argument would make the check unnecessary.

### 11. Keep it simple, but not simpler

Do not add a wrapper, a trait, or a parameter that no code uses. Add an
abstraction when a second user needs it, not before. Do not remove a type
that keeps an invariant only to save lines.

### 12. Comments describe the code, not its history

A comment says what the code does and why. The commit message gives the
history. Do not write "used to", "now", "since 0.3", or "moved from" in a
comment. A comment can describe stored data from an earlier version, such
as a store from before version 0.9 that has a `HEAD` file, because the code
still reads that data.

**Exception.** A few comments still describe history, for example in
`wasm/exports/src/wire.rs`, `core/src/policy.rs` and `core/src/store.rs`.

## Write plain technical English

Write all prose in plain technical English, close to ASD-STE100. This rule
covers code comments, documentation, CHANGELOG entries, commit messages,
pull request descriptions, and the text of roadmap items. A new reader must
understand each sentence on the first read.

1. Give each sentence a concrete subject, a plain verb, and an explicit
   object. Say who does what to what.
2. Make only people and programs act. Do not write that data, bytes,
   documents or notes "promise", "say", "plan", "name" or "keep" something.
3. Do not write riddles, wordplay or slogans. State the fact plainly.
4. Use common words. Define each term of art (for example fold, register,
   policy, head, claim, binds) in one plain sentence where it first
   appears, or link to its definition. Do not put two terms of art in one
   phrase.
5. Put one idea in each sentence. Write instructions in the imperative.
   Put a condition first.
6. Keep the words "the" and "a". Do not drop words to save space.
7. Use the active voice and the simple present tense.
8. Write descriptions as explanations for a stranger, not as notes to
   yourself.

Examples of compressed text, and the plain text to write instead:

| Do not write | Write |
|---|---|
| the history is free, and a schema is its reading | Events are stored without a schema, and a program applies a schema when it reads the events. |
| what the stored bytes promise | which changes affect the stored format |
| It folds and prices them with the core compiled to WebAssembly. | The page computes each item's current value in the browser, with the Prodrome core compiled to WebAssembly. |
| Each note proposed a change before it was made. The SPEC says what landed. | Each design note was written before its change was made. For what was implemented, see `SPEC.md`. |

For a pull request, follow `.github/pull_request_template.md`: write two or
three plain sentences about the change, and one line about how you tested
it.

## Lints

The CI runs `cargo clippy --workspace --all-targets -- -D warnings`. The
modules `digest`, `memo` and `nest` also enable `clippy::pedantic`. The
binaries forbid `unsafe` code. The repository does not enable
`clippy::unwrap_used` or `clippy::wildcard_enum_match_arm` yet, because of
the exceptions to rules 3 and 4.
