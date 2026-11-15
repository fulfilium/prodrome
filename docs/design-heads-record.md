# Design: a file store records its heads at each write

This note was written before the change was made, and revised after a
review on the pull request. For what was implemented, see `SPEC.md`.

## The problem

`EventStore::tips` lists `objects/` on every call, and it reads each file
that its handle's memory does not hold. A process that only wants to know
the store's current heads, for example to name a cached reading by those
heads, pays that listing on every read. For a store of tens of thousands of
objects, the listing is most of the cost.

This change adds a small file that the store updates when a holder of its
lock lets the lock go. A new method, `EventStore::heads`, reads that file
and does not list the directory. `EventStore::tips` does not change, and it
stays the derived value that every decision uses.

## 1. What is recorded

The record holds two values:

- the tips of the store's history: the tips of `Dag::interior` over the
  objects present, which is exactly what `tips()` derives;
- the count of objects in that history: `dag().objects().len()`.

The record is printed in the store's literal grammar (SPEC section 2) as
one constructor, with its fields in name order and the tips sorted. The
canonical print of a list is a tuple, so a record of two tips is:

```
Heads(count=3, tips=('<64 hex digits>', '<64 hex digits>'))
```

The file holds exactly these bytes, with no newline after them.

`Heads` is not an object. It is not named by its hash, it is not in
`objects/`, and no replica sends it. It is a new constructor of a local
file, so it does not change any stored constructor of a release.

**What the record denotes.** The record denotes the heads and the count of
the store's history as a holder of the store's lock saw it when it let the
lock go. Let `O` be the set of objects that the holder's memory held at
that moment. The record is `(tips(interior(O)), |interior(O)|)`. For every
change that the store makes, the record is removed before the change and
written after it (section 3), so a record that a reader finds is never
older than a change of the store's own. Only a door that the store does
not own can make the record older than the objects (section 5).

The heads and the count of the record are also a frontier in the sense of
SPEC law 47: `Token::of(heads.tips)` is the token that a quiet pull of the
whole store records. A later change can let a store of record answer a
quiet pull from the record. This change does not alter `Frontiers`.

## 2. Where the record is

The record is the file `root/.heads`, beside `objects/`, `.gitattributes`
and `.lock`.

The record is derived state, and it is local to one machine. Two clones of
a store must never commit it, because two clones that each wrote would
change it in different ways, and a merge would conflict. The request asked
that version control ignore `.heads` as it ignores `.lock`, but nothing
ignores `.lock` today: the `roadmap-data` branch commits `roadmap/.lock`.
So the store writes a `.gitignore`:

- Before the store writes the record into a root that holds no
  `.gitignore`, it writes one, durably, with the one line `/.heads`. The
  store writes `.gitattributes` in the same way: once, where none exists,
  and never again.
- A `.gitignore` that a host already has belongs to the host, and the
  store never rewrites it. SPEC section 3 tells such a host to add
  `/.heads`.
- This change does not add `.lock` to the file.

A committed record still cannot make a reader see a wrong history: if a
merge leaves conflict markers in the file, the file does not parse, and
`heads()` derives instead.

## 3. When the store records

**The mechanism.** The record is removed before the store changes
`objects/`, and written again after:

1. Before the first placement of an object under one hold of the lock,
   the store removes `.heads` and syncs the root. It syncs the root also
   when the file is already absent, because a host may have deleted it
   without a sync. If the removal or the sync fails, the write fails
   before it places anything.
2. The store places its objects as today: each one staged, synced and
   renamed in, and `objects/` synced once.
3. When the holder lets the lock go, the store writes the record from its
   memory: it unlinks any record that is there, creates `.heads` anew and
   writes it, with no temp file, no sync of the file and no sync of the
   root. No record is written over another, so a power loss cannot mix
   the bytes of two records.

