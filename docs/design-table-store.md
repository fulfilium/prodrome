# Design: a store kept in a table, beside the store of files

Sections 1 to 8 of this note were written before any code of the change,
and section 9 records what the experiments of section 8 decided. For what
is implemented, see `SPEC.md` and `CHANGELOG.md`. The note answers issue
#82.

A **store** today is a directory, `objects/`, with one file for each
object, named by the SHA-256 hash of the file's bytes (SPEC section 3).
This note designs a second form of a store: one SQLite database file that
holds the same objects in an indexed table, with each write in one
transaction. The note takes one decision as given. The store of files stays
the form for every store that is kept in git, merged by git or published,
and it stays the reference form that `SPEC.md` describes. The second form
is for stores that need no git.

Terms used below:

- An **object** is one of the constructors of SPEC section 3. Its **print**
  is its bytes. Its **name** is the hash of its print.
- A **husk** is the stored form of a `Change` whose sealed parts were
  dropped. A store keeps the husk under the name of the whole change, so
  the husk's bytes do not hash to its name.
- The **history** of a store is the largest set of its objects that is
  closed under parents (SPEC law 40). An object outside it **waits** for a
  parent that the store lacks.
- The **tips** of a history are the objects that no object of the history
  names as a parent. The **heads** of a store are its tips and the count of
  the objects of its history.
- An **entity** is what a schema's events are about, for example one todo.
  A change's `deps` name only changes of the same entity.
- A **fold** is the computation that reads an entity's current values from
  its events. A **register** is one value that a fold reads.
- A **frontier** is the set of tips of what a replica holds. A **token**
  is the hash of a frontier's tips (SPEC law 47).
- A **replica** is any value that implements the trait `Replica`: the file
  store `EventStore`, `MemoryStore`, and `Overlay`.

## The claims

1. **The interface.** Today's `Replica` trait is the interface that both
   forms share, and nothing in it needs a directory. Everything that only a
   directory can do (`flock`, the rename of a temp file, the listing and
   the `stat` of each file, `.heads`, `.gitattributes`, the quarantine
   directory) is an inherent method of `EventStore` or a private function
   of `store.rs`. The change puts those effects behind one small trait,
   `Prints`, so that `EventStore` keeps one memory, one `Decision` and one
   set of write rules over either form. The table form then adds two
   answers from its indexes: one entity's objects, and the objects that
   rest on an object. Both are provided methods of `Replica`, as `heads()`
   is today. A program that uses the table form can no longer bring
   objects in by another door, such as a git merge or a copy of files.
2. **The engine is SQLite.** On the operations that a running program
   repeats (a read on an open handle, an append, a post, one entity's
   objects), both engines are between 7 and more than 10,000 times faster
   than the store of files with a warm page cache, and on an open of a new
   process they are 1.4 to 2.6 times faster (measured).
   Where an operation takes more than 30 ms, SQLite and redb are within a
   factor of 2.5 of each other (measured). The engine is not the
   bottleneck: decoding and folding 50,000 objects
   costs about 700 ms of a cold open in every form. SQLite wins on what the
   measurements cannot separate: an online backup that is documented and
   that a test confirmed, a command-line tool that every operator has,
   removal of a husk's dropped bytes from the file, a recovery after a kill
   in milliseconds, and a smaller file. Its cost is a C library, 55 s more
   of a clean build, and no build for the browser.
3. **The same objects, byte for byte.** A table row holds a name and the
   exact bytes that the file of that name holds: the whole print, or the
   husk. The row holds no flag for the form, because the form is derived
   by hashing the bytes, as the store of files derives it. Every index is
   derived from the rows, is written in the same transaction as its row,
   and is checked by `fsck`, which rebuilds it from the rows and compares.
4. **The rule.** Keep a store as files when another party takes it through
   git: it is published, forked, merged by git, or reviewed as a diff. Keep
   it in a table otherwise.
5. **What replaces git.** A backup is one command, `VACUUM INTO`, that a
   writer does not stop. Sync between machines is the replica's own sync
   (`sync`, `accept`, and pulls by token), which the library already has.
   A second machine holds a full replica and pulls after each write or on
   a schedule that the host chooses.
6. **The roadmap.** The change closes no roadmap item alone. It gives the
   index that issue #83's per-entity frontier asks for, and it makes the
   quiet pull of `quiet-pull-from-heads-record` exact for the table form.
   It leaves the other items as they are, and it makes
   `read-without-copies` the largest remaining cost of a cold open.
7. **The stages.** There are four stages, each a pull request that lands
   alone. The first runs the store laws over more than one backend from
   one suite and adds no backend. A consumer changes only the call that
   opens a store.
8. **The case against.** Most of the measured gap comes from two costs of
   the store of files that a smaller change could reduce: the listing of
   `objects/` on every read, and one `fsync` for each file of a post. If a
   change to the store of files closes the gap to within a factor of ten,
   stop after the first stage.

## 1. What the backend is, as types

### 1.1 What both forms share today

`Replica` (in `core/src/store/replica.rs`) has six required methods and two
provided ones. Each has a law in SPEC section 3 or 9:

| Method | What it answers | Law |
|---|---|---|
| `schema()` | the schema that the replica parses every print at | §5 |
| `held()` | the history, its fold and its tips, of one look | 44, 45 |
| `tips()` | the tips of the history | §3 "Tips are derived" |
| `heads()` | the tips and the count, provided from `held()` | 60 to 65 |
| `kept(name)` | the bytes kept under a name, whole or husk, unchecked | 74 |
| `print(name)` | only bytes that hash to the name, provided | §3 |
| `append(event)` | the name of the change written, or of its twin | 35, 36, 76 |
| `receive(seeds, print_of, sender)` | what it took, parents first, all verified | 35, 39 |

