# Design: what a program must read to use the core

This note answers issue #92. It measures the public face of the crate
`prodrome-core`, and it proposes how to make what one task must read
smaller, in stages. Section 7 records stage 1, which the same pull request
as this note made. Section 8 records stage 3, the data crate, which a
later pull request made before stage 2. Section 9 records the stage after
it, which moved the instant conversions to the data crate and shortened
the data crate's doc comments. Section 10 records the stage after that,
which made the crates `prodrome-memo`, `prodrome-dag` and
`prodrome-declared`. Section 11 records the stage after that, which made
the crates `prodrome-store`, `prodrome-tags`, `prodrome-nest` and
`prodrome-digest`. Section 12 records the stage after that, which made
the crates `prodrome-term`, `prodrome-fpl` and `prodrome-todo`. Section 13
records the stage after that, which made the crate `prodrome-reading` and
left `prodrome-core` as a facade, and says what remains of the issue.
Section 14 reviews every public item of the crates and completes the
issue. For what is implemented, see `SPEC.md` and `CHANGELOG.md`.

Terms used below:

- The **public face** of a crate is what a reader of its public API reads:
  the signature of each public item and its doc comment.
- A **unit** is a part of an application that an agent changes as one
  piece, for example one module. An agent that changes a unit reads the
  unit and the public face of each crate that the unit depends on.
- A **token** is four characters of the face. The budget that an
  application gives to the face of one dependency is 25,000 tokens.

## 1. The face today, by module

### Method

This note uses two measures. Both count four characters as a token.

**The rustdoc measure.** `scripts/face.py` reads the JSON that rustdoc
writes for the crate, walks every item that is reachable from the crate
root, and counts two numbers for each item: the characters of its
signature, and the characters of its doc comment. The script's own doc
comment says what it counts as a signature. It does not count trait impls,
such as `impl Display for Hash`, because their methods have no doc comment
of their own. This measure is what the rendered documentation shows.

```console
$ RUSTC_BOOTSTRAP=1 nix develop -c cargo rustdoc -p prodrome-core --lib \
    --target-dir target/face -- -Z unstable-options --output-format json
$ python3 scripts/face.py target/face/doc/prodrome.json
```

`RUSTC_BOOTSTRAP=1` lets the stable toolchain write rustdoc's JSON, which
is an unstable output format. Add `--all-features` to the first command to
count the module `laws`, which is behind a feature, and add `--by-file` to
the second command to count by source file.

**The line measure.** An application gives each dependency a reading
budget with a tool that reads source lines, not rustdoc. The tool counts
every module doc line, and every `pub` item whatever the visibility of its
module, with its doc lines and its attributes as written (the prefix and
the indentation count). It cuts a function at its first `{`, and it counts
the body of a type or a trait without the bodies of its methods. That tool
reported 140.2 thousand tokens at the merge base, and 121.7 thousand after
the first pass of stage 1. `scripts/face.py --lines` follows that rule:

```console
$ python3 scripts/face.py --lines
```

To come close to the tool's numbers, the script also counts `pub(crate)`
and `pub(super)` items and the methods of `impl` blocks. It gives 131.1
thousand tokens at the merge base and 119.4 thousand after the first pass,
which is within 7 and 2 percent of the tool's numbers. The rest of the
difference is in details of the tool that this note could not see. The
line measure is about twice the rustdoc measure, because it counts private
and crate-visible items, the prefix and the indentation of each doc line,
and whole bodies of types.

The budget and the targets of this note are stated in the line measure,
because the application spends its budget by that measure.

### The numbers

The columns "before" are measured at the merge base `85e5083`. The columns
"after" are measured after both passes of stage 1 (section 7). The table
counts every module with the feature `laws` on. The module `lib` is the
crate root, whose doc comment holds the quick-start example.

| Module | Items | Signature chars | Doc chars before | Doc chars after | Tokens before | Tokens after | Line tokens before | Line tokens after |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| `store` | 162 | 6,341 | 37,734 | 20,915 | 11,018 | 6,814 | 27,128 | 22,066 |
| `laws` | 125 | 3,182 | 13,750 | 12,106 | 4,233 | 3,822 | 11,651 | 11,220 |
| `literal` | 156 | 5,574 | 17,275 | 13,588 | 5,712 | 4,790 | 9,436 | 8,449 |
| `memo` | 103 | 2,598 | 18,655 | 14,551 | 5,313 | 4,287 | 8,732 | 7,678 |
| `dag` | 101 | 2,408 | 8,219 | 6,965 | 2,656 | 2,343 | 8,442 | 8,124 |
| `fold` | 109 | 5,061 | 13,845 | 12,251 | 4,726 | 4,328 | 7,125 | 6,737 |
| `digest` | 39 | 1,207 | 9,016 | 3,998 | 2,555 | 1,301 | 5,046 | 3,707 |
| `fpl` | 105 | 4,297 | 10,099 | 7,748 | 3,599 | 3,011 | 4,959 | 4,348 |
| `declared` | 97 | 1,629 | 7,586 | 6,354 | 2,303 | 1,995 | 4,766 | 4,446 |
| `nest` | 61 | 2,279 | 9,053 | 6,565 | 2,833 | 2,211 | 4,764 | 4,096 |
| `tags` | 50 | 2,046 | 6,005 | 5,454 | 2,012 | 1,875 | 4,163 | 4,016 |
| `schema` | 47 | 2,182 | 12,510 | 8,770 | 3,673 | 2,738 | 3,889 | 2,886 |
| `event` | 106 | 3,824 | 7,219 | 5,282 | 2,760 | 2,276 | 3,785 | 3,297 |
| `sign` | 53 | 1,703 | 7,687 | 4,500 | 2,347 | 1,550 | 3,744 | 2,908 |
| `policy` | 21 | 720 | 7,421 | 2,051 | 2,035 | 692 | 2,731 | 1,277 |
| `registers` | 36 | 1,526 | 5,407 | 4,713 | 1,733 | 1,559 | 2,658 | 2,489 |
| `term` | 117 | 2,550 | 2,309 | 2,298 | 1,214 | 1,212 | 2,653 | 2,658 |
| `observe` | 16 | 682 | 5,257 | 3,160 | 1,484 | 960 | 2,007 | 1,451 |
| `lib` (the crate root) | 1 | 0 | 6,443 | 5,294 | 1,610 | 1,323 | 1,944 | 1,604 |
| `schedule` | 22 | 1,019 | 2,917 | 2,473 | 984 | 873 | 1,854 | 1,739 |
| `view` | 36 | 1,058 | 5,064 | 3,678 | 1,530 | 1,184 | 1,804 | 1,441 |
| `chain` | 31 | 869 | 5,288 | 3,521 | 1,539 | 1,097 | 1,787 | 1,357 |
| `reference` | 53 | 1,987 | 2,793 | 2,032 | 1,195 | 1,004 | 1,555 | 1,354 |
| `payload` | 18 | 1,186 | 4,152 | 2,444 | 1,334 | 907 | 1,433 | 983 |
| `todo` | 9 | 491 | 995 | 892 | 371 | 345 | 1,223 | 1,197 |
| `breaks` | 14 | 347 | 1,135 | 1,217 | 370 | 391 | 520 | 545 |
| `topo` | 5 | 292 | 862 | 812 | 288 | 276 | 506 | 497 |
| `snapshot` | 14 | 421 | 702 | 737 | 280 | 289 | 340 | 350 |
| `spread` (private) | 0 | 0 | 0 | 0 | 0 | 0 | 178 | 178 |
| `change` | 8 | 278 | 180 | 195 | 114 | 118 | 146 | 152 |
| `genesis` | 11 | 336 | 152 | 178 | 122 | 128 | 120 | 120 |
| total, with `laws` | 1,726 | 58,093 | 229,730 | 164,742 | 71,955 | 55,708 | 131,100 | 113,381 |
| total, default features | 1,601 | 54,911 | 215,980 | 152,636 | 67,722 | 51,886 | | |

The line measure reads every source file, whatever the features, so it
has one total.

Doc comments are 80 percent of the face before stage 1: 229,730 of
287,823 characters. Code blocks in doc comments, which are doc tests, are
10,653 of those characters. Of the 950 public items with a doc comment,
517 have a doc of at most 120 characters. The ten largest items before
stage 1, in characters of signature and doc:

| Item | Kind | Chars |
|---|---|--:|
| `prodrome` (the crate root) | module | 6,443 |
| `schema::headed` | module | 4,122 |
| `digest::view` | function | 3,523 |
| `memo::file` | module | 3,271 |
| `store::Decision` | struct | 2,627 |
| `observe::next_change` | function | 2,534 |
| `declared` | module | 2,432 |
| `store` | module | 1,977 |
| `payload::Payload` | trait | 1,820 |
| `tags` | module | 1,770 |

Six of the ten largest items are module heads. A module head is read by
every reader of the module, so its length costs the most.

## 2. What of the face repeats the code or the specification

### The rule

A doc comment keeps only what the code cannot say, each as one sentence:

- an invariant that the types do not carry, which is what a caller must
  keep true;
- a hazard, which is what goes wrong when a caller misuses the item;
- a unit or a format;
- an outside fact, such as a system call, a file system, or a rule of
  CPython that the printer copies;
- a reason that the reader would otherwise lose;
- the number of a law and its statement in one line;
- what the order of a check protects, what the code does under the lock
  and why, and what a crash at each step leaves.

A doc comment deletes narration, a restatement of the signature, history,
emphasis in capitals, and paragraphs that repeat `SPEC.md`. It cites the
section of `SPEC.md` instead. Each kept doc comment starts with one
sentence that states the item's contract.

### The sample

Three modules were cut first by the rule, as a sample: `policy`, `event`
and `schema`.

| Module | Doc chars before | Doc chars after | Cut |
|---|--:|--:|--:|
| `policy` | 7,421 | 3,187 | 57 % |
| `event` | 7,219 | 5,276 | 27 % |
| `schema` | 12,510 | 11,139 | 11 % |
| the three | 27,150 | 19,602 | 28 % |

The cuts differ because the modules differ. The docs of `policy` repeated
SPEC §5 at length and told the history of the module. The docs of `event`
gave each valid-text rule and each format, which the rule keeps. Most of
the doc of `schema` is the module head of `schema::headed`, which is a doc
test, and the contracts of trait methods that a host implements, which the
rule keeps.

These are the cuts of the first pass. Applied to the whole default face, a
cut of 28 percent of the doc characters estimated a face of 52,700 tokens
in the rustdoc measure. The first pass over the whole crate cut 20
percent, to 57,181 tokens. A review then found that the largest modules
still repeated the specification, and a second pass over `store`,
`digest`, `schema`, `fpl`, `sign` and `policy` brought the face to 51,886
tokens. Section 7 gives the measured result of both passes.

## 3. What is public that no outside program needs

`pub` in this crate marks two kinds of items: items that an application
uses, and items that only the crate's own modules, its tests, `cli/` or
`wasm/` use. The tree cannot tell the second kind apart for certain,
because an application outside the tree may use any public item. The
count below finds, for each public item at the top of a module, which code
outside its own module names it. The count is a search for the name as a
word, so it finds a few false users, and the number of items that only the
library uses is a lower bound.

The count was made at the merge base, over the 400 public items at the top
of a module (types, traits, functions, constants and macros). The size of
each item includes its fields, its variants and the methods of its
inherent impls.

| Who names the item outside its module | Items | Tokens |
|---|--:|--:|
| other modules of the core only | 46 | 2,895 |
| the core's tests only | 48 | 6,527 |
| `cli/`, `wasm/` or `github/` only | 4 | 168 |
| the tests and `cli/` or `wasm/` only | 21 | 2,053 |
| an application could name it (all others) | 281 | 41,862 |

The first row is the set of candidates for `pub(crate)`, for example
`literal::print_float`, `registers::BitSet`, `topo::Topo` and
`dag::Reach`. The other rows are not all candidates: the tests exercise the
public API on purpose, so `store::Doors` and `tags::TagSet`, which a host
needs, are in the second row. A reviewer must decide item by item. The
largest items of the second and fourth rows are `tags::TagSet`,
`store::Frontiers`, `chain::Compiled`, `nest::Nest`, `observe::Observation`
and `declared::Declaration`.

All candidates together are about 17 percent of the face, and the
certain candidates of the first row are about 4 percent. So narrowing
alone cannot bring the face under the budget. Three tools can hold the
candidates:

- **`pub(crate)`.** It fits an item that only the crate's own modules use.
  It removes the item from the face. It breaks a program outside the tree
  that names the item, at compile time.
- **A `#[doc(hidden)]` module, for example `prodrome::internal`.** It fits
  an item that `cli/`, `wasm/` or the tests use. The item stays public, so
  nothing breaks, but rustdoc and the face no longer show it. The module's
  name tells a reader that it has no stability promise.
- **A feature.** It fits a group of items that only one kind of program
  wants, as the feature `laws` holds the law kit today. A program that
  wants the items turns the feature on. A program that named an item
  without the feature breaks at compile time.

This crate is public, and its version is below 1.0, so Semantic Versioning
lets a minor release break the API. A narrowing therefore happens in one
release: the `CHANGELOG.md` lists each narrowed item under "Changed" or
"Removed", with its new path where it has one. No item is kept
`#[deprecated]` for a release, and no re-export is kept at an old path,
except the re-exports by which `prodrome-core` keeps the paths of the
items that move to another crate of the library. Narrowing changes no
stored byte, no conformance vector and no law.

## 4. How the core divides into crates

### The order that the modules have today

A script counted the dependencies between the modules from the paths
`crate::<module>` in the code at the merge base, without comments and
tests, and found the strongly connected components of that graph. Twenty
of the thirty modules form one component, so each of them depends, through
others, on each other one:

> `chain`, `change`, `dag`, `event`, `fold`, `fpl`, `genesis`, `literal`,
> `memo`, `nest`, `payload`, `policy`, `registers`, `schedule`, `schema`,
> `sign`, `snapshot`, `store`, `term`, `todo`.

So the modules have no layer order yet, and a split must first cut the
edges below. The other ten modules (`breaks`, `declared`, `digest`,
`observe`, `reference`, `spread`, `tags`, `topo`, `view`, and `laws`) each
sit above or below the component. Each edge in the table is a use of an
item that belongs in a lower layer than its module:

| From | To | Items | The cut |
|---|---|---|---|
| `event` | `sign` | `Signed`, `KeyAdded`, `KeyRevoked` | Move these object types to the data crate; the proof stays with the store. |
| `event` | `term`, `payload` | the todo schema's events, `TodoEvent` and its `mk_*` | Move the todo schema's events to the todo crate. `Envelope<E>` and the names stay. |
| `schema` | `fold` | `Product` | Move the register algebra of `fold/` (`Product`, `Register`, `Order`, `Climb`, `Frontier`, `read`) to the data crate. |
| `schema` | `store` | `Husking`, `HuskObject`, `Other` | Move `store/husk.rs` to the data crate. |
| `schema` | `fpl`, `policy` | `Env`, `FplError`, `Policy` | Move the traits `Price`, `History`, `Bind` and `Row` to the reading crate. |
| `snapshot` | `dag` | `Dag` | Move `attest` and `Attestation` to the store crate. |
| `payload` | `term` | `Payload::spec` | Move the trait `Payload` to the todo crate; the field helpers (`string_field`, `required`) stay in the data crate. |
| `registers`, `dag`, `nest`, `tags`, `declared` | `fold` | `read`, `Product`, `Order` | Solved by the move of the register algebra. |
| `term` | `fpl`, `schedule` | `Delta`, `Instant`, the `mk_*` constructors | `term` and the constructors move together, into the FPL crate. |

### The proposed crates

| Crate | Holds | Depends on |
|---|---|---|
| `prodrome-data` | `literal`, the names and the envelope of `event`, `genesis`, `change`, `snapshot`, `schema`, the register algebra of `fold/`, the object types of `sign`, `store/husk.rs`, `topo`, the field helpers of `payload` | none of these |
| `prodrome-memo` | `memo` | data |
| `prodrome-digest` | `digest` | data, memo |
| `prodrome-dag` | `dag`, `registers`, `policy`, the proof of `sign`, `fold::read` | data, memo |
| `prodrome-declared` | `declared` | data |
| `prodrome-store` | `store`, `nest`, `tags`, `attest` | data, memo, dag |
| `prodrome-fpl` | `term`, `fpl`, `schedule`, `chain`, `breaks`, `observe` | data |
| `prodrome-reading` | the readings of `fold.rs`, `view`, the traits `Price`, `History`, `Bind`, `Row` | data, store, fpl |
| `prodrome-todo` | `todo`, `reference`, `Payload`, `TodoEvent` | data, fpl, reading |
| `prodrome-core` | re-exports every crate above at today's paths, and holds `laws` behind its feature | all |

This table is the proposal as the stages made it. The first proposal put
`dag`, `registers`, `policy`, the proof of `sign` and `declared` in the
store crate. Section 10 made `prodrome-dag` and `prodrome-declared`
instead, and the data crate holds `Standing`, `Everything` and
`Untrusted`. The faces below are the estimates of the first proposal.

Signing stays in the store crate, because `dag::verify` reports forged
signatures with the proof, and the proof reads a `Dag`. A separate signing
crate would need a trait in the data crate that `verify` calls, which is a
new abstraction with one user.

### The face of each crate

The face of each proposed crate is the sum of its items' faces. A small
script in the same manner as `scripts/face.py` assigned each reachable item
to a crate by its file and its name, as the table above says. For the line
measure, the script scaled each item by the ratio of its module's line
measure to its module's rustdoc measure, so the line columns are
estimates.

The counts are with the feature `laws`, which a split would keep as a
feature of the facade.

