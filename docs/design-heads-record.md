# Design: a file store records its heads at each write

This note was written before the change was made. The code follows it only
after a reviewer answers it on the pull request. For what was implemented,
see `SPEC.md`.

## The problem

`EventStore::tips` lists `objects/` on every call, and it reads each file
that its handle's memory does not hold. A process that only wants to know
the store's current heads, for example to name a cached reading by those
heads, pays that listing on every read. For a store of tens of thousands of
objects, the listing is most of the cost.

This change adds a small file that each write through `EventStore` updates.
A new method, `EventStore::heads`, reads that file and does not list the
directory. `EventStore::tips` does not change, and it stays the derived
value that every decision uses.

## 1. What is recorded

The record holds two values:

- the tips of the store's history: the tips of `Dag::interior` over the
  objects present, which is exactly what `tips()` derives;
- the count of objects in that history: `dag().objects().len()`.

The record is printed in the store's literal grammar (SPEC section 2) as
one constructor, with its fields in name order and the tips sorted:

```
Heads(count=3, tips=['<64 hex digits>', '<64 hex digits>'])
```

`Heads` is not an object. It is not named by its hash, it is not in
`objects/`, and no replica sends it. It is a new constructor of a local
file, so it does not change any stored constructor of a release.

**What the record denotes.** The record denotes the heads and the count of
the store's history as of the last completed write through `EventStore`
that renamed the record into place. Let `O` be the set of objects that this
write's memory held after the write placed its objects, under the lock.
The record is `(tips(interior(O)), |interior(O)|)`. The record does not
denote the store as it is now. It denotes the store as one write saw it.
Section 5 says when those two differ.

The heads and the count of the record are also a frontier in the sense of
SPEC law 47: `Token::of(heads.tips)` is the token that a quiet pull of the
whole store records. So a store of record can answer a quiet pull from the
record, without a listing. This change does not alter `Frontiers`, and a
later change can use the record there.

## 2. Where the record is

The record is the file `root/.heads`, beside `objects/`, `.gitattributes`
and `.lock`. The leading dot puts it among the store's other control files.

The record is derived state, and it is local to one machine. Two clones of
a store must never commit it, because two clones that each wrote would
change it in different ways, and a merge would conflict.

**Where this note differs from the request.** The request asks that a
directory under version control ignore `.heads` as it ignores `.lock`.
Today nothing ignores `.lock`: the store writes no `.gitignore`, and the
`roadmap-data` branch of this repository commits `roadmap/.lock`. So
"as it ignores the lock" gives no rule to copy. This note proposes a rule:

- The first write to a store whose root holds no `.gitignore` writes one,
  durably, with the one line `/.heads`. The store writes `.gitattributes`
  in the same way today: once, where none exists, and never again.
- A `.gitignore` that a host already has belongs to the host, and the
  store never rewrites it. SPEC section 3 tells such a host to add
  `/.heads`.
- This change does not add `.lock` to the new file. That is a separate
  change, and it would affect the `roadmap-data` branch, which commits the
  lock today.

Even a committed record cannot make a reader see objects that are not
present, by sections 5 and 7. If a merge leaves conflict markers in the
file, the file does not parse, and `heads()` derives instead. So a mistake
here can cost a host a conflicted merge, but it cannot cost a wrong
answer.

## 3. When a write records

Each write through `EventStore` updates the record at its end, under the
lock that the write already holds, after its objects are placed and
`objects/` is synced. These are the writes:

- `init`, `append`, `snapshot` and `merge`, which all end in
  `EventStore::write`;
- `receive`, `adopt` and `adopt_objects`, which all end in
  `EventStore::copy_in_from`;
- the several appends of one `Decision`. The request names `append_each`,
  but the code has no such method. The batch write of the code is a
  `Decision` that holds the lock across several appends, and each of its
  appends ends in `EventStore::write`. So each append of a `Decision`
  updates the record;
- `quarantine` (through `fsck`), which moves files out of `objects/`. It
  looks at the store again after the moves and records what it then reads;
- `record_heads` (section 5).

The record is written as an object is written: staged in a temp file in
the root, synced, renamed over `.heads`, and the root synced. This is
`put_durably`, which `.gitattributes` uses today. A crash leaves the old
record or the new one, and at worst a temp file in the root, which
`verify` already reports.

**A write that changes nothing.** An append that finds a twin, a write of
an object that the store holds, and a receive that takes nothing place no
object. Such a write reads the record (one small file) and compares it
with the derived heads that its memory holds. It rewrites the record only
when the two differ. So a write that changes nothing costs one read and no
sync, and it also repairs a lagging record (law 3 of section 7).

**One commit point.** For an object, the rename of its file is the commit:
from that moment the object is in the store. The record is an index that
the write updates after the commit. If the write cannot update the record,
for example because the disk is full, the write still returns `Ok`,
because its objects are in the store. The record then lags, which is a
state that the laws allow, and `verify` reports it. This is a choice for
the reviewer: the alternative is to return the error, which tells the
caller that a write failed when its objects are in fact in the store.

**Cost.** A write that places objects pays two more syncs: the temp file
and the root. The directory sync is not needed for safety, because a lost
rename leaves a lagging record, which the laws allow. This note keeps the
sync because the request asks for it. A reviewer can choose to drop it.

## 4. What reads the record

