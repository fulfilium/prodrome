# Design: the empty term, and the two layers

Status: implemented, 2026-09-28 (Unreleased). SPEC §7, §7.2 and law 18 of
§9 are the contract; this page is the reasoning.

## Why

Some objects have no claim on attention: a note, a reference, a proposal
nobody has priced. Today that is `spec: Option<Term>`, and the absence leaks
out as special cases: rows with no value, a `Ref` to an unpriced todo that
fails to link, and every reader deciding for itself where the unpriced rows
go. A term that means "no temporal value" makes FPL total, and turns those
cases into ordinary evaluation.

## The term

`Absent` is a new leaf of `TermF`. Its value is `∅` at every instant, so a
term's value becomes `[0, 1] ∪ {∅}`. `∅` is the identity of composition: it
neither raises nor lowers anything it is composed with.

| Term | With `∅` |
|---|---|
| `Absent` | `∅` |
| `Conj(terms, p)` | the power mean over the members that have a value; `∅` when none do |
| `Gate(gate, body)` | an absent gate is no gate: `⟦body⟧`; an absent body is `∅` |
| `Offset`, `Shift`, `Importance`, `OffsetBy` | `∅` in, `∅` out |
| `Within(window, p, a)` | the power mean over the samples that have a value; `∅` when none do |
| `Piecewise` | the piece in force, which may be `Absent` (priced from a date, or unpriced after one) |
| `After`, `Recur` | `pending` may be `Absent` |
| `Ref(x)` for a known todo with no spec | links to `Absent`, no longer a refusal |

`LinkError::Unknown(x)` stays for an id the store has never seen, and
`LinkError::Cycle` is unchanged.

**No stored byte moves.** `Absent` is a new constructor (§1). A record whose
`spec` is `None` reads as `Absent`, so an old store reads exactly as before,
except that a `Ref` to an unpriced todo now links. `Conj([])` keeps its
stored meaning of 0.5; only a `Conj` whose members are all absent is `∅`.

**Readers.** `entry.value` becomes a number, `absent` or a `LinkError`. A list
orders absent rows after every priced one, by id, so ordering is defined in
one place and not by each host.

**Laws** (§9, with vectors):
- `Conj(ts ++ [Absent], p) = Conj(ts, p)` whenever `ts` has a value.
- A term with no `Absent` evaluates exactly as it does today.
- `link(Ref(x))` is `Absent` exactly when `x` is known and has no spec.
- `explain` and the series carry `∅` where a node has no value, and `Absent`
  is exact, with no breakpoints.

## The two layers

With every spec `Absent`, a store is close to git's object database:
immutable objects named by their hash, a DAG of parents, union as merge, and
tips derived from the objects, which is git without refs. It differs from git
in three ways:
- it stores events, one fact about one entity, and not snapshots;
- time is data, so a fold answers "what was believed at `t`";
- a policy decides which writers bind and which only claim.

FPL, meaning terms, fulfillment and composition, is the layer on top that
turns that store into a prioritisation database. The empty term is what makes
the boundary clean: an unpriced object is simply a layer-one object.

This is a direction, not a split. The crates stay as they are, in this
repository, until the code shows a boundary worth a crate of its own.

## What it enables

A host can keep objects that are not todos in the same store, each with its
own payload type and, when it matters, its own price. One application's next use is
proposals: a draft email or a changeset is an object whose id is the hash of
its content. Performing it appends its completion, carrying the response. A
second submission of the same object reads that completion and returns the
same response, so the effect runs once. Its price is usually `Ref` to the
todo it serves.