None of these methods needs a directory. The memory store and the overlay
implement all of them without one. So the smallest interface that both
forms share is `Replica` as it is.

The store of files does several things outside the trait. Each one is an
inherent method of `EventStore`, and each one uses a directory:

- `decide()` takes an exclusive `flock` on `root/.lock`;
- `look()` lists `objects/` and compares each file's `stat` with what the
  memory saw;
- `place()` writes each print to a temp file, syncs it, renames it, and
  syncs the directory once;
- `husk()` replaces a file by a rename;
- `fsck()` moves files into `quarantine/`;
- `verify()` reports temps, strays, `HEAD` and `refs/`, receipts in
  `quarantine/`, and a stale `.heads`;
- `heads()` and `record_heads()` read and write `.heads`.

All of the other inherent methods (`init`, `append`, `append_all`,
`snapshot`, `merge`, `adopt`, `adopt_objects`, `load`, `ancestors`,
`events`) work over the memory and a `Decision`. They would work the same
over any place that keeps prints.

### 1.2 The one new trait: where a store of record keeps its prints

The difference between the two forms is only where the prints are kept and
how a writer excludes other writers. The change names that part with one
trait, and makes `EventStore` generic over it, with the directory as the
default:

```rust
/// Where a store of record keeps its prints: a directory, or a table.
pub trait Prints {
    /// One writer's exclusion of every other writer, in any process.
    type Hold<'p>: Placing where Self: 'p;
    /// What one look saw: the names and `stat` values of the files, or an
    /// arrival number and a count of replacements.
    type Mark;
    fn hold(&self) -> Result<Self::Hold<'_>, ProdromeError>;
    /// The names placed, replaced or set aside since `mark`, and the new mark.
    fn since(&self, mark: &Self::Mark) -> Result<(Changes, Self::Mark), ProdromeError>;
    /// The bytes kept under `name`, unchecked.
    fn kept(&self, name: &Hash) -> Option<Vec<u8>>;
}

/// The writes that only a holder makes.
pub trait Placing {
    fn place(&mut self, batch: &[(Hash, Vec<u8>)]) -> Result<(), ProdromeError>;
    fn replace(&mut self, name: &Hash, husk: &[u8]) -> Result<(), ProdromeError>;
    fn set_aside(&mut self, name: &Hash) -> Result<(), ProdromeError>;
    fn finish(self, heads: &Heads) -> Result<(), ProdromeError>;
}

pub struct EventStore<E: Schema, Pol = Untrusted, P: Prints = Directory> { .. }
```

`Prints` is a trait and not an enum of the two forms, because the table
form lives in its own crate (section 2.4), and the core cannot name a type
of a crate that depends on it. Each form marks a look in its own way, so
the mark is an associated type: the directory's mark is today's `Seen`
values, and the table's mark is the last arrival number and a count of
replacements. `Changes` lists what the memory reads again and what it
forgets.

The laws of `Prints`, each one a function of the trait alone:

- **Exclusion.** While a `Hold` lives, no other holder of the same place,
  in this process or another, completes a `place`, a `replace` or a
  `set_aside`.
- **Exact look.** `since(mark)` answers every name whose bytes changed after
  the look that made `mark`. A memory brought level by it equals a memory
  that reads every print again (SPEC law 45, "incremental equals cold").
- **Placement.** After `place(batch)` and the `finish` of its hold return
  `Ok`, `kept(n)` answers the given bytes for every name `n` of the batch.
  At every instant before that, and after a stop at any point, a reader
  sees a parents-first prefix of what the hold placed. For the directory
  the prefix can be any length. For the table it is empty or everything.
- **Replacement.** A stop at any point of `replace` leaves the old bytes or
  the new bytes under the name (SPEC law 75), in both forms.
- **Finish.** `finish(heads)` records the heads where the form keeps a
  record: the directory writes `.heads` by today's steps, and the table
  commits the heads with the transaction.

With this trait, `Memory`, `Decision`, the twin check, the deps rule and
`receive` stay one piece of code. `Record`, the steps that keep `.heads`,
moves into the directory form, whose `finish` runs them. The table form
holds a lock that every process honours (a SQLite write transaction that
begins with `BEGIN IMMEDIATE`), so a `Decision` can be asked of it (SPEC
section 3, "A decision is the store of record's"). A commit with
`synchronous=FULL` syncs the write-ahead log before it returns, so a
decision on the table form is written through, as on the directory.

### 1.3 What the table form adds

The table form answers three questions from an index, without the whole
history in memory: one entity's objects, the objects that rest on an
object, and the heads. The heads are `heads()`, which exists. The other two
are new provided methods of `Replica` whose default reads `held()`, so
`MemoryStore`, `Overlay` and the store of files answer them too, as they
answer `heads()` today:

```rust
/// One entity's objects in the prodrome `genesis`, parents first.
fn entity(&self, genesis: &Hash, key: &E::Key) -> Result<Vec<(Hash, Envelope<E>)>, ProdromeError>;
/// The objects of the history that name `name` as a parent.
fn resting_on(&self, name: &Hash) -> Result<BTreeSet<Hash>, ProdromeError>;
```

The laws:

