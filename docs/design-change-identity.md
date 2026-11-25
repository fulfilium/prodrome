# Design: a change is named by what it says and what it supersedes

Status: IMPLEMENTED, 2026-09-30. The contract it proposed, "Draft A", is
folded into SPEC.md's §3, §5, §6, §7 and §9 (laws 19–29). This page is the
reasoning. The overlay (F) is later work.

Reviewed once. The first round's four open questions are settled: the
agreeing-twins rule is in, `Created` and `Tended` depend on nothing, terms
add no deps, and `merge` stays for legacy stores. Sections A to G below answer
the second round.

**Superseded in part, 2026-10-01: a change's deps are its entity's heads.**
§2 below decided that `deps` are the frontiers of the registers the event
writes. That recorded one writer's add (the content register) and complete
(the state register) of the same entity as concurrent, though the writer
wrote one after the other: nothing causal ordered them, so a reader that
needed "which came last" (an undo, a history in order) fell back on the
stamped instant or the name, and a host stamped its acts a microsecond
apart to order them. That is time deciding order, which SPEC §1 forbids.
`deps` are now the entity's HEADS: every write to it the writer holds, in
any register or none, that no other write to it descends from. Each written
register's frontier is beneath them, so supersession is unchanged and per
register: a write supersedes exactly the writes to its own registers it
descends from. The alternative, "the written registers' frontiers plus the
other heads seen", is the same set once reduced to an antichain, which §5
requires, so it adds a case and nothing else. What is given up is that
writes to different registers of one entity, made by a writer that saw one
before writing the other, commute as names: the second's name now records
the first. No reading changes with it (each register's frontier and
reading are as they were); changes to different entities stay independent.
Law 19's test is now over other entities, not other registers; laws 36
and 37 are new (SPEC §9). Objects written before this keep their bytes and
read as they did.

## The problem

Reading the store the way the Pijul maintainers would:

1. **A change is named by its POSITION.** `append` seals the event on every
   tip (§3), so an object's name hashes the whole global tip it was written
   on. The same logical change, made on two replicas, re-proposed after a
   rejection, or resubmitted by a host that retried, is two different
   objects. So idempotence, "the second time is a no-op", is not a property
   of the database. Every host that needs it builds it again.
2. **Its dependencies are everything before it**, never what it needs. A
   `Completed` on todo `a` "depends on" an unrelated edit to todo `b` because
   `b` happened to be at the tip. So two changes to different todos are never
   independent in the DAG, only in the folds.
3. **A merge is an object.** A `Woven` over two tips is a fact written by
   whoever merged, named by what they held.

The evidence is in this repository. `prodrome-github` works around point 1
three times. It dedups replays by "an event whose canonical print the store
already holds is not written again" (`github/src/lib.rs`, module doc). It
dedups `/price accept` by `(todo, actor, at, note)` (`Known::accepted`). And
it dates the bot's claims at the latest instant of EVERY event in the store
(`proposed`), because §3's clock rule is checked against ancestors and a
claim's ancestors are the whole store. Each of these is the database's job.

## What stays

The registers (§6.6) already are Pijul's best idea: a conflict is a VALUE (a
frontier of two or more writes), it is shown, and the next write that
descends from it settles it. What is missing is only at the bottom: what a
write descends FROM.

So these do not move: events (§4), standing (§5), content addressing (a name
is the hash of the print), the union of files as the merge, and every stored
byte. `Sealed` and `Woven` stay readable forever (§1: add kinds, never change
shipped fields).

## The core proposal

### 1. Three new objects

```
Genesis(label='home', nonce='9f2c…')
Change(genesis='<name>', deps=('3f…', 'a0…'), event=Completed(todo='a', …))
Snapshot(genesis='<name>', tips=('…', '…'), previous='<name>' or '')
```

- `Genesis` starts a prodrome (A below). `Snapshot` attests a state (C
  below).
- `Change`: `deps` is a tuple of object names, sorted and distinct, possibly
  empty. `event` is required: a change without an event would be a merge, and
  there are none.
