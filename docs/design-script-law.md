# Design: the script law

This note describes the design of law 68, the script law, and the tool
choices behind it. For the law as implemented, see `SPEC.md` §9, law 68,
and `core/src/laws/script.rs`.

## Why a script

Each other law of SPEC §9 checks one statement over generated inputs. Most
of them draw two writers and a few hundred cases. A bug that needs several
features in sequence, such as a partial sync, then a revocation, then a
read through a cache, is out of reach of such a law. The fuzzers of the
document CRDT libraries find their deepest bugs with a different tool: a
program drives several replicas through a random list of actions and checks
every invariant after every action. Loro's `crdt_fuzzer` applies actions
(`Sync`, `SyncAll`, `Checkout`, `Handle`, `Undo` and others) and then
compares every pair of documents, compares a shadow state that event
subscriptions feed with the document, and checks out every recorded
version to compare its value with the value recorded. Yjs's
`applyRandomTests` interleaves random edits with random message deliveries,
disconnections and reconnections, and then compares every user's document,
state vector and delete set. Both programs choose their actions to reorder
and to cut deliveries short, because reordering and partial delivery find
more bugs than more writers do.

## The tool

Three kinds of tool can drive such a script.

- **`proptest`, with a script as a generated `Vec` of steps.** The crate
  already depends on `proptest`, and every law runs through it and through
  `core/tests/common/cases.rs`. A failing script shrinks: proptest removes
  steps and shrinks each step's fields. The `proptest-state-machine` crate
  adds preconditions and a reference model. We took plain `proptest` and
  made every step total instead: each position in a step is taken modulo
  the length of what it picks from, and a step with nothing to pick from
  does nothing. So every shrunk script is still a valid script, and no
  precondition is needed. This adds no dependency, so `Cargo.lock` and the
  nix build do not change.
- **`arbitrary` with `cargo-fuzz`, or `bolero`.** A coverage-guided engine
  can find paths that random generation misses. It needs a nightly
  compiler with sanitizer flags and `libfuzzer-sys`. The flake's toolchain
  is the stable compiler from nixpkgs, and CI runs only that toolchain. A
  second input format (`arbitrary`) would also need a second generator of
  steps beside the proptest strategy, or a decoder from bytes to a seed,
  which gives the engine no useful coverage signal. We did not add a
  coverage-guided target. A later change can add one outside the workspace,
  as `typst-wasm` is, with its own lock file and toolchain.
- **A model checker such as `stateright`.** A model checker explores every
  interleaving of a small model exhaustively. The state here is a set of
  real stores with real signatures, and the interesting bugs depend on the
  data, not on few interleavings. Exhaustive search of such states is not
  practical, and the model would be a second implementation of the store.

## The actions

Each of three actors (`ada`, `ben`, `cai`) holds a replica: a store in
memory, an overlay over one, or a store on disk. The script draws the
kinds. The steps are those of `laws::script::Step`: an append (signed,
unsigned, with a signature that does not verify, or with a dep on a write
to another entity; by the actor or under another actor's name, as a thief
with the actor's key would write); a receipt of chosen objects in any
order, with duplicates; a sync; a sync of all; an offer of a print that the
replica must refuse; a revocation of a key and a new key; a read at an
instant, afresh or through a cache; a reading recorded at a down-set; a
snapshot; a forget of a key of the cache; and an overlay's flush.

The machine leaves out these features, for these reasons:

- **Damage to files on disk, and `fsck`.** A file changed under a store is
  a change made outside the replica's interface. Law 45 checks it with its
  own model of damage.
- **The record of a store of record's frontiers (law 47).** A pull by a
  frontier token involves a store of record and a protocol of pieces.
  Laws 47 and 64 drive that protocol with their own scripts.
- **Legacy envelopes (`Sealed`, `Woven`).** No replica writes them, and
  laws 17 and 22 check how they read.
- **Several prodromes in one replica.** Law 41 checks sets of prodromes.
- **The cache on disk (`memo::File`).** The cache of readings is in memory.
  Laws 51 to 58 check the cache file with their own scripts.

## The invariants

The library has no observer that reports changes to a subscriber. The
nearest thing is the answer of each append and receipt: the names that the
replica took, parents first. The machine applies those answers, in order,
to a shadow (a set of names and a fold built by `registers::extend`), and
checks that the shadow equals the replica. The other invariants are listed
in law 68.

`verify` must find exactly the injected faults. For a forged signature and
a foreign dep, the machine records the finding when it writes the object.
For an object that no key proves, the machine keeps its own record of keys,
signatures and revocations, and computes the expected findings with sets of
names: a revocation keeps the signatures that its writer's replica held.
To make that set exact, a foreign dep is only drawn among tips whose past
holds no write to the change's own entity, so the fault adds no path
between two writes of one entity.

## What the machine found

- A partial sync can take an object of a keyed actor without the signature
  that proves it, because the signature rests on the object and not the
  other way round. `verify` then reports the object as unsigned until the
  signature arrives. This is how the code behaves, and law 68 states it.
  Whether a receipt of an object should also take its signatures is a
  design question for the specification.
- A reading at an instant changes when a change dated at or before the
  instant arrives late, and a reading at a down-set does not. Law 68
  states this.
- An add, a removal and the same add in one tick: the second add is the
  first add's event, and the removal stands (law 20). Another replica that
  held other heads of the entity writes a twin, which the removal does not
  supersede, so the merge is a conflict.
