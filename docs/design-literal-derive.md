# Design: a derive for the literal codec

This note was written with the change, and it records the choices and the
measurements behind it. For the stored form, see `SPEC.md` section 2. The
stored form does not change.

## The problem

A type that a store holds has one stored form: a call of a constructor,
such as `Note(on='block', lines=('one',))`. Before this change, a program
wrote that form by hand, in up to four places: the type, a table of its
constructor's name and field names, a function that prints a value, and a
function that reads it back. The reference payload (`core/src/reference.rs`)
had all four for each of its records, and an application with many stored
types repeats them for each type. When the four places disagree, a value
prints in a form that does not read back.

## Three routes

**A serde data format.** A `Serializer` and a `Deserializer` for the
literal form would let `#[derive(Serialize, Deserialize)]` do the work. This
route was not built, for three reasons:

1. The core would need `serde` as a runtime dependency. The task forbids a
   new runtime dependency of the core for the derive.
2. The refusals of a read could not keep their messages. A read of the
   reference payload refuses a missing field as `Note(...) is missing on`.
   serde reports a missing field through `Error::missing_field`, which gets
   the field's name and not the constructor's. A format can track the
   constructor in its own state, but then each message is rebuilt from the
   visitor's calls, and the order in which a smart constructor refuses (for
   example, `mk_todo` checks `body` before `kind`) is serde's order and not
   the smart constructor's.
3. serde's data model has no datetime. A `Datetime` would cross the format
   as a newtype struct with a magic name.

The size of this route in the browser's module was not measured, because
the route breaks the first constraint before its size matters. A serde
visitor is code that the hand-written codec does not have: one `Visitor`
impl, one field enum and one `MapAccess` loop for each type.

**A procedural macro of our own, on `syn` and `quote`.** This is the route
taken. The macro writes the same calls that the hand-written code made:
`codec::record` to print, and one read of each field by name. The core
already builds `syn` 2, `quote` and `proc-macro2` for `curve25519-dalek`'s
own derive, so the macro adds one crate of its own to a build and no
crate from the registry. `darling` was not used: the attribute grammar has
five options, and `syn`'s `parse_nested_meta` reads them in about 60 lines
(`derive/src/options.rs`).

**A published crate for this form.** No crate was found that prints and
parses this form. `py_literal` reads and writes Python literals, but not a
call of a constructor with keyword fields, and not a datetime. `ron` has
constructors, but it writes `Name(field: value)`, and its strings and
floats differ from CPython's `repr`, which SPEC §2 requires.

## What the derive writes

- A struct with named fields is one constructor. The derive writes
  `Literal` and `Record`. `Record` gives the constructor as data
  (`CONSTRUCTOR`), the values of the fields, and a read of a call whatever
  its name (`from_call`), which the event layer uses for a payload's record.
- An enum has one constructor for each variant. A variant has named fields
  or none, or it holds one record in its one unnamed field. A variant that
  holds a record prints and reads as the record's call: `Email(Draft)`
  prints `Email(to='ada')` when the constructor of `Draft` is `Email`. The
  record's constructor is the variant's, so `rename` does not apply to
  such a variant. The derive of the enum does not see the record's name,
  so a check that the compiler evaluates refuses two variants whose
  constructors have one name (`codec::distinct`).
- A struct with one unnamed field is a newtype, and its literal is its
  field's.

- A struct with named fields, one of which is marked `#[literal(sum)]`,
  has one constructor for each variant of that field's enum. Each
  constructor's fields are the struct's other fields, in declaration order,
  then the variant's fields: `Opened(at, actor, rests_on, title)` and
  `Closed(at, actor, rests_on, why)`. This is the shape of an event whose
  kinds share leading fields. The derive writes the trait `Sum` for every
  enum: the constructor and the values of a variant, and a read of the
  variant that a call names. The struct prints with `codec::headed` and
  reads the variant first, then its own fields. Its `CONSTRUCTORS` lists
  each constructor with all its fields. `const` functions in
  `codec/sum.rs` build that table when the program compiles, because the
  struct's derive does not see the enum's fields. For the same reason, a
  field name that is both the struct's and a variant's is refused by a
  check that the compiler evaluates, and not by the macro.

The derive refuses, with a message that says what to write: a generic type,
a struct with two or more unnamed fields, a variant with unnamed fields
other than one record, an enum without variants, a union, two constructors of one sum with one name,
two fields of one constructor with one name, a stored name that the
grammar cannot read, two fields marked `sum` in one struct, a field name
that is both the struct's and a variant's, and two variants that hold
records with one constructor's name.

