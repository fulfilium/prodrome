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
  or none.
- A struct with one unnamed field is a newtype, and its literal is its
  field's.

The derive refuses, with a message that says what to write: a generic type,
a struct with two or more unnamed fields, a variant with unnamed fields, an
enum without variants, a union, two constructors of one sum with one name,
two fields of one constructor with one name, and a stored name that the
grammar cannot read.

## Stored names

A constructor's name and its fields' names are in every stored object, so a
change of a name is a change of the stored form. The rule: a stored name is
the Rust identifier, unless `#[literal(rename = "...")]` gives one. Every
derived type gives its constructors as data, in `Literal::CONSTRUCTORS`. A
schema pins the constructors of its types in one test, and a rename that
changes a stored name fails that test. `core/tests/all/derive.rs` pins the
reference payload's four constructors. The conformance vectors also hold
these names, so a rename fails them too.

Names given only in attributes were not chosen. They would spell each name
twice, once as the identifier and once in the attribute, which is the
repetition that the derive removes. One pin in one test catches a rename in
one place.

## The same bytes and the same refusals

A derived read reads each field with `Literal::from_field`, which takes the
field's context, such as `Note.lines`. A leaf names the context in its
refusal, as the payload helpers did (`Note.lines must be a tuple`). The
payload helpers in `payload.rs` are now reads of the same functions, so a
hand-written read and a derived one give the same messages.
`Fields::get` keeps its own messages, for the hand-written impls that use
it.

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
- `dag::Heads`: its reads use the messages of `Fields::get`, and the store
  reports them. A derived impl would change those messages.
- `Span` in `core/examples/memo_file.rs`: its literal is a tuple, which a
  derived impl does not write.
- `Verdict` and `Review` in `core/tests/all/codec.rs`, and `Thread` in
  `core/tests/all/derive.rs`: tests of the hand-written API, and of a
  hand-written impl that holds a derived one.

The event layer (`TodoEvent` and the envelopes) stays hand-written. It is
generic over the payload, and its record kind takes its name from the
payload. A derive for a generic type, and for the head of an event in
`schema::headed`, is left for later work.