`EventStore::heads()` reads `.heads`, parses it once into a `Heads` value,
and checks each recorded tip with one `stat` of its object file. It lists
no directory and reads no object.

- If the record parses and every recorded tip is present, `heads()`
  answers the record.
- If the record is missing, does not parse, or names a tip whose file is
  not present, `heads()` derives the heads as `tips()` does, and answers
  them. A record that names an absent head is never served.

`EventStore::tips()` stays the derived truth. `decide`, and every write,
derive the heads from the memory under the lock. No decision reads the
record.

The parsed value is a type that holds the result of the check:

```rust
/// The heads of a store's history and the count of its objects.
pub struct Heads {
    pub tips: BTreeSet<Hash>,
    pub count: usize,
}
```

The parser is the one edge where the record's text becomes a value. Later
code does not check the text again.

## 5. The doors that `EventStore` does not own

A program can change `objects/` without `EventStore`: a version-control
checkout or merge of the directory, a file copy, or an older binary that
does not write the record. Then the record is stale:

- **Objects added by another door.** The record lags. Every recorded head
  is still present, so `heads()` answers the record, and a reader sees an
  older history. The next write through `EventStore`, or a call to
  `record_heads`, records the derived heads.
- **Objects removed by another door.** A recorded head can be absent.
  `heads()` refuses that record and derives.

No clock decides anything here. The record has no timestamp, and the code
compares no directory stamp. Instead:

1. `heads()` refuses a record that names a head that is not present, and
   derives. `verify` compares the record with the derived heads and count,
   and reports any difference as a new finding, `Finding::Stale`,
   that names both values. A missing record is not a finding: a store
   that no write has touched since this change has no record, and that
   store is healthy.
2. A host that brings objects in by another door calls
   `EventStore::record_heads()`. It takes the lock, looks at `objects/`,
   derives the heads, and records them. It does not refuse while a file
   is unread, as a decision does: the record holds the heads of the
   history that the store reads, which a reader computes in the same way.
   SPEC section 3 says that a host that changes `objects/` by another door
   must call `record_heads`, and that until it does, readers of `heads()`
   see the last recorded history.
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
so they answer `heads()` from what they hold. The method belongs on the
`Replica` trait as a provided method:

```rust
/// The heads of what this replica holds, and the count of its objects.
fn heads(&self) -> Result<Heads, ProdromeError> {
    let held = self.held()?;
    Ok(Heads { count: held.dag.objects().len(), tips: held.tips })
}
```

`EventStore` overrides it with section 4. Its fallback is
`EventStore`'s own derivation, which is also what `tips()` and `held()`
read, so no implementer copies the fallback. `Replica for &R` forwards to
the borrowed replica, as it does for every method.

## 7. The laws

These laws go to SPEC section 9 as laws 60 to 63, after law 59. Each law gets a test in the store's tests, through
`cases.rs`. The record is a stored format, so it also gets a standing
test of its bytes.

1. **Recorded equals derived.** After any sequence of `init`, `append`,
   several appends of one `Decision`, `receive` and `quarantine`, from one
   handle or from two handles that interleave under the lock, `heads()`
   equals `(tips(), dag().objects().len())` once no write is in flight. A
   property test draws the scripts of writes.
2. **Never ahead.** At every instant, also between the placement of the
   objects and the rename of the record, the closure of the recorded
   heads is a subset of the objects present. The test stops a write after
   its objects are placed and before the record is renamed, at each step,
   and checks the record against the files.
3. **A lagging record reads as the older heads.** When objects are placed
   and the record is not written (a crash, or another door), `heads()`
   answers the older heads. The next write through the store records the
   derived heads, also a write that places nothing.
4. **A record that names an absent head is not served.** After a head's
   file is removed by another door, `heads()` answers the derived heads,
   and `verify` reports `Finding::Stale` with the record and the
   derived value.

A standing test of the format: a store that one test writes holds `.heads`
with exactly the bytes of section 1, and a record of other bytes (cut
short, with conflict markers, or with an unknown field) is never served.

## 8. What else changes

- SPEC section 3 says today that the store on disk is its `objects/` and
  nothing else, and that no file names a head. The change keeps the first
  claim for every value that a decision uses, and it adds the record as a
  derived, local file that only `heads()` reads. The test
  `an_append_writes_a_change_over_what_it_supersedes` asserts that the root
  holds only `.gitattributes`, `objects` and `.lock`. That test changes to
  list `.heads` and `.gitignore` too. This is a change of the claim that the
  test checks, which this pull request makes in SPEC section 3, and not a
  weaker test.
- `docs/guide.md` section 12 lists the files of a store, and it gains
  `.heads` and `.gitignore`.
- The CHANGELOG says that a store gains two local files, and that no object
  changes.

## 9. Measurement

The pull request will measure `tips()` against `heads()` on a generated
store of 30,000 objects on disk. It will measure each on a new handle in a
new process (cold) and on a handle that has read the store (warm), with
the median of several runs, in a new example under `core/examples/`.

## Questions for the reviewer

1. Should the store write `/.heads` into a new `.gitignore` (section 2), or
   should SPEC only tell the host to ignore it?
2. Should a write whose record update fails return `Ok` (section 3), or
   the error?
3. Should the record write keep the root's directory sync (section 3)?
4. Is `Finding::Stale` the right name for a record that differs from
   the derived heads?
