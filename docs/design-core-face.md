# Design: what a program must read to use the core

This note answers issue #92. It measures the public face of the crate
`prodrome-core`, and it proposes how to make what one task must read
smaller, in stages. Section 7 records stage 1, which the same pull request
as this note makes. The later stages are proposals. For what is
implemented, see `SPEC.md` and `CHANGELOG.md`.

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

`scripts/face.py` measures the face. It reads the JSON that rustdoc writes
for the crate, walks every item that is reachable from the crate root, and
counts two numbers for each item: the characters of its signature, and the
characters of its doc comment. The script's own doc comment says what it
counts as a signature. It does not count trait impls, such as
`impl Display for Hash`, because their methods have no doc comment of
their own. Run it from the repository root:

```console
$ RUSTC_BOOTSTRAP=1 nix develop -c cargo rustdoc -p prodrome-core --lib \
    --target-dir target/face -- -Z unstable-options --output-format json
$ python3 scripts/face.py target/face/doc/prodrome.json
```

`RUSTC_BOOTSTRAP=1` lets the stable toolchain write rustdoc's JSON, which
is an unstable output format. Add `--all-features` to the first command to
count the module `laws`, which is behind a feature, and add `--by-file` to
the second command to count by source file.

This count gives about half the number that an application's own tool
reported for the same crate, about 140,000 tokens. This note could not
reproduce that tool's rule. For comparison, every doc line of `core/src`,
public or private, counted with its `///` prefix and its indentation, is
about 368,000 characters, or 92,000 tokens. Both counts find that doc
comments are most of the face, and the plan below depends on the
proportions between the modules more than on the total.

### The numbers

The columns "before" are measured at the merge base `85e5083`. The table
counts every module with the feature `laws` on. The columns "after" are the result of stage 1 (section 7), measured in the same way on
the branch of this note. The module `lib` is the crate root, whose doc
comment holds the quick-start example.

| Module | Items | Signature chars | Doc chars before | Doc chars after | Tokens before | Tokens after |
|---|--:|--:|--:|--:|--:|--:|
| `store` | 162 | 6,341 | 37,734 | 31,586 | 11,018 | 9,481 |
| `literal` | 156 | 5,574 | 17,275 | 13,663 | 5,712 | 4,809 |
| `memo` | 103 | 2,598 | 18,655 | 14,551 | 5,313 | 4,287 |
| `fold` | 109 | 5,061 | 13,845 | 12,145 | 4,726 | 4,301 |
| `laws` | 125 | 3,182 | 13,750 | 12,106 | 4,233 | 3,822 |
| `schema` | 47 | 2,182 | 12,510 | 11,139 | 3,673 | 3,330 |
| `fpl` | 105 | 4,297 | 10,099 | 9,089 | 3,599 | 3,346 |
| `nest` | 61 | 2,279 | 9,053 | 6,565 | 2,833 | 2,211 |
| `event` | 106 | 3,824 | 7,219 | 5,276 | 2,760 | 2,275 |
| `dag` | 101 | 2,408 | 8,219 | 6,965 | 2,656 | 2,343 |
| `digest` | 39 | 1,207 | 9,016 | 7,890 | 2,555 | 2,274 |
| `sign` | 53 | 1,703 | 7,687 | 6,307 | 2,347 | 2,002 |
| `declared` | 97 | 1,629 | 7,586 | 6,354 | 2,303 | 1,995 |
| `policy` | 21 | 720 | 7,421 | 3,187 | 2,035 | 976 |
| `tags` | 50 | 2,046 | 6,005 | 5,454 | 2,012 | 1,875 |
| `registers` | 36 | 1,526 | 5,407 | 4,713 | 1,733 | 1,559 |
| `lib` (the crate root) | 1 | 0 | 6,443 | 5,294 | 1,610 | 1,323 |
| `chain` | 31 | 869 | 5,288 | 3,521 | 1,539 | 1,097 |
| `view` | 36 | 1,058 | 5,064 | 3,678 | 1,530 | 1,184 |
| `observe` | 16 | 682 | 5,257 | 3,160 | 1,484 | 960 |
| `payload` | 18 | 1,186 | 4,152 | 2,444 | 1,334 | 907 |
| `term` | 117 | 2,550 | 2,309 | 2,298 | 1,214 | 1,212 |
| `reference` | 53 | 1,987 | 2,793 | 2,032 | 1,195 | 1,004 |
| `schedule` | 22 | 1,019 | 2,917 | 2,473 | 984 | 873 |
| `todo` | 9 | 491 | 995 | 892 | 371 | 345 |
| `breaks` | 14 | 347 | 1,135 | 1,217 | 370 | 391 |
| `topo` | 5 | 292 | 862 | 812 | 288 | 276 |
| `snapshot` | 14 | 421 | 702 | 737 | 280 | 289 |
| `genesis` | 11 | 336 | 152 | 178 | 122 | 128 |
| `change` | 8 | 278 | 180 | 195 | 114 | 118 |
| total, with `laws` | 1,726 | 58,093 | 229,730 | 185,921 | 71,955 | 61,003 |
| total, default features | 1,601 | 54,911 | 215,980 | 173,815 | 67,722 | 57,181 |

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

