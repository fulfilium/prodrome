# Simplify: one functor, one schema, one evaluator, one fold

The owner, 2026-09-28: the core ideas are small, so the code should be;
expressive types and names, sparse comments, a mental model readable from the
module names. The reference is his PULER (`TypeSystem.hs`): base functors with
recursion schemes, `Semigroup` instances that are the semilattice, laws in
instances rather than prose.

A file is its main type: one concept per file, the file named for it, the
type declared first after the imports. The layout below names each file by
the type it opens with.

## Where the weight is

Production code is about 11.6k lines (core 8.2k, wasm 1.8k, cli 0.9k, github
0.8k). The design is small; the weight is one field list written by hand
many times.

- `TermF`'s fields are enumerated about twelve times: `map`, `transpose`,
  `children`, `kind`, `to_value`/`from_value`, `TERM_SIGNATURES`,
  `scalar_notes`, the wasm JSON codec and `explanation_json`, `Link::scalars`,
  and a copy of the JSON codec in an application.
- `explain` repeats `eval`'s time routing and calls `eval` at every node.
- The literal codec helpers exist three times (`fpl`, `payload`, `event` and
  `reference`).
- The event classification is written six times (`env_at`, `specs_at`,
  `authored_at`, `history`, `flatten`, `writes_of`), and `fold::Binding` and
  `fold::Env` duplicate `fpl::Outcome` and `fpl::Env`.
- The DAG is read and verified twice (the store and wasm `verify_objects`).

## The target

```
core/src/
  literal/    value, print, parse, codec (trait Lit { to_value, from_value })
  time.rs     Instant, Delta, iso, power_mean
  term/       functor (TermF, traverse, map, cata, para)
              schema  (fields(&TermF<A>) -> (kind, [(name, Slot<&A>)]); build(kind, src))
              read    (trait Reading; read<R: Reading>(&Term, Instant, &Env) -> R; Option<f64> and Explanation)
              smart, normalize, link, breaks (a Monoid), chain
  event.rs    Event<P> { todo, at, actor, body: Body<P> }, Envelope<P>
  dag/        objects (Dag<P>: from_prints, tips, closure, linearise, verify -> [Finding]), store (files around a Dag)
  fold/       write (Write = State | Tend | Spec | Content | Items), register (trait Register: join;
              LastWrite, Timeline, GrowSet, Frontier), entry
  payload.rs, policy.rs, reference.rs
```

The schema is the one place a term's fields are named. The literal printer
and parser, the wasm JSON, the explanation notes and an application's codec all
render it. One evaluator produces a number or an explanation. One fold,
parameterised by the register, gives §6.1–6.6.

Estimated size: about 7.3k production lines, most of the 6.3k test lines kept.

## Must not change

Stored bytes (constructor names, field order, `''` as absent, parse defaults
such as a missing `lead_up` meaning one week), the conformance vectors and
`wasm/conformance/term-json.json` (including its quirks), the verify finding
strings, fulfillment to 1e-9, the twelve wasm functions and their JSON that
an application's web page calls, and the hot-path performance (the `Arc` writes, notes
never built when only a number is asked for). Laws 4 and 6 compare two
implementations: keep them two.

## Stages

Each is one PR, green on the workspace tests, the wasm tests and the wasm
tests of an application that calls them.

0. Snapshot tests: the bytes of all twelve wasm functions over one DAG
   vector, and the verify finding strings.
1. The functor: `traverse`, `cata`, `para`; port the predicates,
   `breakpoints` (a Monoid), `normalize`, the linker's cycle witness.
2. The schema: literal codec first (print vectors, law 1, the fuzzer), then
   the wasm JSON (term-json), then the notes.
3. One evaluator (fpl, series, recur and calibration vectors).
4. `literal::codec` for events, references and payloads (print and hash
   vectors).
5. The `Dag` value, with wasm moved onto it. Coordinate with the change
   identity design (PR #7), which replaces the envelope: keep envelope
   changes at the `Dag` seam.
6. `Write` and the registers; `Env` and `Outcome` unified.
7. The event header and body split, bytes unchanged. Stages 4, 6 and 7 reach
   an application in one pin bump.
8. Boundaries: the wasm wire from the schema, one `Price` for cli and github,
   an application drops its codec.
