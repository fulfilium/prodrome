# Design: an inflationary register reads the greatest of all its writes

This note explains how Prodrome reads an inflationary register, and why.
For the rule as implemented, see `SPEC.md` §6 and law 32, and
`core/src/fold/climb.rs`.

## The fault

A register type can be declared INFLATIONARY: each write must stand at or
above the reading that it supersedes. `Replica::append` refuses a write
that does not. A receipt does not refuse it: a receipt takes every valid
object, so that two stores whose files are put together converge, whatever
the order in which the objects arrive. So another writer can build the
refused write by hand, as a change whose deps name the higher write, and
every replica takes it.

The fold read each register from its frontier: the writes that no other
write descends from. The hand-built write descends from the higher write,
so it replaced the higher write in the frontier, and the reading fell.
`verify` found nothing. An application showed the fault with a store of
proposals: a proposal is dismissed, and a write of "tried once, not sent"
over the dismissal is refused by the append and taken by a receipt. The
fold then read the proposal as one that can be tried again.

So law 32, "an inflationary register only climbs", held only for histories
that appends wrote. A law that holds only for honest writers does not
describe the objects a store can hold.

## What other libraries do

The document CRDT libraries and the lattice-based languages define a
grow-only register in one way: its value is the join of every write that a
replica has seen.

- In the state-based CRDTs of Shapiro, Preguiça, Baquero and Zawirski
  ("A comprehensive study of Convergent and Commutative Replicated Data
  Types", 2011), a max register and a grow-only set merge by the least
  upper bound of the two states. A write below the state has no effect.
  Nothing in the state records which write a writer saw.
- In LVars (Kuper and Newton, 2013), a `put` joins its value into the
  variable's state, so a write below the state changes nothing, and a
  read is a threshold read, which cannot see the order of the writes.
- Bloom^L's `lmax` and `lset` lattices, and the delta-state CRDTs of
  Almeida, Shoker and Baquero, are joins over everything received.
- The registers of Automerge and Yjs that may go back are multi-value
  registers: they keep the causal frontier of the writes, as Prodrome's
  registers that are not inflationary do. None of these libraries reads a
  grow-only register from a causal frontier.

The reason is the same in each library. If the value depends on which write
descends from which, a writer who builds a write with chosen deps can lower
the value. If the value is the join of the writes, the value is a function
of the set of writes, and no arrival order and no choice of deps changes it.

## The definition this change takes

An inflationary register reads the maximal values of ALL its writes, under
the order that its type declares. This is the join that the libraries
above use, for an order that is partial.

Where the order has no join of two values, as for `dismissed` and `failed`
in the proposal machine, the maximal values form an ANTICHAIN, and the
reading holds both: a conflict, as a frontier's reading is. The antichains
of an order, with "the maximal elements of the union" as the join, are a
join-semilattice: an antichain is the set of maximal elements of a down-set,
and the join of two antichains is the antichain of the union of their
down-sets. The reading of a set of writes is then the antichain of their
down-set, so

    read(h₁ ∪ h₂) = max(read(h₁) ∪ read(h₂))

for any two object sets `h₁` and `h₂`, written honestly or not. Where
`h₁ ⊆ h₂`, each value that `h₁` reads stands at or below a value that `h₂`
reads, so the reading never falls as objects arrive.

Each value in the reading is named by the least write that carries it, as
in every reading. To choose among writes of one value, the register takes
only the writes that no write of an equal value descends from. In a history
where every write stands at or above every write that it rests on, these are
exactly the frontier's writes of that value. So the reading of every honest
history is the frontier's reading, write for write, and no honest reading
moves.

The append keeps its refusal, and `receive` keeps taking every valid object.
`verify` reports a write whose value is not at or above every write to its
register that it rests on (`Finding::Below`), so the fault is said, and a
host can find the writer.

The alternative, to keep the frontier and make `receive` refuse a write
below its reading, would make the set of objects that a replica holds
depend on the order of arrival: a replica that received the lower write
before the higher one would hold it, and one that received them in the
other order would refuse it. Two replicas could then never converge by
exchanging files, so this change does not take it.

## How the register is kept incremental

A `Climb` holds two sets. One is the frontier, which a conflict, the deps
of an append and the readers of ancestry still need. The other is the
PEAK: the writes that no other write OUTRANKS. A write `u` outranks a write
`w` when the value of `u` is strictly greater than that of `w`, or when
the values are equal and `u` descends from `w`. This relation is a strict
partial order on writes, so the peak is a function of the set of writes,
and a join can maintain it: a new write removes the writes that it
outranks, and joins unless a write of the peak outranks it. A join compares
the new write with each write of the peak, so its cost grows with the
number of incomparable maximal values, not with the number of writes.

The fold at an instant, under a policy, and at a down-set joins the same
writes that it joined before, into the same register. The cached readings
(`memo::fold`) are readings of folds, so they need no change.

## Cost

Measured on generated stores only, in a release build, each figure the best
of five runs. Each store is written by appends from two replicas in memory
that sync every 13 writes: 6000 writes over 100 entities.

| Store | Fold and read every register | `verify` |
|---|---|---|
| todo (6001 objects), before | 10.7 ms | 13.2 ms |
| todo (6001 objects), after | 8.8 ms | 36.3 ms |
| proposal (2729 objects), before | 4.1 ms | 4.7 ms |
| proposal (2729 objects), after | 3.5 ms | 12.9 ms |

The fold does not get slower: the difference is within the noise of the
runs. The proposal store holds fewer objects than writes, because the
append refused the writes below their reading.

`verify` checks each write against a product of the writes that it rests
on, in its entity. That costs a join for each pair of writes of one entity
whose later write rests on the earlier one, the same order as the ancestry
index that the fold already keeps, and it is the cost that the table shows:
about three times the time of `verify` before. `verify` pays it for every
schema, also for one with no inflationary register, because a product says
whether a write falls only through `Product::grows`.
