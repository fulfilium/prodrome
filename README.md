# prodrome

A temporal, content-addressed event database with fulfillment-priority
semantics.

[![CI](https://github.com/fulfilium/prodrome/actions/workflows/ci.yml/badge.svg)](https://github.com/fulfilium/prodrome/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Prodrome stores a history of events about todos. A program adds events to
the history and never changes or deletes them. To read the store, you choose
a time. Prodrome then computes the state of each todo at that time from the
events dated at or before that time: whether the todo is open, and how well it is
fulfilled. Each todo has a spec. A spec is a formula in FPL, a small fuzzy
temporal logic, and it gives a number from 0 to 1 for each point in time. A
low number means that the todo is urgent. A list sorted by this number shows
the most urgent todo first.

- **Fulfillment changes with time.** For example, the value of a todo with a
  deadline falls as the deadline comes near. A group of todos is only as
  well fulfilled as its weakest member. A todo can wait on another todo.
- **You can read the past.** If you ask for the open todos at a past time
  `t`, Prodrome gives the same answer that it gave at `t`.
- **Replicas merge without a server.** Every store is a full copy, called a
  replica. You can write to a replica offline and then exchange objects with
  another replica. All replicas that hold the same objects give the same
  answers, and they do not need a shared clock. If two people change the
  same field and neither has seen the other's change, Prodrome keeps both
  values and shows them as a conflict. A person then settles the conflict.
- **Each object is named by its hash.** Each object is one file. The name of
  the file is the SHA-256 hash of the file's bytes. So you can check a store
  for damage from its files alone.
- **Your program decides whom to trust.** For each event, Prodrome asks your
  policy whether the event takes effect or is only a suggestion. Prodrome
  keeps both results.
- **It runs in the browser.** The same core compiles to WebAssembly.

## Installation

The crates are not on crates.io yet. Add each crate that the program needs
as a git dependency at a fixed commit. The library has thirteen crates, and
a program rarely needs all of them. Depend on the crates in this table, and
not on the facade `prodrome-core`, unless the program uses most of the
library or the law kit:

| The program | Depends on |
|---|---|
| builds, prints, parses, hashes and time-stamps events, or has a schema of its own | `prodrome-data` |
| reads the registers of an entity from its events under a policy | `prodrome-data`, `prodrome-memo` (the caches), `prodrome-dag` (the DAG of a store's objects, its fold, the policies, the proof of signatures) |
| opens a store, appends to it and reads it under a policy | the three crates above, and `prodrome-store` (the store on disk and in memory, replicas, doors, frontiers) |
| adds a second backend for a replica | the same four crates; the backend implements `store::Replica` |
| admits schemas declared as text | `prodrome-data` and `prodrome-declared` |
| keeps tag sets or histories held in registers | `prodrome-tags` or `prodrome-nest`, each with `prodrome-data`, `prodrome-memo` and `prodrome-dag` |
| keeps a summary tree over a log | `prodrome-digest`, with `prodrome-data` and `prodrome-memo` |
| builds, prints and parses FPL terms | `prodrome-data` and `prodrome-term` |
| also writes the todo schema's events | the same two, and `prodrome-todo` |
| evaluates a term | `prodrome-data`, `prodrome-term` and `prodrome-fpl` |
| reads a todo's price and its row in a list | `prodrome-data`, `prodrome-memo`, `prodrome-dag`, `prodrome-term`, `prodrome-fpl`, `prodrome-todo` and `prodrome-reading`; it reads no store |
| runs the law kit, or uses most of the library | `prodrome-core`, which re-exports every module of the other crates |

The root documentation of `prodrome-core` gives the size of each crate's
public API in tokens, so that a reader knows what a dependency costs to
read. This is a `Cargo.toml` of a program that opens a store:

```toml
[dependencies]
prodrome-dag = { git = "https://github.com/fulfilium/prodrome", rev = "<commit>" }
prodrome-data = { git = "https://github.com/fulfilium/prodrome", rev = "<commit>" }
prodrome-memo = { git = "https://github.com/fulfilium/prodrome", rev = "<commit>" }
prodrome-store = { git = "https://github.com/fulfilium/prodrome", rev = "<commit>" }
```

This is the dependency on the facade:

```toml
[dependencies]
prodrome = { package = "prodrome-core", git = "https://github.com/fulfilium/prodrome", rev = "<commit>" }
```

A program that depends on several of these crates gives each the same
revision. The facade requires the exact version of each of the other crates
that it is released with. The path of an item depends on the dependency: a
program that depends on `prodrome-data` names `prodrome_data::event::Hash`,
and a program that depends on the facade names `prodrome::event::Hash`.

The library needs Rust 1.85 or later. The command-line tool is called
`prodrome`, and it is in the crate `prodrome-cli`. Install it with cargo or
build it with nix:

```console
$ cargo install --git https://github.com/fulfilium/prodrome prodrome-cli
$ nix build github:fulfilium/prodrome#prodrome-cli
```

## Example

A store is a directory. This example creates a store, adds two todos, and
reads the store at three different times. Each command that writes prints
the names of the objects that it adds. The listing below does not show these
names.

```console
$ export PRODROME_ACTOR=ada
$ prodrome init work && cd work
$ prodrome add write-the-report --body "write the report" \
    --deadline 2026-09-15 --at 2026-09-08T09:00:00
$ prodrome add water-the-plants --body "water the plants" \
    --priority 80 --at 2026-09-08T09:00:00
$ prodrome list --at 2026-09-14
2 open at 2026-09-14T00:00:00 — standing behind every writer
0.335  write-the-report  write the report
0.800  water-the-plants  water the plants
$ prodrome done water-the-plants --at 2026-09-14T10:00:00
$ prodrome list --at 2026-09-14T12:00:00
1 open at 2026-09-14T12:00:00 — standing behind every writer
0.251  write-the-report  write the report
$ prodrome list --at 2026-09-13       # a read of the past gives the same answer
2 open at 2026-09-13T00:00:00 — standing behind every writer
0.501  write-the-report  write the report
0.800  water-the-plants  water the plants
$ prodrome verify
ok: 6 objects, 2 heads
```

The value of the report falls as its deadline comes near, so the report
moves to the top of the list. For the same example in Rust, see the
[quick start](core/src/lib.rs) in the crate documentation. `cargo test` runs
it.

A stored type has one literal form: a call of a constructor with named
fields. To give a Rust type that form, write
`#[derive(prodrome::literal::codec::Literal)]` on it. The derive writes the
print, the read, and the table of the constructor's name and field names
from the type's declaration. A test can pin that table, so that a renamed
field fails the test before it changes a stored object. The derive's
documentation lists the forms and the options.

## Where to go next

- [The guide](docs/guide.md) explains the model first. Then it explains the
  Rust API, the files on disk, and WebAssembly.
- [`SPEC.md`](SPEC.md) is the specification. Section 9 lists the laws that
  every implementation must pass.
- [`CHANGELOG.md`](CHANGELOG.md) lists the changes in each release, and
  which changes affect the stored format.
- [The documentation index](docs/README.md) lists the design notes, the
  development guide, and the description of the GitHub issue mirror.
- [The roadmap](docs/roadmap.md) describes the work that is left for this
  crate. The roadmap is itself a Prodrome store, on the
  [`roadmap-data`](https://github.com/fulfilium/prodrome/tree/roadmap-data)
  branch.
- [The viewer](docs/viewer.md) shows the roadmap in the browser, on
  [GitHub Pages](https://fulfilium.github.io/prodrome/).
- Run `cargo doc --open` to read the API documentation.

## Contributing

Issues and pull requests are welcome. See [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
