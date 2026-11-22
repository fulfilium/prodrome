# Design: what a program must read to use the core

This note answers issue #92. It measures the public face of the crate
`prodrome-core`, and it proposes how to make what one task must read
smaller, in stages. Section 7 records stage 1, which the same pull request
as this note made. Section 8 records stage 3, the data crate, which a
later pull request made before stage 2. The other stages are proposals.
For what is implemented, see `SPEC.md` and `CHANGELOG.md`.

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
| `prodrome-store` | `store`, `dag`, `registers`, `policy`, the proof of `sign`, `nest`, `declared`, `tags`, `attest` | data, memo |
| `prodrome-fpl` | `term`, `fpl`, `schedule`, `chain`, `breaks`, `observe` | data |
| `prodrome-reading` | the readings of `fold.rs`, `view`, the traits `Price`, `History`, `Bind`, `Row` | data, store, fpl |
| `prodrome-todo` | `todo`, `reference`, `Payload`, `TodoEvent` | data, fpl, reading |
| `prodrome-core` | re-exports every crate above at today's paths, and holds `laws` behind its feature | all |

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
| 4, each further crate | | | under 25,000 in each of two crates |

Stage 2 reaches its numbers by removing `pub` from items that only their
own module uses, and by a third pass of words over the data modules
(`literal`, `schema`, `event`). Stage 3 reaches 15,000 when the data crate
holds only the data modules, and the items of `literal` that only its
parser and its printer use have lost `pub`. Stage 4 divides the store
crate in two: `prodrome-dag` (`dag`, `registers`, `policy` and the proof
of `sign`, about 15,000 tokens today) and `prodrome-store` (`store`,
`nest`, `declared` and `tags`), whose module `store` must lose about a
third of its count by stage 2.

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
