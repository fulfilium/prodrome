# The Prodrome guide

This guide explains Prodrome in the order that a new reader needs it. Part I
explains the model: what a store contains, and how Prodrome computes an
answer from it. Part II explains how to give a todo a price. Part III
explains the Rust API, the files on disk, and WebAssembly.
[`SPEC.md`](../SPEC.md) is the specification. If this guide and the SPEC
disagree, the SPEC is correct. Each section of this guide names the SPEC
section that it follows.

## Part I. The model

### 1. A store holds a history, and a read computes the state at a time

A store holds a HISTORY. The history is a set of objects. Most objects
record one event, for example "todo X was created" or "todo X was
completed". Each object names the objects that came before it, which are
its parents. A program adds objects to the history. A program never changes
or removes an object. So a history only grows, and you merge two histories
by taking the union of their objects.

To READ a store, you give three inputs, and Prodrome gives one output:

```
read : History × Time × Policy → State
```

- The history is the set of objects in the store.
- The time `t` is the moment that you ask about. Each event has a field
  `at`, which is the time that its writer stamped on it. A read at `t` uses
  only the events whose `at` is at or before `t`.
- The policy is a rule that your program supplies. For each event, the
  policy says whether the event takes effect or is only a suggestion.
  Section 4 explains policies.
- The state gives, for each todo, its status (open, completed or
  cancelled), its content, and its spec.

A SPEC is a formula in FPL, a small fuzzy temporal logic. A spec describes a
function from time to a number:

```
value of a spec : Time → a number from 0 to 1, or ∅
```

This number is the todo's FULFILLMENT. It answers the question "how well do
things go if this todo stays as it is?". One minus the fulfillment is the
todo's urgency. The value `∅` means "no value". A note or a proposal that
needs no attention has the value `∅`.

All other parts of Prodrome are operations on these two functions. The SPEC
states three consequences of the model as laws, in section 9:

- **A read of the past does not change.** An event that is dated after `t`
  does not change the result of a read at `t` (law 2). So you can ask what
  the store showed last week, and you get the answer that it gave last week.
- **The parents decide the order, not the clock.** If one write descends
  from another write, the later write replaces the earlier one. The `at`
  stamps never decide which write wins (SPEC section 1, law 25).
- **Replicas agree.** A read depends only on the set of objects. So two
  replicas that hold the same objects give the same answers (laws 17 and
  45).

### 2. Objects and the DAG

*SPEC section 3.*

The objects and their parent links form a directed acyclic graph, the DAG.
There are a few kinds of objects:

- A `Genesis(label, nonce)` object starts a prodrome. A prodrome is one
  history, and a store can hold more than one.
- A `Change(genesis, deps, event)` object records one event. Its `deps`
  field lists what the writer saw of the event's todo when the writer wrote
  the event. These are the todo's HEADS: the writes to the todo that no
  other write descends from. So the `deps` of a change always belong to the
  same todo. One writer's writes are ordered by their parent links, never
  by their `at` stamps. A write replaces only the earlier writes to the same
  register (see section 3) that it descends from.
- A `Snapshot(genesis, tips, previous)` object records the state of the
  whole history at one point, so that a reader can later check it.
- Stores that were written before version 0.9 hold `Sealed` and `Woven`
  objects. Prodrome still reads them.

If you append an event that the prodrome already holds, the store writes
nothing.

The name of an object is the hash of its canonical text. So the name of a
change depends only on its genesis, its event, and what its writer saw of
its todo. The name does not depend on the place where the change was
written. The command `verify` reports each object that does not hash to its
name, that names a missing parent, that is part of a cycle, or that links
two different prodromes.

### 3. Registers, conflicts and folds

*SPEC section 6.*

