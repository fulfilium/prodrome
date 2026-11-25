# Design: an inflationary register reads the greatest of all its writes

This note explains how Prodrome reads an inflationary register, and why.
For the rule as implemented, see `SPEC.md` §6 and law 32, and
`data/src/fold/climb.rs`.

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

`verify` checks each write to an inflationary register against the
product of the writes to those registers beneath it. It does not build that
product afresh. It keeps the product that each write made, and starts from
the product of the write's nearest writes beneath: the walk goes back from
the write and stops once the writes found, and what they rest on, cover
every write beneath. In a chain the nearest write is the one before, so the
pass is linear in the writes. Where branches are concurrent, a write has a
few nearest writes, and the pass joins the readings of the others into the
product of the first. The finding is the same for any order of the objects:
`climb.rs::verify_finds_below_what_the_pairwise_check_finds` compares it
with the pairwise check, which builds the product afresh for each write.

A schema with no inflationary register (`Product::climbing` is empty, as
for the todo schema) pays nothing: `verify` reads the product of one write,
sees no inflationary register, and does not fold the DAG.

`verify` on generated stores, in a release build: the best of five runs
for the two stores, and of two runs for the two long entities. "Pairwise"
is the first form of this change, which built the product afresh for each
write.

| Store | `main` | pairwise | this change |
|---|---|---|---|
| todo, 5959 objects | 11.8 ms | 32.0 ms | 11.7 ms |
| proposal, 2709 objects | 4.8 ms | 12.7 ms | 11.4 ms |
| one entity, 10,000 writes in a chain | 23.5 ms | 15,552 ms | 87 to 107 ms |
| one entity, 10,000 writes in two branches that merge every 100 | 23.9 ms | 25,768 ms | 78 to 90 ms |

A schema with an inflationary register still pays the check, about 60 to
85 ms for 10,000 writes beside the 24 ms of the rest of `verify`. Most of
that is a fold of the DAG, which `verify` does not otherwise make.

The wasm exports (`nix build .#prodrome-wasm`, `prodrome_bg.wasm`) grow
from 1,376,567 bytes to 1,380,680 bytes, by 4,113 bytes (0.3%).

The script machine's rate, in a release build over 128 todo scripts of up
to 80 steps (`script::a_long_run_of_todo_scripts_keeps_every_invariant`
with `PRODROME_MAX_CASES=128`), each script drawn afresh:

| Machine | Steps | Seconds | Steps per second |
|---|---|---|---|
| before this change | 5379 | 56.1 | 95.9 |
| with the check that no reading falls | 4965 | 48.8 | 101.7 |
| with the host's policy and hook (`run_with`) | 5438 | 57.4 | 94.7 |

The three rates differ by less than the runs differ from each other. The
check that no reading falls reads each entity twice after each step, and
the hook that `script::run` passes does nothing.