- Every name is `sha256(utf8(print(object)))`, as today.
- `parents_of(Change(_, deps, _)) = deps` and `parents_of(Snapshot(_, tips,
  previous)) = tips` plus `previous` when set. That is how §3's tips,
  ancestry, linearisation, `adopt` and `verify` read them.

### 2. `deps` is the frontier of the registers the event writes

When a writer appends `event`, `deps` is the union, over the registers
`writes_of(event)` names (§6.6), of that register's frontier in the writer's
store. The frontier is read STRUCTURALLY, under the policy where everything
binds and with no `t`. The name has to be a function of what the writer held,
and never of a reader's policy or moment. `Created` and `Tended` write no
register, so their `deps` are `()`.

**Every register is one todo's, so `deps` never leave a todo.** A store of
changes is a disjoint union of per-todo DAGs, each rooted at empty-deps
changes. Nothing in the old design had that shape, and D and G lean on it.

What this buys: **a change's name is a function of its genesis, its event and
the part of the past it supersedes, and of nothing else.** Two replicas that
agree about `a`'s state register write byte-identical `Completed(a)` objects,
whatever each holds about every other todo.

### 3. `append` is idempotent

`append(event)` first asks whether the store holds, UNDER THE SAME GENESIS,
an object whose event prints byte-identically to `event`. If it does, it
writes nothing and answers the first such object in the linearisation.
Otherwise it writes `Change(genesis, deps, event)`.

The check covers the whole genesis and not just the current frontier. Take
this case: the bot proposes `P`, a maintainer overrides it with `M`, and then
the delivery that carried `P` is replayed. A frontier-only check would miss
`P`, because `M` has superseded it. It would write `P'` over `M` and bring the
rejected proposal back in the claimed reading. A host that means to assert
`P` again, having seen `M`, is saying something new, and its event differs:
a new `at`, a new note. So identity is exactly as good as the host's
determinism. `at` is data (§1), and the host stamps it.

**The event key.** `event_id(e) = sha256(utf8(print(e)))` is the name of an
event apart from where it was written. It is never stored. `append` uses it
for the check above, and F uses it to recognise a re-proposal.

### 4. No merges, and twins

`append` never writes a `Woven` into a store of changes. After a union of
files, the next write to a register depends on whatever that register's
frontier holds, including a conflict it settles.

Two replicas that disagree about a register can each write the same event
over it: `e₁` over `{x}` and `e₂` over `{y}`. Those are TWINS, two objects
with one `event_id`. They stay distinct, and each supersedes only what its
own deps reach. **A frontier whose writes all carry one event is one
candidate.** Treating twins as one node (the quotient) was rejected: it
would let `e` settle `y` on a view nobody who wrote `e₁` had, and a late twin
would move a node that was already positioned. Naming a change by its event
alone was rejected too: two replicas would write different bytes under one
file name, and the union of files could conflict.

### 5. What `verify` checks, and stores written before this

`verify` checks everything it checks today, reading `deps` as parents, plus:

- `deps` sorted and distinct, and an antichain;
- every dep has the change's genesis and names the change's todo;
- every dep writes at least one register the event writes;
- a snapshot's genesis and closure are well formed (C).

The clock rule (§3) keeps its wording and reads narrower ancestors: a writer
cannot be dated behind what it wrote OVER.

A store with no new object derives and verifies exactly as it did, and reads
exactly as it did except under a conflict (B). In a mixed store, a `Change` may depend on `Sealed` and `Woven` writes.
Their global ancestry overstates what they depend on, which is safe: a later
write can only settle more than the chain said, never less. A reader without
this draft refuses the new constructors by §2's whitelist, as it did for
`Tended` and `Absent`. Under CHANGELOG.md's rule this is a MINOR change.

## A. Genesis, and a set of prodromes

