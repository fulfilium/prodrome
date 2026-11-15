# Design: an answer reads a frontier's reach off the kept order

This note was written before the change was made. It answers two notes
from the review of pull request 65. For what was implemented, see
`SPEC.md`.

## The problem

A pull in pieces asks the store of record again and again, and each answer
names the frontier that the previous answer recorded (SPEC law 47). Today
each answer derives the frontier's reach, `Dag::closure` of the token's
tips, as a set of names. Each record then clones that set, adds the names
that it sent, and derives the tips of the result. For a piece near the end
of a pull, the reach is nearly the whole history, so every piece walks
nearly the whole history again. A pull of `p` pieces over `n` objects
costs time in proportion to `p × n`.

Measured on the DAG of the `frontier_answer` example (30,000 objects: 16
writers, each with its own chain, and a merge every 100 objects of a
writer), with pieces of 1,000 objects, on a DAG that does not change
during the pull. The figures are from three runs on one machine:

| step | time |
|---|---|
| one whole pull (30 pieces, answers and records) | 550 to 600 ms |
| the answer to the first piece | 3.5 to 3.8 ms |
| the answer to the last piece | 20 to 22 ms |
| in it: `Dag::closure` of the token's tips (29,000 names) | 16 to 24 ms |
| in it: the filter of the order by the reach | 3.5 to 4.9 ms |
| the record of the last piece (clone of the reach, tips of the result) | 8.4 to 9.7 ms |

## 1. Which representation of the reach

Two representations avoid the walk.

**A. By the token's tips, kept.** The closure of a set of names is a
function of those names alone, because a name fixes its object and so its
parents. So `Frontiers` could keep, in memory, the reach that it computed
for each recent token, and use it again when a later answer names that
token. A puller never names the same token twice, though: each piece
records a new frontier. So the kept reach must grow with each record:
`reach(new) = reach(old) ∪ sent`. The cost is one set of up to `n` names
for each frontier that is kept, about 3.6 MB at 30,000 objects. Every
answer still filters the whole order by the set (about 3.5 ms), and every
record still clones it.

**B. Read off the kept order (recommended).** Pull request 65 made `Dag`
keep its linear order. The objects of a pull are sent in that order, so
for a pull from nothing over a DAG that does not change, the objects that
a replica holds after `k` pieces are exactly a prefix of the order: the
first objects, up to some count `m`. A prefix of a linear extension is a
down-set, and a down-set is the closure of its tips. So the token's tips
are the tips of that prefix, and the reach is the first `m` objects. The
answer can then name its reach by the count `m`, and the objects that it
lacks are the rest of the order, `order[m..]`, with no set and no filter.

The answer must decide, without a walk, whether a token's tips `T` are the
tips of a prefix. Three facts, which the DAG derives once from its order
and keeps beside it, as it keeps the order:

- `position`: the place of each object in the order;
- `first_child`: for each object, the least position of an object that
  names it as a parent, or the end of the order if no object does;
- `tips_of_prefix`: for each `m`, the number of tips of the first `m`
  objects, computed in one pass over the order.

Then let `m` be one more than the greatest position in `T`. Every
ancestor of a tip comes before it in the order, so the reach of `T` is a
subset of the first `m` objects. The reach is all of them exactly when the
tips of the first `m` objects are `T`. That holds when every `t` in `T`
has `first_child(t) ≥ m`, so `t` is a tip of the prefix, and when
`tips_of_prefix(m)` equals the size of `T`, so the prefix has no other
tip. The check costs a lookup for each tip in `T`, not a walk.

The record of a piece that sends the next `s` objects then derives its
tips from `T` and the sent objects alone. The tips of the first `m + s`
objects are the members of `T`, and the objects of `order[m..m + s]`,
whose `first_child` is at least `m + s`. So a record costs time in
proportion to `|T| + s`.

When `T` is not the tips of a prefix, the answer walks, as it does today.
This happens after a post (`Lacking::record_with` adds objects from
anywhere in the order). It also happens when the DAG grew after the
token was recorded: the new objects can take places among the old ones,
because the order breaks ties by name. In those cases the answer costs
what it costs today, so no case is slower than today.

A prototype of B in the measurement program costs about 40 ms to build
the three kept facts once (about one more linearisation), and then about
0.1 ms for each answer, including its check and the tips of its record.
On the measured pull, that would make the whole pull about 40 ms plus the
lookups of the objects that each answer lists, in place of 550 to 600 ms.

**What B costs in memory.** It needs `first_child` and `tips_of_prefix`
(two integers for each object), and a map from a name to its position. As
a `BTreeMap<Hash, u32>`, that map costs about as much as the kept order:
about 120 bytes for each object, or about 4 MB at 30,000 objects. The DAG
derives the three facts only when an answer first asks for them, and lets
them go with the order when the objects change, through the same
`History::edit`.

**The API.** `Reached::Known(BTreeSet<Hash>)` gives the reach as a set,
and a host can read it. To answer without building that set, `Reached`
gains a variant that names a prefix:

```rust
pub enum Reached {
    Everything,
    /// The first `count` objects of the store's linear order.
    Prefix { count: usize },
    Known(BTreeSet<Hash>),
    Unknown,
}
```

This is a breaking change for a host that matches `Reached`
exhaustively, so the CHANGELOG says so. The answer's bytes do not change:
`lacking()` lists the same objects in the same order, and `record` records
the same tips.

## 2. The law

64. **A frontier that is a prefix reads off the order.** When the tips
    that a token names are the tips of the first `m` objects of the
    store's linear order, the answer reaches exactly those `m` objects
    (`Reached::Prefix { count: m }`), and it lacks the rest of the order.
    Its lacking list, and the frontier that each `record` records, equal
    those of the answer that walks the closure. Every piece of a pull
    from nothing over a DAG that does not change records the tips of a
    prefix.

A property test checks the law over the generated two-writer histories
that law 47 uses, on pulls with every size of piece. It compares each
answer with the answer that the walk gives, and it checks the second
sentence for each piece. Law 47's own tests stay as they are, and they
must still pass.

## 3. The clone in `Dag::tips`

`Dag::tips` returns a clone of the kept set. On the measured DAG (16 tips)
a call takes 0.6 µs once the set is kept, and the first call, which
derives the set, takes about 3 ms. An answer asks for the tips once
(`Frontiers::answer`), and so does `Memory::over`. `Dag::tips_in` asks
once and filters. No reader on the path of an answer asks more than once,
and a quiet answer takes about 2.6 µs in total. So the clone is on no hot
path, and this note proposes no change. A borrowed return would also
change a public signature for no measured gain.

## Questions for the reviewer

1. Is B, with the walk as its fallback, the right scope? Or should the
   change also keep A's reach by token, for pulls during which the store
   grows?
2. Is a breaking new variant of `Reached` acceptable, or should the prefix
   stay internal, with `Reached::Known` built only when a host asks for
   the set?