| Crate | Rustdoc, before | Rustdoc, after stage 1 | Lines, before | Lines, after stage 1 |
|---|--:|--:|--:|--:|
| `prodrome-data` | 17,121 | 14,132 | 25,867 | 23,295 |
| `prodrome-memo` | 5,313 | 4,287 | 8,732 | 7,678 |
| `prodrome-digest` | 2,555 | 1,301 | 5,046 | 3,707 |
| `prodrome-store` | 25,453 | 17,816 | 54,925 | 45,588 |
| `prodrome-fpl` | 9,192 | 7,545 | 13,782 | 12,101 |
| `prodrome-reading` | 3,393 | 2,903 | 4,299 | 3,834 |
| `prodrome-todo` | 3,081 | 2,577 | 4,673 | 4,174 |
| `prodrome-core` (the facade's root doc and `laws`) | 5,843 | 5,145 | 13,594 | 12,824 |

### Which crates a unit reads

The counts below are the uses of each module in one large application, as
data. Most of its units use `event` and `literal` and little else.

| Uses | Modules | A unit that uses them reads |
|---|---|---|
| 1,166 | `event` 661, `literal` 339, `schema` 75, `change` 26, `genesis` 21, `text_literal` 19, `word_literal` 11, `payload` 10, `snapshot` 3, `topo` 1 | data |
| 511 | `store` 142, `policy` 128, `sign` 104, `dag` 64, `registers` 31, `declared` 23, `tags` 19 | data, memo, store |
| 218 | `fpl` 181, `term` 30, `observe` 6, `breaks` 1 | data, fpl |
| 49 | `digest` 26, `memo` 23 | data, memo, digest |
| 55 | `fold` 40 (part data, part reading), `view` 13, `todo` 2 | data, memo, store, fpl, reading, todo |
| 7 | `laws` 7 | the facade |

`text_literal` and `word_literal` are macros of `literal`. So 58 percent of
the application's uses of the core need only the data crate.

### The targets

The target has two parts, both in the line measure:

- no crate's face is over 25,000 tokens;
- a unit that only builds and reads events reads under 15,000 tokens.

After stage 1 and the split of the table above, neither part holds. The
data crate is 23,295 tokens, which is 8,300 over its target, and the store
crate is 45,588 tokens, which is 20,600 over. In the rustdoc measure both
parts would hold (14,132 and 17,816), so the gap is mostly in what the line
measure counts and rustdoc does not show.

Most of that gap is items that are not public to an application. In the
script's line measure, 9,500 of the 22,066 tokens of the module `store`
are `pub(crate)` and `pub(super)` items and the private methods of `impl`
blocks, and so are 2,700 of the 8,449 tokens of `literal` and 5,000 of the
8,124 tokens of `dag`. The line measure counts an item whatever its
visibility, so `pub(crate)` does not remove an item from it, and
`#[doc(hidden)]` does not either. An item leaves the line measure when it
loses `pub` altogether, or when it moves to a crate that the unit does not
depend on.

So the stages of section 5 have these targets, in the line measure:

| After stage | The whole core | `prodrome-data` | `prodrome-store` |
|---|--:|--:|--:|
| 1, the doc comments (done) | 113,381 | 23,295 (estimate) | 45,588 (estimate) |
| 2, the narrowing | 100,000 | 19,000 | 36,000 |
| 3, the data crate | | 15,000 | |
| 3, as made (section 8) | 92,381 without the data crate | 22,260 | |
| 3, the instants and the words (section 9) | 92,204 without the data crate | 19,231 | |
| 4, the readings, as made (section 10) | 66,077 without the data, memo, declared and dag crates | 19,654 | 28,767 (`store`, `nest`, `tags`, still in the core) |
| 4, the store, as made (section 11) | 33,374 without the crates below it | 19,756 | 21,214 (`store` and `snapshot`) |
| 4, the terms, the evaluator and the todo schema, as made (section 12) | 17,038 without the crates below it | 19,913 | 21,214 |
| 4, the readings, in their crate (section 13) | 13,392 without the crates below it, of which `laws` is 11,220 | 19,913 | 21,214 |
| 4, each further crate | | under 25,000 by the application's tool | under 25,000 |

The targets of 15,000 and 19,000 for the data crate were set before
stage 3. Since section 9, the target of every crate, the data crate with
it, is the budget of 25,000 tokens by the application's own tool, which
counts 14 to 17 percent more than `scripts/face.py --lines`. So a crate
is under the budget when it is under about 21,400 tokens by this note's
line measure.

Stage 2 reaches its numbers by removing `pub` from items that only their
own module uses, and by a third pass of words over the data modules
(`literal`, `schema`, `event`). Stage 3 reaches 15,000 when the data crate
holds only the data modules, and the items of `literal` that only its
parser and its printer use have lost `pub`. Stage 4 divides the store
crate: section 10 made `prodrome-dag` (`dag`, `registers`, `policy` and
the proof of `sign`) and `prodrome-declared`, and `prodrome-store`
(`store`, `nest` and `tags`) remains.

## 5. The stages

Each stage is one pull request that lands alone.

1. **The doc comments, by the rule of section 2.** Words only. A program
   changes nothing. The conformance vectors and the law tests do not
   change, and the words-only check of section 7 proves that no code
   changed. Done in the pull request of this note: the line measure fell
   from 131,100 to 113,381 tokens.
2. **The narrowing of section 3,** with the target of "The targets"
   above. A program that names a narrowed item changes its import to the
   item's new path, or turns on a feature. No test changes its
   assertions; a test that names a narrowed item changes its import.
   Each crate's stage narrows the items that its move touches, so this
   stage narrows only what remains in the facade at the end.
3. **The data crate.** Made before stage 2, because it is the stage that
   lowers what most units read; section 8 records it. The pull request
   first cut the edges of section 4 that touch the data crate, inside
   `prodrome-core`, as moves of items between modules. Then it moved the
   files to `prodrome-data`, and `prodrome-core` re-exports them at the
   same paths. A program that depends on `prodrome-core` changes nothing.
   A program that wants only the data crate changes its dependency and
   its import paths, from `prodrome::event` to `prodrome_data::event`.
   The conformance suites stay in `prodrome-core`'s tests, which read every
   crate through the re-exports, so every vector and every law test runs
   unchanged.
4. **Each further crate,** one pull request each, by the same method as
   stage 3, in this order:
   1. the store crate, next, because its face is the largest (45,588
      tokens estimated in section 4). It depends on `memo`, so that stage
      moves `memo` to its own crate first, in its first commit, or a pull
      request before it does. Stage 4 of "The targets" then divides the
      store crate in two;
   2. fpl;
   3. digest;
   4. reading;
   5. todo.

   Sections 9 and 10 restate this order by what the units of an
   application are held by.

`SPEC.md` names no Rust module, so no stage changes it. `CHANGELOG.md`
announces each stage under "Changed", with the paths a program may change.

## 6. What argues against the split

- **Compile time.** Eight crates compile in parallel, but each crate
  boundary stops inlining without `#[inline]` and costs a separate
  metadata pass. The generic code of the core (`Schema`, the folds) is
  monomorphised in the application anyway, so the loss is small, but it is
  not zero. Section 8 measures it for the data crate: a clean build of the
  workspace takes about 0.6 seconds more.
- **Version skew.** Eight crates mean eight versions. An application that
  depends on `prodrome-data` 0.10 and on `prodrome-store` 0.11 gets two
  copies of `Hash`, and the compiler reports that a `Hash` is not a
  `Hash`. The crates must release in lock step, with exact version
  requirements between them, which `cargo` can express but which every
  release must check.
- **The cost to a small program.** A program that wants the whole core
  today adds one dependency. After the split it still adds one, the
  facade, but its error messages and its documentation name eight crates,
  and `cargo tree` shows eight.
- **Cuts that cost design.** Section 4 found that twenty modules form one
  cycle. Each cut in its table is a decision about where a type belongs,
  and two of them move items out of the files that hold their concept
  today (`event.rs` holds both the envelope and the todo events). A cut
  made only to split a crate can make the code harder to read.
- **Words and narrowing might be enough.** If the face of the whole core
  fitted the budget, one crate would be simpler than eight for every
  reader. After stage 1 the whole face is 113,381 tokens in the line
  measure and 51,886 in the rustdoc measure, which are more than four and
  two times the budget. So this argument does not hold today.

Stage 1 would have been enough if it had brought the whole face under
25,000 tokens. It cut 14 percent in the line measure and 23 percent in the
rustdoc measure, and the rule keeps most of what remains, so no further
pass of words alone reaches the budget. The work should stop
after stage 2 if a later count finds that most of an application's units
use the store or the evaluator as well as the data, because then most
units would read most crates and the split would save them little.

## 7. Stage 1, as made

Stage 1 changed the doc comments of the public items of `core/src` by the
rule of section 2, and nothing else.

**Two passes.** The first pass covered every public doc comment of
`core/src`. A review found that it cut the docs of items by only about a
tenth, so a second pass went over `store`, `digest`, `schema`, `fpl`,
`sign` and `policy` again with the same rule.

**The face.**

| Measure | Before | After the first pass | After the second pass |
|---|--:|--:|--:|
| rustdoc, default features | 67,722 | 57,181 | 51,886 |
| rustdoc, with `laws` | 71,955 | 61,003 | 55,708 |
| doc characters, default features | 215,980 | 173,815 | 152,636 |
| lines (`scripts/face.py --lines`) | 131,100 | 119,376 | 113,381 |
| lines, the application's own tool | 140,200 | 121,700 | not measured |

The doc tests did not change. The table of section 1 gives each module.

**What the passes kept.** The first pass kept, as one sentence each, about
61 hazards, about 106 invariants or obligations of a caller, about 72
outside facts (formats, units, system calls, file systems, rules of CPython
and of other standards), and about 77 citations of a law by its number.
These counts are the sums of the counts that each part of the pass
reported. The second pass deleted a kept sentence only where `SPEC.md`, a
design note or the remaining doc still states it, and each part of the
pass listed those sentences and where they are still stated.

**The review of the deletions.** After each pass, a second reader read
every deleted line against the list of section 2. The readers and the
review of the pull request restored these sentences, which were the only
place where their reason was written:

- why `Product::climbed` passes for a subset read under the policy where
  everything binds (`fold/product.rs`);
- why a write below what it rests on cannot lower the reading of a
  `Climb` (`fold/climb.rs`);
- why the heap of `topo::linear` orders keys correctly (`topo.rs`);
- why a named actor's lifecycle events claim under `Untrusted`: such an
  actor reads input that an attacker can control (`policy.rs`);
- that a husk binds under `Proven` where the proof vouches for it
  (`policy.rs`);
- why the policy of a store defaults to `Untrusted` (`store.rs`);
- why a husk reads as its whole for good: keys are only destroyed
  (`store/husk.rs`);
- why `tags::alias::unaliased` never returns an error (`tags/alias.rs`);
- that the `Least` of a price is neither a register's join nor FPL's
  `Conj` (`schema.rs`).

The readers also corrected five new sentences that were false
(`registers::deps_for`, `store::MemoryStore`, `HuskObject::of`, the law
number in the head of `policy`, and the width of a content record in
`event`), and the second pass removed advice to delete `.heads`, which
SPEC §3, "The record of heads", calls no remedy.

**The check that only words changed.** With every blank line and every
line whose first characters are `//` removed, each file of `core/src`
equals the same file at the merge base:

```console
$ base=$(git merge-base HEAD origin/main)
$ strip() { grep -vE '^[[:space:]]*(//.*)?$'; }
$ for f in $(git diff --name-only "$base" -- core/src); do
>   diff <(git show "$base:$f" | strip) <(strip < "$f") > /dev/null \
>     || echo "code changed: $f"
> done
```

The loop prints nothing.

**Links.** `cargo doc` with warnings denied passes with the default
features, with `--all-features` and with `--no-default-features`. At the
merge base it failed with 12 errors with the default features (broken and
ambiguous intra-doc links), and the first pass fixed them by rewording.

**Errors found in `SPEC.md`.** The passes found three, and did not change
`SPEC.md`, because this pull request changes words of the code only. Issue
#95 lists them. SPEC
§9 has two items numbered 40. SPEC §5 cites law 41 for a proof that is a
reading of the history, which is law 42. SPEC §3 cites law 39 for the
restricted join of `accept`, but law 39 is about nests.

## 8. Stage 3, as made

Stage 3 made the crate `prodrome-data` in five commits that each build
and pass the three commands of `AGENTS.md`: the cuts inside the core, the
move of the files, the words of the derive, the narrowing, and the
documents. Later commits answer the review.

**The cuts.** The first commit moved the items that the data crate holds
into a private module `data` of the core, and left each old path as a
module that re-exports them. `scripts/edges.py core/src data` then
showed that no module under `data` named a module outside it. The second
commit moved `core/src/data/` to `data/src/`. The CI's lint check runs
`python3 scripts/edges.py data/src`, which shows that the data crate
names no other crate of the workspace. Each edge of the table of
section 4 was cut by a move, as follows:

| Edge | The cut, as made |
|---|---|
| `event` to `sign` | `Signed`, `KeyAdded`, `KeyRevoked`, `PublicKey` and `Signature` moved to the data crate. `Secret` stays in the core, because `Secret::sign_whole` reads a `Dag`, and an inherent method cannot be in another crate than its type. The data crate holds `public_of` and `signed_by`, the two bodies of `Secret`'s methods that build a key and a signature. |
| `event` to `term`, `payload` | The todo schema's events and their `mk_*` stay in the core's `event`. The names, `Envelope` and the hashing moved. |
| `schema` to `fold` | The register algebra moved. `fold::read` and `fold::readings` stay in the core, because they read a `Policy`, an `fpl::Instant` and a `Schedule`, which are not data. `registers::Stamp` moved with the algebra, because `Product` names it, and so did `BitSet`, which a `Stamp`'s place holds. |
| `schema` to `store` | `store/husk.rs` moved whole, as the module `husk`. |
| `schema` to `fpl`, `policy` | `Price`, `History`, `Bind` and `Row` stay in the core's `schema`. |
| `snapshot` to `dag` | `attest` and `Attestation` stay in the core's `snapshot`. |
| `payload` to `term` | `Payload`, `RECORD_PREFIX`, `record_signature` and `spec_field` stay in the core's `payload`. The field reads moved. |

No edge needed a new trait. `prodrome-core` re-exports each moved module
with a glob, so a program that depends on it changes nothing. `cli/`,
`wasm/`, `github/` and every test build with no change of an import. Two
tests include the example of `schema::headed` by its file path, and that
path changed. The example names `prodrome_data`, because the data
crate's documentation shows it, and the fixture `dag-sizes` depends on
the data crate for it.

**The dependencies of the data crate.** chrono (the calendar check of
`literal::Datetime`), ed25519-dalek (a public key must be a point, and
`Signed::verifies`), memchr (the parser of `literal`), sha2 (the name of
an object), thiserror (`ProdromeError`) and prodrome-derive (the derive
that `literal::codec` re-exports).

**The derive.** The derive writes `::prodrome::…` by default, so a program
that depends on `prodrome-core` changes nothing. A program that depends
on `prodrome-data` alone writes `#[literal(crate = prodrome_data)]`, an
option that the derive already had. The types that derive inside the
data crate write `crate = crate`. `data/tests/alone.rs` derives through
the data crate, and `core/tests/all/derive.rs` derives through the
facade.

**The narrowing.** The move narrowed four items: `literal::print_float`
became crate-visible, and `newtype_str!`, `BitSet` and `topo::Topo` became
`#[doc(hidden)]`. Nineteen items that were crate-visible became public and
`#[doc(hidden)]` in the data crate, because the core uses them and
`pub(crate)` does not reach across crates: `Stamp::position`,
`Stamp::ancestry`, `Stamp::placed`, `Stamp::open`, `Place` and
`Place::open`, `Scope`, `Hash::of_bytes`, `Hash::of_digest`,
`Hash::digest`, `ProdromeError::io`, the functions `field`, `hashes`,
`hashes_value`, `sorted_distinct` and `rests_on` of `event`, and `bytes`,
`public_of` and `signed_by` of `sign`. Each is marked where it is
declared: a method cannot move to a separate module, so one rule serves
every item. The core requires the exact version of the data crate, because
these items carry no promise between versions.

**The boundary of a `Stamp`.** A program can build a `Stamp` with any place
through `Stamp::placed` and `Place`, but only that program's own reads of
that `Stamp` see the place, because a store folds its own `Folded` from
the `Node`s of its DAG. To close this door, a later change can move the
fold of the DAG (`Folded`, `Node`, `fold`, `extend`, `since` and `heads`
of `registers`) into the data crate, and make the place crate-visible
again.

**The face.** "Before" is the merge base `6364741`, with the line measure
of the time.

| Measure | `prodrome-core` before | `prodrome-core` after, without the data crate | `prodrome-data` |
|---|--:|--:|--:|
| lines (`scripts/face.py --lines`) | 113,539 | 92,381 | 22,260 |
| rustdoc, default features | 52,026 | 38,346 | 14,083 |
| rustdoc, with `laws` | 55,848 | 42,168 | |

The two crates together are 1,102 tokens more than the core before by
the line measure, and 403 more by the rustdoc measure: the heads of the
modules that re-export, the root of the data crate and its hidden items.
A unit that only builds and reads events reads 22,260 tokens, which is
under the budget of 25,000 and over the target of 15,000 in "The
targets". The rest of that target needs stage 2 and the third pass of
words over `literal`, `schema` and `event`. The largest modules of the
data crate by the line measure are `literal` (8,455 tokens) and `fold`
(4,946).

**Compile time.** A clean build in the dev profile, with four cores,
twice each:

| Build | Before | After |
|---|--:|--:|
| the workspace | 18.5 s, 19.3 s | 19.0 s, 20.1 s |
| `prodrome-core` | 14.8 s, 14.2 s | 14.2 s, 14.1 s |
| `prodrome-data` alone | | 5.9 s, 6.7 s |

**The tests.** The law tests and the conformance vectors stay in
`prodrome-core`'s tests and run unchanged through the re-exports. The
unit tests and doc tests of the moved files run in the data crate: 87
unit tests and 12 doc tests before, and 75 and 8 in the core with 12 and
4 in the data crate after. The new test `data/tests/alone.rs` builds an
event and two objects, prints them, parses them back, and checks their
names against `sha256sum`, with the data crate as its only crate of the
workspace.

## 9. The instants and the words of the data crate, as made

An application that adopted the data crate counted which of its units
still depend on the core. Four of those units depend on the core only
through the conversions between a stored `literal::Datetime` and an
instant. The application's tool also measured the face of the data crate
at about 26,000 tokens, over the budget of 25,000. This stage moved the
conversions to the data crate and made the data crate's doc comments
shorter. It made four commits: the cut inside the core, the move of the
file, the words, and the root doc. A fifth commit records the stage in
this note and in `CHANGELOG.md`.

**The move.** The method is the method of stage 3. The first commit moved
the items into a private module `instant` of the core, and `fpl`
re-exported them. At that commit, `python3 scripts/edges.py core/src
instant` printed that the module names no module outside the set. The
second commit moved `core/src/instant.rs` to `data/src/instant.rs`, and
`python3 scripts/edges.py data/src` passes after it. `prodrome-core`
re-exports each item at its old path, so `cli/`, `wasm/`, `github/` and
every test build with no change of an import.

| Item | The path that `prodrome-core` keeps | The path in the data crate |
|---|---|---|
| `Instant` | `prodrome::fpl::Instant` | `prodrome_data::instant::Instant` |
| `instant_of` | `prodrome::fpl::instant_of` | `prodrome_data::instant::instant_of` |
| `datetime_of` | `prodrome::fpl::datetime_of` | `prodrome_data::instant::datetime_of` |
| `iso` | `prodrome::fpl::iso` | `prodrome_data::instant::iso` |
| `parse_iso` | `prodrome::fpl::parse_iso` | `prodrome_data::instant::parse_iso` |
| `FplError` | `prodrome::fpl::FplError` | `prodrome_data::instant::FplError` |

`Instant` is a type alias of `chrono::NaiveDateTime`, not a type of the
evaluator. It has no inherent method, so no part of it stays in the core.
`FplError` moved because `parse_iso` returns it, and a change of that
return type would change what a program observes. The two `From` impls
between `FplError` and `ProdromeError` moved with it, because Rust allows
an impl of a foreign trait only in the crate of one of the two types. The
core's `fpl` still uses `FplError` for every refusal of a term. The
functions `instant_of` and `datetime_of` are `#[inline]`, because the
folds call them for each event, across the crate boundary. No trait was
added.

Section 8 kept `fold::read` and `fold::readings` in the core in part
because they read an `fpl::Instant`. That reason is gone, but they still
read a `Policy` and a `Schedule`, which are not data.

**The other candidates.** The test is the test of stage 3: an item is data
if it reads or writes a stored value and names no evaluator, store or
policy.

| Candidate | Moved | Reason |
|---|---|---|
| `Instant`, `instant_of`, `datetime_of`, `iso`, `parse_iso` | yes | They read and write the instant of an event, and they name only `chrono` and `literal::Datetime`. |
| `FplError` | yes | `parse_iso` returns it. It is a newtype of a message. |
| `event::Lifecycle` | no | It names only data types, but it is a record of the todo schema (SPEC §4). A program gets or stores a `Lifecycle` only through a `TodoEvent`, whose `SpecRevised` holds a `Term` and whose `Authored` holds a `Payload`. So a unit that uses a `Lifecycle` depends on the core in any case. Moved alone, it frees no unit, and it splits the records of the todo schema between two crates. It moves with the todo crate. |
| The todo schema's constructors (`mk_created`, `mk_completed`, `mk_cancelled`, `mk_reopened`, `mk_tended`, `mk_spec_revised`, `mk_record`) | no | Each returns a `TodoEvent<P: Payload>`. They belong with the reference schema in the todo crate, not in the data crate. |
| `Delta`, `div_delta`, `total_seconds`, `delta_from_hours`, `hours` | no | They copy CPython's `timedelta` arithmetic for the evaluator. A stored span is already `literal::Timedelta`, in the data crate. |

**The narrowing.** This stage narrowed no item. Every public item of the
data crate was checked for the code that names it outside its own file.
The items that only the data crate uses are already crate-visible
(`literal::print_float`, `event::lower_hex`, `event::optional_hash`). Of
the items that only the data crate and the tests name, the code that
`#[derive(Literal)]` writes names `codec::arm`, `call_of`,
`read_or_default`, `headed_len`, `headed_names`, `headed_table` and
`names_field`, so they must stay public. The others (`Int::from_i64`,
`Timedelta::ZERO`, `payload::tuple_or_empty`, `payload::expect_call`,
`Frontier::is_conflict`, `codec::Site`, `codec::Reader`,
`printable::is_printable`, `literal::print_str`) are parts of the API
that a program outside the library can reasonably need.

**The words.** The doc comments of the data crate had a third pass by the
rule of section 2. The doc characters of `data/src` fell from 50,899 to
37,135, with the new module `instant` included. Each item that is
`#[doc(hidden)]` for the core has one doc line, which starts with "For
the core; no promise." The pass listed each deleted sentence that gave a
reason, an invariant or a hazard, with the place where it is still
stated: `SPEC.md`, a design note, or the doc that remains. One detail
that was stated nowhere else was restored: the path that `Topo::settle`
gives to `cycle` starts and ends at the key. With blank lines
and comment lines removed, each file of `data/src` equals the file
before the pass. The root doc of the data crate lists, in eleven lines, what
a program needs to define a schema, stamp an event, print and parse it,
put it in an object and name the object.

**The face.** "Before" is the merge base `30ce2c0`, measured with the
scripts of this tree. Section 8 gives 22,260 for the data crate, because
it measured before the commits that answered its review.

| Measure | `prodrome-data` before | `prodrome-data` after | `prodrome-core` before | `prodrome-core` after |
|---|--:|--:|--:|--:|
| lines (`scripts/face.py --lines`) | 22,291 | 19,231 | 92,533 | 92,204 |
| rustdoc, default features | 14,083 | 11,669 | 38,480 | 38,311 |
| rustdoc, with `laws` | | | 42,302 | 42,133 |

The core's figures do not count the data crate. The line measure counts
the items that are `#[doc(hidden)]` and the module roots: it counts
`lib.rs` as the module `lib`, and every `//!` line of every file. So the
script needs no flag for them. The hidden items that are not methods,
with the impls of the hidden types, are 955 of the 19,231 tokens, and 948
of the 22,291 before. The rustdoc measure leaves the hidden items out.

The application's own tool gave about 26,000 tokens where
`scripts/face.py --lines` gave 22,291, which is 17 percent more. This
note could not run that tool. If the ratio holds, the data crate is
about 22,500 tokens by that tool, which is under the budget of 25,000.

**The timings.** The benchmark of section 8, in the release profile with
no link-time optimisation: a store of 15,000 objects over 100 todos,
about 420 bytes each. The benchmark measures a cold read (`dag()`),
`registers::fold`, `view::entries`, `Dag::verify` and one append on a
warm handle. Each binary takes the median of ten runs of each operation.
The base binary and the branch binary ran in turn, eleven times each, and
the table gives the median of the eleven medians, in milliseconds.

| Operation | Before | After | Change |
|---|--:|--:|--:|
| cold read | 84.6 | 86.9 | +2.7 % |
| `registers::fold` | 22.8 | 22.7 | −0.3 % |
| `view::entries` | 27.4 | 26.7 | −2.3 % |
| `Dag::verify` | 26.0 | 26.0 | +0.1 % |
| append | 21.4 | 20.8 | −3.1 % |

No operation is slower by more than 3 percent.

**What remains, in order.** The units of the application that still
depend on the core are held by the readings and by `declared`, so these
come next:

1. the readings: `fold::read` and `fold::readings`, `policy` and
   `registers`, which a unit needs to read the registers of an entity
   from its events;
2. `declared`, the schemas that a store declares as data;
3. the rest of the store crate of section 5, item 4.1;
4. fpl, digest and todo, in the order of section 5, item 4.

## 10. The readings, as made

The application of section 9 counted again which of its units depend on
the core, at the merge base `588e306`. Twenty-eight units depend on it.
The module `policy` is named by nineteen of them, more than any other
module that is not in the data crate. Three units need only `declared`,
and one unit needs only `memo`. The units that read the registers of an
entity need `dag`, `registers`, `policy` and the reading `fold::read`
together. This stage made three crates: `prodrome-memo`, `prodrome-dag`,
which holds the readings, and `prodrome-declared`. It also moved the
data of `policy` to `prodrome-data`.

**The method.** The method is the method of stage 3. Each commit below
builds and passes the three commands of `AGENTS.md`:

1. The cut of `memo`: the module names the data crate at its own paths.
   `python3 scripts/edges.py core/src memo` passes.
2. The move of `core/src/memo/` to `memo/src/`.
3. The cut of the readings. `fold::read`, with the private functions
   `scan` and `admits` that it calls, moved to a private module `reading`
   of the core. The modules `dag`, `registers`, `policy`, `sign` and
   `spread` name the data crate and the memo crate at their own paths.
   `Standing`, `Everything`, `Untrusted` and the module `declared` moved to
   the data crate. `python3 scripts/edges.py core/src dag registers policy
   sign spread reading` and `python3 scripts/edges.py data/src` pass.
4. The move of those modules to `dag/src/`. The module `reading` became
   the module `fold` of the new crate.
5. The methods of the new crate that became public keep the facts of
   their values (see below).
6. The move of `declared` from the data crate to `declared/src/`, after
   the review found the data crate over the budget with it.
   `python3 scripts/edges.py declared/src` passes, and the crate names
   only public items of the data crate.
7. The pass of words over `prodrome-declared`.

Other commits record the stage in this note and fix the words of the
docs. The CI's lint check runs `scripts/edges.py` over `data/src`,
`memo/src`, `declared/src` and `dag/src` with no module named. In that
form, the script prints each use of the facade `prodrome` in a crate of
the library, which also breaks the build. The comment in the workflow
says so: the proof of each cut is the module-set form above, which runs
before the files move. `prodrome-core` re-exports every moved item at
its old path, so `cli/`, `wasm/`, `github/` and the tests build with no
change of an import. No trait was added, and no item was renamed or
narrowed.

**Where a policy lives.** A policy is a predicate over the objects of a
history. The test of section 9 decides where each part goes: an item is
data if it reads or writes a stored value and names no evaluator, store
or history.

| Item | What it names | Crate |
|---|---|---|
| `Standing` | nothing | `prodrome-data` |
| `Everything` | nothing | `prodrome-data` |
| `Untrusted` | `Actor`, in a set of actors | `prodrome-data` |
| `Policy` | `Hash`, `Schema`, `Standing`, and `Dag` in `Policy::at` | `prodrome-dag` |
| `Proven` | `Proof`, `PublicKey` and `Dag` | `prodrome-dag` |

`Policy::at` gives a policy at a history, which is a `Dag`. To put the
trait below the DAG, the trait would need a type parameter for the
history, and every host that implements `Policy` would change. So the
trait is in `prodrome-dag`, with its impls for `Everything` and
`Untrusted`. Rust allows these impls there, because the trait is local
to that crate. The impl for `Untrusted` reads the roster through the
public method `Untrusted::actors`, because the field of `Untrusted` is
private to the data crate. A unit that names `Untrusted` or `Everything`
only to pass a policy on reads the data crate alone.

The table of section 4 lists the edge `schema` to `policy`. That edge is
in the trait `schema::Bind`, which reads a `Policy` and stays in the core
with `Price`, `History` and `Row`. It moves with the reading crate of
section 4.

**The crate of the readings.** `prodrome-dag` holds:

| Module | Items |
|---|---|
| `dag` | the whole module: `Dag`, `Heads`, `Finding`, `Unread`, `Excluded`, `Reach` and the others |
| `registers` | the whole module: `Folded`, `Node`, `fold`, `extend`, `since`, `heads`, `deps_for`, and a re-export of `prodrome_data::stamp::Stamp` |
| `policy` | `Policy` and `Proven`, and a re-export of `prodrome_data::policy` |
| `sign` | `Secret`, `Registrar`, `Proved`, `Proof` and `Checks`, and a re-export of `prodrome_data::sign` |
| `fold` | `read`, and a re-export of `prodrome_data::fold` |
| `spread` | the threads of a map over a vector, hidden |

`Product` is in `prodrome_data::fold`, with the rest of the register
algebra, since stage 3. `fold::sole` is in the data crate too.
`prodrome_dag::fold` and `prodrome::fold` re-export both. These items stay
in the core:

| Item | Reason |
|---|---|
| `fold::readings` | It returns a `Schedule`, and `Schedule::shift` takes an `fpl::Delta`, the span of the evaluator. It calls the hidden `prodrome_dag::fold::scan`. |
| the readings of a todo (`fold::Registers`, `env`, `specs`, `content`, `flatten`, `price`, `link_specs`, `member`, `stores`) | They name `Term`, `Env` and the todo schema. They move with the reading crate. |
| `snapshot::attest`, `Attestation` | They read a `Dag`, and section 4 puts them in the store crate. |

`prodrome-dag` depends on `prodrome-data`, on `prodrome-memo`, because
`sign::Checks` keeps the results of signature checks in a `memo::Lru`, and
on ed25519-dalek, for `Secret`. The core no longer depends on
ed25519-dalek or on memchr.

**The items that became public.** Twenty-four items of `prodrome-dag` are
public and `#[doc(hidden)]`, because the core uses them and `pub(crate)`
does not reach across crates. Each doc starts with "For the core; no
promise.": `fold::scan`, `fold::admits`, `spread::spread`, `dag::Prints`
and thirteen methods of `Dag` that the store calls, `dag::lacked_among`,
`Heads::new`, `Folded::rewrite`, and four methods of `Reach`.
`Node::placed` stays crate-visible, so only a `Dag` places a node, as
section 8 requires. The review asked of each item whether a program
outside the library can use it to put a value's kept facts out of step
with what the value holds, so that a later public call trusts a wrong
fact:

- `Dag::husk` kept the tips, the order and the index on its caller's word
  that the husk rests on what the held object rests on. It now compares
  the two, and lets the facts go where they differ. The test
  `a_husk_of_another_change_leaves_no_stale_facts` in
  `core/tests/all/husk.rs` puts the husk of another change under a name,
  and fails without the comparison.
- `Folded::rewrite` passes over a stamp whose key or instant differs
  from the event's, so a stamp keeps its place in its entity's stream.
- `Reach::insert` passes over a position past the order.
- `Dag::grow`, `Dag::forget` and `Dag::settle` change the history only
  through `History::edit`, which lets the kept facts go.
- `Dag::set_unread` and `Dag::hold_husks` change only the caller's own
  `Dag`: no kept fact reads the unread prints or the names of the husks.
  `excluded`, `verify` and `whole` report the unread prints, and a name
  held as a husk is vouched for in place of proven, and refused by
  `Secret::sign_whole`. Both answers are stricter, never looser.
- `Heads::new` makes a `Heads`, which `Heads::parse` already makes from
  any text.
- The other items read and change nothing: `held`, `holds`,
  `into_prints`, `order_among`, `is_root`, `node`, `read_prints`,
  `lacked_among`, `scan`, `admits`, `spread`, and `outside`, `close` and
  `tips` of `Reach`.

Small functions that a fold or a frontier calls for each object crossed
a crate boundary, so they are `#[inline]`: the methods of `Standing`,
`Untrusted::actors`, and `len`, `is_empty`, `contains`, `has` and `insert`
of `Reach`.

The unit tests of `dag`, `sign` and `policy` use the todo schema, which
stays in the core. They moved unchanged, except for their imports, to
`core/tests/all/dag_steps.rs`, `sign.rs` and `policy.rs`: 17 tests,
among them the law 59 test, which passed once at 20,000 cases. The test
of `spread` stays in the new crate. The two `compile_fail` examples of
`registers::Node` name the new crate and a generic schema. The first
builds a `Node` from its private fields. The second calls
`Node::placed`, the constructor that only a `Dag` may call. A check
outside the tree showed that each fails for the reason that it states.

**`declared`.** `Declaration` names `Hash`, and the literal types
`Signature` and `Vocabulary`. `Declared` names `Actor`, `Hash`, the
register algebra (`Climb`, `Frontier`, `Product`, `Register`, `grows`),
literal values, the field read `payload::required`, `Stamp` and `Schema`.
Every one of these is a public item of the data crate, so `declared` is
data. But with it, the data crate was 24,107 tokens by
`scripts/face.py --lines`, which is 27,500 to 28,200 by the application's
tool, over the budget. A pass of words cannot free the 2,200 tokens that
were needed, because most of the docs of `declared` state its contract.
So `declared` is the crate `prodrome-declared`, on the data crate, with
no hidden item and no impl of its own trait on a foreign type. The core
re-exports it as `prodrome::declared`. A store reads a declaration
through `EventStore::at`, which stays in the core. A unit that `declared`
alone holds reads the data crate and `prodrome-declared`.

The pass of words in the new crate moved the module example to the test
`declared/tests/admit.rs`, removed the docs that restate a method's name
or a field, and made the words in capitals plain. With blank lines and
comment lines removed, each file equals the file before the pass. The
crate fell from 4,458 to 4,151 tokens.

**What stays for the store crate.** The modules `store` (20,655 tokens),
`nest` (4,096) and `tags` (4,016) stay in the core, with `snapshot::attest`.
Together they are 28,767 tokens, 3,767 over the budget by this note's
measure and about 7,000 over by the application's tool. `scripts/edges.py`
shows that `tags` and `nest` name no module of the store: `tags` names
the event, the register algebra, `fold::read` and `Everything`, and `nest`
names the same and `Dag`. So the next stage can do these things:

- Move `tags` to `prodrome-dag`, which is then about 18,450 tokens.
  `tags` reads a tag set as the registers of an entity under
  `Everything`, which is a reading. The store crate is then about 24,750
  tokens, still over the budget by the application's tool.
- Move `nest` with `tags`, or to a crate of its own on `prodrome-dag`, to
  bring the store crate to about 20,650 tokens.
- Remove `pub` from the items of `store` that only their own file uses.
  The items of `store` that are `pub(crate)` or `pub(super)` are about
  1,700 tokens, so this alone is not enough.

`store::Sender` is named by eight units of the application, but it is
`prodrome_data::husk::Sender` since stage 3. A unit that names only
`Sender` and the husk types of `store` can change its path and drop the
core now.

**The face.** "Before" is the merge base `588e306`. The column "by the
application's tool" multiplies the line measure by 1.14 and 1.17, the
ratios that sections 9 and the review found.

| Crate | Lines, before | Lines, after | By the application's tool, after | Rustdoc, before | Rustdoc, after |
|---|--:|--:|--:|--:|--:|
| `prodrome-core`, without the crates below | 92,204 | 66,077 | 75,300 to 77,300 | 38,311 | 26,591 |
| `prodrome-data` | 19,231 | 19,654 | 22,400 to 23,000 | 11,669 | 11,967 |
| `prodrome-memo` | | 7,697 | 8,800 to 9,000 | | 4,304 |
| `prodrome-declared` | | 4,151 | 4,700 to 4,900 | | 1,747 |
| `prodrome-dag` | | 14,424 | 16,400 to 16,900 | | 5,664 |
| all | 111,435 | 112,003 | | 49,980 | 50,273 |

The rustdoc measure is with the default features. With the feature
`laws`, `prodrome-core` is 42,133 tokens before and 30,413 after. The
crates together are 568 tokens more than before by the line measure: the
root docs of the three new crates, the heads of the modules that
re-export, and the marks of the hidden items, less the pass of words over
`declared`. Every crate below the core is under the budget by both
measures.

A unit that reads the registers of an entity under a policy reads
`prodrome-data`, `prodrome-memo` and `prodrome-dag`: 19,654 + 7,697 +
14,424 = 41,775 tokens (47,600 to 48,900 by the application's tool), in
place of the 92,204 tokens of `prodrome-core` and the 19,231 of
`prodrome-data`. The root doc of `prodrome-dag` lists, in four items, how
a program holds a DAG, folds it, reads the registers of an entity under a
policy, and proves and checks signatures.

**Which crates a unit reads.** The table has one row for each holder set
of the count at the start of this section. A holder set is the set of
modules outside the data crate that a unit names, without the instants.
"All five" is `prodrome-core` with the four crates below it, 112,003
tokens. Before this stage, every one of these units read `prodrome-core`
and `prodrome-data`, 111,435 tokens.

| The modules that hold the unit | Units | Crates after | Tokens after |
|---|--:|---|--:|
| `declared` | 3 | data, declared | 23,805 |
| `memo` | 1 | data, memo | 27,351 |
| `policy`, `sign` | 1 | data, if the unit names only `Untrusted`, `Everything`, `Standing` and the object types of `sign`; otherwise data, memo, dag | 19,654 or 41,775 |
| `fpl`, `policy` | 1 | as the row above, if the unit's names of `fpl` are the instant conversions; otherwise all five | 19,654, 41,775 or 112,003 |
| `declared`, `fold`, `policy`, `registers` | 1 | data, memo, dag, declared, if the unit's names of `fold` are `read`, `sole`, `Product` and the rest of the register algebra | 45,926 |
| `declared`, `store` | 1 | all five; data and declared if its names of `store` are `Sender` and the husk types | 112,003 or 23,805 |
| `fpl`, `policy`, `term` | 1 | all five | 112,003 |
| `fold`, `policy`, `registers`, `tags` | 1 | all five, until `tags` moves | 112,003 |
| `dag`, `memo`, `sign`, `store` | 1 | all five | 112,003 |
| `dag`, `fold`, `policy`, `registers`, `store` | 1 | all five | 112,003 |
| six or more modules | 15 | all five | 112,003 |

So five units, and up to eight, read at most 45,926 tokens after this
stage, in place of 111,435. The other units are held by `store`, `tags`,
`term` or `fpl`, which the stages below move.

**The timings.** The benchmark of section 9, in the release profile with
no link-time optimisation, on a generated store of 15,000 objects over
100 todos, about 570 bytes each. Each binary takes the median of ten runs
of each operation. The base binary and the branch binary ran in turn,
eleven times each, on one store, and the table gives the median of the
eleven medians, in milliseconds. Each run appends ten objects, so the
store grows by 220 objects over the runs, alike for both binaries. The
branch is the tree with `prodrome-declared`.

| Operation | Before | After | Change |
|---|--:|--:|--:|
| cold read | 134.6 | 132.6 | −1.5 % |
| `registers::fold` | 31.1 | 30.0 | −3.3 % |
| `view::entries` | 34.2 | 34.5 | +0.9 % |
| `Dag::verify` | 33.8 | 32.5 | −3.7 % |
| append | 27.0 | 27.0 | −0.0 % |

No operation is slower by more than 3 percent. The runs vary by more
than these changes, so the operations that are faster are not faster
because of this stage.

**Compile time.** A build of the workspace in the dev profile, with four
cores, twice each, each time in a new target directory. "Touch" is the
build after `touch data/src/lib.rs`, twice after each clean build.

| Build | Before | After |
|---|--:|--:|
| clean | 26.6 s, 25.4 s | 25.4 s, 26.6 s |
| touch | 2.3 s, 2.6 s, 2.4 s, 2.7 s | 2.4 s, 2.5 s, 2.4 s, 2.6 s |

The three new crates do not make a clean build or the common edit slower
by a measurable amount.

**What remains, in order.** The units of the application that still
depend on `prodrome-core` are held by `store`, `tags`, `term` and `fpl`:

1. the store crate: `store`, `nest`, `tags` and `snapshot::attest`, under
   the budget by the application's tool, by the moves above;
2. fpl: `term`, `fpl`, `schedule`, `chain`, `breaks` and `observe`;
3. digest;
4. the reading crate: the readings of a todo, `fold::readings`, `view`,
   and the traits `Price`, `History`, `Bind` and `Row`;
5. todo.

## 11. The store, as made

The application of sections 9 and 10 adopted the merge base `a5ecf0c`
and counted again which of its units depend on `prodrome-core`. Nine units
left the core at that revision, and nineteen still depend on it. Of the
nineteen, five units name only items of the store (`EventStore`,
`Replica`, `Doors`, the frontiers), one unit names only `tags`, and one
unit names only `digest`. One more unit names only the husk types of the
store and `event::Lifecycle`. This stage made four crates:
`prodrome-store`, `prodrome-tags`, `prodrome-nest` and
`prodrome-digest`. It also moved `Lifecycle` to the data crate.

**The method.** The method is the method of stage 3. Each commit below
builds and passes the three commands of `AGENTS.md`:

1. The cut. The modules `store`, `nest`, `tags`, `digest` and `snapshot`
   name the data crate, `prodrome-memo` and `prodrome-dag` at their own
   paths, and not through the modules of the core that re-export them.
   After this commit, `python3 scripts/edges.py core/src store nest tags
   snapshot digest` prints that no module of the set names a module
   outside it.
2. The move of `core/src/store.rs`, `core/src/store/` and
   `core/src/snapshot.rs` to `store/src/`, of `nest` to `nest/src/`, of
   `tags` to `tags/src/`, and of `digest` to `digest/src/`. The CI's lint
   check runs `scripts/edges.py` over the source directory of each new
   crate.
3. The move of `Lifecycle` to `data/src/event/lifecycle.rs`.

Another commit records the stage in this note, in `CHANGELOG.md`, in
`README.md` and in `docs/development.md`. `prodrome-core` re-exports every
moved item at its old path, so `cli/`, `wasm/`, `github/` and the tests
build with no change of an import. No trait was added, and no item was
renamed or narrowed.

**The edges into the core.** At the merge base, `python3 scripts/edges.py
core/src store nest tags snapshot` printed 34 uses of a module outside the
set. Each use named a module of the core that re-exports a lower crate:
`event`, `schema`, `fold`, `policy`, `registers`, `dag`, `literal`,
`genesis`, `change`, `topo`, `memo` and the hidden `spread`. Each item
that the uses name is in a lower crate:

| The core's module | The items that the set names | Their crate |
|---|---|---|
| `event` | `Hash`, `Envelope`, `Actor`, `mk_woven`, `parents_of`, `rests_on`, `canonical`, `canonical_envelope`, `hashes`, `hashes_value`, `sorted_distinct` | `prodrome-data` |
| `schema` | `Schema`, `headed` | `prodrome-data` |
| `fold` | `Order`, `Product`, `sole`; `read` | `prodrome-data`; `prodrome-dag` |
| `policy` | `Untrusted`, `Everything`; `Policy` | `prodrome-data`; `prodrome-dag` |
| `registers` | `Stamp`; `Folded`, `Node`, `Genesis`, `Prodrome`, `fold`, `deps_for` | `prodrome-data`; `prodrome-dag` |
| `dag` | `Dag`, `Heads`, `Finding`, `Unread`, `Reach`, `decode`, `lacked_among`, `Prints` | `prodrome-dag` |
| `literal`, `genesis`, `change`, `topo` | `ProdromeError`, `Datetime`, `Value`, `Call`, the codec, `mk_genesis`, `mk_change`, `topo` | `prodrome-data` |
| `memo` | `name`, `Tree`, `Algebra` | `prodrome-memo` |
| `spread` | `spread` | `prodrome-dag` (hidden) |

So the code of these modules names no item of the evaluator, of `term`,
of the todo's events, of `view` or of the readings of a todo, and the cut
moved no item and no body. Three other kinds of use named the core:

- The unit tests of `store` and the doc tests of `store::Decision` drive a
  store with the todo schema: `TodoEvent`, its `mk_*`, `reference::Todo`
  and `change::Change`. These tests read private items of the store, such
  as the injected faults and the record of heads, so they cannot move to
  `core/tests/`. `prodrome-store` has `prodrome-core` as a
  dev-dependency, which Cargo allows. The unit tests are a second build of
  the store crate, and the core links the first, so the tests pass only
  values of the data crate and of `prodrome-dag` between the two. The
  tests name the core as `prodrome::`, and the doc tests name the store
  as `prodrome_store::store::`. The two `compile_fail` doc tests were
  checked without the mark: each fails because an `Overlay` or a
  `MemoryStore` has no method `decide`, which is what each states.
- Four doc links named a module that the new crate cannot see:
  `crate::declared` in `EventStore::at`, `crate::store::accept` and
  `crate::store::Held` in `nest`, and `crate::fold::RegisterType` in
  `nest::Heads`. The first three name the crate in words, and the fourth
  links to `prodrome_data::fold::RegisterType`.
- The `compile_fail` example of `tags` names `prodrome_tags` and
  `prodrome_data`. It was checked without the mark: it fails at the
  assertion of `Names::CHECKED`, which is what it states.

**`digest`.** `digest` names `Hash` and `topo` of the data crate, and
`memo`. So it moved in this stage, to `prodrome-digest` on
`prodrome-data` and `prodrome-memo`, with no risk to the store: no module
of the store names it, and it names no module of the store.

**How the modules divide.** `store`, `nest`, `tags` and the attestation
together are about 29,000 tokens by `scripts/face.py --lines`, over the
budget. The test is what a program needs together:

| Crate | Holds | Depends on | Why apart |
|---|---|---|---|
| `prodrome-store` | `store`, and `snapshot` with `attest` and `Attestation` | data, dag | A program that takes objects through a door names `Doors`, `Sender`, `Replica` and the frontiers together. `attest` reads a `Dag`, and the store calls it to write a snapshot. |
| `prodrome-tags` | `tags` | data, dag | A program that keeps tags reads an entity's registers under `Everything`, and needs no store. |
| `prodrome-nest` | `nest` | data, memo, dag | No unit of the application names `nest`. In `prodrome-dag` it would add 4,124 tokens to every reader of the DAG. |
| `prodrome-digest` | `digest` | data, memo | It names no DAG. |

Section 10 proposed to put `tags` in `prodrome-dag`. That would add 4,036
tokens to each unit that reads `prodrome-dag` and not `tags`, such as the
five units of the store in the table below, so `tags` is a crate of its
own. `prodrome-store` holds `snapshot` as
`prodrome-dag` holds `policy`: the module re-exports
`prodrome_data::snapshot` and adds the items that read a `Dag`.
`prodrome-store` does not depend on `prodrome-memo` itself, but
`prodrome-dag` does, so a program on the store reads the memo crate too.
No item of `store` is `pub(crate)`, and its `pub(super)` items are named
by other files of the module, so no narrowing was possible.

**`Lifecycle`.** `Lifecycle` is the record of the fields of `Completed`,
`Cancelled`, `Reopened` and `Tended`: a `TodoId`, a `Datetime`, an
`Actor` and a note. Each field is an item of the data crate, so
`Lifecycle` is data by the test of section 9. Section 9 kept it in the
core, because a program reads a `Lifecycle` out of a `TodoEvent`. The
count of this stage shows a unit that names `Lifecycle` and no
`TodoEvent`, so the unit reads only data, and the move frees it. The
other records of the todo schema stay in the core: `SpecRevised` holds a
`Term`, `Authored` holds a `Payload`, and no unit is held by `Created`.
The data crate is 19,756 tokens by `scripts/face.py --lines` after the
move, 102 more than before.

**`EventStore::schema`.** `schema` is a method of the trait `Replica`,
so a program that calls it on an `EventStore` imports the trait.
`EventStore` already has the inherent methods `policy` and `root`. An
inherent method `schema` would serve every caller that names an
`EventStore`, and the trait method would call it. A generic caller over
`R: Replica<E>` needs the trait in any case. This stage adds no method,
because it changes no item of the store's API, so the unit that imports
`Replica` only for `schema` still does. `Doors` is a plain enum, and a
unit that names it reads the store crate.

**A second backend for a replica.** The trait `Replica` is in
`prodrome_store::store`, and no type of the file system is in its
signature: it names `Schema`, `Hash`, `Heads`, `Held`, `Sender` and
`ProdromeError`. A backend in another crate implements it with a
dependency on `prodrome-store`, which also compiles the store on disk and
`rustix` on Unix. If issue #82 needs a crate without the files, a later
cut can move `Replica`, `Held`, `sync` and `accept` out of `store`. This
stage does not make that cut harder, because it moved the module whole.

**The face.** "Before" is the merge base `a5ecf0c`. The rustdoc measure
is with the default features. With the feature `laws`, `prodrome-core` is
30,413 tokens before and 18,963 after.

| Crate | Lines, before | Lines, after | Rustdoc, before | Rustdoc, after |
|---|--:|--:|--:|--:|
| `prodrome-core`, without the crates below | 66,077 | 33,374 | 26,591 | 15,141 |
| `prodrome-data` | 19,654 | 19,756 | 11,967 | 12,030 |
| `prodrome-memo` | 7,697 | 7,697 | 4,304 | 4,304 |
| `prodrome-declared` | 4,151 | 4,151 | 1,747 | 1,747 |
| `prodrome-dag` | 14,424 | 14,424 | 5,664 | 5,664 |
| `prodrome-store` | | 21,214 | | 6,394 |
| `prodrome-nest` | | 4,124 | | 2,234 |
| `prodrome-tags` | | 4,036 | | 1,890 |
| `prodrome-digest` | | 3,739 | | 1,330 |
| all | 112,003 | 112,515 | 50,273 | 50,734 |

The crates together are 512 tokens more than before by the line measure:
the root docs of the four new crates, the module doc of `lifecycle`, and
the lines of the core that re-export.

`prodrome-store` is 21,214 tokens by the line measure, of which the
module `store` is 20,660. The application's tool counts 15 percent more
than `scripts/face.py --lines` for the data crate, 9 percent more for
memo, and 3 to 5 percent less for dag and declared. The store is code and
signatures as dag is, but the note cannot run that tool, so the range of
those ratios gives 20,200 to 24,400 tokens by that tool. Both ends are
under the budget of 25,000. Every crate below the core is under the
budget by both measures.

**Which crates a unit reads.** The table has one row for each unit of the
count at the start of this section. Before this stage, each of these
units read `prodrome-core` and the four crates below it, 112,003 tokens.
"All nine" is `prodrome-core` with the eight crates below it, 112,515
tokens.

| Unit | The items that hold it | Crates after | Tokens after |
|---|---|---|--:|
| a mail reader | `store::Replica` | data, memo, dag, store | 63,091 |
| a registry of stores | `store::Doors` | data, memo, dag, store | 63,091 |
| a local door | the frontiers of the store | data, memo, dag, store | 63,091 |
| a server | the frontiers and `Replica` | data, memo, dag, store | 63,091 |
| a store wrapper | `EventStore` and the errors of the frontiers | data, memo, dag, store | 63,091 |
| a topics unit | `tags` | data, memo, dag, tags | 45,913 |
| a digest unit | `digest` | data, memo, digest | 31,192 |
| a lines unit | the husk types and `Lifecycle` | data, with the paths `prodrome_data::husk` and `prodrome_data::event::Lifecycle` | 19,756 |
| a chains unit | the store, `TodoEvent`, `mk_completed`, `mk_created` | all nine | 112,515 |
| a command line | `Alone`, `Shared`, `TodoEvent`, four `mk_*`, `Term` | all nine | 112,515 |
| an API unit | the store, the evaluator, the events and `view` | all nine | 112,515 |
| a records unit | `EventStore`, the evaluator, terms, events and `view` | all nine | 112,515 |
| eight more | the evaluator, terms, the todo's readings and events, `view` | all nine | 112,515 |

So eight units read at most 63,091 tokens after this stage, in place of
112,003, and together they read 412,316 tokens in place of 896,024. The
lines unit reads the data crate alone when it names the husk types and
`Lifecycle` at their paths in the data crate. The other units of the
table are held by the todo's events, `term`, `fpl` or `view`.

**The timings.** The benchmark of section 9, in the release profile with
no link-time optimisation, on a generated store of 15,000 objects over 100
todos, about 620 bytes each. Each binary takes the median of ten runs of
each operation, and each run appends ten objects on a warm handle. The
base binary and the branch binary ran in turn, eleven times each, in
three rounds on one store. The second round ran the branch first. The
table gives the median of the 33 medians of each binary, in milliseconds.

| Operation | Before | After | Change |
|---|--:|--:|--:|
| cold read | 153.6 | 154.9 | +0.9 % |
| `registers::fold` | 41.4 | 42.4 | +2.5 % |
| `view::entries` | 51.7 | 51.8 | +0.2 % |
| `Dag::verify` | 62.4 | 60.9 | −2.4 % |
| append | 32.2 | 32.1 | −0.1 % |

No operation is slower by more than 3 percent. The first round alone gave
+4.2 percent for a cold read and +5.0 percent for `registers::fold`, and
the next two rounds gave −0.3 and −2.8 percent, and +1.4 and +0.2
percent. `registers::fold` is in `prodrome-dag`, which this stage does not
change, so these differences are variation of the machine. No `#[inline]`
was added: the store called the data crate and `prodrome-dag` across a
crate boundary before this stage too, and the new boundary between the
core and the store is crossed once for each operation.

**Compile time.** A build of the workspace in the dev profile, with four
cores, twice each, each time in a new target directory. "Touch" is the
build after `touch data/src/lib.rs`, twice after each clean build.

| Build | Before | After |
|---|--:|--:|
| clean | 29.3 s, 28.2 s | 29.0 s, 27.3 s |
| touch | 2.8 s, 2.6 s, 2.7 s, 3.0 s | 2.9 s, 3.0 s, 2.5 s, 3.2 s |

The four new crates do not make a clean build or the common edit slower
by a measurable amount.

**The tests.** The integration tests of the core run unchanged through
the re-exports: 459 tests. The 33 unit tests and 3 doc tests of the store
moved with it, so the core's unit tests fell from 57 to 24, and its doc
tests from 4 to 1. The `compile_fail` example of `tags` runs in
`prodrome-tags`. No law was added or changed. The two property tests that
moved are each over one small value, a crash point among five and a set
of up to five stray names, and they ran at their full counts of 32 and 64.

**What remains, in order.** The units of the application that still
depend on `prodrome-core` after this stage each name the todo's events,
`term`, `fpl` or `view`:

1. fpl: `term`, `fpl`, `schedule`, `chain`, `breaks` and `observe`. Each
   of these units names a term or the evaluator, or an event that holds
   a term.
2. the reading crate: the readings of a todo, `fold::readings`, `view`,
   and the traits `Price`, `History`, `Bind` and `Row`;
3. todo: `todo`, `reference`, `Payload`, and `TodoEvent` with its `mk_*`
   and its other records.

The chains unit and the command line name the todo's events and `Term`,
and not the readings. If the stage of fpl finds with `scripts/edges.py`
that `TodoEvent` and its `mk_*` name only fpl and the data crate, the
todo's events can move before the reading crate, and those two units
then read the data crate, fpl, the store and the events.

## 12. The terms, the evaluator and the todo schema, as made

The application of sections 9 to 11 adopted the revision `39c408f` and
counted again which of its units depend on `prodrome-core`. Eight more
units left the core at that revision, and eleven still depend on it. By
`scripts/face.py --lines`, the core was 33,374 tokens at the merge base
`b32472b` of this stage, over the budget, and each of the eleven units
read it whole. A term, the evaluator or a todo's event holds ten of the
eleven. This stage made three crates: `prodrome-term`, which holds the
terms as data, `prodrome-fpl`, which holds the evaluator, and
`prodrome-todo`, which holds the todo schema's events and its other data.

**The method.** The method is the method of stage 3. Each commit below
builds and passes the three commands of `AGENTS.md`:

1. The cut. The parts of `fpl` that build, print and parse a term, with
   the environment that a term reads, moved to a private module `syntax`
   of the core, and `fpl` re-exported it. The todo schema's vocabulary,
   its registers and what each event writes moved from `todo`, `fold` and
   `fold/write.rs` to a private module `todo_schema`, and `todo` and
   `fold` re-exported them. `Segment` moved from `prodrome-nest` to the
   data crate (see below). After this commit, each of these commands
   prints that no module of the set names a module outside it:

   ```console
   $ python3 scripts/edges.py core/src term schedule syntax
   $ python3 scripts/edges.py core/src term schedule syntax fpl chain breaks observe
   $ python3 scripts/edges.py core/src term schedule syntax event payload todo_schema reference
   ```

   The modules of each set name the data crate at its own paths, so the
   script also proves that they name no module of the core that
   re-exports a crate above the data crate.
2. The move of `term`, `schedule` and `syntax` to `term/src/` (`syntax`
   became the module `fpl` of the new crate), of `fpl`, `chain`, `breaks`
   and `observe` to `fpl/src/`, and of `event`, `payload`, `reference` and
   `todo_schema` to `todo/src/` (`todo_schema` became the module `todo`).
   The CI's lint check runs `scripts/edges.py` over the source directory
   of each new crate.
3. The test `todo/tests/alone.rs`, below.
4. The mark `#[inline]` on `divide_and_round` (see "The timings").

Another commit records the stage in this note, in `CHANGELOG.md`, in
`README.md` and in `docs/development.md`. `prodrome-core` re-exports every
moved item at its old path, so `cli/`, `wasm/`, `github/` and the tests
build with no change of an import. No trait was added, and no item was
renamed or narrowed.

**The terms as data.** At the merge base, `python3 scripts/edges.py
core/src term` printed five uses of a module outside the set: `term`
named `Delta`, `Instant`, `PRIORITY_POWER`, `FplError`, the smart
constructors and the private `piecewise` of `fpl`, and `Schedule` of
`schedule`. None of these is a part of the evaluator. A term holds a
`Schedule` in a `Piecewise`, so `schedule` is part of the terms too. The
constructors named one more crate, which the script does not see because
the core re-exports it: `mk_ref_in` checks an entity by the rule of
`nest::Segment`, and `prodrome-nest` depends on `prodrome-dag`. A
`Segment` is a name with a rule for valid text, as `TodoId` is, so it
moved to `prodrome_data::segment`, and `prodrome-nest` re-exports it at
`prodrome_nest::Segment`. With that move, `term`, `schedule` and the
constructors stand on the data crate alone.

The terms could be part of the data crate, or a crate of their own. The
data crate is 19,913 tokens by `scripts/face.py --lines` after this
stage, and the terms are 6,914. Together they would be 26,827 tokens,
which is over the budget by this note's measure and about 30,500 to
31,400 by the application's tool. So the terms are the crate
`prodrome-term`, on the data crate. A program that builds a term and
stores it reads 26,827 tokens in place of the 113,095 of `prodrome-core`
and the crates below it.

`prodrome-term` holds these items. Each keeps its path through the core.

| Items | The path in `prodrome-term` | The path that `prodrome-core` keeps |
|---|---|---|
| `Term`, `TermF`, `CurvePoint`, `normalize`, `schema` and its items | `prodrome_term::term::…` | `prodrome::term::…` |
| `Schedule` and the rest of `schedule` | `prodrome_term::schedule::…` | `prodrome::schedule::…` |
| the smart constructors: `mk_flat`, `mk_decay`, `mk_curve`, `mk_conj`, `mk_least`, `least_of`, `mk_offset`, `mk_gate`, `mk_shift`, `mk_within`, `mk_importance`, `mk_offset_by`, `mk_after`, `mk_recur`, `mk_periodic`, `mk_ref`, `mk_ref_in`, `mk_absent`, `mk_piecewise`, `checklist` | `prodrome_term::fpl::…` (file `fpl/construct.rs`) | `prodrome::fpl::…` |
| the spans: `Delta`, `div_delta`, `total_seconds`, `delta_from_hours`, `hours` | `prodrome_term::fpl::…` (file `fpl/delta.rs`) | `prodrome::fpl::…` |
| the print and the parse: `print_term`, `parse_term`, `Term::to_value`, `Term::from_value`, the impl of `Literal` for `Term` | `prodrome_term::fpl::…` (file `fpl/printed.rs`) | `prodrome::fpl::…` |
| what a term reads: `Env`, `Outcome`, `Candidates` | `prodrome_term::fpl::…` (file `fpl/env.rs`) | `prodrome::fpl::…` |
| `PRIORITY_POWER`, and the re-exports of `Instant`, `instant_of`, `datetime_of`, `iso`, `parse_iso` and `FplError` | `prodrome_term::fpl::…` | `prodrome::fpl::…` |

`Env`, `Outcome` and `Candidates` moved because the todo schema's
registers name them (see below), and because they are data: SPEC §6.1
defines the environment as a snapshot of history, and each of the three
is a plain value with no code of the evaluator. A second evaluator, in
this language or another, reads the same three values. `Term::to_value`,
`Term::from_value` and the impl of `Literal` for `Term` must be in the
crate of `Term`, because the first two are inherent methods and the trait
is in another crate. The module `term::schema` names each field of each
layer, so a second evaluator reads a term's shape from `prodrome-term`
alone, and the print of SPEC §2 is its format.

Four items became public and `#[doc(hidden)]`, because the evaluator or
the core calls them and `pub(crate)` does not reach across crates. The
doc of `piecewise`, which `chain` and `fold::flatten` call, starts with
"For the core; no promise.". The docs of `us_of`, `after` and `ratio`,
the arithmetic of spans in microseconds that the evaluator calls for each
node, start with "For the evaluator; no promise.". None of them changes a
value that a later call trusts.

**The evaluator.** `fpl` (the rest: `Closed`, `fulfillment`, `explained`,
`Explanation`, `Note`, `Scalar`, `link`, `link_in`, `LinkError`, `Stores`,
`Member` and the others), `chain`, `breaks` and `observe` are the crate
`prodrome-fpl`, on the data crate and `prodrome-term`. Its module `fpl`
re-exports `prodrome_term::fpl`, so `prodrome_fpl::fpl` and
`prodrome::fpl` hold the same items, as `prodrome_dag::policy` and
`prodrome::policy` do. These items stay in it:

| Item | Reason |
|---|---|
| `WITHIN_SAMPLES` | It is the number of samples of the evaluation of `Within`. |
| `Closed`, `holds_absent` | A `Closed` term is what evaluation takes (SPEC §7.2), and `link` makes one. |
| `wire_key`, `scalar`, `scalars` | They make the notes of an explanation. |

The face of `prodrome-fpl` is 5,705 tokens by `scripts/face.py --lines`
and 4,290 by the rustdoc measure, far under the budget, so this stage
does not divide it. A program that asks what changes next
(`observe`), a program that explains a price (`explained`, `Note`,
`Explanation`) and a program that only calls `fulfillment` each read the
data crate, `prodrome-term` and `prodrome-fpl`: 32,532 tokens. A division
would save each of them at most the 5,705 tokens of the crate.

**The todo schema.** `TodoEvent`, its records, its `mk_*` and `Payload`
name only the terms and the data crate: with `event` and `payload` in the
set of the evaluator, `scripts/edges.py` prints no use outside it, and
`event` and `payload` name no module of the evaluator. One edge goes up,
and the script does not show it, because it is a rule of Rust: the impl
of `Schema` for `TodoEvent` must be in the crate of `TodoEvent`, because
`Schema` is in the data crate. That impl names `fold::Registers` and
`fold::Kind`, which the note listed with the readings of a todo, and
`Write`. These are the todo schema's data, as follows:

| Item | What it names | Moved to |
|---|---|---|
| `TODO_SIGNATURES`, `TodoVocabulary` and the impl of `Schema` | the terms' signatures, `Payload`, the data crate | `prodrome_todo::todo` |
| `State`, `Spec`, `Content` and their impls of `RegisterType` | `Discrete` of the data crate | `prodrome_todo::todo` |
| `Registers`, its impl of `Product`, `outcomes`, `specs`, `records` and `bind` | the register algebra of the data crate, `Env` and `Candidates` | `prodrome_todo::todo`, re-exported at `prodrome::fold::Registers` |
| `Write`, `Kind` | `Outcome`, `Term`, `Authored` | `prodrome_todo::todo`, re-exported at `prodrome::fold::{Write, Kind}` |

`Registers::bind` names `Env`, and `Write` names `Outcome`, so the cut
needed those two in the terms crate, a move of data. The crate-visible
method `Registers::worlds`, which computes the price of each world with
`mk_flat` and `checklist`, became a private function of the core's
`todo`, because only the impls of `Price` and `History` call it. These
items stay in the core's `todo`:

| Item | Reason |
|---|---|
| the impls of `Price`, `History`, `Row` and `Bind` for `TodoEvent` | The traits are in the core's `schema`, and they move with the reading crate. Rust allows an impl of a local trait for a type of another crate. |
| `todo::Reading` | It is the type of `Row::reading`. |

So the todo schema's events are the crate `prodrome-todo`, on the data
crate and `prodrome-term`, with no dependency on the evaluator:
`event`, `payload`, `reference` (behind the default feature `reference`,
which the core's feature of the same name turns on), and `todo`. The
derived types of `reference` name the data crate with
`#[literal(crate = prodrome_data)]`. A program that writes a todo with a
decaying price reads the data crate, `prodrome-term` and
`prodrome-todo`: 31,870 tokens, and nothing of how a price is computed.
The test `todo/tests/alone.rs` is that program: it builds a decaying
spec, writes three events and an object that holds one, prints them,
parses them back, and checks the object's name against `sha256sum`. Its
only crates of the workspace are those three.

**What is left of the core.** The core holds `fold` (the readings of a
todo, and the re-exports), `schema` (the traits `Price`, `History`,
`Bind` and `Row`), `todo` (the impls of those traits, and `Reading`),
`view`, `laws` behind its feature, and the root doc. Its face is 17,038
tokens by `scripts/face.py --lines`, of which `laws` is 11,220, and 4,802
by the rustdoc measure with the default features (8,624 with `laws`).
The core no longer uses `chrono` itself, so `chrono` is a
dev-dependency.

**The face.** "Before" is the merge base `b32472b`. The rustdoc measure
is with the default features. The crates that this stage does not change
keep their faces: `prodrome-memo` 7,697, `prodrome-declared` 4,151,
`prodrome-dag` 14,424, `prodrome-store` 21,214, `prodrome-tags` 4,036 and
`prodrome-digest` 4,319 by the line measure.

| Crate | Lines, before | Lines, after | Rustdoc, before | Rustdoc, after |
|---|--:|--:|--:|--:|
| `prodrome-core`, without the crates below | 33,374 | 17,038 | 15,141 | 4,802 |
| `prodrome-data` | 19,756 | 19,913 | 12,030 | 12,143 |
| `prodrome-nest` | 4,124 | 4,011 | 2,234 | 2,149 |
| `prodrome-term` | | 6,914 | | 3,649 |
| `prodrome-fpl` | | 5,705 | | 4,290 |
| `prodrome-todo` | | 5,043 | | 3,436 |
| all twelve crates | 113,095 | 114,465 | | |

The crates together are 1,370 tokens more than before by the line
measure: the root docs and module heads of the three new crates and of
`segment`, the lines of the core that re-export, and the marks of the
hidden items. Every crate is under the budget by both measures.

**Which crates a unit reads.** The table has one row for each unit of the
count at the start of this section. Before this stage, each of these
units read `prodrome-core` and the eight crates below it, 113,095
tokens. "All twelve" is `prodrome-core` with the eleven crates below it,
114,465 tokens. A unit that names a constructor at `prodrome::fpl` names
it at `prodrome_term::fpl` instead, and so on for the other paths of
`CHANGELOG.md`.

| Unit | Held by | Crates after | Tokens after |
|---|---|---|--:|
| a chains unit | `event::TodoEvent` | data, term, todo | 31,870 |
| an API unit | `observe::Observation`, `observe::NextChange` | data, term, fpl; its tests also read the core, for `view::entries` | 32,532 |
| a release unit | `TodoEvent`, `mk_completed`, `Term`, `TermF` | data, term, todo | 31,870 |
| a base unit | the evaluator and the terms alone | data, term, fpl | 32,532 |
| a command line | the events, `mk_absent`, `mk_decay`, `mk_flat`, `Term` | data, term, todo | 31,870 |
| an assistant unit | `fulfillment`, `link`, `Payload`, `view::entries` | all twelve | 114,465 |
| a records unit | the events, `todo::TodoVocabulary`, the terms, `view::Entry` | all twelve | 114,465 |
| an inbox unit | `fold::price`, `schema::Price`, the events, the terms | all twelve | 114,465 |
| a signatures unit | the readings of `fold`, the evaluator, `view::entries` | all twelve | 114,465 |
| a todo unit | the evaluator, the events, `view::{Confidence, Provisional, list_order}` | all twelve | 114,465 |
| a browser module | the readings of `fold`, the evaluator, the events | all twelve | 114,465 |

So five units leave the core in this stage, and each reads at most 32,532
tokens in place of 113,095. The eleven units together read 847,464
tokens in place of 1,244,045. Each of the six units that still read the
core names `view` or a reading of `fold` or `schema`, which the next
stage moves. Without its name of `view::entries`, the assistant unit
would read the data crate, `prodrome-term`, `prodrome-fpl` and
`prodrome-todo`: 37,575 tokens.

**The timings.** The benchmark of section 9, in the release profile with
no link-time optimisation, on a generated store of 15,009 objects over
100 todos, about 400 bytes each, with two more operations for this
stage: `fulfillment` 2,000 times over a closed term of 60 nested levels
(each a `Conj` of the level below, a shifted `Periodic` of a `Decay`, and
a `Flat`, under an `Offset`), and `observe::next_change` 20 times over
such a term of 12 levels. Each binary takes the median of ten runs of each
operation. Each run appends the same ten tendings on a warm handle, so
after the first run each append finds its object in the store. The base
binary and the branch binary ran in turn, eleven times each, in five
rounds on one store. The second and the fifth round ran the branch first.
The table gives the median of the 55 medians of each binary, in
milliseconds, and the change in the count of instructions that
`valgrind --tool=cachegrind` gives for one run of the operation.

| Operation | Before | After | Change | Change in instructions |
|---|--:|--:|--:|--:|
| cold read | 135.4 | 132.4 | −2.2 % | −0.03 % |
| `registers::fold` | 28.3 | 28.9 | +2.0 % | −0.00 % |
| `view::entries` | 68.0 | 68.2 | +0.3 % | +0.02 % |
| `Dag::verify` | 39.3 | 39.5 | +0.3 % | +0.11 % |
| append | 25.8 | 25.1 | −2.8 % | not counted |
| `fulfillment` | 23.8 | 24.2 | +1.6 % | −0.34 % |
| `observe::next_change` | 2.78 | 2.89 | +4.2 % | −0.25 % |

The rounds vary by more than these changes: one round alone gave −11
to +12 percent for each operation. `observe::next_change` is the
shortest operation, and its median is over 3 percent. A second run took
the median of 21 runs of `next_change` 200 times, in 20 pairs of the two
binaries: +2.9 percent, with each binary's runs between 21 and 30
milliseconds. The counts of instructions decide it: the branch runs 0.25
percent fewer instructions for `next_change` and 0.34 percent fewer for
`fulfillment`, so the evaluator's loop does no more work across the new
crate boundary. The evaluator calls these functions of `prodrome-term`
for each node or each step, so they are `#[inline]`: `Term::new`,
`Term::out`, `Term::into_out`, `Outcome::at`, `us_of`, `after`, `ratio`,
`div_delta`, `total_seconds`, and `divide_and_round`, which `div_delta`
calls. The functions of `Schedule`, `TermF` and the register algebra are
generic, so the caller compiles them.

**Compile time.** A build of the workspace in the dev profile, with four
cores, twice each, each time in a new target directory, in two rounds.
"Touch" is the build after `touch data/src/lib.rs`, twice after each
clean build.

| Build | Before | After |
|---|--:|--:|
| clean | 24.0 s, 23.9 s, 25.1 s, 25.2 s | 26.1 s, 26.8 s, 25.5 s, 26.7 s |
| touch | 2.3 s, 2.3 s, 2.3 s, 2.3 s, 2.8 s, 2.2 s, 2.5 s, 2.5 s | 2.4 s, 2.4 s, 2.3 s, 2.5 s, 2.4 s, 2.3 s, 2.3 s, 2.3 s |

A clean build takes about 1.5 seconds more. A probable cause is that the
core waits for `prodrome-term` and then for `prodrome-fpl` and
`prodrome-todo`, whose code it compiled as modules of one crate before.
The build after an edit of the data crate does not change.

**The size of the wasm.** `nix build .#prodrome-wasm` gives a module
(`web/prodrome_bg.wasm`) of 1,411,073 bytes at the merge base and
1,415,649 bytes after this stage: 4,576 bytes, or 0.32 percent, more.
The profile `wasm-release` uses fat link-time optimisation, so the
compiler sees the code of every crate. This note did not find which code
the 4,576 bytes are.

**The tests.** The integration tests of the core run unchanged through
the re-exports: 465 tests. The core's unit tests fell from 24 to 14: one
moved to `prodrome-term` (the round trip of `term::schema`) and nine to
`prodrome-todo` (the tests of `event`, `todo`, `write` and `reference`).
`prodrome-fpl` has no unit test; the core's integration tests check the
evaluator's laws through the re-exports. No law was added or changed,
and no property test moved.

**What remains, in order.** The units of the application that still
depend on `prodrome-core` after this stage each name `view`, a reading of
`fold`, or a trait of `schema`:

1. the reading crate: the readings of a todo (`fold::env`, `specs`,
   `content`, `flatten`, `price`, `link_specs`, `member`, `stores` and
   `readings`), `view`, the traits `Price`, `History`, `Bind` and `Row`
   with their impls for `TodoEvent`, and `todo::Reading`. These name the
   data crate, `prodrome-dag` (`fold::read`, `registers::Node`),
   `prodrome-term`, `prodrome-fpl` and `prodrome-todo`, and no module of
   the store. They are about 3,800 tokens by `scripts/face.py --lines`.
   The six units above that still read the core would then read the
   crates below the core and this one.
2. what is left: `laws` behind its feature, the root doc, and the
   re-exports of the facade.

The stages of section 5 named fpl, digest, reading and todo. Digest was
made in section 11, and fpl and the data of todo in this section, so
only the reading crate is left. The impls of the reading traits for
`TodoEvent` go with that crate, because the traits do.

## 13. The readings, in their crate

At the merge base `dc41c12` of this stage, six units of the application of
sections 9 to 12 still depended on `prodrome-core`. Each named `view`, a
reading of `fold` or a trait of `schema`, and the core depended on every
other crate of the library, so each of the six read all twelve crates:
114,465 tokens by `scripts/face.py --lines`. This stage made the crate
`prodrome-reading`, which holds those modules, and left in `prodrome-core`
the law kit and the re-exports.

**The method.** The method is the method of stage 3. Each commit below
builds and passes the three commands of `AGENTS.md`:

1. The cut. The modules `fold`, `schema`, `todo` and `view` name the data
   crate, `prodrome-dag`, `prodrome-term`, `prodrome-fpl` and
   `prodrome-todo` at their own paths, and not through the modules of the
   core that re-export them (see "The edges").
2. The move of the four files to `reading/src/`, whole, with the new
   crate in `README.md` and `docs/development.md`. The CI's lint check
   runs `scripts/edges.py` over `reading/src`.

Other commits answer the review of section 12 (see "The follow-ups"), and
one records the stage in this note and in `CHANGELOG.md`. `prodrome-core` re-exports each module of the
new crate at its old path, so `cli/`, `wasm/`, `github/` and the tests
build with no change of an import. No trait was added and no item was
renamed. Two hidden items were narrowed: `after` and `ratio` became private
(see "The follow-ups").

**The edges.** At the merge base, `python3 scripts/edges.py core/src fold
schema todo view` already printed that no module of the set names a
module outside it. That proves less than in the stages before, because
the other modules of the core are now `pub use` lines in `lib.rs`, which
the script does not read, so it cannot see a use of them. The four
modules named eight of them: `event`, `payload`, `fpl`, `policy`,
`registers`, `schedule`, `term` and `literal`. The cut replaced each such
path with the path in the lower crate. After the cut, the four files name
no path `crate::` outside the set, and the paths that they name outside
the crate are of five crates only:

```console
$ grep -ohE 'prodrome_[a-z]+' core/src/{fold,schema,todo,view}.rs | sort -u
prodrome_dag
prodrome_data
prodrome_fpl
prodrome_term
prodrome_todo
```

The proof that holds after the move is Cargo's: `prodrome-reading`
compiles with those five crates as its only dependencies of the
workspace. Each item that the four modules name is in one of them:

| The core's module | The items that the set names | Their crate |
|---|---|---|
| `event`, `payload` | `TodoEvent`, `TodoId`, `Authored`, `Payload`; `Hash` | `prodrome-todo`; `prodrome-data` |
| `fpl` | `Env`, `FplError`, `Instant`, `Candidates`, `Outcome`, `least_of`, `piecewise`, `mk_absent`, `mk_flat`, `checklist`, `instant_of`, `iso`; `Member`, `Stores`, `Closed`, `LinkError`, `link_in`, `fulfillment` | `prodrome-term`; `prodrome-fpl` |
| `policy` | `Everything`; `Policy` | `prodrome-data`; `prodrome-dag` |
| `registers` | `Stamp`; `Folded`, `Prodrome`, `Genesis`, `Node`, `fold` | `prodrome-data`; `prodrome-dag` |
| `schedule`, `term` | `Schedule`, `Term` | `prodrome-term` |
| `literal` | `Datetime`, `ProdromeError` | `prodrome-data` |

`fold` also re-exports `prodrome_dag::fold`, which holds `read` and the
hidden `scan` and `admits`, and `prodrome_todo::todo::{Kind, Registers,
Write}`. No module of the set names `store`, `snapshot`, `memo`,
`declared`, `tags`, `nest` or `digest`, so the cut moved no item and no
body: it changed only the paths of the imports. `prodrome-reading` does
not depend on `prodrome-memo` itself, but `prodrome-dag` does, because
`sign::Checks` keeps the results of signature checks in a `memo::Lru`. So
a program on the reading crate reads the memo crate too.

**One crate or several.** The face of `prodrome-reading` is 4,077 tokens
by `scripts/face.py --lines`, far under the budget, so the budget asks for
no division. The test is what a program needs together:

- A program that prices (`fold::price`, `flatten`, `specs`, `env`) and a
  program that lists (`view::entries`, `Entry`, `list_order`) are not
  different programs by the edges. `view::entries` calls `fold::stores`,
  `fold::member` and `fold::read`, so every program that lists also
  prices. A crate of the pricing alone would save a program that prices
  only the 1,558 tokens of `view`.
- A program with its own schema and no todos implements `Price`,
  `History`, `Bind` and `Row`. These traits name only `Term`, `Env` and
  `FplError` of `prodrome-term` and `Policy` of `prodrome-dag`, and
  `fold::readings`, `env`, `flatten`, `price`, `link_specs`, `member`,
  `stores` and `view::entries` are generic over the schema. So the
  general part could stand on four crates without `prodrome-todo`. But
  two rules of Rust tie the todo's part to it. The impls of the four
  traits for `TodoEvent` must be in the crate of the traits or in
  `prodrome-todo`, and in `prodrome-todo` they would make that crate
  depend on `prodrome-dag` and the evaluator, which every program that
  only writes a todo's events would then read. And the methods of
  `Entry<TodoEvent<P>>` (`outcome`, `content`, `is_open`, `state`, `at`,
  `claimed`) are inherent methods, which must be in the crate of `Entry`.
  There are two ways to move them out of that crate: as functions, or as
  the methods of a new trait. Each renames items or adds a trait only to
  make a cut.

  A third way keeps the names. The six methods read only `self.reading`
  and `self.claim`, and both have the type `Row::Reading`. So they can be
  one inherent impl that is bounded on the associated type, `impl<E:
  Row<Reading = todo::Reading>> Entry<E>`. The general crate would then
  hold the traits, the generic folds, `view` and this impl, and a small
  crate would hold the four impls of the traits for `TodoEvent`. The cost
  is a fourteenth crate, with its lines in `README.md`,
  `docs/development.md`, the lint check of `scripts/edges.py` and the
  re-exports of `prodrome-core`. The general crate must also keep the
  type `todo::Reading` and `lowered`, because the bound names the type, so
  one type of the todo stays in the general part. The trigger is the first
  program that has a schema of its own and wants the general part without
  the 5,043 tokens of `prodrome-todo`. This stage does not make the cut.
  Until then, the general part and the todo's part are one crate, and a
  program with its own schema reads the 5,043 tokens of `prodrome-todo`
  that it does not use: 63,808 tokens in place of 58,765.

**What is left of the core.** `prodrome-core` holds the root doc, the
re-exports, and `laws` behind its feature. Its face is 13,392 tokens by
`scripts/face.py --lines`, of which `laws` is 11,220, and 1,991 by the
rustdoc measure with the default features (5,813 with `laws`). A program
reads no item of the core to read a price or a row.

`laws` stays in the facade. Its functions check the laws of SPEC §9 over
a store's objects, an event or replicas, and they name `store`
(`MemoryStore`, `EventStore`, `Overlay`, `Replica`, `Held`, `Sender`,
`sync`, `accept`), `snapshot`, `sign`, `memo`, `dag`, `registers` and
`fold::read`. What it tests is the whole library below the readings, not
one crate. The crate closest to it is `prodrome-store`, which depends on
all that it names but `prodrome-memo`. The line measure reads every
source file whatever the features, so `laws` in the store crate would
bring that crate from 21,214 to about 32,400 tokens, over the budget, for
every program that opens a store. A crate of its own would save a program
that runs the kit only the 2,172 tokens of the facade's root doc. A
program that runs the kit names it in its `[dev-dependencies]`, so the
kit costs nothing to the unit that a program ships.

**The face.** "Before" is the merge base `dc41c12`. The rustdoc measure is
with the default features. The crates that this stage does not change
keep their faces: `prodrome-data` 19,913, `prodrome-memo` 7,697,
`prodrome-declared` 4,151, `prodrome-dag` 14,424, `prodrome-store`
21,214, `prodrome-nest` 4,011, `prodrome-tags` 4,036, `prodrome-digest`
4,319 and `prodrome-todo` 5,043 by the line measure.

| Crate | Lines, before | Lines, after | Rustdoc, before | Rustdoc, after |
|---|--:|--:|--:|--:|
| `prodrome-core`, without the crates below | 17,038 | 13,392 | 4,802 | 1,991 |
| `prodrome-core` with `laws` | | | 8,624 | 5,813 |
| `prodrome-reading` | | 4,077 | | 3,211 |
| `prodrome-term` | 6,914 | 6,861 | 3,649 | 3,649 |
| `prodrome-fpl` | 5,705 | 5,793 | 4,290 | 4,290 |
| all thirteen crates | 114,465 | 114,931 | | |

The crates together are 466 tokens more than before by the line measure:
the root doc of the new crate (284), and in the core the lines that
re-export it and the paragraph of the root doc that names it. `after` and
`ratio` moved from `prodrome-term` to `prodrome-fpl` (see "The
follow-ups"), with the module doc of their new file. Every crate is under
the budget by both measures.

A program that reads a todo's price and its row in a list reads the data
crate, `prodrome-memo`, `prodrome-dag`, `prodrome-term`, `prodrome-fpl`,
`prodrome-todo` and `prodrome-reading`: 19,913 + 7,697 + 14,424 + 6,861
+ 5,793 + 5,043 + 4,077 = 63,808 tokens by the line measure, and 36,697
by the rustdoc measure, in place of the 114,465 tokens of all twelve
crates. It reads no file of the store's replicas, tags, nests, digest or
declared schemas. Of the 63,808 tokens, 7,697 are the memo crate, which
the program reads only because `sign::Checks` in `prodrome-dag` names
`memo::Lru`.

**Which crates a unit reads.** The table has one row for each unit of
section 12 that still read the core. Before this stage, each read all
twelve crates, 114,465 tokens.

| Unit | Held to the core by | Crates after | Tokens after |
|---|---|---|--:|
| an assistant unit | `view::entries` | data, memo, dag, term, fpl, todo, reading | 63,808 |
| a records unit | `view::Entry` | data, memo, dag, term, fpl, todo, reading | 63,808 |
| an inbox unit | `fold::price`, `schema::Price`; `payload::spec_field` is `prodrome_todo::payload::spec_field` | data, memo, dag, term, fpl, todo, reading | 63,808 |
| a signatures unit | `fold::{content, env, flatten, link_specs, member, specs}`, `view::entries` | data, memo, dag, term, fpl, todo, reading | 63,808 |
| a todo unit | `view::{Confidence, Provisional, list_order}` | data, memo, dag, term, fpl, todo, reading | 63,808 |
| a browser module | `fold::{env, flatten, link_specs, specs}` (`fold::Write` is `prodrome_todo::todo::Write`) | data, memo, dag, term, fpl, todo, reading | 63,808 |

So no unit of the application needs `prodrome-core` after this stage.
The six units together read 382,848 tokens in place of 686,790. A unit
that names a reading at `prodrome::fold` names it at
`prodrome_reading::fold` instead, and so on for the paths of
`CHANGELOG.md`. An agent that changes `view::entries` reads the file
`reading/src/view.rs` and the faces of the seven crates: 63,808 tokens.
Each of the seven crates is under the budget of 25,000 tokens that the
application gives to one dependency, and the largest, the data crate, is
19,913.

**The follow-ups.** The review of section 12 asked for five changes,
each made in its own commit:

- `after` and `ratio` were public and `#[doc(hidden)]` in `prodrome-term`,
  and only the evaluator calls them. They are private functions of a new
  module `span` of `prodrome-fpl`. `us_of` stays in `prodrome-term`,
  because the print of a span calls it. The doc of `piecewise` names its
  callers: `chain` and `link` in `prodrome-fpl`, and `fold::flatten` in
  `prodrome-reading`.
- `AGENTS.md` named files that earlier stages moved, and named other
  files by a bare name. Each path now names a file from the root of the
  repository. The new script `scripts/doc_paths.py` prints each path in
  backticks in `AGENTS.md`, `CONTRIBUTING.md` and `README.md` that names
  no file, and the CI's lint check runs it.
- `CHANGELOG.md` says that `prodrome-core` no longer turns on chrono's
  feature `clock`, and names the two items of `prodrome-term` that are
  public and hidden.
- The CI's lint check runs `cargo doc` over the libraries with warnings
  denied, with the default features, with all features and with none:
  about 40 seconds from a cold target directory. Five links in the docs
  of the wasm crates were broken or ambiguous, and they are fixed.
- The timing of `observe::next_change`: see "The timings".

**The timings.** The benchmark of section 12, in the release profile with
no link-time optimisation, on a generated store of 15,011 objects over
100 todos, about 250 bytes each. Each binary takes the median of ten runs of
each operation, and each run appends the same ten tendings on a warm
handle. Every run is pinned to one core with `taskset -c 3`. The base
binary and the branch binary ran in turn, eleven times each, in three
rounds on one store; the second round ran the branch first. The table
gives the median of the 33 medians of each binary, in milliseconds, and
the change in the count of instructions that `valgrind --tool=cachegrind`
gives for one run of the operation.

| Operation | Before | After | Change | Change in instructions |
|---|--:|--:|--:|--:|
| cold read | 210.3 | 211.5 | +0.5 % | not counted |
| `registers::fold` | 27.6 | 28.0 | +1.4 % | not counted |
| `view::entries` | 55.2 | 55.9 | +1.2 % | −0.02 % |
| `fulfillment` | 32.8 | 33.4 | +1.7 % | 0 |
| `observe::next_change` | 3.29 | 3.37 | +2.4 % | 0 |
| `Dag::verify` | 39.0 | 40.1 | +2.8 % | not counted |
| append | 377.5 | 375.6 | −0.5 % | not counted |

No operation is slower by more than 3 percent. The two binaries run the
same number of instructions for `fulfillment` (245,227,492) and for
`next_change` (28,887,637), and each function runs the same number in
both. `registers::fold` and `Dag::verify` are in `prodrome-dag`, which
this stage does not change. So the changes of the table are the
variation of the machine and of the layout of the code, which is about
2 to 3 percent for these operations.

**The timing of `next_change`.** Section 12 measured
`observe::next_change` 4.2 percent slower, then 2.9 percent, with 0.25
percent fewer instructions. This stage measured it again, pinned to one
core, against the binary of `b32472b`, the merge base of section 12. Each
binary takes the median of ten runs of `next_change` 20 times over a term
of 12 levels.

| Run | `b32472b` | This stage | Change |
|---|--:|--:|--:|
| 1: 101 medians of each binary, in alternating pairs | 3.245 ms | 3.358 ms | +3.5 % |
| 2: 101 medians of each binary, in triples with the third binary below | 3.230 ms | 3.287 ms | +1.8 % |

The quartiles of the 101 medians of each binary overlap in both runs: in
run 1, 3.181 to 3.803 ms and 3.278 to 3.684 ms. The instructions decide
the question:

- This stage runs 28,887,637 instructions for one run of `next_change`,
  and `b32472b` runs 28,973,897, which is 0.30 percent more. The binary
  of the merge base `dc41c12` runs exactly as many as this stage.
- The functions differ in what the compiler inlined. At this stage
  `TermF::children` is a function of its own (51,840 instructions and
  6,480 indirect branches, which are its calls and returns), where at
  `b32472b` it was inlined into its callers. In exchange, the drop of a
  `TermF<()>`, a growth of a vector of floats and the collection of a set
  of instants are inlined or gone, so the total is lower.
- The simulated predictor of cachegrind gives at this stage 8.8 percent
  more mispredicted indirect branches than at `b32472b`, but the increase
  is in functions whose code is the same in both binaries, such as
  `breaks::local` and `Term::try_cata`. Such a change follows from the
  addresses of the code, not from the work.
- A third binary, this stage with `#[inline]` on `TermF::children`, ran
  in run 2. It runs 29,042,833 instructions and its median was 3.282 ms,
  1.6 percent over `b32472b`, as this stage is. So the call to
  `children` does not cost the difference.

So `next_change` does no more work than at `b32472b`, and no call that the
compiler stopped inlining explains its time. The difference, between 1.8
and 3.5 percent in two runs of the same method, is the layout of the code.
No `#[inline]` was added.

**Compile time.** `cargo build --workspace --all-targets -j 4` in the dev
profile, twice each, each time in a new target directory, in two rounds.
"Touch" is the build after `touch data/src/lib.rs`, twice after each
clean build. These builds include every test target, so they are longer
than the builds of section 12.

| Build | Before | After |
|---|--:|--:|
| clean | 164.8 s, 156.9 s | 159.2 s, 155.3 s |
| touch | 19.3 s, 17.1 s, 26.0 s, 22.0 s | 23.7 s, 23.4 s, 22.7 s, 24.9 s |

A clean build does not change. The build after an edit of the data crate
may take about 2 seconds more, the median of 23.6 seconds in place of
20.7, which is inside the spread of the runs before. A probable cause is
that the core and its tests now wait for `prodrome-reading`, which waits
for `prodrome-fpl` and `prodrome-todo`.

**The size of the wasm.** `nix build .#prodrome-wasm` could not run in the
environment of this stage, because it could not fetch the flake's input
`crane`. So a script ran the same steps by hand, with the tools of the
flake's nixpkgs: `cargo build --profile wasm-release --target
wasm32-unknown-unknown -p prodrome-wasm`, then `wasm-bindgen --target
web` 0.2.127, then `wasm-opt -Os` with the features that `flake.nix`
names. The module `prodrome_bg.wasm` is 1,416,276 bytes at the merge
base and 1,416,398 bytes after this stage: 122 bytes, or 0.01 percent,
more. The module that cargo writes, before `wasm-bindgen`, is 1,637,402
and 1,637,535 bytes.

**The tests.** The integration tests of the core run unchanged through the
re-exports: 465 tests, and one long run that is ignored by default. The
14 unit tests of `fold` and `view` moved with them, so the core has no
unit test and `prodrome-reading` has 14. They name the lower crates in
place of the core's modules, and their assertions do not change. The doc
test of the core's root runs in the core. No law was added or changed,
and no property test moved.

**What remains of issue #92.** The issue asks for three things: shorter
doc comments, a narrower public face, and a division of the core into
crates in the order of its modules. Stage 1 shortened the doc comments.
The stages of sections 8 to 13 divided the core, and every crate is under
the budget. No unit of the application needs the core, and the core holds
nothing that a program must read. Two things remain:

1. The narrowing of section 3. Stage 3 narrowed four items, and each
   later stage narrowed none, because each moved items whole. Section 3
   listed 46 items that only other modules used, and items that only the
   tests or `cli/` used. Many of them are now in other crates, some as
   `#[doc(hidden)]` items that the crate above calls. Nobody has decided
   item by item which of them a program outside the library can need.
   The line measure counts a hidden item, so only the loss of `pub`
   removes one from it.
2. The release that publishes the crates, which is its own decision.

So this stage advances the issue and does not complete it.

## 14. What is public, crate by crate

Section 3 listed, before the crates existed, the public items that no
outside program needs, and stage 3 narrowed four of them. Each later stage
moved items whole, so every public item of the one crate became a public
item of twelve, and the crates above made some items `pub` and
`#[doc(hidden)]` to cross a boundary. This section reviews every public
item of the thirteen crates, one by one, and narrows what no outside program
needs. It is the second item of the list at the end of section 13.

**Which program is an outside program.** A program is outside when it is not
a crate of the library. The evidence of a need is one of these:

- A program of this repository names the item: the command line (`cli/`),
  the GitHub program (`github/`), the wasm crates (`wasm/`, `wasm/exports/`)
  or an example (`core/examples/`).
- The specification, the guide or a doc test names it. The release notes
  in `CHANGELOG.md` are history: they name items that no longer exist, so
  they are not a caller.
- The author of another schema, or of another backend for a replica, needs
  it to do that work.

The crates of the library, the law kit (`core/src/laws`) and the tests of
this repository are not evidence. A test of this repository can still need
an item, and no change of this section removes an item that a test names.

### The method

`scripts/outside.py` finds who names each item. It lists the items that
rustdoc reports for each crate (with `--document-hidden-items`), copies the
tracked files to a new directory, and puts
`#[deprecated(note = "@@N")]` before each item. It then compiles every
target of the workspace in the copy, with every feature. The compiler warns
at every place that names a deprecated item, and the note of the warning
gives the item. The script sorts each place by its file. A use inside the
item's own crate does not count. Run it from the root of the repository:

```console
$ python3 scripts/outside.py --summary
$ python3 scripts/outside.py
```

The first form prints a count for each crate, and the second prints one
line for each item. The compiler does not warn at a use in code that
`#[derive(Literal)]` writes, so the script adds the names that the derive's
source writes under `literal::codec`. The script does not read documents. A
first run, in which the script narrowed each item to `pub(crate)` and the
compiler listed the errors, agreed on the narrowed items, and showed one case that the deprecation form
handles better:
`Decision::dag` is shadowed by a trait method of the same name when it is
narrowed, so a narrowing alone does not show a use. The deprecation form
does not have this fault, because it does not change which method a call
reaches. A search of `SPEC.md`, `docs/guide.md` and `README.md` for the name of
each narrowed and each new hidden item found none; the type `KeyAdded` is
named, and its two conversions are not.

### The rules

Each item has one decision:

- **Keep.** The item is part of the face. The reason is one of: a program
  of this repository names it; a document names it; it completes a type
  that a program uses (an accessor, a constructor or a conversion of a
  public type); the author of a schema or a backend implements or calls it;
  or it appears in the signature of a public item, so that a caller can
  name it.
- **Narrow.** No code outside the crate names the item, tests included, the
  crate itself calls it, and no outside program needs it. It becomes
  `pub(crate)`. An item that nothing calls at all is not narrowed, because
  an unused `pub(crate)` item is dead code. These items are functions of
  the language that SPEC §7 defines (`fpl::power_mean`, `fpl::offset`,
  `fpl::phase`, `fpl::min_fulfillment`, `fpl::member_shares`), or helpers
  that a payload author calls, and each stays public with that reason.
- **Cross-crate only.** Another crate of the library, the derive or a test
  must name the item, and no outside program should. It stays `pub`, and it
  carries the rule below.

**The rule for an item that crosses a crate boundary.** The item is `pub`
and `#[doc(hidden)]`, and its doc starts with "For this library's own code;
no promise." The sentence replaces "For the core" and "For the evaluator",
which named crates that are no longer the callers. The rule is one rule for
every crate. A hidden item is not part of the face of rustdoc, and it may
change in any release of the library. The crates release together with
exact version requirements, so no program that follows the rule sees a
change. 59 items are hidden, 27 of them methods and
32 functions, types and aliases. Ten of them are new in this stage.
They are the helpers that `#[derive(Literal)]` calls (`codec::read`,
`read_or_default`, `call_of`, `headed_len`, `headed_names`,
`headed_table`, `names_field`, `distinct`, `Site` and `Reader`), which
section 9 had kept public and documented "For the derive".

**Would a private shared crate remove the need?** No. A crate that only the
library depends on would hold the free functions and the types of the list
below (32 items) with no hidden mark, but crates.io requires every
dependency of a published crate to be published, so the shared crate is
public too. Its items stay `pub`, and its name and its documentation would
carry the "no promise" instead of an attribute. It cannot hold the
27 methods, because an inherent method must be in the crate of its
type, so the types would have to move into it, or their private fields
would become public. `Stamp::placed` and the methods of `Dag`, `Reach`,
`Folded`, `Heads` and `BitSet` are such methods. The cost is a fourteenth
crate that every crate depends on, a move of the types that the methods
belong to, and a face that a reader of the moved types reads in the shared
crate and not in the crate that names them. The review does not recommend
it. The rule above costs one attribute and one sentence for each item.

### What is public, by crate

The tables list every item of each crate. A row names an item, or several
items that have the same decision and the same reason. A number in
parentheses, as in `Hash` (+6), counts the members of a type: its
methods, associated constants, fields and variants. Where members differ
from the type, a clause names the classes: P is named by a program of this
repository, L by another crate, the derive or the law kit, T by tests only,
and N by nobody outside the crate. A member that is hidden or narrowed has
its own row. The column "Who names them" lists the programs, crates and
tests that the script found. Where a kept item has no caller, the column
says "none" and names the outside author for whom it is kept. "core tests" is `core/tests/`. A program
`wasm` or `wasm exports` is `wasm/` or `wasm/exports/`.

A module is public when one of its items is, so the tables list no module.
The macro `text_literal!` of `prodrome-data` is kept because the wasm
exports call it. The macro `word_literal!` is kept for the author of a
schema whose type prints as one word of a closed set, the pair of
`text_literal!`; only tests call it. The hidden
macro `newtype_str!` crosses to `prodrome-todo`.


### `prodrome-data`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `bitset` | `BitSet` (+5: 5 L) | cross-crate only | prodrome-dag | Another crate of the library calls it; no outside program should. |
| `change` | `mk_change` | keep | examples, wasm, wasm exports | Named by a program of this repository. |
| `change` | `Change` | keep | core tests, prodrome-dag, prodrome-store | The change object (SPEC §3). |
| `event` | `Hash` (+6), `Envelope` (+14), `parents_of`, `mk_sealed`, `mk_woven`, `EventVocabulary` (+1), `canonical`, `canonical_envelope`, `seal_hash`, `parse_envelope`, `parse_event` | keep | examples, github program, wasm, wasm exports | Named by a program of this repository. |
| `event` | `rests_on` | cross-crate only | prodrome-dag, prodrome-store | Another crate of the library calls it; no outside program should. |
| `event` | `sorted_distinct` | cross-crate only | prodrome-nest | Another crate of the library calls it; no outside program should. |
| `event` | `ENVELOPE_SIGNATURES` | keep | core tests, prodrome-declared, prodrome-todo | The constructors of an envelope with their fields (SPEC §3); a test pins them. |
| `event` | `field` | cross-crate only | prodrome-todo | Another crate of the library calls it; no outside program should. |
| `event` | `hashes_value` | cross-crate only | prodrome-nest | Another crate of the library calls it; no outside program should. |
| `event` | `hashes` | cross-crate only | prodrome-nest | Another crate of the library calls it; no outside program should. |
| `event` | `event_id` | keep | core tests | A stored format or a name (SPEC §3, §4). |
| `event/lifecycle` | `Lifecycle` | keep | prodrome-todo | A record of the todo's lifecycle events (SPEC §4). |
| `fold/climb` | `Climb` (+8: 4 L, 4 T) | keep | core tests, prodrome-declared, wasm tests | The register algebra (SPEC §6): the author of a schema of one's own calls it. Its members complete the type. |
| `fold/frontier` | `Frontier` (+6) | keep | examples | Named by a program of this repository. |
| `fold/order` | `Discrete` (+1) | keep | examples | Named by a program of this repository. |
| `fold/order` | `Order` | keep | core tests, prodrome-declared, prodrome-nest, prodrome-tags, wasm tests | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `fold/order` | `Total` | keep | core tests, prodrome-declared, wasm tests | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `fold/order` | `Inflationary` | keep | core tests, prodrome-declared, wasm tests | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `fold/order` | `grows` | keep | core tests, prodrome-declared | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `fold/order` | `maximal` | keep | core tests | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `fold/product` | `Product` | keep | examples, wasm, wasm exports | Named by a program of this repository. |
| `fold/register` | `Register`, `RegisterType` | keep | examples | Named by a program of this repository. |
| `fold/register` | `GrowSet` | keep | prodrome-todo | The register algebra (SPEC §6). |
| `fold/sole` | `Only` | keep | wasm exports | Named by a program of this repository. |
| `fold/sole` | `Sole` | keep | core tests | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `fold/sole` | `Hold` | keep | core tests | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `fold/sole` | `One` | keep | core tests, prodrome-tags, tests of prodrome-data | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `fold/sole` | `Whole` | keep | core tests, tests of prodrome-data | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `fold/sole` | `Bodies` | keep | core tests, prodrome-tags | The register algebra (SPEC §6): the author of a schema of one's own implements it. |
| `genesis` | `mk_genesis` | keep | examples, wasm, wasm exports | Named by a program of this repository. |
| `genesis` | `Genesis` | keep | none | Appears in the signature of `mk_genesis`. |
| `husk` | `Husking`, `HuskObject` (+6), `Other` (+1), `Sender` (+2) | keep | examples, wasm, wasm exports | Named by a program of this repository. |
| `husk` | `Husk` (+5: 1 L, 4 N) | keep | prodrome-store | The husk format (SPEC §3): a second backend reads and writes husks. Its members complete the type. |
| `husk` | `NotAHusk` | keep | prodrome-store | The husk format (SPEC §3): a second backend reads and writes husks. |
| `husk` | `Stored` (+4: 1 L, 1 N) | keep | core tests, prodrome-dag, prodrome-store | The husk format (SPEC §3): a second backend reads and writes husks. Its members complete the type. |
| `husk` | `Hashed` (+3: 1 L) | keep | prodrome-dag, prodrome-store | The husk format (SPEC §3): a second backend reads and writes husks. |
| `instant` | `Instant`, `FplError` (+1), `instant_of`, `datetime_of`, `iso`, `parse_iso` | keep | cli, examples, github program, wasm, wasm exports | Named by a program of this repository. |
| `literal` | `ProdromeError` (+8), `Datetime` (+9), `Call` (+4), `Value` (+16), `Signature` (+4), `Vocabulary`, `Table` (+2), `Open`, `print_literal`, `parse_literal` | keep | cli, examples, github program, wasm, wasm exports | Named by a program of this repository. |
| `literal` | `Int` (+5: 1 N, 1 P, 3 T) | keep | core tests | The values of SPEC §2. Its members complete the type. |
| `literal` | `Finite` (+2: 1 L, 1 P) | keep | core tests, prodrome-term | The values of SPEC §2. |
| `literal` | `Timedelta` (+7: 2 L, 1 N, 3 P, 1 T) | keep | core tests, prodrome-term | The values of SPEC §2. Its members complete the type. |
| `literal` | `print_str` | keep | core tests | The values of SPEC §2. |
| `literal/codec` | `Literal`, `no_arm`, `text` | keep | examples, wasm exports | Named by a program of this repository. |
| `literal/codec` | `Constructor` | keep | core tests, prodrome-tags, prodrome-todo, tests of prodrome-data, the derive | The hand-written part of the codec; a schema of one's own uses it when it does not derive. |
| `literal/codec` | `record` | keep | core tests, the derive | The hand-written part of the codec; a schema of one's own uses it when it does not derive. |
| `literal/codec` | `headed` | keep | core tests, prodrome-todo, the derive | The hand-written part of the codec; a schema of one's own uses it when it does not derive. |
| `literal/codec` | `Fields` (+5: 3 L, 2 T) | keep | core tests, prodrome-tags, tests of prodrome-data | The hand-written part of the codec; a schema of one's own uses it when it does not derive. Its members complete the type. |
| `literal/codec` | `arm` | keep | core tests | The hand-written part of the codec; a schema of one's own uses it when it does not derive. |
| `literal/codec` | `word` | keep | core tests | The hand-written part of the codec; a schema of one's own uses it when it does not derive. |
| `literal/codec` | `tuple` | keep | none; kept for the author of a hand-written codec: reads a tuple | A read helper of the hand-written codec; the derive's output does not call it. |
| `literal/codec/blank_field` | `to_value` | keep | none | The derive's option `with` names it. |
| `literal/codec/pinned` | `assert_pinned` | keep | core tests, wasm tests | A test helper that pins a type's constructors. |
| `literal/codec/record` | `call` | keep | examples | Named by a program of this repository. |
| `literal/codec/record` | `Record` | keep | core tests, prodrome-dag, prodrome-todo, the derive | The trait that the derive implements for a record. |
| `literal/codec/record` | `Site` | cross-crate only | none | A parameter or result type of the derive's helpers. |
| `literal/codec/record` | `Reader` | cross-crate only | none | A parameter or result type of the derive's helpers. |
| `literal/codec/record` | `required` | narrow | none | Only its own crate names it, and the crate calls it; no outside program needs it. |
| `literal/codec/record` | `read` | cross-crate only | prodrome-todo, the derive | `#[derive(Literal)]` writes calls of it; no other program should. |
| `literal/codec/record` | `read_or_default` | cross-crate only | the derive | `#[derive(Literal)]` writes calls of it; no other program should. |
| `literal/codec/record` | `call_of` | cross-crate only | the derive | `#[derive(Literal)]` writes calls of it; no other program should. |
| `literal/codec/sum` | `Sum` | keep | the derive | The trait that the derive implements for a sum. |
| `literal/codec/sum` | `headed_len` | cross-crate only | the derive | `#[derive(Literal)]` writes calls of it; no other program should. |
| `literal/codec/sum` | `headed_names` | cross-crate only | the derive | `#[derive(Literal)]` writes calls of it; no other program should. |
| `literal/codec/sum` | `headed_table` | cross-crate only | the derive | `#[derive(Literal)]` writes calls of it; no other program should. |
| `literal/codec/sum` | `names_field` | cross-crate only | the derive | `#[derive(Literal)]` writes calls of it; no other program should. |
| `literal/codec/sum` | `distinct` | cross-crate only | the derive | `#[derive(Literal)]` writes calls of it; no other program should. |
| `literal/codec/text_field` | `to_value` | keep | none | The derive's option `with` names it. |
| `literal/codec/text_field` | `from_field` | keep | none | The derive's option `with` names it. |
| `literal/printable` | `UNICODE_VERSION`, `digest` | keep | wasm exports | Named by a program of this repository. |
| `literal/printable` | `is_printable` | keep | core tests | The printable set of SPEC §2. |
| `payload` | `string_field`, `datetime_field`, `tuple_field` | keep | examples | Named by a program of this repository. |
| `payload` | `required` | keep | core tests, prodrome-declared, prodrome-todo, wasm tests | A field read for the author of a payload (SPEC §4). |
| `payload` | `as_string` | keep | core tests | A field read for the author of a payload (SPEC §4). |
| `payload` | `string_or_empty` | keep | prodrome-todo | A field read for the author of a payload (SPEC §4). |
| `payload` | `bool_field` | keep | prodrome-declared | A field read for the author of a payload (SPEC §4). |
| `payload` | `tuple_or_empty` | keep | none; kept for the author of another payload: reads a tuple field | A read helper for the author of a payload. |
| `payload` | `strings` | keep | core tests, wasm tests | A field read for the author of a payload (SPEC §4). |
| `payload` | `expect_call` | keep | none; kept for the author of another payload: reads a nested call | A read helper for the author of a payload. |
| `policy` | `Standing` (+5), `Everything`, `Untrusted` (+3) | keep | cli, examples, github program, wasm exports | Named by a program of this repository. |
| `schema` | `Schema` | keep | examples, wasm exports | Named by a program of this repository. |
| `schema/headed` | `Event` (+3: 2 L, 1 P) | keep | core tests, prodrome-tags, tests of prodrome-data | The headed schema: an author of a schema of one's own builds events with it. |
| `schema/headed` | `Body` | keep | core tests, prodrome-tags, tests of prodrome-data | The headed schema: an author of a schema of one's own builds events with it. |
| `schema/headed` | `KeyFields` | keep | prodrome-tags | The headed schema: an author of a schema of one's own builds events with it. |
| `schema/headed` | `Constructors` | keep | core tests, prodrome-tags, tests of prodrome-data | The headed schema: an author of a schema of one's own builds events with it. |
| `segment` | `Segment` (+2: 2 L) | keep | core tests, prodrome-nest, prodrome-term | A path segment, with its rule for valid text; `prodrome-nest` and `prodrome-term` name it. |
| `sign` | `KeyAdded` (+5) | keep | wasm exports | Named by a program of this repository. |
| `sign` | `bytes` | cross-crate only | prodrome-dag | Another crate of the library calls it; no outside program should. |
| `sign` | `Signed` (+5: 2 L) | keep | core tests, law kit, prodrome-dag | The signature objects (SPEC §3, §5). |
| `sign` | `KeyRevoked` | keep | none | Appears in the signature of `mk_key_revoked`. |
| `sign` | `mk_key_revoked` | keep | core tests, law kit | The signature objects (SPEC §3, §5). |
| `sign` | `public_of` | cross-crate only | prodrome-dag | Another crate of the library calls it; no outside program should. |
| `sign` | `signed_by` | cross-crate only | prodrome-dag | Another crate of the library calls it; no outside program should. |
| `snapshot` | `Snapshot` | keep | prodrome-store | The snapshot object (SPEC §3). |
| `snapshot` | `mk_snapshot` | keep | core tests, law kit, prodrome-store | The snapshot object (SPEC §3). |
| `stamp` | `Stamp` (+7) | keep | examples | Named by a program of this repository. |
| `stamp` | `Place` (+4: 1 L) | cross-crate only | prodrome-dag | Another crate of the library calls it; no outside program should. |
| `stamp` | `Scope` | cross-crate only | prodrome-dag | Another crate of the library calls it; no outside program should. |
| `topo` | `linear` | keep | law kit, prodrome-dag, prodrome-digest | The one causal order (SPEC §3). |
| `topo` | `Topo` (+2: 2 L) | cross-crate only | prodrome-fpl | Another crate of the library calls it; no outside program should. |

### `prodrome-memo`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `balance` | `WIDEST` | keep | core tests | The balance of a tree (design §6.2); the digest calls it. |
| `balance` | `balance` | keep | core tests, prodrome-digest | The balance of a tree (design §6.2); the digest calls it. |
| `cache` | `Keep`, `Lru` (+1), `Cache` (+13) | keep | examples | Named by a program of this repository. |
| `cache` | `Key` | keep | core tests, law kit, prodrome-dag | The caches (design §6.2). |
| `cache` | `Finding` | keep | core tests | The caches (design §6.2). |
| `cache` | `Lookup` (+4: 1 T) | keep | core tests, law kit, prodrome-dag | The caches (design §6.2). Its members complete the type. |
| `cache` | `Evict` | keep | core tests | The caches (design §6.2). |
| `file` | `File` (+1) | keep | examples | Named by a program of this repository. |
| `file` | `SLACK` | keep | core tests | The cache file (SPEC §6). |
| `file` | `BATCH` | keep | core tests | The cache file (SPEC §6). |
| `fold` | `Report` (+3), `fold` | keep | examples | Named by a program of this repository. |
| `fold` | `Tree` | keep | core tests, prodrome-digest, prodrome-nest | The cached fold (design §6.2). |
| `fold` | `Algebra` | keep | core tests, prodrome-digest | The cached fold (design §6.2). |
| `fold` | `Outcome` | keep | core tests | The cached fold (design §6.2). |
| `fold` | `Visit` | keep | none | Appears in the signature of `Report::visits`. |
| `fold` | `Counts` | keep | none | Appears in the signature of `Report::by_depth`. |
| `front` | `Front` (+1) | keep | examples | Named by a program of this repository. |
| `holder` | `Holder` | keep | core tests | The interface of a cache holder (law 57). |
| `holder` | `Memory` | keep | core tests | The interface of a cache holder (law 57). |
| `lib` | `name` | keep | examples | Named by a program of this repository. |

### `prodrome-dag`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `dag` | `Dag` (+33) | keep | examples, github program, wasm exports | Named by a program of this repository. |
| `dag` | `Prints` | cross-crate only | prodrome-store | Another crate of the library calls it; no outside program should. |
| `dag` | `decode` | keep | core tests, prodrome-store | A second backend decodes a stored object with it (SPEC §3). |
| `dag` | `lacked_among` | cross-crate only | prodrome-store | Another crate of the library calls it; no outside program should. |
| `dag/finding` | `Finding` (+18), `Excluded` (+3), `Unread` (+5) | keep | cli, wasm exports | Named by a program of this repository. |
| `dag/heads` | `Heads` (+4: 2 L, 2 P) | keep | core tests, law kit, prodrome-store | The record of heads that a store keeps (SPEC §3). |
| `dag/reach` | `Reach` (+4: 1 L, 1 N, 2 T) | keep | prodrome-store | The reach set of a frontier; a second backend answers frontiers with it. Its members complete the type. |
| `fold` | `read` | keep | wasm, wasm exports | Named by a program of this repository. |
| `fold` | `scan` | cross-crate only | prodrome-reading | Another crate of the library calls it; no outside program should. |
| `fold` | `admits` | cross-crate only | prodrome-reading | Another crate of the library calls it; no outside program should. |
| `policy` | `Policy`, `Proven` (+4) | keep | examples, wasm, wasm exports | Named by a program of this repository. |
| `registers` | `Folded` (+10), `Genesis`, `Prodrome`, `Node` (+5), `fold`, `deps_for` | keep | cli, examples, github program, wasm, wasm exports | Named by a program of this repository. |
| `registers` | `Elsewhere` | keep | none | Appears in the signature of `Folded::insert`. |
| `registers` | `extend` | keep | core tests, law kit | The fold of the DAG (SPEC §6.6), which a program that reads registers calls. |
| `registers` | `since` | keep | core tests | The fold of the DAG (SPEC §6.6), which a program that reads registers calls. |
| `registers` | `heads` | keep | core tests, law kit | The fold of the DAG (SPEC §6.6), which a program that reads registers calls. |
| `sign` | `Secret` (+4), `Registrar` (+2), `Proof` (+5) | keep | wasm exports | Named by a program of this repository. |
| `sign` | `Proved` | keep | core tests | The proof of signatures (SPEC §5). |
| `sign/checks` | `Checks` (+2: 1 L, 1 T) | keep | core tests, law kit | The cache of signature checks; a host that proves signatures passes one. Its members complete the type. |
| `spread` | `spread` | cross-crate only | prodrome-store | Another crate of the library calls it; no outside program should. |

### `prodrome-declared`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `admit` | `Law` (+9: 1 N) | keep | core tests, tests of prodrome-declared | The laws that a declaration must keep (design §6.0). Its members complete the type. |
| `admit` | `Refusal` | keep | none | Appears in the signature of `Form::parse` and `Declaration::admit`. |
| `event` | `Datum` (+6), `Declared` (+4) | keep | wasm, wasm exports | Named by a program of this repository. |
| `event` | `Key` | keep | none; kept for the second program that reads a declared schema: names a declared key | The events of a declared schema. |
| `event` | `Registers` | keep | none; kept for the second program that reads a declared schema: names its registers | The events of a declared schema. |
| `language` | `Form` (+5: 2 T) | keep | core tests | The syntax tree of a declaration; a program that reads a declaration walks it. Its members complete the type. |
| `language` | `Event` | keep | core tests | The syntax tree of a declaration; a program that reads a declaration walks it. |
| `language` | `Field` | keep | core tests | The syntax tree of a declaration; a program that reads a declaration walks it. |
| `language` | `Type` | keep | core tests | The syntax tree of a declaration; a program that reads a declaration walks it. |
| `language` | `Poset` | keep | core tests | The syntax tree of a declaration; a program that reads a declaration walks it. |
| `language` | `Register` | keep | core tests | The syntax tree of a declaration; a program that reads a declaration walks it. |
| `language` | `Write` | keep | core tests | The syntax tree of a declaration; a program that reads a declaration walks it. |
| `lib` | `Declaration` (+9), `Slot` (+1) | keep | wasm exports | Named by a program of this repository. |
| `order` | `Valued` (+1: 1 P) | keep | none | Appears in the signature of `Declaration::value`. |

### `prodrome-store`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `snapshot` | `Attestation` | keep | law kit | The attestation of a snapshot (SPEC §3). |
| `snapshot` | `attest` | keep | law kit | The attestation of a snapshot (SPEC §3). |
| `store` | `EventStore` (+27) | keep | cli, examples, wasm exports | Named by a program of this repository. |
| `store/decision` | `Decision` (+5: 2 N, 3 T) | keep | core tests | The decision of an append (SPEC §3). Its members complete the type. |
| `store/decision` | `Stopped` | keep | core tests | The decision of an append (SPEC §3). |
| `store/doors` | `Doors` (+2) | keep | examples | Named by a program of this repository. |
| `store/frontier` | `Since` (+2), `KEPT`, `Frontiers` (+3), `Lacking` (+4) | keep | examples | Named by a program of this repository. |
| `store/frontier` | `Token` (+3: 2 N, 1 T) | keep | core tests | The frontier tokens of a door (SPEC §3). Its members complete the type. |
| `store/frontier` | `NotAToken` | keep | none | Appears in the signature of `Token::parse`. |
| `store/frontier` | `Reached` | keep | core tests | The frontier tokens of a door (SPEC §3). |
| `store/frontier` | `Unrecorded` | keep | core tests | The frontier tokens of a door (SPEC §3). |
| `store/frontier` | `Unanswered` | keep | none | Appears in the signature of `Frontiers::answer`. |
| `store/frontier` | `Records` | keep | core tests | The frontier tokens of a door (SPEC §3). |
| `store/frontier` | `Recorded` | keep | core tests | The frontier tokens of a door (SPEC §3). |
| `store/memory_store` | `MemoryStore` (+2) | keep | examples, wasm exports | Named by a program of this repository. |
| `store/overlay` | `Overlay` (+5: 3 L, 1 N, 1 T) | keep | core tests, law kit | The overlay of a store (SPEC §3). Its members complete the type. |
| `store/replica` | `Replica` | keep | examples, wasm exports | Named by a program of this repository. |
| `store/replica` | `Held` | keep | core tests, law kit | The replica interface of a store (SPEC §3). |
| `store/replica` | `accept` | keep | core tests, law kit | The replica interface of a store (SPEC §3). |
| `store/replica` | `sync` | keep | core tests, law kit | The replica interface of a store (SPEC §3). |

### `prodrome-tags`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `alias` | `Alias` (+2), `AliasEvent`, `AliasVocabulary` | keep | wasm exports | Named by a program of this repository. |
| `alias` | `OfItself` | keep | core tests | The aliases of a tag (law 48). |
| `alias` | `aliased` | keep | core tests | The aliases of a tag (law 48). |
| `alias` | `unaliased` | keep | core tests | The aliases of a tag (law 48). |
| `lib` | `Tagging` | keep | wasm exports | Named by a program of this repository. |
| `membership` | `Presence` (+2), `Pair` (+2), `MembershipEvent`, `MembershipVocabulary`, `added`, `removed` | keep | wasm exports | Named by a program of this repository. |
| `membership` | `Membership` (+1: 1 P) | keep | core tests | The membership events of a tag (law 48). |
| `set` | `TagSet` (+10: 10 T) | keep | core tests | The tag set (law 48). Its members complete the type. |
| `set` | `Disagreement` | keep | core tests | The tag set (law 48). |

### `prodrome-nest`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `heads` | `Heads` (+4: 4 T) | keep | core tests | The heads of a nest (design §6.3). Its members complete the type. |
| `heads` | `Nests` | keep | core tests | The heads of a nest (design §6.3). |
| `heads` | `pointer` | keep | core tests | The heads of a nest (design §6.3). |
| `history` | `History` (+12: 12 T) | keep | core tests | The history monad of a nest (design §6.3). Its members complete the type. |
| `level` | `Nest` (+6: 1 N, 5 T) | keep | core tests | The levels of a nest (design §6.3). Its members complete the type. |
| `level` | `Level` | keep | core tests | The levels of a nest (design §6.3). |
| `level` | `Leaf` (+1: 1 T) | keep | core tests | The levels of a nest (design §6.3). Its members complete the type. |
| `level` | `Holding` (+2: 2 T) | keep | core tests | The levels of a nest (design §6.3). Its members complete the type. |
| `level` | `Set` (+1: 1 T) | keep | core tests | The levels of a nest (design §6.3). Its members complete the type. |
| `path` | `Path` (+6: 3 N, 3 T) | keep | core tests | The path of a nest (design §6.3). Its members complete the type. |

### `prodrome-digest`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `lib` | `Measure`, `Leaf` (+4), `Node` (+7), `Tree` (+5), `tree`, `Measured`, `Part` (+3), `View` (+3), `pending`, `view` | keep | examples | Named by a program of this repository. |
| `lib` | `Cycle` | keep | core tests | The summary tree (design §6.5). |
| `lib` | `read` | keep | core tests | The summary tree (design §6.5). |
| `lib` | `zoom` | keep | core tests | The summary tree (design §6.5). |
| `waters` | `Waters` (+4) | keep | examples | Named by a program of this repository. |

### `prodrome-term`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `fpl` | `PRIORITY_POWER` | keep | examples | Named by a program of this repository. |
| `fpl/axis` | `FIRST` | keep | tests | The first and last instant of the time axis (SPEC §7, "The time axis"). |
| `fpl/axis` | `LAST` | keep | prodrome-fpl, tests | The first and last instant of the time axis (SPEC §7, "The time axis"). |
| `fpl/axis` | `advance` | cross-crate only | prodrome-fpl, tests | Another crate of the library calls it; no outside program should. |
| `fpl/axis` | `retreat` | cross-crate only | prodrome-fpl | Another crate of the library calls it; no outside program should. |
| `fpl/axis` | `delta_of_us` | cross-crate only | prodrome-fpl | Another crate of the library calls it; no outside program should. |
| `fpl/construct` | `mk_flat`, `mk_decay`, `mk_curve`, `mk_conj`, `mk_least`, `mk_offset`, `mk_gate`, `mk_shift`, `mk_within`, `mk_importance`, `mk_offset_by`, `mk_after`, `mk_recur`, `mk_periodic`, `mk_ref`, `mk_ref_in`, `mk_absent`, `mk_piecewise` | keep | cli, examples, github program, wasm exports | Named by a program of this repository. |
| `fpl/construct` | `least_of` | keep | prodrome-fpl, prodrome-reading | The smart constructors of terms (SPEC §7). |
| `fpl/construct` | `checklist` | keep | prodrome-reading | The smart constructors of terms (SPEC §7). |
| `fpl/construct` | `piecewise` | cross-crate only | prodrome-fpl, prodrome-reading | Another crate of the library calls it; no outside program should. |
| `fpl/delta` | `Delta`, `delta_from_hours` | keep | cli, examples, wasm exports | Named by a program of this repository. |
| `fpl/delta` | `us_of` | cross-crate only | prodrome-fpl | Another crate of the library calls it; no outside program should. |
| `fpl/delta` | `div_delta` | keep | prodrome-fpl | The spans of terms (SPEC §2). |
| `fpl/delta` | `total_seconds` | keep | none; kept for the second evaluator: reads a span as seconds | The span in seconds, as `timedelta.total_seconds` gives it, for a program that prints a span. |
| `fpl/delta` | `hours` | keep | prodrome-fpl | The spans of terms (SPEC §2). |
| `fpl/env` | `Outcome` (+4), `Candidates`, `Env` (+4) | keep | cli, examples, wasm, wasm exports | Named by a program of this repository. |
| `fpl/printed` | `print_term`, `parse_term` | keep | cli, examples, github program, wasm, wasm exports | Named by a program of this repository. |
| `schedule` | `Schedule` (+17) | keep | wasm exports | Named by a program of this repository. |
| `schedule` | `Unordered` | keep | none | Appears in the signature of `Schedule::new`. |
| `term/functor` | `CurvePoint` (+3), `Term` (+7) | keep | cli, examples, github program, wasm exports | Named by a program of this repository. |
| `term/functor` | `TermF` (+22: 1 L, 2 N, 2 P) | keep | core tests, prodrome-fpl | The term functor (SPEC §7). Its members complete the type. |
| `term/normalize` | `normalize` | keep | examples | Named by a program of this repository. |
| `term/schema` | `Slot` (+9), `Fields`, `fields`, `FieldSource`, `Field` (+2), `build` | keep | wasm exports | Named by a program of this repository. |
| `term/schema` | `Signatures` (+1: 1 L) | keep | none | Appears in the signature of `signatures`. |
| `term/schema` | `signatures` | keep | core tests, prodrome-todo | The shape of each term layer (SPEC §2). |

### `prodrome-fpl`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `breaks` | `series_knots` | keep | examples, wasm | Named by a program of this repository. |
| `breaks` | `Breaks` | keep | none | Appears in the signature of `breakpoints`. |
| `breaks` | `breakpoints` | keep | core tests | The breakpoints of a term (SPEC §7). |
| `breaks` | `SAMPLES` | narrow | none | Only its own crate names it, and the crate calls it; no outside program needs it. |
| `breaks` | `Knot` | keep | none | Appears in the signature of `Series::knots`. |
| `breaks` | `Series` | keep | none | Appears in the signature of `series_knots`. |
| `chain` | `compile` | keep | wasm | Named by a program of this repository. |
| `chain` | `ChainError` | keep | core tests | The chain compiler (SPEC §7.1). |
| `chain` | `Link` (+7: 1 N, 1 P, 2 T) | keep | core tests | The chain compiler (SPEC §7.1). Its members complete the type. |
| `chain` | `Compiled` (+7: 1 N, 2 P, 4 T) | keep | core tests | The chain compiler (SPEC §7.1). Its members complete the type. |
| `chain` | `compile_chain` | keep | core tests | The chain compiler (SPEC §7.1). |
| `chain` | `chain_order` | keep | core tests | The chain compiler (SPEC §7.1). |
| `fpl` | `Closed` (+4), `holds_absent`, `fulfillment`, `Scalar` (+4), `Note` (+3), `Explanation` (+3), `wire_key`, `scalar`, `scalars`, `explained`, `Member` (+2), `Stores` (+4), `link`, `link_in` | keep | cli, examples, wasm, wasm exports | Named by a program of this repository. |
| `fpl` | `WITHIN_SAMPLES` | keep | core tests | The evaluation of a term (SPEC §7). |
| `fpl` | `power_mean` | keep | none | SPEC §7 defines the power mean of `Conj`; a second evaluator checks its results against it. |
| `fpl` | `offset` | keep | none | SPEC §7 defines `Offset`; a second evaluator checks its results against it. |
| `fpl` | `bound` | keep | none; kept for the second evaluator: reads the events that a link binds | The events that a link binds, which a program that builds its own environment calls. |
| `fpl` | `last_tended` | keep | core tests | The evaluation of a term (SPEC §7). |
| `fpl` | `phase` | keep | none | SPEC §7 defines the instant that a `Periodic` reads; a second evaluator checks its results against it. |
| `fpl` | `min_fulfillment` | keep | none; kept for the second evaluator: checks the bound of Theorem IV.1 | The bound of Theorem IV.1, which a program that explains a price calls. |
| `fpl` | `member_shares` | keep | none; kept for the second evaluator: reports each member's share | The shares of a conjunction's members, which a program that explains a price calls. |
| `fpl` | `LinkError` | keep | core tests, prodrome-reading | The evaluation of a term (SPEC §7). |
| `observe` | `Rounding` (+3), `Observation` (+5), `NextChange` (+2), `next_change` | keep | wasm, wasm exports | Named by a program of this repository. |
| `observe` | `observed` | keep | core tests | The precision of a view (SPEC §7). |

### `prodrome-todo`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `event` | `Authored` (+5), `TodoEvent` (+12), `mk_created`, `mk_completed`, `mk_cancelled`, `mk_reopened`, `mk_tended`, `mk_spec_revised` | keep | cli, examples, github program, wasm, wasm exports | Named by a program of this repository. |
| `event` | `Created` | keep | none | The record of a `Created` event, a variant of `TodoEvent` (SPEC §4). |
| `event` | `SpecRevised` | keep | none | The record of a `SpecRevised` event, a variant of `TodoEvent` (SPEC §4). |
| `event` | `mk_record` | keep | none; kept for the second todo program: builds a record event | The constructor of an `Authored` event, beside the other `mk_*` of the todo schema. |
| `payload` | `Payload` | keep | wasm exports | Named by a program of this repository. |
| `payload` | `RECORD_PREFIX` | keep | none; kept for the author of another payload: pins the record's first fields | The fields that every record kind starts with; the author of a payload checks a signature against it. |
| `payload` | `record_signature` | keep | none; kept for the author of another payload: pins the record's signature | The signature of a record kind; the author of a payload pins it in a test. |
| `payload` | `spec_field` | keep | none; kept for the author of another payload: reads the spec field | A read helper for the author of a payload. |
| `reference` | `Todo` (+12), `mk_source`, `mk_note`, `mk_subtodo`, `mk_authored` | keep | cli, examples, github program, wasm, wasm exports | Named by a program of this repository. |
| `reference` | `MarkupSource` (+1: 1 N) | keep | none | Appears in the signature of `Todo::body` and `Todo::detail`. Its members complete the type. |
| `reference` | `NoteSite` (+12: 3 N) | keep | none | Appears in the signature of `Note::on`. Its members complete the type. |
| `reference` | `Source` | keep | core tests | The reference payload (feature `reference`). |
| `reference` | `Note` | keep | core tests | The reference payload (feature `reference`). |
| `reference` | `SubTodo` | keep | core tests | The reference payload (feature `reference`). |
| `reference` | `mk_todo` | keep | none; kept for the second todo program: builds the reference record | The constructor of the reference payload's record, beside `mk_note` and `mk_subtodo`. |
| `todo` | `TodoVocabulary` | keep | examples, wasm exports | Named by a program of this repository. |
| `todo` | `TODO_SIGNATURES` | keep | core tests | The todo schema's registers (SPEC §4, §6). |
| `todo` | `State` | keep | core tests | The todo schema's registers (SPEC §4, §6). |
| `todo` | `Spec` | keep | core tests | The todo schema's registers (SPEC §4, §6). |
| `todo` | `Content` | keep | core tests, prodrome-reading | The todo schema's registers (SPEC §4, §6). |
| `todo/registers` | `Registers` (+8: 3 L, 1 P) | keep | prodrome-reading | The registers of a todo (SPEC §6). |
| `todo/write` | `Write` (+6), `Kind` (+5) | keep | cli, wasm, wasm exports | Named by a program of this repository. |

### `prodrome-reading`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `fold` | `readings`, `env`, `specs`, `content`, `flatten`, `link_specs` | keep | cli, examples, wasm, wasm exports | Named by a program of this repository. |
| `fold` | `price` | keep | core tests | The readings of a todo (SPEC §6.1 to §6.5). |
| `fold` | `member` | keep | none | The member that a store's readings group (SPEC §6.4); `view::entries` calls it. |
| `fold` | `stores` | keep | core tests | The readings of a todo (SPEC §6.1 to §6.5). |
| `schema` | `History`, `Bind`, `Row` | keep | wasm exports | Named by a program of this repository. |
| `schema` | `Price` | keep | core tests, wasm tests | The traits of a priced schema (SPEC §6). |
| `todo` | `Reading` (+2) | keep | wasm exports | Named by a program of this repository. |
| `view` | `Price` (+3), `Provisional` (+3), `Confidence` (+4), `Entry` (+20), `list_order`, `entries` | keep | cli, examples, wasm, wasm exports | Named by a program of this repository. |

### `prodrome-core`

| Module | Items | Decision | Who names them outside the crate | Reason |
|---|---|---|---|---|
| `laws` | `Broken` | keep | core tests | The law kit (SPEC §9): a host runs it in its own tests. |
| `laws/history` | `history` | keep | examples | Named by a program of this repository. |
| `laws/holding` | `holding` | keep | core tests | The law kit (SPEC §9). |
| `laws/printed` | `printed` | keep | core tests | The law kit (SPEC §9). |
| `laws/reading` | `Registers` (+3: 2 N, 1 T) | keep | core tests | The law kit compares readings. Its members complete the type. |
| `laws/schema` | `free` | keep | core tests | The law kit (SPEC §9). |
| `laws/schema` | `routed` | keep | core tests | The law kit (SPEC §9). |
| `laws/schema` | `valued` | keep | core tests | The law kit (SPEC §9). |
| `laws/script` | `Script` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script` | `Ran` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script` | `run` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script` | `run_with` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script` | `Rate` (+5: 2 N) | keep | none | Appears in the signature of `long_run`. Its members complete the type. |
| `laws/script` | `long_run` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script/step` | `Who` (+7: 1 N, 3 T) | keep | core tests | The law kit's script machine (law 68). Its members complete the type. |
| `laws/script/step` | `Kind` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script/step` | `Writing` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script/step` | `Refusal` | keep | none | Appears in the signature of `refusal`. |
| `laws/script/step` | `Root` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script/step` | `Through` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script/step` | `Step` (+13: 1 N) | keep | core tests | The law kit's script machine (law 68). Its members complete the type. |
| `laws/script/step` | `a_script` | keep | core tests | The law kit's script machine (law 68). |
| `laws/script/step` | `an_honest_script` | keep | core tests | The law kit's script machine (law 68). |
| `laws/shuffle` | `Shuffle` (+3), `a_shuffle` | keep | examples, wasm exports | Named by a program of this repository. |
| `laws/synced` | `synced` | keep | core tests | The law kit (SPEC §9). |


No row has the reason "unknown". The count of items by who names them,
before and after this stage, from `scripts/outside.py --summary` (each
field and each variant is an item):

| Crate | Programs | Other crates | Tests only | Nobody | All | Programs | Other crates | Tests only | Nobody | All |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| `prodrome-data` | 108 | 167 | 40 | 64 | 379 | 108 | 167 | 40 | 51 | 366 |
| `prodrome-memo` | 18 | 16 | 30 | 10 | 74 | 18 | 16 | 30 | 10 | 74 |
| `prodrome-dag` | 55 | 59 | 20 | 9 | 143 | 55 | 59 | 20 | 8 | 142 |
| `prodrome-declared` | 20 | 0 | 44 | 9 | 73 | 20 | 0 | 44 | 9 | 73 |
| `prodrome-store` | 43 | 17 | 29 | 15 | 104 | 43 | 17 | 29 | 15 | 104 |
| `prodrome-tags` | 19 | 0 | 18 | 1 | 38 | 19 | 0 | 18 | 1 | 38 |
| `prodrome-nest` | 0 | 0 | 39 | 7 | 46 | 0 | 0 | 39 | 7 | 46 |
| `prodrome-digest` | 25 | 0 | 16 | 2 | 43 | 25 | 0 | 16 | 2 | 43 |
| `prodrome-term` | 41 | 52 | 18 | 16 | 127 | 41 | 56 | 19 | 16 | 132 |
| `prodrome-fpl` | 48 | 2 | 30 | 14 | 94 | 48 | 2 | 31 | 12 | 93 |
| `prodrome-todo` | 41 | 8 | 16 | 53 | 118 | 41 | 8 | 16 | 53 | 118 |
| `prodrome-reading` | 45 | 0 | 6 | 7 | 58 | 45 | 0 | 6 | 7 | 58 |
| `prodrome-core` | 1 | 0 | 54 | 23 | 78 | 1 | 0 | 54 | 23 | 78 |
| all | 464 | 321 | 360 | 230 | 1375 | 464 | 325 | 362 | 214 | 1365 |

The first five columns are before and the last five are after. The 15
narrowed items left the count, all from "nobody". The change merged from
`main` (the time axis) adds five items to `prodrome-term` and removes none. Of the items that nobody names, the
public fields and variants are the bulk. They are the shape of a type that
a program constructs or matches, so each stays with its type.

### What was narrowed

| Crate | Item |
|---|---|
| `prodrome-dag` | `Folded::push` |
| `prodrome-data` | `BitSet::open` |
| `prodrome-data` | `Change::to_value` |
| `prodrome-data` | `Change::from_call` |
| `prodrome-data` | `Genesis::to_value` |
| `prodrome-data` | `Genesis::from_call` |
| `prodrome-data` | `required` |
| `prodrome-data` | `Signed::from_call` |
| `prodrome-data` | `KeyAdded::to_value` |
| `prodrome-data` | `KeyAdded::from_call` |
| `prodrome-data` | `KeyRevoked::to_value` |
| `prodrome-data` | `KeyRevoked::from_call` |
| `prodrome-data` | `Snapshot::to_value` |
| `prodrome-data` | `Snapshot::from_call` |
| `prodrome-fpl` | `SAMPLES` |

The objects of a change, a genesis, a snapshot and a key are read and
printed through `Envelope`, so no outside program calls the conversions of
the object types. `BitSet::open` belongs to a hidden type. `Folded::push`
folds a node without the checks of `Folded::insert`. `breaks::SAMPLES` is a
tuning constant that only the drawing of a curve uses. `codec::required` is
the body of `payload::required`, which stays public.

### Every item that crosses a boundary

| Crate | Module | Item | Who names it | New in this stage |
|---|---|---|---|---|
| `prodrome-dag` | `dag` | `Prints` | prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::held` | prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::holds` | prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::forget` | core tests, prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::set_unread` | prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::hold_husks` | prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::husk` | core tests, prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::into_prints` | prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::settle` | core tests, prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::grow` | core tests, prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::order_among` | prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::is_root` | prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::read_prints` | prodrome-store | before |
| `prodrome-dag` | `dag` | `Dag::node` | law kit, prodrome-store | before |
| `prodrome-dag` | `dag` | `lacked_among` | prodrome-store | before |
| `prodrome-dag` | `dag/heads` | `Heads::new` | prodrome-store | before |
| `prodrome-dag` | `dag/reach` | `Reach::outside` | prodrome-store | before |
| `prodrome-dag` | `dag/reach` | `Reach::insert` | prodrome-store | before |
| `prodrome-dag` | `dag/reach` | `Reach::close` | prodrome-store | before |
| `prodrome-dag` | `dag/reach` | `Reach::tips` | prodrome-store | before |
| `prodrome-dag` | `fold` | `scan` | prodrome-reading | before |
| `prodrome-dag` | `fold` | `admits` | prodrome-reading | before |
| `prodrome-dag` | `registers` | `Folded::rewrite` | prodrome-store | before |
| `prodrome-dag` | `spread` | `spread` | prodrome-store | before |
| `prodrome-data` | `bitset` | `BitSet` | prodrome-dag | before |
| `prodrome-data` | `event` | `Hash::of_bytes` | core tests, law kit, prodrome-dag, prodrome-declared, prodrome-memo, prodrome-store | before |
| `prodrome-data` | `event` | `Hash::of_digest` | prodrome-memo | before |
| `prodrome-data` | `event` | `Hash::digest` | prodrome-memo | before |
| `prodrome-data` | `event` | `rests_on` | prodrome-dag, prodrome-store | before |
| `prodrome-data` | `event` | `sorted_distinct` | prodrome-nest | before |
| `prodrome-data` | `event` | `field` | prodrome-todo | before |
| `prodrome-data` | `event` | `hashes_value` | prodrome-nest | before |
| `prodrome-data` | `event` | `hashes` | prodrome-nest | before |
| `prodrome-data` | `literal` | `ProdromeError::io` | prodrome-memo, prodrome-store | before |
| `prodrome-data` | `literal/codec/record` | `Site` | the derive's helpers | new |
| `prodrome-data` | `literal/codec/record` | `Reader` | the derive's helpers | new |
| `prodrome-data` | `literal/codec/record` | `read` | prodrome-todo, the derive | new |
| `prodrome-data` | `literal/codec/record` | `read_or_default` | the derive | new |
| `prodrome-data` | `literal/codec/record` | `call_of` | the derive | new |
| `prodrome-data` | `literal/codec/sum` | `headed_len` | the derive | new |
| `prodrome-data` | `literal/codec/sum` | `headed_names` | the derive | new |
| `prodrome-data` | `literal/codec/sum` | `headed_table` | the derive | new |
| `prodrome-data` | `literal/codec/sum` | `names_field` | the derive | new |
| `prodrome-data` | `literal/codec/sum` | `distinct` | the derive | new |
| `prodrome-data` | `sign` | `bytes` | prodrome-dag | before |
| `prodrome-data` | `sign` | `public_of` | prodrome-dag | before |
| `prodrome-data` | `sign` | `signed_by` | prodrome-dag | before |
| `prodrome-data` | `stamp` | `Stamp::position` | prodrome-dag | before |
| `prodrome-data` | `stamp` | `Stamp::ancestry` | prodrome-dag | before |
| `prodrome-data` | `stamp` | `Stamp::placed` | prodrome-dag | before |
| `prodrome-data` | `stamp` | `Stamp::open` | prodrome-dag | before |
| `prodrome-data` | `stamp` | `Place` | prodrome-dag | before |
| `prodrome-data` | `stamp` | `Scope` | prodrome-dag | before |
| `prodrome-data` | `topo` | `Topo` | prodrome-fpl | before |
| `prodrome-term` | `fpl/axis` | `advance` | prodrome-fpl, tests | before |
| `prodrome-term` | `fpl/axis` | `retreat` | prodrome-fpl | before |
| `prodrome-term` | `fpl/axis` | `delta_of_us` | prodrome-fpl | before |
| `prodrome-term` | `fpl/construct` | `piecewise` | prodrome-fpl, prodrome-reading | before |
| `prodrome-term` | `fpl/delta` | `us_of` | prodrome-fpl | before |

### What repeats

For each crate, the ten longest docs of the rustdoc face, and each doc of an
item that this stage touched, were read against the rule of section 2. A doc
keeps what a caller cannot read from the type. The cuts delete narration,
the restatement of a signature or a name, and paragraphs that repeat
`SPEC.md`, which the doc cites instead. A first pass cut more than the rule allows: about thirty cuts removed a
law, a precondition, a no-op case, a cost or a reason that a caller cannot
read from the type. A review listed them, and each is back in one sentence:
the laws 77, 88 and 89, the husk reading of `Dag`, the closure of `≤` in a
declaration, the `Ok` of a schema with no inflationary register, the
lock and the memory of the cache file, and the rest of the list. The
twin of an event in `Decision::append_all`, the single prodrome of a store
in `TagSet::of` and the reading of each breakpoint in `observed` are also
in place. The rule keeps a law, an inverse, a precondition, an error or
no-op case, a cost, and determinism across replicas, as well as the items
of section 2. The doc tests did not change. With each blank
line and each comment line removed, each file that the pass changed equals
the file before it, apart from the lines of the narrowing and of the hidden
marks above.

### The facade

The root doc of `prodrome-core` has a new first section, "Which crate to
depend on". Its first table gives, for each crate, what it holds and its
size in tokens. Its second table gives, for each kind of program, the
crates to depend on and the sum of their sizes: a program that writes
events with a store, one that reads a todo's price, one with a schema of
its own and no todos (three sizes, by how much it does), one that adds a
second backend, and one that uses the whole library. The paragraphs that
named each crate in prose were removed, because the tables say the same
with the cost. `README.md` has a table of the same kinds of program and
their crates, and says that a program depends on the facade only when it
uses most of the library or the law kit. `docs/guide.md` names the crate
of each item it describes, and says that the law kit needs a dependency on
`prodrome-core`.

### The face

The columns "before" are the merge base `b882e3e`, which includes the
change of the time axis. The rustdoc measure is
with the default features. The line measure is `scripts/face.py --lines`.
"Doc chars" are the doc characters of the rustdoc face. "Ten longest" are
the doc characters of the ten items of each crate that had the longest
docs at `ed70e10`, before and after this stage.

| Crate | Doc chars before | after | Ten longest before | after | Rustdoc tokens before | after | Line tokens before | after |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| `prodrome-data` | 31,434 | 30,427 | 8,234 | 8,070 | 12,143 | 11,514 | 19,913 | 20,039 |
| `prodrome-memo` | 14,618 | 13,357 | 8,973 | 7,712 | 4,304 | 3,988 | 7,697 | 7,363 |
| `prodrome-dag` | 17,641 | 16,663 | 6,030 | 5,173 | 5,664 | 5,410 | 14,424 | 14,239 |
| `prodrome-declared` | 5,361 | 4,820 | 2,778 | 2,237 | 1,747 | 1,612 | 4,151 | 4,003 |
| `prodrome-store` | 20,179 | 19,624 | 7,254 | 6,699 | 6,394 | 6,256 | 21,214 | 21,067 |
| `prodrome-tags` | 5,517 | 5,345 | 3,445 | 3,273 | 1,890 | 1,847 | 4,036 | 3,990 |
| `prodrome-nest` | 6,437 | 6,115 | 3,938 | 3,616 | 2,149 | 2,068 | 4,011 | 3,926 |
| `prodrome-digest` | 5,735 | 5,285 | 3,735 | 3,285 | 1,794 | 1,682 | 4,319 | 4,196 |
| `prodrome-term` | 9,332 | 8,668 | 3,935 | 3,369 | 3,897 | 3,731 | 7,474 | 7,226 |
| `prodrome-fpl` | 13,612 | 12,553 | 5,455 | 4,521 | 4,334 | 4,063 | 5,989 | 5,742 |
| `prodrome-todo` | 8,609 | 7,440 | 5,112 | 3,892 | 3,436 | 3,144 | 5,043 | 4,731 |
| `prodrome-reading` | 9,847 | 9,364 | 4,240 | 3,757 | 3,211 | 3,090 | 4,077 | 3,949 |
| `prodrome-core` | 7,641 | 7,582 | 13,506 | 13,380 | 1,989 | 1,975 | 13,391 | 13,356 |
| all thirteen | 155,963 | 147,243 | 76,635 | 68,984 | 52,952 | 50,380 | 115,739 | 113,827 |

With the feature `laws`, `prodrome-core` is 5,811 tokens by the rustdoc
measure before and 5,780 after. The line measure falls less than the
rustdoc measure for two reasons. It counts `pub(crate)` items, so a
narrowing removes no line of it, and each `#[doc(hidden)]` mark and the
longer sentence "For this library's own code; no promise." add lines. The
data crate is 126 tokens larger by the line measure, 20,039 against 19,913,
and under the 21,400 of "The targets".

**Which crates a unit reads.** The sums use the line measure. The sets are
the sets of the root doc of `prodrome-core`. "Before" is the sum of the
faces at `b882e3e`.

| A program that | Crates | Tokens before | Tokens after |
|---|---|--:|--:|
| writes events with a store and reads them under a policy | data, memo, dag, store | 63,248 | 62,708 |
| reads a todo's price and its row in a list | data, memo, dag, term, fpl, todo, reading | 64,617 | 63,289 |
| has its own schema and folds it under a policy | data, memo, dag | 42,034 | 41,641 |
| has its own schema and prints and parses events | data | 19,913 | 20,039 |
| uses the whole library | all thirteen | 115,739 | 113,827 |

Section 13 gave 63,808 tokens for the second row. Each sum falls by the
cuts of this stage, except the sums that hold the data crate alone, which
rises by 126 tokens, and no program changes its set of crates.

### The checks

- The three commands of `AGENTS.md` pass on the merged tree.
- `cargo doc --locked --workspace --no-deps --lib` with warnings denied
  passes with the default features, with `--all-features` and with
  `--no-default-features`.
- No test, example, bench, wasm export or CLI of this repository lost an
  item that it names: every target compiles, and the tests pass. No test
  changed, and no conformance vector or vector of `wasm/conformance/`
  changed, so the conformance run is untouched.
- `nix flake check` did not run: the environment cannot fetch the flake's
  input `crane` (HTTP 403). This stage changes no file of the wasm, the
  viewer, `typst/` or `flake.nix`, so `AGENTS.md` does not require it. The
  CI runs it.

### What remains of issue #92

The issue asks for three things, in this order: shorten the doc comments to
what the code and `SPEC.md` do not say, narrow what is public, and divide
the core into crates in the dependency order of its modules. Stage 1 and
this stage shortened the docs, this stage reviewed every public item and
narrowed 15, and stages 3 to 13 made the crates. Every crate is under the
budget of 25,000 tokens, and the table above has no item whose decision is
unknown. So the text of the issue is done, and the pull request names it
with `Completes: gh-92` (the id of the roadmap item for the issue). The
release that publishes the crates is a separate step and is not part of
the issue.