The record needs no sync, because a lost record, or one that is cut short
at any byte, is not served: it is absent, or it does not parse. The record
ends with its own closing parenthesis, so no proper prefix of a record is
a record. A read also refuses a text that is not the canonical print of
the value that it parses to.

**Once for each hold of the lock.** A `Decision` holds the lock across
several appends. It removes the record before its first placement, and it
writes the record once, when the `Decision` is dropped. The other writes
(`init`, `append`, `snapshot`, `merge`, `receive`, `adopt`,
`adopt_objects`) each take a `Decision` and drop it at their end. The
request names `append_each`, but the code has no such method: the batch
write of the code is a `Decision` that appends several times.

**The state between the steps** is one value that the `Decision` holds, a
sum of three cases:

- the record is untouched, because nothing was placed under this hold;
- the record is removed, and a placement may be cut short;
- the record is removed, and every placement is done and in memory.

On drop, the first case reads the record and rewrites it only if it
differs from the derived heads. This repairs a record that a door the
store does not own made stale, and it writes a record for a store that
has none. The second case writes nothing, so a failed placement leaves no
record. The third case writes the record.

**`quarantine`** (through `fsck`) moves files out of `objects/`. It takes
the same three steps: it removes the record durably before the first move,
and after the moves it looks at the store again and writes the record.

**`record_heads`** (section 5) takes the lock, looks at `objects/`, and
takes the first case.

**A crash.** Stop a write of the store at any instant:

- A killed process leaves every step done so far. Before step 1 ends, the
  old record describes the old objects. After step 1 and before step 3,
  there is no record. After step 3, the record describes the new objects.
- A power loss keeps a prefix of the steps that were made durable. The
  removal is durable before any object is placed, and the objects are
  durable before the record is written. So the old record never returns
  beside new objects. The new record can be lost or cut short, and then
  it is not served.

In every case, a record that `heads()` serves is the derived one.

**Errors.** If the store cannot write the final record, the write still
returns `Ok`, because its objects are in the store. The record is then
absent, which is a healthy state. If the store cannot remove the record,
the write fails before it places anything.

**A reader during a write** finds no record, and derives the heads as
`tips()` does. That costs a listing and the reads of the new files, and
the answer is exact.

**Cost.** A write that places objects pays one more directory sync, for
the removal, and an unsynced write and rename of a small file.

## 4. What reads the record

`EventStore::heads()` reads `.heads`, parses it once into a `Heads` value,
and checks each recorded tip with one `stat` of its object file. It lists
no directory and reads no object.

- If the record parses and every recorded tip is present, `heads()`
  answers the record.
- Otherwise, `heads()` derives the heads as `tips()` does, and answers
  them. A record that names an absent head is never served.

`EventStore::tips()` stays the derived truth. `decide`, and every write,
derive the heads from the memory under the lock. No decision reads the
record.

`Heads` is a type with private fields and accessors. The parser and the
derivation are its only makers, so a `Heads` value holds the result of the
check. The type belongs to the DAG, because it is a fact of a history: the
tips of the history and the count of its objects.

## 5. The doors that the store does not own

A program can change `objects/` without `EventStore`: a version-control
checkout or merge of the directory, a file copy, or an older binary that
does not write the record. Then the record can be stale:

- **Objects added by another door.** The record lags. Every recorded head
  is still present, so `heads()` answers the record, and a reader sees an
  older history. The lag ends at the next hold of the lock (a write, or a
  `decide`), at a call of `record_heads`, or when a program deletes the
  record.
- **Objects removed by another door.** A recorded head can be absent.
  `heads()` refuses that record and derives.

No clock decides anything here. The record has no timestamp, and the code
compares no directory stamp. Instead:

1. `heads()` refuses a record that names a head that is not present.
   `verify` compares a record that parses with the derived heads and
   count, and reports a difference as `Finding::StaleHeads { recorded,
   derived }`. A missing record, or one that does not parse, is not a
   finding. `verify` takes no lock, so it reads the record before and
   after its look at the objects, and compares only when both reads found
   the same record.