Prodrome reads the DAG separately for each pair of event kind and todo. For
each pair, it keeps a REGISTER. A register is the set of the latest writes
to one field of one todo: the writes that no later write descends from. The
distinct events in a register are its CANDIDATES. If a register has one
candidate, that candidate is the value. If a register has more than one
candidate, there is a CONFLICT. Prodrome shows the conflict to the caller.
The next write that descends from all the candidates settles the conflict.

A FOLD is a function that walks the history and computes one part of the
state at a time `t`. Each fold reads the registers at `t`:

- `env` gives each todo's candidate outcomes (completed, cancelled or
  open). It also gives each time that the todo was tended. The tended times
  form a set that only grows, and Prodrome merges two such sets by union.
- `specs` gives each todo's candidate specs.
- `content` gives each todo's candidate records.
- `flatten` gives each todo's whole history as one function of time. If the
  spec register has a conflict, `flatten` uses the most urgent candidate,
  through the term `Least`.

### 4. Policies: an event binds or claims

*SPEC section 5.*

Prodrome does not decide whom to trust. Your program gives Prodrome a
POLICY. For each event, the policy answers one question: does this event
BIND (take effect), or does it only CLAIM (make a suggestion)? Prodrome
computes two states:

- The CONFIRMED state uses your policy. A claim does not change it.
- The CLAIMED state treats every event as binding.

Prodrome stores and shows each claim, but a claim never changes the
confirmed state.

The crate has two ready-made policies:

- `Untrusted` takes a list of actor names. The lifecycle events of these
  actors only claim.
- `Proven` lets an event bind only when a signature proves who wrote it.

If you need a different rule, implement the `Policy` trait.

### 5. Records, payloads and schemas

*SPEC sections 4 and 6.*

A program opens a store with a SCHEMA. The schema lists the event kinds and
the registers that each event kind writes. The todo schema is the
reference schema. Six of its event kinds belong to the database, and their
fields define the database's behaviour:

- `Created` creates a todo.
- `Completed`, `Cancelled` and `Reopened` change its status.
- `Tended` records one pass at a recurring todo. It does not change the
  status.
- `SpecRevised` gives the todo a new spec.

The seventh event kind belongs to your program. A record has the form
`KIND(todo, at, actor, <your fields>)`. Your program implements the
`Payload` trait, which gives:

- the name of the record kind;
- the fields, and how to parse and print them;
- the two things that the folds read from a record: the spec that it
  carries, and the number of checklist items that it has.

After you release a payload, do not change the fields of its stored form.
The same rule applies to the database's own event kinds.

## Part II. Pricing work

### 6. FPL terms

*SPEC section 7.*

You build a spec from these terms:

| Term | Meaning |
|---|---|
| `Flat` | the same value at every time |
| `Decay` | a value that falls toward a deadline |
| `Conj` | a power mean of its members, so the weakest member has the most effect |
| `Within` | a term sampled over a window of days ahead |
| `After` | a term that starts when another todo is completed |
| `Recur` | a term that restarts each time a todo is tended |
| `Periodic` | a term that repeats on the calendar |
| `Piecewise` | a different term from each given time on |
| `Ref` | the fulfillment of another todo, for subtodos and groups |
| `Absent` | no value, `∅`; it does not raise or lower the value of a term that contains it |

A term that contains a `Ref` is OPEN, because its value depends on another
todo. The function `link` replaces each `Ref` with the function of the todo
that it names. If that todo has no spec, `link` uses `Absent`. The result is
a `Closed` term. `link` returns an error if a `Ref` names a todo that the
store does not know, or if the references form a loop.

Four functions read a closed term:

- `fulfillment` computes its value at one time.
- `explain` does the same computation, and returns the value at every node
  of the term.
- `series_knots` gives the points of its curve over a window of time.
- `observe::next_change` gives the next time at which the displayed value
  changes, at the precision of a view (for example, whole percent). A
  client can redraw at that time, so it does not need a timer that ticks.

### 7. Pricing your own work

