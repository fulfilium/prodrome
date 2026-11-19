# Development

This page explains how to build and test Prodrome, what the CI runs, what
the conformance vectors are, and where each part of the repository is. If
you change the semantics, change [`SPEC.md`](../SPEC.md) in the same pull
request, and add a law or a vector that tests the change.

## Testing

```console
$ nix flake check              # fmt, tests and clippy, in the nix sandbox
$ nix develop -c cargo test    # the same tests, with the flake's toolchain
$ PRODROME_MAX_CASES=32 nix develop -c cargo test   # each law runs at most 32 cases
```

A law is a property that the code must satisfy for all inputs. A property
test checks a law on many generated inputs, called cases. Each property law
states its full number of cases. The variable `PRODROME_MAX_CASES` limits
each law to that number of cases. Every law still runs, with fewer cases.
One function, in `core/tests/common/cases.rs`, applies this limit, and the
configuration of every law calls it.

## CI

The workflow `.github/workflows/ci.yml` runs two jobs. Both use the flake's
toolchain.

- The `cargo` job is the fast loop. It runs two checks in parallel, through
  `nix develop`:
  - `lint` runs fmt, clippy with `-D warnings`, and the doctests.
  - `test` runs every other test in the workspace. Then it runs
    `prodrome verify` and `prodrome list` on the `roadmap-data` branch, with
    the binary that the tests built.

  Each check restores its `target/` directory from its last run on main.
  The script `.github/scripts/target-sources.sh` compares the git blob name
  of each source file with the blob name that `target/` was built from. If
  the names are equal, the file has not changed. So a pull request
  recompiles only the crates that it changes. On a pull request, each law
  runs at most 32 cases. On main, and in the nightly run, each law runs its
  full number of cases. The nightly run also uses new random seeds.
- The `nix` job is the gate. It runs `nix flake check`. This runs all of the
  checks above with the full number of cases, in the nix sandbox. It also
  builds the wasm, the viewer, the Typst package, and the release binaries.
  The job runs on pull requests and on main, and it uploads its results to
  the public `bmabsout` Cachix cache.

The limit of 32 cases comes from a measurement. Each law ran with limits
from 2 to 128, and the results were compared with two runs at the full
number of cases. With a limit of 32, the laws together cover 98.7% of the
core's lines that the full runs cover. The fuzz laws are a separate case
(see below). Among the other laws, a limit of 16 leaves eight laws with
less coverage than one full run, and a limit of 32 leaves three. The lowest
of these three reaches 90% of its full coverage. The four laws in
`core/tests/all/fuzz.rs` search 4096 random inputs for rare cases, so a
limit does not suit them. Only main and the nightly run execute them.

## Conformance vectors

The directory `conformance/` holds vectors. A vector is a fixed input with
its expected output, and every implementation must reproduce it. The laws in
SPEC section 9 are property tests over generated logs and over pairs of
replicas.

The vectors are written in the crate's own literal grammar (SPEC section
2). The crate's own parser reads them, with a second closed vocabulary. If
the vectors used a second format, such as JSON, the project would have to
keep two formats consistent. Each stored object, event and term in a vector
is a string that contains its canonical text, because the vector tests
exactly those bytes.

The project took most vectors from an earlier reference implementation.
These vectors are different:

- **`conformance/view/*.py`** (SPEC section 6.7) is generated from a seed,
  because no reference implementation exists for it now. This crate's own
  log generator (`a_log` in `core/tests/common/mod.rs`) creates it with a
  fixed seed. The files are fixed: `cargo test` only reads them. To
  regenerate them, run
  `cargo run --example generate_view_vectors -p prodrome-core` by hand.
  `nix flake check` and the CI never run this command.
- **`conformance/absent/*.py`** (SPEC section 9.18) is generated in the same
  way, for `Absent`. It contains terms, exact series, and logs from the
  generators that the laws of `∅` use (`core/tests/common/terms.rs`, and
  `a_log_with_absence`). The files are fixed. To regenerate them, run
  `cargo run --example generate_absent_vectors -p prodrome-core` by hand.
- **`conformance/link.py`** (SPEC section 7.2) is written by hand from the
  laws of `link`. It contains a few specs, the text that each one links to,
  their values at a few times, and one loop that `link` rejects. A reader
  can check every number in it by hand.
- **`conformance/recur.py`** (SPEC section 7.3) is written by hand in the
  same way. It contains a chore that was tended twice and claimed once, a
  rent payment that repeats, and their values.

The directory `wasm/conformance/` holds the vectors for the JSON interface:
the output of `to_json` and `explain` for the same 150 terms. These files
were fixed when the JSON code moved to that crate.

## Repository layout

```
core/         prodrome-core, the database. The module list in core/src/lib.rs
              gives the SPEC section of each module. The module `reference`
              is the payload of the vectors.
derive/       prodrome-derive, the procedural macro `#[derive(Literal)]`,
              which the core re-exports as prodrome::literal::codec::Literal
cli/          prodrome-cli, the `prodrome` binary. It gives the commands, with
              the reference payload and the reference policy. It is the only
              code in the workspace that reads the system clock.
wasm/         prodrome-wasm, the core compiled for the browser with the
              reference payload. `wasm/exports/` holds the exports, which work
              for any schema, and `json`, the JSON format of terms.
conformance/  the vectors, in the grammar of SPEC section 2
docs/         the guide, the design notes, and these pages
.github/      the CI, the workflows that mirror and price issues, and the
              workflow that publishes the viewer
SPEC.md       the specification

              OPTIONAL PARTS. Nothing above depends on them:
github/       prodrome-github, the binary that mirrors GitHub issues into a store
typst-wasm/   Typst compiled for the browser, in its own workspace
typst/        prodrome-typst, the Typst package of page layouts
viewer/       the static web app that GitHub Pages publishes
```

**The core does not depend on a markup language.** `core/` and `cli/` store
the text of a record as an opaque string. They never parse it as Typst,
Markdown, or any other format. The issue mirror writes issue titles as
plain text. The viewer displays a store whose items contain Typst. The
viewer is an optional part on top of the core, and an example of how to use
the core.