2. A host that brings objects in by another door calls
   `EventStore::record_heads()`, or deletes `.heads`. Deleting the record
   is always safe: the next reader derives, and the next hold of the lock
   records. SPEC section 3 says that a host that changes `objects/` by
   another door must do one of the two, and that until it does, readers of
   `heads()` see the last recorded history.
3. **Is there a cheaper exact check that needs no clock and no listing?**
   No. To see that a file was added to a directory, a program must list the
   directory or trust a stamp of the directory. The modification time of
   a directory is a clock, and the change counters that some file systems
   expose are directory stamps, and they are not portable. The check that
   remains is the one in point 1: one `stat` for each recorded head. That
   check finds a removed head. It does not find an added object, and it
   does not find a removed object below a head. A reader that must be
   exact calls `tips()`.

## 6. The other stores

`MemoryStore`, `Overlay` and `Replica for &R` hold their heads in memory,
so they answer `heads()` from what they hold. The method is a provided
method of the `Replica` trait, which builds `Heads` from `held()`.
`EventStore` overrides it with section 4, and its fallback is the same
derivation that `tips()` reads, so no implementer copies the fallback.
`Replica for &R` forwards to the borrowed replica, as it does for every
method.

## 7. The laws

These laws go to SPEC section 9 as laws 60 to 63, after law 59.

60. **Recorded equals derived.** After any sequence of `init`, `append`,
    several appends of one `Decision`, `receive` and `quarantine`, from one
    handle or from two handles that interleave under the lock, the record
    on disk parses, and it equals `tips()` and the count of the objects
    present. So `heads()` serves it. A property test draws the scripts of
    writes.
61. **A served record is the derived one.** Stop a write of the store at
    any instant, by a killed process (every step done so far is visible) or
    by a power loss (a prefix of the durable steps persists, and an
    unsynced record can be lost or cut at any byte). Then a record that
    `heads()` serves equals `tips()` and the count of the objects present.
    The test builds each state that a stop can leave, and a placement that
    fails after the removal leaves no record.
62. **Another door's objects read as the older heads.** Objects that a
    program adds to `objects/` without the store read as the older heads,
    until a hold of the lock, a call of `record_heads`, or a deletion of
    the record. After any of the three, `heads()` equals the derived heads.
63. **A record that names an absent head is not served.** After a head's
    file is removed by another door, `heads()` answers the derived heads,
    and `verify` reports `Finding::StaleHeads` with the record and the
    derived value.

A standing test of the format: a store that the test writes holds `.heads`
with exactly the bytes of section 1. No proper prefix of the record, no
text with conflict markers, no unknown field and no non-canonical print is
served.

## 8. What else changes

- SPEC section 3 says today that the store on disk is its `objects/` and
  nothing else, and that no file names a head. The change keeps the first
  claim for every value that a decision uses, and it adds the record as a
  derived, local file that only `heads()` reads. The test
  `an_append_writes_a_change_over_what_it_supersedes` asserts that the root
  holds only `.gitattributes`, `objects` and `.lock`. That test changes to
  list `.heads` and `.gitignore` too, because the claim changes in SPEC.
- `docs/guide.md` section 12 lists the files of a store, and it gains
  `.heads` and `.gitignore`.
- The CHANGELOG says that a store gains two local files, and that no object
  changes.

## 9. Measurement

The pull request measures `tips()` against `heads()` on a generated store
of 30,000 objects on disk, each on a new handle in a new process (cold) and
on a handle that has read the store (warm). It also measures an append on
the same store before and after the change. Each figure is the median of
several runs on one machine.

## Answers from the review

1. The store writes `.gitignore` once, where none exists.
2. A failed final write of the record returns `Ok`. A failed removal fails
   the write before it places anything.
3. The store syncs the root once, after the removal, and not after the
   final rename.
4. The finding is `Finding::StaleHeads { recorded, derived }`.