**Decision: a `Change` carries its genesis, as a field.** The owner's
direction is that a prodrome is identified by its genesis. So the same
`Created(a)`, with the same `at` and actor, written into two prodromes must
be two objects. The alternative was to leave the field out and have
empty-deps changes depend on the genesis object instead. That makes "which
prodrome" a walk to the root, and it allows an edge that crosses geneses,
which then has to be refused. A field is checked in O(1) per edge by
`verify`, and it keeps "`Created` depends on nothing" literally true.

**What a genesis is.**

- In a store made only of changes, it is a `Genesis(label, nonce)` object.
  `label` is the host's human name for the prodrome. `nonce` is 32 hex
  characters drawn once at creation, so that two prodromes a host calls by
  the same label are still two. The prodrome's identity is the object's name.
  `Genesis` has no parents and no event. `init` writes it, once.
- In a legacy store, it is the `Sealed("", …)` object at the root. A `Change`
  added to a legacy store names it. A legacy store whose objects already
  reach two roots (two geneses woven together before this draft) is ONE
  prodrome whose genesis is the least root by name. `verify` reports it as a
  joined legacy forest, and nothing moves.

**A set of prodromes is a prodrome.** A store may hold objects of several
geneses: a forest. Unions of files stay unions.

**The disjointness law.** Every object has exactly one genesis. No `deps`
edge and no snapshot edge crosses geneses. So the union of two stores of
different geneses is DISJOINT: no object belongs to both, and no fold of one
can see the other.

**Folds factor per genesis.** Every fold, register state and entry of a
union equals the union of the per-genesis ones. A todo is keyed by `(genesis,
todo)`: the same `TodoId` in two prodromes is two todos. `Entry` gains its
genesis. In a single-genesis store every row reads as today, so the
conformance vectors do not move. `list_order` breaks value ties by `(genesis,
id)`. `append`'s idempotence check (3) and the frontiers it reads for `deps`
(2) are scoped to the writer's genesis.

**The qualified `Ref(genesis, todo)` is left to a later design, and nothing
here blocks it.** `link` already takes a `specs` map. Per genesis, a
qualified reference is a lookup into another genesis's map, and `link`'s
cycle path generalises to `(genesis, todo)` steps. An unqualified `Ref(todo)`
means "in my own genesis", which is what it means in every store today.

## B. A conflict prices as its most urgent candidate

**Today.** A register with more than one write is projected onto the write
"latest in the linearisation": the one with the greatest name among
incomparable ones. That is deterministic and arbitrary. Between `Completed`
and a concurrent `Reopened`, a hash decides whether the todo reads as done.
The conflict is shown beside the answer, but the price hides the urgent side.

**The rule.** A register's reading is its CANDIDATES: the distinct events
of its frontier (agreeing twins are one). A todo's price under a conflict is
the MINIMUM fulfillment over its candidates. Low is urgent, so a conflict
rises and never sinks. Nothing picks a winner.

**A new FPL leaf for the minimum: `Least(terms)`.** Min cannot be written
with today's constructors. `Conj`'s `p → −∞` limit is min, but `inf` is not
storable, and `Gate` is a max. `Least(terms)` is the minimum of the members
that have a value, and `∅` when none does, so `Absent` is its identity, as
for `Conj`. `Least([t])` is `t`. `Least` is commutative, associative and
idempotent. `normalize` pushes it under `Piecewise` like `Conj`. It is in the
exact fragment when its members are, and its breakpoints include the
crossings. It is a new constructor, so no stored byte moves (§1).

**Worlds.** For todo `x` at moment `t`, a WORLD picks one candidate from each
of `x`'s state, spec and content registers (an empty register contributes
nothing). A world's log is the log minus the OTHER candidates of those
registers. Before the fork every world agrees, and `Least`'s idempotence
plus `mk_piecewise` collapse the equal pieces.