Applied to the whole default face, a cut of 28 percent of the doc
characters estimated a face of 52,700 tokens. The whole pass cut 20
percent, to 57,181 tokens, because the modules of files, locks and crashes
(`store`, `memo`, `digest`) keep many hazard and crash sentences.
Section 7 gives the measured result of the whole pass.

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
lets a minor release break the API. Each narrowing still follows three
steps. First, a release marks each item `#[deprecated]`, with a note that
names the replacement, and the `CHANGELOG.md` lists it under "Deprecated".
Second, the next minor release makes the change and lists it under
"Changed" or "Removed". Third, an item that moves to the internal module
keeps a re-export at its old path for one release, also deprecated.
Narrowing changes no stored byte, no conformance vector and no law.

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
to a crate by its file and its name, as the table above says.

The counts are with the feature `laws`, which a split would keep as a
feature of the facade.

| Crate | Tokens before stage 1 | Tokens after stage 1 |
|---|--:|--:|
| `prodrome-data` | 17,121 | 14,814 |
| `prodrome-memo` | 5,313 | 4,287 |
| `prodrome-digest` | 2,555 | 2,274 |
| `prodrome-store` | 25,453 | 21,036 |
| `prodrome-fpl` | 9,192 | 7,880 |
| `prodrome-reading` | 3,393 | 2,989 |
| `prodrome-todo` | 3,081 | 2,576 |
| `prodrome-core` (the facade's root doc and `laws`) | 5,843 | 5,145 |

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

The target has two parts: no crate's face is over 25,000 tokens, and a
unit that only builds and reads events reads under 15,000 tokens. After
stage 1 and the split, both parts hold by the measure of section 1: the
largest crate, `prodrome-store`, has 21,036 tokens, and `prodrome-data`
has 14,814. The margin of the second part is small. A unit that uses the
store reads three crates, 40,137 tokens in all, but each crate is under
the budget for one dependency.

By the measure of the application, which counts about twice as many
characters, the targets do not hold: `prodrome-data` would be about 31,000
tokens. So the stages after the split must still narrow the data crate
(section 3) and shorten its longest docs, mainly `literal`, `schema` and
the module head of `schema::headed`.

## 5. The stages

Each stage is one pull request that lands alone.

1. **The doc comments, by the rule of section 2.** Words only. A program
   changes nothing. The conformance vectors and the law tests do not
   change, and the words-only check of section 7 proves that no code
   changed.
2. **The narrowing of section 3.** The release marks each candidate
   `#[deprecated]` first, and the next release narrows it. A program that
   names a narrowed item changes its import to `prodrome::internal::...`,
   or turns on a feature. No test changes its assertions; a test that
   names a narrowed item changes its import.
3. **The data crate.** The pull request first cuts the edges of section 4
   that touch the data crate, inside `prodrome-core`, as moves of items
   between modules with re-exports at the old paths. Then it moves the
   files to `prodrome-data`, and `prodrome-core` re-exports them at the
   same paths. A program that depends on `prodrome-core` changes nothing.
   A program that wants only the data crate changes its dependency and
   its import paths, from `prodrome::event` to `prodrome_data::event`.
   The conformance suites stay in `prodrome-core`'s tests, which read every
   crate through the re-exports, so every vector and every law test runs
   unchanged.
4. **Each further crate,** in the order memo, store, fpl, digest, reading,
   todo, one pull request each, by the same method as stage 3. The derive
   macro writes `::prodrome::...` paths today; the stage that moves
   `literal` makes the macro name the data crate, or keeps the facade's
   path and requires the facade.

`SPEC.md` names no Rust module, so no stage changes it. `CHANGELOG.md`
announces each stage under "Changed", with the paths a program may change.

## 6. What argues against the split

- **Compile time.** Seven crates compile in parallel, but each crate
  boundary stops inlining without `#[inline]` and costs a separate
  metadata pass. The generic code of the core (`Schema`, the folds) is
  monomorphised in the application anyway, so the loss is small, but it is
  not zero, and nobody has measured it.
- **Version skew.** Seven crates mean seven versions. An application that
  depends on `prodrome-data` 0.10 and on `prodrome-store` 0.11 gets two
  copies of `Hash`, and the compiler reports that a `Hash` is not a
  `Hash`. The crates must release in lock step, with exact version
  requirements between them, which `cargo` can express but which every
  release must check.
- **The cost to a small program.** A program that wants the whole core
  today adds one dependency. After the split it still adds one, the
  facade, but its error messages and its documentation name seven crates,
  and `cargo tree` shows seven.
- **Cuts that cost design.** Section 4 found that twenty modules form one
  cycle. Each cut in its table is a decision about where a type belongs,
  and two of them move items out of the files that hold their concept
  today (`event.rs` holds both the envelope and the todo events). A cut
  made only to split a crate can make the code harder to read.
- **Words and narrowing might be enough.** If the face of the whole core
  fitted the budget, one crate would be simpler than seven for every
  reader. After stage 1 the whole face is 57,181 tokens by the measure of
  section 1, which is more than twice the budget, and narrowing removes at
  most 17 percent. So this argument does not hold today.

Stage 1 would have been enough if it had brought the whole face under
25,000 tokens. It cut 16 percent, and the rule keeps most of what remains,
so no further pass of words alone reaches the budget. The work should stop
after stage 2 if a later count finds that most of an application's units
use the store or the evaluator as well as the data, because then most
units would read most crates and the split would save them little.

## 7. Stage 1, as made

Stage 1 changed the doc comments of the public items of `core/src` by the
rule of section 2, and nothing else.

**The face.** The default face fell from 67,722 to 57,181 tokens, a cut of
16 percent. The doc characters fell from 215,980 to 173,815, a cut of 20
percent; the doc tests did not change. With the feature `laws`, the face
fell from 71,955 to 61,003 tokens. The table of section 1 gives each
module.

**What the pass kept.** The pass kept, as one sentence each, about 61
hazards, about 106 invariants or obligations of a caller, about 72 outside
facts (formats, units, system calls, file systems, rules of CPython and of
other standards), and about 77 citations of a law by its number. These
counts are the sums of the counts that each part of the pass reported.

**The review of the deletions.** A second reader read every deleted line
against the list of section 2, and restored four sentences that were the
only place where their reason was written:

- why `Product::climbed` passes for a subset read under the policy where
  everything binds (`fold/product.rs`);
- why the heap of `topo::linear` orders keys correctly (`topo.rs`);
- why a named actor's lifecycle events claim under `Untrusted`: such an
  actor reads input that an attacker can control (`policy.rs`);
- why `tags::alias::unaliased` never returns an error (`tags/alias.rs`).

The reader also corrected three new sentences that were false
(`registers::deps_for`, `store::MemoryStore`, and the law number in
the head of `policy`).

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
ambiguous intra-doc links), and the pass fixed them by rewording.

**Errors found in `SPEC.md`.** The pass found three, and did not change
`SPEC.md`, because this pull request changes words of the code only. SPEC
§9 has two items numbered 40. SPEC §5 cites law 41 for a proof that is a
reading of the history, which is law 42. SPEC §3 cites law 39 for the
restricted join of `accept`, but law 39 is about nests.