## The path of the core

The code that the derive writes names the core as `::prodrome`. A program
that imports `prodrome-core` under another name (`package =
"prodrome-core"` in its manifest) gives that name with
`#[literal(crate = path)]` on the type, as `#[serde(crate = "...")]` does
for serde. The option changes only the paths in the written code, so it
does not change a print or a read.

## Stored names

A constructor's name and its fields' names are in every stored object, so a
change of a name is a change of the stored form. The rule: a stored name is
the Rust identifier, unless `#[literal(rename = "...")]` gives one. Every
derived type gives its constructors as data, in `Literal::CONSTRUCTORS`. A
schema pins the constructors of its types in one test with
`codec::assert_pinned`, which compares them with a text of one
`Name(field, …)` on each line. A rename that changes a stored name fails
that test. `core/tests/all/derive.rs` pins the reference payload's four
constructors, and each schema of this repository pins its own. The conformance vectors also hold
these names, so a rename fails them too.

Names given only in attributes were not chosen. They would spell each name
twice, once as the identifier and once in the attribute, which is the
repetition that the derive removes. One pin in one test catches a rename in
one place.

## The same bytes and the same refusals

A derived read reads each field with `Literal::from_field`, which takes the
field's context, such as `Note.lines`. A leaf names the context in its
refusal, as the payload helpers did (`Note.lines must be a tuple`). The
payload helpers in `payload.rs` are reads of the same functions, and so are
`Fields::of`, `Fields::among` and `Fields::get`. A hand-written read and a
derived one therefore give the same messages. Every refusal of a field
names the field. A leaf that words its own read, such as `String`, names it
in its words (`Note.lines must be a tuple`). Any other type reads with
`from_value`, and the default `from_field` leads the refusal with the field
(`Heads.count: expected a natural number …`). A refusal inside a nested
record therefore reads as a path. A sum names its type in its
refusal, whether it is derived or written by hand with `no_arm`:
`Reject is not one of Verdict's constructors`.

The words of a refusal are not part of the stored form, and no reader may
depend on them. The language that a read accepts is part of the stored
form. When `Fields` took the derive's words, a property compared the old
`Fields` with the new one, and four readers of this repository as they
were with the same readers as they are (SPEC law 69,
`core/tests/all/fields.rs`). Over generated values and values changed in
up to four places, both accept the same values and read the same value.

