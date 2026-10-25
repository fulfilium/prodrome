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

The crates are not on crates.io yet. To use the library, add it as a git
dependency at a fixed commit:

```toml
[dependencies]
prodrome = { package = "prodrome-core", git = "https://github.com/fulfilium/prodrome", rev = "<commit>" }
```

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