A price is a fulfillment. It answers the question "how well do things go if
this stays as it is?". It does not answer "how important is this?". A low
price means that the todo is urgent. The command-line tool gives a todo a
price in one of two ways:

- `--priority N` sets a constant value from 0 to 100. Use 10 for a critical
  problem, 35 to 45 for a real consequence within weeks, 70 to 85 for a
  nice-to-have, and 90 or more for work that is parked.
- `--deadline YYYY-MM-DD` sets a `Decay`. The value falls from `--start`
  (default 55) to `--end` (default 5). The fall starts `--lead-up-days`
  (default 3) before 17:00 on the deadline day.

Some guidelines:

- For a deadline, use a decay toward the date, not a constant low value.
  Then the todo becomes urgent at the right time, and not before.
- If a todo waits on another todo, make its spec a `Ref` to that todo. Then
  it is exactly as urgent as the todo that it waits on.
- For a group of todos, use a `Conj` of `Ref`s. The weakest member of the
  group has the most effect on the group's value.
- For a recurring chore, use a `Recur`. Its term restarts at each `Tended`
  event, so its value falls with the time since the last pass.
- The command `revise` writes a new spec that applies from its time on. The
  old spec still gives the value at every earlier time.

The roadmap's
[`PRICING.md`](https://github.com/fulfilium/prodrome/blob/roadmap-data/PRICING.md)
is one example of a price scale, with reference points from 10% to 90%.

### 8. The chain compiler

*SPEC section 7.1.*

`After` and `Recur` are the terms that read the history. The function
`chain::compile(term, env)` looks up each of these terms in a snapshot of
the history. It returns a term that gives the same values and does no
lookups:

- each link becomes one offset;
- a chain becomes one schedule;
- a recurrence becomes one piece for each pass.

If a todo upstream in a chain is cancelled, its link compiles to the value
1.0. The value alone does not show the cancellation, so the compiler also
reports it. The compiler is an optimisation, not a second definition of
the semantics. A law (law 13) states that the compiled term and the
original term give the same values. The original path still works, and a
program compiles a term only if it asks to.

### 9. The view

*SPEC section 6.7.*

The function `entries` combines the folds into one row for each todo. A row
contains:

- the todo's outcome and its claim;
- its function of time;
- its value: a number, `absent`, or the reason that `link` failed;
- its current content;
- its conflicts;
- the objects that mention it.

The function `list_order` gives the order of a list: the most urgent todo
first, then every row that has no number.

## Part III. Using the crate

### 10. Your program: a payload and a policy

A program that uses the core supplies two things:

- **A payload**, which implements `payload::Payload`. The payload gives the
  name of its record kind, the fields of a record in order, the vocabulary
  of nested values in those fields, a parse function, a print function, and
  the two values that the folds read (the spec of a record and its checklist
  length). The default feature `reference` includes
  `prodrome::reference::Todo`, which is the payload of the vectors in
  `conformance/`. If your program has its own payload, turn the feature off
  with `default-features = false`.
- **A policy**, which implements `policy::Policy`. A policy is one function
  from an event to `Binds` or `Claims`. This function is all of SPEC
  section 5. `policy::Untrusted` is the reference policy.
  `Untrusted::none()` lets every event bind.

The `prodrome` command-line tool is one such program. It uses the reference
payload, the reference policy, and the system clock. It is the only code in
the workspace that reads the system clock.

To check your schema with the laws that this crate checks its own schemas
with, enable the `laws` feature in your tests' dependency on the crate.
The module `prodrome::laws` gives each law of SPEC section 9 that asks
nothing of a schema as a function. `printed` checks that each print parses
and prints back. `free`, `routed` and `valued` check that a reading is a
function of the object set. `synced` checks that a sync is a join. Each
function takes data: drawn histories (`history`, `a_shuffle`) or the
objects that your stores already hold. It answers `Ok`, or the law that it
found broken, with what differs.

### 11. The Rust quick start

The [quick start](../core/src/lib.rs) in the crate documentation is a
doctest, so `cargo test` runs it. It does these steps:

1. Open a store with `EventStore::<TodoEvent<Todo>>::new(&dir,
   Untrusted::none())`, and call `init` to write its genesis.
2. Call `append` with a `Created` event, and then with a `SpecRevised` event
   whose spec is a `Decay`. Each event carries its own `at`. The store has
   no clock.
3. Read the DAG with `store.dag()`. The store checks the hash of every
   object as it reads it.
4. Compute the state at one time: call `registers::fold` on the nodes, and
   then call `env` and `flatten` on the one prodrome.
5. Call `link` on the todo's function, and compute its value with
   `fulfillment`. The value has the type `Option<f64>`, and `None` means
   `∅`.
6. Or call `view::entries` to get one row for each todo.

Run `cargo doc --open` to read the documentation of every module. The module
list in `core/src/lib.rs` gives the SPEC section of each module.

### 12. The store on disk

*SPEC section 3 gives the full rules for this section.*

```
<root>/
  objects/<sha256>.py     one object, as its canonical text
  .gitattributes          written by the store if the file does not exist
  .lock                   held while a program appends
  quarantine/<sha256>.py  a file that fsck moved because its hash is wrong
```

The canonical text of an object is an expression in a small grammar that
uses Python literal syntax (SPEC section 2). The parser accepts only that
grammar, and it does not evaluate anything.

No file records the heads. Prodrome computes the TIPS of a store: the
objects that no other object names as a parent. So you can keep a store in
git and merge it with `git merge`. Two clones that each appended objects
only added files, so the merge cannot conflict. A store from before version
0.9 has a file `HEAD`, and a directory `refs/` if it had several heads.
`prodrome verify` asks you to delete them.

If the program crashes during a write, the store contains at most one
temporary file. `prodrome verify` reports it. If a file does not hash to its
name, or does not parse, every read leaves it out, together with every
object that depends on it. The read reports what it left out. While such a
file exists, every write fails. `prodrome fsck` moves each such file to
`quarantine/`, and then the store accepts writes again. `prodrome verify`
does the same checks and changes nothing.

**If you keep a store in git, its `.gitattributes` must contain
`objects/** -text -diff`.** The first write to a store creates this file if
it does not exist, so commit the file. Without it, git can convert line
endings on checkout, and then the files no longer hash to their names.

### 13. WebAssembly

```console
$ nix build .#prodrome-wasm
```

This command builds `result/web/` for a bundler and `result/nodejs/` for a
script. Each export parses its arguments, calls the core, and returns JSON.
The exports are `verify_objects`, `fold`, `registers`, `entries`,
`fulfillment`, `explain`, `compile`, `link`, `series_knots`,
`next_change`, `term_json`, `lifecycle`, `seal` and `merge_object`.

The file `wasm/exports/src/json.rs` defines the JSON form of a term. The
crate needs this separate format because JavaScript reads JSON, and the
core uses its own grammar. A record uses the field names of its payload in
JSON. The module also has `Todos`, which gives the same exports for any
schema. If your program has its own schemas, build one module for all of
them with the macro `prodrome_wasm_exports::schema!`. Each schema becomes
its own class. The header of `wasm/exports/src/lib.rs` explains how.

## Topics that this guide does not cover yet

- Worked FPL examples with plotted curves: `Within`, `After`, `Recur` and
  `Periodic`, each with its curve over one month.
- Sync between replicas, the overlay, and a `Decision` (design note section
  6.1).
- Signatures and keys (SPEC sections 3 and 5).
- A schema that is written as text (SPEC section 6).
- Nested histories and sets of prodromes (design note section 6.3).
- The digest, a summary tree over a log (design note section 6.5).

Until the guide covers these topics, read [`SPEC.md`](../SPEC.md) and the
design notes in this directory.