- **Entity.** `entity(g, k)` is exactly the objects of the history whose
  event is about the entity `k` in prodrome `g`, in an order where each
  parent comes before its child. It equals the filter of `held()` by the
  same entity.
- **Resting on.** `resting_on(n)` equals one step of `Dag::above([n])`
  over `held()`, without `n`.
- **Heads.** `heads()` equals the tips and the count of `held()` (SPEC law
  65). On the table form this holds at every instant, because the tip index
  changes in the same transaction as the rows. So the table form keeps no
  `.heads` file, and laws 61 to 63, which describe a record that another
  door can make stale, do not apply to it.

**A post of several objects as one transaction.** `receive` is already the
post: it verifies every print before it takes any, and it places parents
first. On the table form it is also atomic: a reader sees none of the post
or all of it. The law is the Placement law of `Prints`. No reading of the
library depends on atomicity, because each prefix of a post is closed under
parents and is a valid history (SPEC law 40). So the change adds no type
for it. A host that must show all of a post or none of it opens the table
form.

**An append that reads one entity.** On the table form, an append needs
the entity's heads (its `deps`), a twin check, and the reading of the
inflationary registers that it writes. All three depend only on the
entity's objects, because `deps` never leave the entity and a twin has the
same event, and so the same entity (SPEC section 3, "Appending writes a
`Change`"). Stage 4 adds this law:

- **Entity append.** For every history and every event, an append decided
  over `entity(genesis, key)` and the genesis writes the same bytes, or
  answers the same twin, as an append decided over the whole history.

**A frontier for a pull.** `Frontiers::answer` takes a `Dag`. A store of
record is a long-running process that keeps its `Dag` in memory, so the
table form answers a pull with the same code over the same `Dag`, and SPEC
law 47 holds unchanged. The table form answers a quiet pull (a pull whose
token is the hash of the store's tips) from its tip index, without the
`Dag`.

### 1.4 What a program can no longer do with the table form

- It cannot bring objects in by another door. A git checkout, a git merge,
  or a copy of files into `objects/` has no counterpart. A union of two
  table stores is a `sync`.
- It cannot read a store with `ls`, `cat`, `grep` or `git log`. It reads
  it with the `sqlite3` tool (section 2.4).
- It cannot keep the store's own history of arrivals in git. The table
  form records an arrival number for each row, which is local and decides no
  value.
- It cannot open the store in the browser through this crate (section
  2.4).

A program that keeps the store of files loses nothing.

## 2. Which engine, by measurement

### 2.1 The prototype and the machine

Each prototype was a scratch crate outside the repository, not committed.
It used `prodrome-core` from this branch and generated one store in three
forms:

- **The data.** One `Genesis` and 49,999 `Change` objects of the reference
  todo schema (`reference::mk_authored`), 50,000 objects in all. One entity
  has 5,000 writes, and the other 44,999 writes are spread over 500
  entities. Each change rests on its entity's last change. The mean print
  is 479 bytes, and the prints are 23.9 MB in all. Each event has its own
  instant, so the twin check of issue #80 finds no candidate.
- **The store of files.** The objects were written as files, then
  `record_heads()` ran. The measurements use `EventStore` as it is.
- **SQLite.** SQLite 3.50.2, bundled by `rusqlite` 0.37, with
  `journal_mode=WAL`, `synchronous=FULL` and `secure_delete=ON`. Tables:
  `object(seq, name, print)`; and the derived indexes `rests(parent, seq)`,
  `entity(key, seq)`, `event(id, seq)`, `tip(name)` and `entity_tip(key,
  name)`.
- **redb.** redb 3.1.3, with its default durability (each commit syncs),
  and the same tables as key-value tables.
- **The machine.** A virtual machine with 4 virtual CPUs (Intel Xeon at
  2.1 GHz), 15 GiB of memory, Linux 6.18, ext4 on a virtio block device,
  Rust 1.97. A `fsync` on a virtual disk may cost less than on real
  hardware. Power loss was not tested.

Each figure is the median of 7 runs, each in a new process, with the page
cache warm. The second figure of a cell, where there is one, is the median
of 5 runs after the page cache was dropped (`echo 3 >
/proc/sys/vm/drop_caches`). Mutating runs each wrote new objects, so later
runs saw a slightly larger store.

What the prototype left out, and what that does to the figures:

- The table append did not check inflationary registers. With the check,
  an append reads one entity's objects and folds them. The measured cost
  of that read is the row "One entity's objects", 68 ms for 5,000 writes.
  A cached fold of the entity (issue #83, "A snapshot") would remove it.
  (Estimated.)
- The table open built the `Dag` with `Dag::from_prints` and folded it. It
  did not take the interior, which the store of files does.
- The table heads counted the rows with `count(*)`. A count kept in one
  row would answer in microseconds. (Estimated.)

### 2.2 The figures

All figures in this table are measured. Times are in milliseconds, warm
cache first and cold cache second.

| Operation | Store of files | SQLite | redb |
|---|---|---|---|
| Open, read every object, build the `Dag` and the fold | 1,291 / 2,299 | 783 / 870 | 912 / 1,058 |
| The same, reading the bytes alone | not separable | 62 / 76 | 103 / 181 |
| A read on a handle that has read the store (the look) | 125 / 121 | 0.010 / 0.015 | 0.002 / 0.003 |
| One entity of 5,000 writes, new process, to its fold | 1,296 / 2,548 | 68 / 112 | 84 / 272 |
| The same, on a handle that has read the store | 130 / 128 | not measured | not measured |
| An append, on a handle that has read the store | 142 / 132 | 1.6 / 3.1 | 1.4 / 4.1 |
| An append, in a new process (for a table, after the connection opens) | 1,267 / 2,302 | 1.6 / 3.1 | 1.4 / 4.1 |
| A post of 256 objects | 239 / 231 | 27 / 69 | 32 / 68 |
| The heads, in a new process | 1.1 / 16.6 | 2.8 / 28.2 | 1.8 / 4.3 |
| A frontier's answer, 256 objects lacking, new process, in law 47's order | 1,167 / 2,155 | 251 / 243 | 306 / 314 |
| The same, on a handle that holds the `Dag` | 2.7 / 2.8 | same code as files | same code as files |
| A frontier's answer in arrival order (a range read) | not possible | 0.6 / 0.9 | 1.9 / 4.5 |
| Peak memory of a process that opens and reads every object | 317 MiB | 269 MiB | not measured |
| Peak memory to read one entity / to append | not measured; at least as for the open, because both read every object first | 35 MiB / 12 MiB | 53 MiB / not measured |

Size on disk, measured:

| Form | Size |
|---|---|
| Store of files, bytes of the files | 23.9 MB |
| Store of files, blocks allocated (4 KiB for each file) | 200.6 MiB |
| The same files in a git pack (`git gc`) | 10.3 MB |
| SQLite, after a checkpoint | 47.0 MB: the object table 29.3 MB, the indexes 17.7 MB |
| redb, without a compaction | 67.4 MB |

A kill in the middle of a post, measured. A child process posted batches
of 2,560 objects in a loop, and the parent killed it with `SIGKILL` after a
delay. Then a new process opened the store and checked it.

| Form | Trials | What the kill left | Open after the kill |
|---|---|---|---|
| Store of files | 10 | 9 trials left part of a post (for example 77 of 2,560 objects); each left one temp file, which `verify` reports; the history was closed under parents in every trial | 1.0 to 1.5 s, the same as any open |
| SQLite | 10 | every trial left whole posts only; `integrity_check` was `ok`; the tip index equalled the derived tips | 2.4 to 11.4 ms |
| redb | 10 | every trial left whole posts only; the tip index equalled the derived tips | 66 to 214 ms, growing with the file (55,000 to 142,000 objects) |

Other measurements:

- **A backup while a writer posts.** SQLite `VACUUM INTO`: 6 of 6 copies
  passed `integrity_check` and held whole posts only; each took 0.18 to
  0.38 s for 52,000 to 104,000 objects. redb has no online backup. A plain
  copy of its file opened in 6 of 6 trials with whole posts only, in 0.07
  to 0.26 s. redb does not document such a copy as safe, so 6 passes prove
  nothing.
- **A husk's dropped bytes.** A test stored a print that held a marker,
  replaced it by a husk, and searched the database file for the marker.
  SQLite with `secure_delete=ON`: the marker was in the main file until
  the next checkpoint, and absent from the main file and the log after it.
  SQLite with `secure_delete=OFF`: the marker stayed after the checkpoint.
  redb: the marker stayed after the commit, and was gone after three more
  commits and after `compact()`. redb gives no guarantee of either.
- **A check of the table.** SQLite `integrity_check`: 688 ms. A rehash of
  50,000 prints: 67 ms (in Python, with C hashing).
- **The build.** A clean release build of `rusqlite` with the bundled
  SQLite: 54.7 s. Of `redb`: 6.9 s.
- **The browser target.** `cargo build --target wasm32-unknown-unknown`:
  redb builds. `rusqlite` with the bundled SQLite fails, because the C
  compiler finds no `stdio.h` for that target.

### 2.3 What the figures say

- **Decoding dominates a cold open.** SQLite reads the bytes of 50,000
  objects in 62 ms, and the open with the `Dag` and the fold takes 783 ms.
  So about 720 ms is the core's decode and fold, which every form pays.
  The table form saves about 500 ms of a warm open and 1.4 s of a cold one.
- **The look is the largest cost of a warm store of files.** Every read on
  a warm handle lists 50,000 entries and compares their `stat` values: 125
  ms. An append and a post on a warm handle pay it first. The table form
  replaces it with one read of a number: 0.01 ms.
- **An append without the whole.** The store of files must read the whole
  store before its first append in a process: 1.3 s. The table form reads
  the entity's heads from an index: 1.6 ms, or about 70 ms with the
  inflationary check (estimated from the entity row).
- **A post.** The store of files syncs each of 256 files before its rename:
  239 ms, about half of it the look. One SQLite commit: 27 ms.
- **The order of a frontier's answer.** Law 47 orders the lacking objects
  by Kahn's walk over the whole store, with the smallest name first among
  the objects that are ready. An arrival number is a different order, so a
  table cannot answer law 47's order from a range read: it reads the names
  and parents of every object (about 250 ms cold) or keeps the `Dag` in
  memory (2.7 ms). Section 1.3 keeps law 47 and the `Dag` in memory.
- **The two engines are close.** Where an operation takes more than 30
  ms, SQLite and redb are within a factor of 2.5 of each other. The larger
  ratios are between times under 30 ms. The largest gaps that matter are
  redb's reads with a cold cache and its recovery after a kill.

### 2.4 Why SQLite, and what else was weighed

| Question | SQLite | redb |
|---|---|---|
| Speed (section 2.2) | within 2.5 times of redb on every row above 30 ms | within 2.5 times of SQLite on every row above 30 ms |
| Recovery after a kill | 2 to 11 ms (measured) | 66 to 214 ms, growing with the file (measured) |
| Backup while it is written | `VACUUM INTO` and the backup API, documented and tested | none documented; a copy of the file is not documented as safe |
| Inspection by a person | `sqlite3`, installed almost everywhere | no tool; a person needs a program |
| A husk's dropped bytes | gone from the file after a checkpoint, with `secure_delete=ON` | stay until the page is reused or the file is compacted |
| Size at 50,000 objects | 47.0 MB | 67.4 MB |
| Build | C, compiled by `cc`; 54.7 s clean | pure Rust; 6.9 s clean |
| Browser target | does not build out of the box | builds |
| Licence | SQLite is in the public domain; `rusqlite` and `libsqlite3-sys` are MIT | MIT or Apache-2.0 |
| Format stability | file format stable since 2004 | the file format changed at a major version, and a program upgrades an older file |

**Does the table form need the browser target?** No. A page holds its
replica as a `MemoryStore`. Where a page persists its replica, it keeps the
prints in the browser's own storage, which is already a table (section
4). So the table
form goes in its own crate, and `prodrome-core` and the WebAssembly build
stay free of C.

**What durable means on each file system.** With `synchronous=FULL` in WAL
mode, SQLite syncs the write-ahead log at each commit. On ext4, XFS and
ZFS that commit survives a power loss, where the drive honours a flush of
its cache. On macOS, `fsync` does not flush the
drive's cache: the table form sets `PRAGMA fullfsync=ON` and
`checkpoint_fullfsync=ON` there. The store of files is already safe there,
because Rust's `File::sync_all` uses `F_FULLFSYNC` on macOS. WAL mode needs
shared memory between processes, so the table form refuses a network file
system. The store of files has the same limit in practice, because `flock`
is not reliable over NFS.

**What a reader sees during a write.** In WAL mode a reader sees the last
committed transaction and never part of one, and it does not block the
writer. One writer runs at a time. A reader that holds a transaction for a
long time stops the checkpoint, and the log grows until the reader ends.
The store of files lets a reader see any parents-first prefix of a post
(measured: 9 of 10 kills left part of a post).

**How a person inspects a table.** The `sqlite3` tool reads the file
while a writer runs, and the names are the file names of the store of
files:

```console
$ sqlite3 store.sqlite "SELECT print FROM object WHERE name = '<64 hex digits>'"
$ sqlite3 store.sqlite "SELECT count(*) FROM object"
```

**Other engines that were weighed, and why not:**

- **LMDB** (through `heed`): fast reads and a hot copy (`mdb_env_copy`),
  but a C library like SQLite, with a map size that a host must set in
  advance, and only a dump tool for inspection. It brings SQLite's costs
  without SQLite's tools.
- **fjall** and **sled**: pure Rust key-value stores. fjall is young, and
  sled's file format is not stable. A store that must open for years needs
  a stable format.
- **RocksDB**: a large C++ build, and tuning that this workload does not
  need.
- **A pack file of our own**: one append-only file of prints with an index
  rebuilt at open, as git's pack files are. The crate would keep the bytes
  and need no dependency. The crate would then own a crash recovery, a
  compaction for husks and a backup procedure, which is the work an engine
  already does and has tested. It is the strongest alternative, and
  section 8 returns to it.
- **IndexedDB**: the browser's table. It serves only the page, and a page
  already holds its replica in memory (section 4).

## 3. The same objects, byte for byte

**What a row holds.** The table `object` has three columns:

- `seq`, an integer that counts arrivals. It is local, and no value
  depends on it.
- `name`, the 64 lowercase hex digits of the name, as text, which is
  exactly the file name without `.py`. The prototype used 32 raw bytes.
  Text costs about 3.2 MB more at 50,000 objects, for the column and its
  unique index (estimated). An operator then types the same name in both
  forms.
- `print`, the exact bytes that `objects/<name>.py` would hold: the whole
  print, or the husk.

The row holds no column for the form. A reader hashes the bytes against
the name, as `dag::decode` does for a file: bytes that hash to the name are
the whole, and other bytes are a husk where the schema has husks, or are
tampered. A second column would be a second truth that could disagree with
the bytes.

**Moving a store between the forms.** A copy takes the hold of both
stores, reads every name that the source keeps (in the history and
waiting), and places the same bytes under the same names. It refuses while
the source holds a print that is no object, as every write does: run
`fsck` first. The laws:

- **A copy changes no print.** For every store `S`, the names and
  bytes of `copy(S)` are the names and bytes of `S`. A copy from the files
  to a table and back leaves the same files in `objects/`.
- **A check of one is a check of the other.** `verify(copy(S))` reports
  the same findings of the `Dag` (`Dag::verify`) as `verify(S)`. Each form
  then reports the findings that only it can have.

Findings that only the store of files has: temps and strays in
`objects/`, a leftover `HEAD` or `refs/`, and a stale `.heads`. Findings
that only the table form has: an index row that the object rows do not
derive, and an error of SQLite's own `integrity_check`. The quarantine of
the table form is a table `aside(name, computed, print)`, and `verify`
reports each of its rows as the store of files reports each file of
`quarantine/`.

**A husk in the table form.** `husk(names)` replaces the bytes of each row
in one transaction, and checkpoints the log after the commit. With
`secure_delete=ON`, the dropped bytes are then gone from the database file
(measured, section 2.2). They are not gone from a backup taken before, from
the blocks that the file system freed, or from an SSD's spare cells. The
erasure that SPEC section 3 relies on is the destruction of the key, in
both forms. So the table form may drop a husked object's bytes in the same
way as the store of files: it replaces them with the husk, and it never
takes the whole back (SPEC law 73).

**What the table derives, and how it proves it.** Each index is a function
of the object rows:

- `rests(parent, seq)`: the parents of each object, from `parents_of`;
- `entity(genesis, key, seq)`: each change's prodrome and entity. The
  prototype held one prodrome and left out the genesis;
- `event(id, seq)`: the hash of each change's event print, for the twin
  check. This is the key that issue #80 proposes for the twin check in
  memory;
- `tip(name)` and the count: the heads of the history;
- `entity_tip(genesis, key, name)`: each entity's heads, the `deps` of the
  next append to it.

Each index row is written in the same transaction as its object row, so a
crash cannot leave an index that disagrees with the rows: SQLite's commit
is atomic, and the kill test found the tip index equal to the derived tips
after each of 10 kills. A bug, or a person who edits rows with `sqlite3`,
can still make them disagree. So `fsck` on the table form does four
things: it runs `integrity_check`, it rehashes every print, it sets aside
every print that fails its hash and is no husk, and it rebuilds every index
from the rows into temporary tables and compares them with the stored
ones. It reports each difference as a finding and repairs the index from
the rows. At 50,000 objects this costs about 3.8 s: 0.7 s for
`integrity_check` and 0.07 s for the rehash (measured), and about 3 s for
the rebuild, which decodes every print (about 0.7 s) and writes the tables
(about 2.3 s, from the 2.28 s that the prototype took to build all the
tables) (estimated). The table form does not run this at open, because
its commit already makes the rows and the indexes one write.

**Local rows beside the objects.** Issue #82 asks that the table form also
hold the local keys and snapshots that a program keeps beside a store
today. Frontier records (`frontier::Records`) and snapshots of folds are
derived and safe to back up, so the table crate keeps them in tables of the
same file. Secret keys are not: a backup of the file would copy them. So
the table crate keeps secret keys in a second database file, which the
store attaches, and which `VACUUM INTO` does not copy.

## 4. The rule for which store goes where

**The rule, in one sentence:** keep a store as files when another party
takes it through git (it is published, forked, merged by git, or reviewed
as a diff), and keep it in a table otherwise.

The questions behind the rule:

1. **Is the store published, or merged by another party?** Then git is the
   way that party takes it, and a store of files is a directory that git
   merges without conflict. This question decides alone.
2. **Does anything need git's history of the store?** The objects are
   already the history of the content. Git adds who committed which
   objects and when, and a review of each addition as a diff. If a person
   reviews additions, the store is files.
3. **How fast does it grow?** The store of files pays for its size on
   every warm read (125 ms at 50,000 objects, and the listing grows with
   the count) and 4 KiB of disk for each object. A store that only its own
   program reads, and that grows by thousands of objects a day, belongs in
   a table.
4. **Who backs it up?** If a git remote is the backup, the store is files.
   If the host backs it up, or a second replica is the backup, the store
   is a table.

The rule applied to the shapes that a program has:

| Shape | Form | Why |
|---|---|---|
| A public record that others fork | files | Others take it through git and send changes as pull requests. |
| A private record in a private repository | files | Its writers merge it through git, and the remote is its backup. |
| A high-volume log with signatures and sealed content | table | Only its program reads it, it grows fast, and a husk must leave the file. Signatures are objects (`Signed`), so they need nothing of the form. |
| A cache that can be rebuilt | table | Nothing backs it up. If `fsck` finds damage, the host deletes the file and pulls again. |
| A device's own replica in a browser | neither | The page holds a `MemoryStore`. Where the page persists it, it keeps each print by name, with the same bytes, in the browser's storage, which is already a table. The table crate does not build for the browser, and the page does not need it. |

## 5. What git gave for free, and what replaces it

| What git gave | What replaces it for the table form |
|---|---|
| A backup: each clone and each remote holds every object | `VACUUM INTO <file>`, one command that a writer does not stop (measured: 6 of 6 consistent). A second machine that holds a full replica is a second backup. |
| A way to know that a backup worked | Open the copy and run `fsck` on it. An empty report means that every print hashes to its name or is a husk, and that every index agrees with the rows. |
| Sync between machines | The replica's sync (`sync`, `accept`) and pulls by token (`Frontiers`), which the library has. The host supplies the wire. |
| A history of the store itself | The DAG is the history of the content. The arrival number orders arrivals on one machine. Who wrote an object is in its event, and a `Signed` object proves it. Who copied it in is not recorded. |
| Recovery from a bad write | A wrong event is corrected by a new event, in both forms. A damaged object is set aside by `fsck` and fetched again from a replica. A restore from a backup returns the whole store to an earlier state. Git's revert of a commit that added files has no counterpart: a store never deletes an object. |
| Compression | None. The table holds the prints uncompressed: 47.0 MB against a 10.3 MB git pack (measured). |

**What a second machine holds, and how often.** It holds a full replica: a
table store of the same objects. It pulls from the store of record after
each write, or on a schedule that the host chooses, and the backup can lose
the writes since its last pull. A pull that sends nothing costs one read of
the tip index on the table form.

**A machine that is offline for a week.** When it returns, it pulls with its
last token. The store of record keeps the most recent 512 frontiers
(`frontier::KEPT`). If the store of record records 512 other frontiers in
that week, for example for 512 posts, the token is gone, and the pull starts
from nothing and sends the whole store (SPEC law 47(c)): about 24 MB at
50,000 objects (estimated from the print size). If the machine wrote while
it was offline, it posts its objects. A post is a union, so nothing
conflicts.

**What an operator runs at night.** One command makes the backup, and one
command checks it, with the CLI of stage 3:

```console
$ sqlite3 store.sqlite "VACUUM INTO 'backup-2026-11-21.sqlite'"
$ prodrome fsck --store backup-2026-11-21.sqlite
```

The second command prints nothing for a healthy backup. To restore, the
operator stops the writer, copies the backup over the store, and starts
the writer.

## 6. What it settles on the roadmap

The open items on the branch `roadmap-data`, as `prodrome list` printed
them on 2026-11-21, that touch the store:

| Item | Effect |
|---|---|
| `gh-82` | Advanced by this note. Closed by stage 3. |
| `gh-80` (the twin check is quadratic) | Changed. The table's `event` index is the key that #80 proposes. Fix #80 in `Memory` first, with that key, so that both forms share it. |
| `gh-81` (a check of chosen objects) | Untouched. The check reads a `Dag`, and both forms give one. |
| `gh-83` (store primitives) | Changed. Its "frontier for each entity" is the table's `entity_tip` index, and stage 4 offers it through `Replica::entity`. Its "skeleton" is the table's `rests` index. Admission, snapshots and sealed slots are untouched. |
| `ancestry-beyond-bitsets` | Untouched. The bitset is for each entity and lives in the fold, which both forms share. At 5,000 writes the entity's read and fold took 68 ms (measured), so the item stays as priced. |
| `lazy-content-by-hash` | Changed in part. The table form serves a skeleton (names and parents) without the prints from its `rests` index. The split of an object into a skeleton and content is a change of the stored format, and this note does not make it. |
| `read-without-copies` | Untouched, and more valuable. The core's decode and fold are about 720 ms of a 783 ms cold open of the table form (measured), so this item is then the largest cost of a cold open. |
| `quiet-pull-from-heads-record` | Changed. On the table form a quiet pull reads the tip index, which is exact. The item stays open for the store of files. |
| `plugin-store-admission` | Untouched. |

The roadmap holds no open item named "store growth" or "a frontier's
tips". Issue #82 refers to both. The nearest items are
`ancestry-beyond-bitsets`, above, and the completed `kept-linear-order`
and `frontier-token`, which this note does not change.

## 7. The stages

Each stage is one pull request that lands alone, with its laws. Sizes are
estimated.

1. **One suite of store laws over every backend.** The laws of the store
   of record (40, 45, 60 to 65, and 72 to 79) are tested today with steps
   that move files: quarantine by a move, a stop between the steps of a
   rename. This stage restates each step as an operation that every store
   of record has (place, replace, set aside, a stop after each step), and
   runs the laws from one suite over a factory of stores. The suite runs
   over the store of files and, where a law applies, over `MemoryStore`.
   The script law's `Kind` (law 68) already runs three replicas, and it
   stays. No backend is added, and no observable behaviour changes, so
   `SPEC.md` changes only to name the tests. Size: about 600 lines of
   tests changed. It deletes the second copy of the scripted writes in the
   tests of `record.rs` and `husk.rs`, if they fit the suite; the pull
   request counts the lines. A consumer changes nothing.
2. **`Prints` inside the core.** The directory's effects move from
   `store.rs` (a file that `AGENTS.md` lists as too large) into
   `store/directory.rs`, behind the trait of section 1.2, and
   `EventStore` gains the type parameter with `Directory` as its default.
   The suite of stage 1 passes unchanged. Size: about 700 lines moved and
   200 lines new. It deletes nothing. A consumer changes nothing.
3. **The table crate.** A new crate, `prodrome-sqlite`, implements `Prints`
   over SQLite, the copy between the forms, `fsck` with the index check, and
   `frontier::Records`. `SPEC.md` section 3 gains "A store kept in a table",
   with the laws of sections 1.2 and 3 (placement is atomic, heads are
   exact, a copy changes no print, a check of one is a check of the other)
   and the new findings. The pull request adds a `CHANGELOG.md` entry, which
   states that no stored object changes. The CLI opens a table when the path
   of `--store` ends in `.sqlite`. The suite of stage 1 runs over the table.
   Size: about 900 lines and tests. It deletes nothing, because the store of
   files stays. A consumer changes only the call that opens the store.
4. **Answers from the indexes.** `Replica::entity` and
   `Replica::resting_on` become provided methods, and the table overrides
   them. The table's append reads one entity, under the entity-append law
   of section 1.3, and its quiet pull reads the tip index. `SPEC.md` gains
   the laws. Size: about 500 lines. It deletes `Memory`'s twin map if #80
   lands as an index by event hash, because both forms then use the same
   key. A consumer changes nothing, and may call the new methods.

A fifth stage would answer a pull in arrival order from a range read
(measured: 0.6 ms against 251 ms cold). It changes law 47(a), which fixes
the order, and so changes what a replica observes. Do it only if a store of
record cannot keep its `Dag` in memory.

## 8. What argues against doing it at all

The strongest case against the change:

- **Most of the gap is two costs of the store of files, and each has a
  smaller fix.** A warm read lists 50,000 files (125 ms). A write could
  bump a counter file, as it rewrites `.heads` today, and a warm handle
  could read only that file. A post syncs each file (about 120 ms for 256
  files). One pack file for each batch, or one `syncfs`, could make it one
  sync. Both fixes stay inside the reference form. The counter has a
  price: another door's files would be seen only at the next hold of the
  lock, as under SPEC law 62 for the record of heads today, so `held()`
  would become exact only for the store's own writes.
- **The cold open barely moves.** The table form opens in 783 ms against
  1,291 ms (measured). The rest is the core's decode and fold, and
  `read-without-copies` attacks that for both forms.
- **Two backends double the surface.** Every store law must hold on both,
  every store bug can need two fixes, and `SPEC.md` grows a second
  description of a store. Operators learn two ways to back up and two ways
  to inspect.
- **A C dependency.** SQLite adds 55 s to a clean build (measured), a C
  compiler to the build of every program that uses the table crate, and
  memory-unsafe code under the crate's safe API.
- **Partial posts are already harmless.** Every prefix of a post is closed
  under parents and is a valid history (law 40), so no reading of the
  library needs atomicity.
- **The disk is cheap where it matters.** 200 MiB of blocks is the cost on
  one file system, and git stores the same files in 10.3 MB.
- **With a pack file of our own, the crate keeps one form.** An append-only
  file of prints with a rebuilt index removes the per-file costs without a
  second model of a store, at the price of a recovery and a compaction that
  the crate must own.

**What would make me stop after the first stage.** Run two experiments on
the store of files after stage 1: a warm handle that reads a write counter
in place of the listing, and a post with one sync. Stop after stage 1 if
both are true:

- the warm read and the append of the store of files come within a factor
  of ten of the table's (measured here: 125 ms against 0.01 ms, and 142 ms
  against 1.6 ms); and
- no program has a store that only its own program reads, that grows by
  more than about 100,000 objects a year, or that must remove a husk's
  bytes from its file.

Stage 1 is worth keeping in either case, because it tests the store laws
over every replica from one suite.

## 9. What the experiments decided

This section was written after stage 1 and the two experiments of section
8, and after the changes that they led to. Each figure below is measured:
the median of 7 runs, each in a new process, in milliseconds, on a store
generated by the method of section 2.1 (50,000 objects, a mean print of 472
bytes), warm cache first. The cold figure, after the slash, is the same
operation after the page cache was dropped, on a handle that had read the
store before the drop. The cold figures of the SQLite column were measured
by the method of section 2.2 instead, with the cache dropped before the
process began, so the two cold columns are not compared here. The example
`core/examples/store_doors.rs` lays out the store and runs each operation
of the first three columns, opened either way; its documentation gives the
commands.

**The experiments.** A handle that has read the store reads one small file,
the mark of the last hold, in place of the listing of `objects/`. A batch of
256 prints or more is made durable by one `syncfs`, on Linux 5.8 or later
and on ext4, XFS, btrfs or ZFS. Below 256 prints, a sync of each file costs
less, because one `syncfs` also waits for every other program's data on
the file system (about 150 ms with 64 MiB of it, measured).

**What a decision reads.** The one figure that the two experiments did not
move was the append, because a decision listed `objects/` so that it saw
every door: a `git merge`, a copy of files, a second binary. So a store now
says, when it is opened, whether other programs may change it
(`Doors::Shared`, the default) or whether nothing but this library writes
it (`Doors::Alone`). A decision in a store opened as alone trusts the mark,
as a read does. The choice changes no stored byte. An object that arrives by
a door that such a store said it lacks is not read as history by a handle
that has read the store, `verify` reports the record of heads as stale, and
a fresh handle reads the object (SPEC laws 84 to 86).

| Operation (warm / cold) | Store of files before | Store of files now, shared | Store of files now, alone | SQLite (section 2.2) |
|---|---|---|---|---|
| A read on a handle that has read the store | 130 / 291 | 0.066 / 1.8 | 0.074 / 1.5 | 0.010 / 0.015 |
| An append on such a handle | 130 / 282 | 121 / 287 | 5.8 / 10.6 | 1.6 / 3.1 |
| A post of 256 objects | 237 / 380 | 150 / 319 | 30 / 66 | 27 / 69 |
| A post of 2,560 objects | 1,180 / 1,357 | 331 / 679 | 181 / 287 | not measured |

**The outcome.** Section 8 stops after stage 1 if two conditions hold. The
first is that the warm read and the append of the store of files come
within a factor of ten of the table's. Once a store may say that it has no
other door, they do: the warm read is seven times the table's, and the
append under four times. A store that has other doors keeps an append of
120 ms, which is the price of seeing them. The second condition is that no
program has a store that only its own program reads and that grows fast,
or that must remove a husk's bytes from its file. That condition is not
met: a store opened with `Doors::Alone` is exactly such a store. The
second condition asked whether such a store needed the table, and the
figures answer it for that store: opened as alone, the store of files
comes within the factor of ten that the first condition sets. So the
outcome rests on the measured figures of a store opened as alone, and not
on the absence of such stores. Stages 2 to 4 are not built now. A store
that must remove a husk's bytes from its file is not answered by these
figures, and it is the second item of the list below.

**What would reopen them.** The store of files still cannot give these:

- a post that a reader sees whole or not at all, where today a reader can
  see any prefix of a post, parents first;
- one file in place of one file for each object, which costs 200 MiB of
  blocks at 50,000 objects on ext4, and from which a husk's dropped bytes
  can be removed;
- one entity's objects read without the whole history in memory, which
  today a handle reads before its first append.

A program that needs one of these reopens issue #82, which stays open with
this list.