| Reader | Reads a multi-candidate register as |
|---|---|
| `env` (§6.1) | `outcomes[x]` is the SET of candidate bindings, with "open" a candidate where `Reopened` is one. The tendings are unchanged: a set has no candidates. |
| `After(x, …)` (§7) | the minimum over `x`'s candidate bindings of the reading under each. The compiler (§7.1) writes that as `Least` of the per-candidate compiled terms, so law 13 holds as stated. |
| `specs` (§6.2) | the candidate specs. |
| `content`, `authored_at` (§6.3) | the candidate records. Content has no price, but it carries the checklist length, which `flatten` reads. |
| `flatten` (§6.4) | the pieces fall where they fall today. The term in each piece is `Least` over the worlds in force at that piece's instant, each world flattened exactly as today. One world means today's term, byte for byte. |
| `entries` (§6.7) | `outcome` is the candidate set, with one member where there is no conflict. `value` is the fulfillment of the linked `Least` term, so it is the minimum. `content` is the candidate names, sorted. `conflicts` is unchanged. `claim` compares the candidate sets of the two readings. |

**Legacy stores move too, and only there.** B changes how a conflict READS,
not what is stored, so it applies to a legacy DAG as well. `conformance/dag.py`
pins `env` on DAGs with conflicts, and those rows change: a state conflict
reads as its candidate set, not as the write the hash order picked. Every
other field and every conflict-free row stays (law 22).

**Why worlds and not per-register minima.** Taking "the most urgent spec"
and "the most urgent state" separately can build a combination no branch
wrote. A world is always a reading that some writer's view contains.

**Cost, stated.** The number of worlds is the product of the three
candidate counts. A conflict is shown and settled by the next write, so it is
rare and short-lived, and each extra candidate is one more `flatten` of one
todo. If a real store ever makes this expensive, a cap on shown worlds is a
host concern, and `conflicts` still names them all.

**Laws.**

- One candidate per register reads exactly as today, bit for bit.
- An entry's value is at most every world's value, and equals the least.
- The reading does not depend on the order the linearisation puts
  incomparable writes in. This is the property "latest in the linearisation"
  could not have.
- `Least` is a semilattice with `Absent` as identity. `normalize` and
  `compile` preserve it.

## C. Attestation

**Today.** Every object names every tip, so the newest tip's name is a
Merkle root of the whole history. An application's attestable-history site
publishes that root: "everything below this hash existed before it".

**With changes, no change commits to anything outside its todo.** Something
has to name a whole state.

**Decision: `Snapshot(genesis, tips, previous)`, in core; WHEN to write one
is the host's call.**

- `tips` is the genesis's tips at the moment of writing, sorted and distinct.
  Its name is a Merkle root of their closure, like a git commit over its tree
  or Pijul's channel state.
- `previous` is the last snapshot the writer held, or `''`, so snapshots form
  a chain.
- A snapshot has no event. No fold reads it, and it is never a dep: `deps`
  come from register frontiers, and a snapshot writes no register.
- After a snapshot is written it is the genesis's one tip until the next
  change. New changes are concurrent with it, which is true and harmless.

**What it proves.**

- **Inclusion.** An object is in `closure(S)` iff it existed when `S` was
  written, because a name cannot be computed before what it hashes.
- **Order between states.** With `S₁` on `S₂`'s `previous` chain, an object
  in `closure(S₂)` and not in `closure(S₁)` was added after `S₁`. That is
  "written after that" at the granularity the host snapshots at, and it is
  what the site needs.
- **Order between changes in one todo** is still the DAG's, finer than
  before.

**Why core.** A snapshot is an object in `objects/`, hashed and printed like
every other one. `verify` must check that its closure is present and
single-genesis, `adopt` must copy what it names, and the tips derivation must
know that it names parents. If a host defined it, every reader would need
that host's code to verify a store. And `Woven(parents, None)`, a merge with
no event, already is a snapshot without a genesis or a chain: this is the
same structure given its real job. The CADENCE is the host's: one application
writes one per site build and publishes its name.

## D. Order

**Which readings still need a global order.** With B, none of the folds do:

- `env`, `specs`, `content` and `authored_at` become register projections.
  A frontier is defined by ancestry alone.