Before the hand-written codec of the reference payload was deleted, two
properties compared it with the derived one (commit "derive Literal, and
derive the reference payload beside the hand-written codec"):

1. Over generated payloads, the derived print equals the hand-written
   print, for the record and for each record that it nests.
2. Over generated payloads changed in up to four places (a field dropped, a
   value replaced, a call renamed), the derived read equals the
   hand-written read: the same value, or the same refusal with the same
   message. Each part of the changed value was also read as a `Source`, a
   `Note`, a `SubTodo` and an `Actor`.

Both properties passed 200,000 cases each in a release build. In a sample
of 20,000 cases, 46% of the changed values read as a payload, and the rest
gave more than 60 different refusals, among them each refusal of the smart
constructors. The read of the whole payload was compared only for a call
named `Authored`, because the event layer reads a payload only from such a
call.

A record with a sum is proved the same way, against a hand-written codec
that stays in `core/tests/all/derive.rs`: a table of fields for each shape
and two matches. The two codecs print the same values, and they read the
same lines changed in up to four places, refusals and messages included.
Both properties passed 100,000 cases each in a release build. In a sample
of 20,000 changed lines, 47% read as a line, and the rest gave 26 different
refusals.

## Measurements

The figures are from this repository's build, on generated data only.

| | Before | After |
|---|---|---|
| Lines of `reference.rs`, `payload.rs` and `event.rs` | | 173 fewer |
| `prodrome_wasm.wasm`, `wasm-release`, before `wasm-bindgen` | 1,591,205 bytes | 1,592,264 bytes |
| Cold `cargo build --workspace`, dev profile, two runs | 22.2 s, 22.1 s | 21.3 s, 21.4 s |

The module grows by 1,059 bytes (0.07%). This note does not attribute the
growth to single functions. The derived reads differ from the hand-written
ones in one way: they pass each field's context as `&dyn Display` and
format it only when they refuse, and each leaf has a second read that takes
the context. The
cold build shows no cost from the new build step: the macro crate compiles
in parallel with the core's other dependencies.

## Hand-written impls that stay

- `String`, `bool`, the unsigned integers, `Datetime`, `NaiveDate`,
  `Option`, `Vec` and `BTreeSet`: the leaves and containers that a derived
  impl is built from.
- `Hash`: it holds its digest twice, as words and as text, so it is not a
  newtype.
- `Term`: the FPL signature table prints and reads a term.
- `Span` in `core/examples/memo_file.rs`: its literal is a tuple, which a
  derived impl does not write.
- `Verdict` and `Review` in `core/tests/all/codec.rs`, and `Thread` in
  `core/tests/all/derive.rs`: tests of the hand-written API, and of a
  hand-written impl that holds a derived one.

The event layer (`TodoEvent` and the envelopes) stays hand-written. It is
generic over the payload, and its record kind takes its name from the
payload. A derive for a generic type, and for the head of an event in
`schema::headed`, is left for later work.

## Additions for a host's hand-written codecs

An application that adopted the derive kept many of its stored types by
hand. Most of them read through `Fields`, whose refusals had other words
than the derive's. The types had the same shape and accepted the same
literals. The following changes let such types move to the derive. They
change no print, no stored form, and no language that a read accepts.

- `Fields` refuses in the words of `codec::read` (see "The same bytes and
  the same refusals"). There is one wording, and no second read mode.
- `#[literal(crate = path)]` names the core for a program that imports it
  under another name.
- `TodoId` is a `Literal`, so a newtype over it derives.
- A variant can hold one record (see "What the derive writes").
- `codec::assert_pinned` pins a schema's constructors in a test (see
  "Stored names").

The figures are from this repository's build, on generated data only.

| | Before | After |
|---|---|---|
| `prodrome_wasm.wasm`, `wasm-release`, before `wasm-bindgen` | 1,592,264 bytes | 1,592,267 bytes |
| Build of `prodrome-derive` after `cargo clean -p prodrome-derive`, runs 2 and 3 | 0.85 s, 0.96 s | 1.11 s, 1.04 s |
| Build of the core's test binary after a touch of `core/tests/all/derive.rs`, three runs | 10.6 s, 9.3 s, 8.5 s | 10.8 s, 9.6 s, 9.2 s |

The test binary of the second column holds more derived types and more
tests, so its build time is an upper bound of the cost. The difference is
within the spread of the runs.

## Shapes that the derive does not take

Each of these shapes still needs a hand-written impl. The list says what
the derive would need to take it.

- **A literal that is not a call.** A newtype over `Option<Hash>` derives,
  and prints `None` or the hash. An enum whose variants print `None` and a
  hash does not derive: the derive prints a variant as a call. It would
  need a variant that prints as its field's literal, and a read that tries
  the variants in order, which makes the order of the variants part of the
  stored form.
- **An integer.** A newtype over `u32`, `u64` or `usize` derives. A signed
  integer has no `Literal` impl. It needs one impl for each signed type,
  beside the unsigned ones in `codec.rs`, and no change to the derive.
- **A set read in any order.** `BTreeSet` reads only its sorted print, so
  that a set has one print. A read in any order accepts more literals,
  which is a second language. A host gives such a field its own codec with
  `#[literal(with = path)]`, which the derive takes today.
- **An event head, a body of `schema::headed`.** A record with a
  `#[literal(sum)]` field prints the same calls as an `Event<B>`. The
  derive does not write `Body`: its `EVENTS`, `fields` and `parse`. It
  would need an impl of those three from a derived `Sum`: `EVENTS` from
  `headed_table` with the key's fields, `at` and `actor` as the head;
  `fields` from `Sum::arm`; and `parse` from `Sum::from_arm` and three
  reads of the head. `writes`, `asks` and `admits` stay the host's.
- **A generic record, such as `Patch<A>`.** The derive refuses a generic
  type. It would need to add the bound `A: Literal` to each type parameter
  of the impls. A record with a sum would also need its compile-time check
  of field names moved into an associated constant, because a `const _`
  item cannot name a type parameter.
- **A sum where one variant prints two constructors.** A variant prints
  one constructor, or the one constructor of the record that it holds. A
  variant that holds a sum, and prints that sum's constructors, would need
  the held sum's table in the outer sum's `CONSTRUCTORS`, which a `const`
  function can join as `headed_table` does, and a read that asks the held
  sum's table for the name of the call.
