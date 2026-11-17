# Contributing to Prodrome

Thank you for your interest in Prodrome. Issues and pull requests are
welcome. This page explains the workflow for a change: how to set up the
project, how to test a change, how to open a pull request, and how to add
roadmap items. [`docs/development.md`](docs/development.md) gives more
detail about the tests, the CI, and the layout of the repository.

## Set up

The project uses a nix flake. The flake gives the Rust toolchain and all
other tools that the CI uses:

```console
$ git clone https://github.com/fulfilium/prodrome
$ cd prodrome
$ nix develop
```

If you do not use nix, install Rust 1.85 or later with rustup. You can then
build and test the Rust crates with cargo. You need nix to build the wasm
package, the viewer, and the Typst package.

## Test your change

Run these checks before you push. The CI runs the same checks, so a change
that passes them locally usually passes in the CI:

```console
$ nix develop -c cargo fmt --all --check
$ nix develop -c cargo clippy --locked --workspace --all-targets -- -D warnings
$ PRODROME_MAX_CASES=32 nix develop -c cargo test --locked --workspace
```

The variable `PRODROME_MAX_CASES=32` limits each property test to 32
generated cases, so the tests finish in about a minute. Before you ask for
a review, run the full gate. It runs every test at its full number of cases
and builds every package:

```console
$ nix flake check
```

## Open a pull request

Put one change in each pull request. A small pull request is easier to
review, and if it breaks something, it is easier to find and revert. If
your change needs a preparatory refactor, open the refactor as its own pull
request first.

Describe the change in two or three plain sentences, as you would explain
it to a colleague. Add one line that says how you tested it. The template
`.github/pull_request_template.md` gives this form. The commits give the
details.

## Name the roadmap items that a pull request completes

If your pull request completes an item on the roadmap, add a line to its
description that starts with `Completes:` and names the item's id:

```
Completes: digest-objects
```

A line can name several items, separated by commas. If the pull request
completes nothing, do not add a line. The ids are on the
[viewer](https://fulfilium.github.io/prodrome/), and `prodrome list` on the
branch `roadmap-data` prints them.

The workflow `.github/workflows/completes.yml` reads these lines:

- On every change to the pull request, a check refuses a line that names
  an item that the roadmap does not hold, or that is not an item id. The
  check says which id it refused. Correct the line, and the check runs
  again.
- When the pull request merges, the workflow records each item as completed,
  at the instant of the merge, with the note "completed by pull request
  #N". If you correct a line after the merge, the next merge into `main`
  records it.

If your work completes only part of an item, do not name it. Change the
item's text in a pull request against `roadmap-data` instead, as the section
"Roadmap items arrive by pull request" explains.

## A change to semantics needs the SPEC and a test

[`SPEC.md`](SPEC.md) is the specification, and this crate is its reference
implementation. Other implementations depend on the SPEC, so the SPEC must
always describe what the code does. If your change alters what a program
can observe, such as a value, a stored byte, or a result of `verify`, do
these things in the same pull request:

1. Change `SPEC.md` to describe the new behaviour.
2. Add a law in SPEC section 9 and a property test that checks it, or add a
   vector in `conformance/`. The test shows that the code follows the new
   text.
3. Add an entry to [`CHANGELOG.md`](CHANGELOG.md). If the change affects the
   stored format, say so.

Do not change the fields of a stored constructor that a release contains.
Stores that exist today contain those bytes, and the name of each object is
the hash of its bytes. To store new data, add a new constructor.

## Roadmap items arrive by pull request

The roadmap is a Prodrome store on the branch `roadmap-data`.
[`docs/roadmap.md`](docs/roadmap.md) explains it. To add or change an item,
write the objects with the `prodrome` CLI, and open a pull request against
`roadmap-data`. Do not edit or delete an existing object. Each object is
named by the hash of its bytes, so the diff of the pull request contains
exactly the objects that it adds, and a reviewer can read each one. If you
mark an item as done, name the evidence in the note: a merged pull request,
or the code that does the work.

## License

Unless you explicitly state otherwise, any contribution that you
intentionally submit for inclusion in the work, as defined in the
Apache-2.0 license, is dual licensed under MIT or Apache-2.0, without any
additional terms or conditions. The License section of the README gives the
full text.
