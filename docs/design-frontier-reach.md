# Design: an answer reads a frontier's reach in the positions of the kept order

This note was written before the change was made, and revised after a
review on the pull request. It answers two notes from the review of pull
request 65. For what was implemented, see `SPEC.md`.

## The problem

A pull in pieces asks the store of record again and again, and each answer
names the frontier that the previous answer recorded (SPEC law 47). Today
each answer derives the frontier's reach, `Dag::closure` of the token's
tips, as a `BTreeSet<Hash>`. Then it filters the whole order by that set.
Each record clones the set, adds the names that it sent, and derives the
tips of the result. For a piece near the end of a pull, the reach is
nearly the whole history. So a pull of `p` pieces over `n` objects costs
time in proportion to `p × n`, and each unit of that work is an operation
on a 64-character name: a comparison of strings, a map lookup, or a copy.

The measurement program `core/examples/frontier_pull.rs` pulls the DAG of
the `frontier_answer` example (30,000 objects: 16 writers, each with its
own chain, and a merge every 100 objects of a writer) from nothing, in
pieces of 1,000 objects, on a DAG that does not change. It pulls twice:

- **store order**: each piece is the next objects that `lacking()` lists,
  recorded with `record`;
- **host order**: each piece is the next objects of a linear extension of
  the host's own, which takes the chain of the writer with the latest
  object first. The host records each piece with `record_with`. Each piece
  is closed under parents over what the replica holds. None of the 29
  frontiers of this pull is a prefix of the store's order.

Today, from three runs on one machine:

| pull | total | last answer | last record |
|---|---|---|---|
| store order | 518 to 614 ms | 20 to 21 ms | 7 to 9 ms |
| host order | 837 to 1,047 ms | 22 to 29 ms | 37 to 48 ms |

## 1. The first version of this note, and why it is not enough

The first version proposed to read the reach off the kept order when the
token's tips are the tips of a prefix of the order (B). A prototype took
about 40 ms to build its facts once, and then about 0.1 ms for each
answer, so the store-order pull would take about 40 ms. But a host that
sends pieces in its own order and records them with `record_with` holds
down-sets that are not prefixes. On the host-order pull, B's check fails
on every answer, and the pull costs what it costs today. A prefix is a
special case of a down-set, so the general piece is a representation of
any down-set that is cheap to walk.

## 2. The general piece: a reach is a set of positions

What makes the walk slow is that it runs over names. The kept order gives
each object a position, so a reach can be a set of positions, with one
bit for each object of the order. For 30,000 objects that is under 4 KB.

**What the DAG derives beside its order.** `History` derives an index of
the order in the same pass as the order, and lets it go with the order
through `History::edit`. The tips, the order and the index are one value
that is not generic over the schema, derived from one small generic
function that lists each object's name with the names it rests on. The
index holds:

- each object's parents, as positions: one flat array of positions, and
  for each position the start of its parents in that array;
- the positions sorted by the name of the object at each position. A
  binary search over that array finds the position of a name, and it
  copies no name.

That is three arrays of integers: about 8 bytes for each object for each
of the first and the last, and 8 bytes for each parent edge. At 30,000
objects that is about 0.7 MB. Nothing is kept for each token.

**The operations, for any token.** A `Reach` is a set of positions over one
DAG's order:

- The closure of the token's tips is a walk over positions, with a bit
  test in place of an insert into a set of names.
- What the answer lacks is the order at the positions whose bit is clear,
  in the order.
- A record sets the bits of the objects that it sent. `record` sets them
  by position, because the objects that it sends are the first objects
  that the answer lacks. `record_with` walks the closure of the names that
  the host sent.
- The tips of a reach are its members that no member names as a parent.
  The record marks the parents of each member in a second set of bits,
  and keeps the members that are not marked.

A prefix is a reach whose bits are the first `m`. It needs no check and no
special case. When the store grows between two pieces, the DAG lets its
order and its index go and derives them again, and a token's tips are
read again in the new positions. So nothing can be wrong after growth.

**The API.** `Reached::Known` holds a `Reach` in place of a
`BTreeSet<Hash>`:

```rust
pub enum Reached<'d> {
    Everything,
    /// The objects that the frontier's tips rest on, the tips included.
    Known(Reach<'d>),
    Unknown,
}

impl Reach<'_> {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn contains(&self, name: &Hash) -> bool;
    /// The names, in the store's linear order.
    pub fn iter(&self) -> impl Iterator<Item = &Hash>;
}
```

A `Reach` borrows the DAG's order and index, so `Reached` gains a
lifetime, the lifetime that `Lacking` already has. No answer builds a
`BTreeSet<Hash>`. A host that wants one collects `iter()`. This change
breaks a host that reads the set today, and the CHANGELOG says so under
Changed. The bytes of an answer do not change: `lacking()` lists the same
objects in the same order, and each record records the same tips and the
same count.

The new types go in new files: the index in `dag/src/dag/index.rs`, and
`Reach` in `dag/src/dag/reach.rs`. `Frontiers` stops calling
`Dag::closure` and `tips_among`, which stay for their other users.

## 3. The law

64. **A reach in positions is the reach in names.** For any token whose
    tips the store holds, the answer reaches the closure of those tips:
    the `Reach` contains exactly those names, and its length is their
    count. The answer lacks the other objects, in the store's linear
    order. Each `record` and each `record_with` records the tips of what
    the answer reaches joined to what was sent (for `record_with`, with
    what that rests on), and the count of that set.

The specification is the walk over names, which the test keeps for
itself: the closure of a set of names, and the names of a set that no
member names as a parent. The property test draws pulls over the
generated two-writer histories that law 47 uses:

- pulls in the store's order, with every size of piece;
- pulls in another linear extension, recorded with `record_with`;
- posts between pieces;
- a store that grows between two pieces.

It compares each answer, each `record` and each `record_with` with the
specification. Law 47's tests stay as they are, apart from reading the
reach through the `Reach` API in place of the set.

## 4. Measurement

The measurement program, before and after the change, from three runs on
one machine. B is the prototype of the first version of this note: about
40 ms for the store-order pull, and today's cost for the host-order pull,
on which its check fails for every frontier.

| pull | today | B | positions |
|---|---|---|---|
| store order, total | 518 to 614 ms | about 40 ms | 84 to 88 ms |
| store order, last answer | 20 to 21 ms | about 0.1 ms | 0.35 to 2.2 ms |
| host order, total | 837 to 1,047 ms | as today | 77 to 87 ms |
| host order, last answer | 22 to 29 ms | as today | 0.41 to 0.65 ms |
| host order, last record | 37 to 48 ms | as today | 0.47 to 0.64 ms |

The walk over positions no longer shows in an answer: the answer to the
last piece takes under 1 ms in most runs. Two costs remain. The first
answer of the store-order pull takes 21 to 25 ms, because it derives the
index once. The rest of a pull's time is the list of the objects that
each answer lacks: `lacking()` gives each one with its object, which costs
a map lookup for each object listed, and an early answer lists nearly the
whole store. A reach kept for each token would save none of that, so this
change keeps nothing for each token.

## 5. The clone in `Dag::tips`

`Dag::tips` returns a clone of the kept set. On the measured DAG (16 tips),
a call takes 0.6 µs once the set is kept, and the first call, which
derives the set, takes about 3 ms. An answer asks for the tips once, and
so does `Memory::over`. `Dag::tips_in` asks once and filters. No reader on
the path of an answer asks more than once, and a quiet answer takes about
2.6 µs in total. So the clone is on no hot path, and `Dag::tips` does not
change.