- The tendings are a set.
- `history` is indexed by `at`, which is data (§1). `history.at(t)` is the
  registers folded at `t`, and no causal order enters it.
- `flatten`'s pieces fall at instants, and each piece's term is `Least` over
  worlds, which is order-free.

**The two readers that still use one.** `stream` lists a todo's nodes "in
causal order", and a list needs one order. The register state's positions
(`Folded::position`) number objects for the ancestry index. Neither changes
a value: `stream` is a presentation, and positions are an index.

**Kahn plus min-heap on name stays.** It is deterministic, a function of the
object set, and law 8 checks it exactly against `conformance/dag.py`, so
changing it would move vectors for no semantic gain. A stream is its
restriction to the todo's nodes. Since `deps` never leave a todo, that
restriction is Kahn over the todo's own sub-DAG. Keying the heap on `(at,
name)` for readability was considered and rejected: it would move law 8's
vectors, and a host that wants time order sorts the stream by `at`.

**Law.** No reading except `stream`'s order depends on the linearisation.
Folding any linear extension of the DAG gives the same entries.

## E. Trust

**Naming is structural, belief is not.** `deps` are read with everything
binding, because a name records what the writer held. The confirmed
reading's registers still fold only what the policy binds (§6.6: a claim is
"skipped exactly as `chronological` and the folds skip it"). Nothing in this
draft changes that.

**An untrusted change cannot settle a conflict between trusted writes in the
confirmed reading.** Say `T₁ ∥ T₂` both bind and `U` claims, with `deps(U) ⊇
{T₁, T₂}`. In the confirmed reading `U` does not bind, so it neither joins
the frontier nor removes anything from it: the frontier stays `{T₁, T₂}`. A
LATER binding write `T₃` that descends from `U` does settle both. Its writer
held `U` and therefore everything `U` names (`adopt` copies closures). That
is §6.6's "a descending write settles", unchanged.

**A register whose candidates differ in standing.** Each reading has its own
candidates. The confirmed reading's are the binding ones, and when none binds
the register is unwritten there, as today. The claimed reading's are all of
them. So under B, a claim can never raise the confirmed price, which is law
11. A claim that is more urgent than the confirmed answer is SHOWN: the
entry's `claim` names the claimed candidates and `confidence` says the answer
is not the confirmed reading whole. Agreeing twins cannot differ in standing,
because standing is a function of the event (§5) and twins carry one event.

**Laws.** A claiming change, whatever its deps, leaves every confirmed
frontier as it was. The confirmed price under a conflict is the least over
BINDING candidates only.

## F. Staging as an overlay

This is a check, not a design: the overlay is a later task. The overlay
reads `base ∪ own` and writes only into `own`. Its changes carry the base's
genesis, and their deps are the frontiers of `base ∪ own`.

- **Accept is a same-name move.** A change's bytes are a function of its
  genesis, its deps and its event, and none of those says which directory
  holds it. So accepting `P` moves the file `objects/<P>.py` from the overlay
  to the base. The name does not change, and nothing that names `P` has to
  change. The one rule: `P`'s deps must be in the base, or be accepted with
  it, parents first, as `adopt` already writes. If the base moved on
  meanwhile, `P` lands concurrent with the newer writes, and the result is a
  conflict that B prices and §6.6 shows.
- **Reject keeps a receipt**, outside `objects/` (it is not a fact of the
  store). The receipt carries `P`'s name and `event_id(P.event)`.
- **A re-proposal is recognisable.**
  - Re-proposed over the same frontier, it is the SAME OBJECT: same genesis,
    same deps and same event give the same name, and the receipt names it.
  - Re-proposed after the base moved, the deps differ, so the name differs.
    It is a twin, and `event_id` still matches the receipt.
  - Re-proposed with a new `at` or note, it is a new claim, and neither key
    matches. That is correct: it says something new.

  Whether a matching re-proposal is refused or re-queued is the overlay's
  policy. The database supplies both keys.

## G. What is deleted

The implementing session should delete more than it adds.

**SPEC text.**

- §3, "Appending seals on every tip". Legacy writers aside, `append` writes
  a `Change`.
- §3, "`merge(parents)` writes a `Woven`" becomes a legacy-only note.
- §6.1–6.3's "last binding write wins" definitions. The folds become the
  register projections of B, stated once.
- §6.6, "Projections pick the write latest in the linearisation".
- §6.7, `content = chosen_of(…)`. It becomes the candidate set.
- Law 6's "registers equal the folds on any DAG". It is now a definition, so
  there is nothing left to check between two implementations.
- Law 17(b)'s "the next `append` writes the same `Woven`".

**Code.**

- `core/src/registers.rs`: `Folded::chosen`, `chosen_of`, and the
  `max_by_key(position)` projection. `env_of`, `specs_of` and `content_of`
  read candidates.
- `core/src/fold.rs`: `chronological` and the last-writer-wins loops of
  `env_at`, `specs_at` and `authored_at`. They were a second definition that
  law 6 kept equal to the registers. The registers are now the one
  definition (§1, "one evaluator"), and `history` reads them at each instant.
- The GLOBAL ancestry index. `BitSet` over every object is O(n²) bits, and
  §6.6 states it as a limit. Since `deps` never leave a todo, ancestry is only
  ever asked within a todo, so the index becomes per `(genesis, todo)`: O(Σ
  kᵢ²) for kᵢ writes to todo i. Legacy objects keep the global index, and
  only a mixed store pays for both.
- `core/src/store.rs`: the tip-sealing branch of `append` (`mk_woven` with an
  event). `merge` stays for legacy stores.
- `github/src/lib.rs`: the replay dedup (append does it), and `proposed`'s
  dating past every event in the store (it dates past its deps).
  `Known::accepted` stays: it keys on the comment, not the event.

## The laws (Draft A, §9)

Each law names the property test that checks it. The tests are new unless
noted, and run on the existing generators, extended to draw diverging writers,
replays, conflicts and several geneses.

| Law | Statement | Checked by |
|---|---|---|
| 19 | **A name is its genesis, event and view.** `append(event)` writes a function of its genesis, `event`, and the structural frontiers of the registers `event` writes. Adding objects that write other registers, and do not carry `event` (law 20), changes no name. | `core/tests/all/change.rs::name_ignores_other_registers` |
| 20 | **Append is idempotent.** Appending an event whose print the genesis holds writes nothing and answers the first object carrying it. Replaying any sequence of appends onto the store they produced leaves its object set unchanged. That includes the replay after a rejection (3). | `change.rs::replay_is_a_no_op`, `change.rs::replay_after_rejection_writes_nothing` |
| 21 | **Independence is structural.** `deps` never leave a todo, so changes to different todos are concurrent, and every fold reads the same whichever the linearisation puts first. | `change.rs::deps_name_one_todo`, `fold_laws.rs::disjoint_todos_commute` |
| 22 | **No stored byte moves.** Every `conformance/dag.py` store, and every generated store of `Sealed` and `Woven`, derives, linearises and verifies exactly as before, and reads exactly as before wherever no register has two candidates. `dag.py`'s `env` changes exactly where law 24 says it must: at a todo with a state conflict, it becomes the candidate set. Law 18 set this precedent for the view vectors. | the existing `dag.rs`, `tips.rs`, `fold_laws.rs` and vector tests, green, with `dag.py`'s conflicted `env` rows regenerated and the diff checked to touch nothing else |
| 23 | **Geneses are disjoint.** Every object has one genesis. No edge crosses geneses. A fold of a union equals the union of the per-genesis folds, keyed by `(genesis, todo)`. | `change.rs::no_edge_crosses_geneses`, `fold_laws.rs::union_folds_per_genesis` |
| 24 | **A conflict prices as its most urgent candidate.** With one candidate per register, every reading is today's, bit for bit. Otherwise an entry's value is the least over its worlds. Agreeing twins are one candidate. | `fold_laws.rs::one_candidate_reads_as_before`, `fold_laws.rs::conflict_is_its_most_urgent_world`, `registers.rs::agreeing_twins_are_one_candidate` |
| 25 | **The linearisation decides no value.** Folding any linear extension of the DAG gives the same entries. Only `stream`'s order may differ. | `fold_laws.rs::any_linear_extension_folds_alike` |
| 26 | **`Least` is a semilattice with `Absent` as identity.** It is commutative, associative and idempotent, and `Least([t]) = t`. `normalize` and `compile` preserve its reading (laws 5, 13), and it round-trips (law 1). | `fpl_laws.rs::least_is_a_semilattice`, with `Least` drawn by `a_term()` so laws 1, 5, 13 and 14 cover it |
| 27 | **A claim settles nothing it is not trusted to.** A claiming change, whatever its deps, leaves every confirmed frontier as it was. The confirmed price reads binding candidates only. | `fold_laws.rs::claim_never_settles` (law 11's property, drawing claims over conflicts) |
| 28 | **A snapshot attests its closure.** Its name changes if any object in its closure does. `verify` reports a snapshot whose closure is incomplete or crosses geneses. On a `previous` chain, each closure contains the one before. | `change.rs::snapshot_commits_to_closure`, `change.rs::snapshot_chain_grows` |
| 29 | **Placement is not identity.** Moving a change's file between two stores of one genesis changes no name. A change re-proposed over the same frontier is the same object, and over a moved frontier it has the same `event_id`. | `change.rs::accept_is_a_same_name_move`, `change.rs::reproposal_is_recognised` |

Law 6 becomes definitional (G). Its surviving content is "a descending write
settles, a merge settles nothing", and law 24's tests keep that.

## Cost, stated

`append` today lists tips. It will fold the structure of ONE TODO to find
its register frontiers, which is small now that `deps` never leave a todo. It
also looks up `event_id`s in the genesis, which is O(n) with no index. §6.6's
monoid action makes a cache honest. The whole register fold on the live chain
was measured at about 1.5 ms (`core/src/registers.rs`, 2026-09-07). The
uncached append costs at most that, once per write.

## Plan for the implementing session

Nothing here is started. In order, each step green before the next:

1. `core/src/event.rs`: the `Genesis`, `Change` and `Snapshot` envelopes and
   their smart constructors, `parents_of`, print and parse, §2's whitelist,
   and `event_id`. Law 1 over them, and the fuzzer.
2. `core/src/fpl.rs`: `Least`, with its semantics, `normalize`, breakpoints,
   explain, the compiler case and the JSON boundary in `wasm/`. Law 26.
3. `core/src/registers.rs`: candidates in place of `chosen`, the per-todo
   ancestry index, `deps_for`, agreeing twins. Delete the projection (G).
4. `core/src/fold.rs` and `core/src/view.rs`: the folds as register
   projections, worlds, `Least` in `flatten`, and the entry's candidate
   fields and genesis. Delete the sequential folds (G). Laws 24, 25 and 27,
   plus every existing vector unchanged (law 22).
5. `core/src/store.rs`: `init` writes a `Genesis`, `append` writes a
   `Change` and is idempotent, `snapshot`, and `verify`'s new checks. Delete
   the tip-sealing branch. Laws 19–21, 23 and 28 in `core/tests/all/change.rs`.
6. `conformance/change.py`, written BY HAND from the laws, like `link.py`:
   exact prints and names, a replay that writes nothing, a twin pair, a
   conflict priced as its most urgent world, two geneses united, a snapshot
   chain, and a mixed store over a `conformance/dag.py` one.
7. `github/`, `cli/` and `wasm/`: the deletions of G, `init` and `snapshot`
   commands, and `Change` wherever they match on envelopes. Law 29 waits for
   the overlay task, but its two tests can land here against two plain
   stores.
8. SPEC: fold Draft A into §3, §5, §6, §7 and §9 and delete the draft
   section. CHANGELOG: under Unreleased, as a MINOR change.
