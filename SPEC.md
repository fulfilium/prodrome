# Prodrome — the specification

Prodrome is a temporal, content-addressed event database: event objects in a
DAG, folds that turn them into belief at a moment, and FPL, a fuzzy temporal
logic whose terms are a todo's fulfillment as a function of time. This
document is the contract; the crates in this repository are its reference
implementation. §9 lists the laws every implementation must pass, bit for bit
where this text says so and to 1e-9 where it says so, and `conformance/*.py`
holds the vectors they are checked against.

## 1. Principles

- **Types first.** Every value below is a closed algebraic type. Invariants
  live in smart constructors that return an error; a record that exists is
  valid. Names that mean different things are different types even when they
  are all strings: `Hash`, `TodoId`, `Actor`, `Name` do not mix.
- **One evaluator.** Fulfillment is computed in one place, and every consumer
  reads numbers it produced.
- **Order is causal, time is data.** Events are ordered by the DAG. The `at` a
  writer stamped is read by the folds as data and never decides a merge.
- **Decoration, not evaluation.** An explanation or a rendering is an
  annotation of the core's structures, the same shape with a value at each
  node, never a second reading of the semantics.
- **Add kinds, never change shipped fields.** A stored constructor's fields
  are frozen; `""` means absent on shipped string fields; evolution is a new
  constructor.

## 2. Values and the literal grammar

Every stored object is one expression of a small Python-literal grammar,
printed canonically and parsed strictly. Printing is total and deterministic:
equal values print byte-identically.

- `None`, `True`, `False`.
- Integers: decimal, optional leading `-`.
- Floats: Python `repr`, the shortest string that round-trips; integral values
  carry `.0`; exponent form when the decimal exponent is `< -4` or `>= 16`;
  `inf` and `nan` are not storable.
- Strings: Python `repr`: single quotes unless the text contains `'` and no
  `"`; escapes `\\`, the delimiter, `\n`, `\r`, `\t`; other non-printable
  characters as `\xNN`, `\uNNNN`, `\UNNNNNNNN` per `str.isprintable`;
  printable non-ASCII as itself.
- `datetime(Y, M, D, h, m, s)`, with a seventh microsecond argument only when
  non-zero; naive local time.
- `timedelta(days=…, seconds=…, microseconds=…)`, only the non-zero parts, in
  that order; `timedelta()` for zero.
- Tuples: `(a, b)`, `(a,)`, `()`.
- Constructor calls `Name(field=value, …)`: every field, in declared order,
  keyword form. The admitted names are the vocabulary of §3, §4 and §7 plus
  `datetime` and `timedelta`. §4's half is the core's kinds UNION the host
  payload's — its record kind and the constructors that kind's fields nest —
  so the whitelist is a union and is still a whitelist; where the two would
  name the same constructor, the core's wins and the host's is unreachable.

Parsing admits exactly this grammar: no names, operators, comprehensions or
attribute access. A call dispatches to its smart constructor, whose error is
the parse error. Anything else is a refusal, never a crash; the parser is
fuzzed.

**The vector vocabulary.** `conformance/*.py` — the evidence §9 is checked
against — is one expression of THIS grammar, printed by this printer and read
by this parser, against a SECOND whitelist that is disjoint from a store's:
`Folds`, `Dags`, `Fpl`, `Series`, `View`, `Links`, `Recurs`, `Changes`;
`FoldCase`, `DagCase`, `FplCase`, `SeriesCase`, `ViewCase`, `LinkCase`,
`RecurCase`, `ChangeCase`; `Bound`, `Open`, `Spec`, `Content`, `Object`,
`Parents`, `Conflict`, `RowConflict`, `Refused`, `Step`, `Price`, `Sample`,
`Knot`, `Asked`, `Row`; and, for an explanation's
decoration, `Node`, `NoteEntry`, `One`, `Many`, `Maps`, `Fields`, `Pair`. A
STORE's parse admits none of these and a VECTOR's parse admits none of §4's
or §7's: two vocabularies, one grammar, one parser, and neither can widen the
other.

Inside a vector, every stored object, event and term is a STRING holding its
canonical print — never a nested literal — because those exact bytes are what
the vector is evidence of (§9.1). Instants are `datetime(...)`; the mappings a
vector needs (by todo id, by object name) are tuples of a named constructor,
since this grammar has tuples and no mapping.

## 3. Objects and the DAG

- An **object** is one of eight constructors.
  - `Genesis(label, nonce)` begins a prodrome. `label` is the host's name for
    it and `nonce` is 32 lowercase hex characters drawn once, so two
    prodromes a host labels alike are two. It has no parents and no event,
    its name is the prodrome's identity, and every object that names it
    rests on it.
  - `Change(genesis, deps, event)` is an event, named by its prodrome and by
    the writes it supersedes. `deps` are object names, sorted, distinct and
    possibly empty; `event` is required, since a change without one would be
    a merge and there are none.
  - `Snapshot(genesis, tips, previous)` attests a state of one prodrome.
    `tips` are sorted, distinct and non-empty, and `previous` is a snapshot's
    name or `''`.
  - `Sealed(prev, event)` and `Woven(parents, event)` are the LEGACY
    envelopes, read forever: `append` writes neither, and `merge` writes a
    `Woven` only over a legacy store's tips. A `Sealed` has one parent,
    `prev == ""` at a legacy root; a `Woven` has two or more, sorted and
    distinct, and its `event` may be `None`: a merge is structure.
  - `Signed(object, key, signature)` is a DETACHED signature: `key`, an
    Ed25519 public key as 64 lowercase hex (a point of the curve), signs the
    object named `object`, and `signature` is the signature's 64 bytes as
    128 lowercase hex. What is signed is the UTF-8 bytes `prodrome object `
    followed by `object`, so a signature covers an object's name and through
    it every byte of the object, its genesis and deps included, and the
    signed object's bytes and name never change: an object gains signatures
    at any time, from any number of keys. A signature VERIFIES when Ed25519's
    strict verification (no small-order key, a canonical scalar) accepts it.
    Signing is deterministic, so one key signing one object writes one
    `Signed`, byte for byte, however often it signs. Its prodrome is its
    object's, which is why it names no genesis.
  - `KeyAdded(genesis, actor, key)` registers `key` for `actor` in the
    prodrome `genesis`. It rests on its genesis alone, so registering a key
    twice is one object and a key may sign before it is registered (a device
    signs what it wrote offline and is registered when next seen).
  - `KeyRevoked(genesis, deps, actor, key)` revokes `key` for `actor` in
    `genesis`, except for the signatures beneath it: `deps` are object
    names, sorted, distinct and possibly empty, and name what its writer
    stands behind, as a change's deps name what its writer saw (§5 reads
    it).

  WHAT AN OBJECT RESTS ON, `parents_of`, is the edges it names and the
  genesis it names, sorted and distinct: a `Change`'s `deps` and `genesis`,
  a `Snapshot`'s `tips`, a non-empty `previous` and `genesis`, a
  `KeyRevoked`'s `deps` and `genesis`, a `KeyAdded`'s `genesis`, a
  `Sealed`'s `prev` (none at a root), a `Woven`'s `parents`, a `Signed`'s
  `object`, and nothing for a `Genesis`. A change rests on its genesis as on
  its deps: an object names its genesis because it is in that prodrome, and
  cannot be held without it. This is the causal order, and every rule below
  that reads parents reads these and nothing else: the tips, the
  linearisation, `ancestors`, the interior, `verify`'s missing parent, and
  `adopt`'s and `receive`'s parents-first walk. So a genesis is beneath
  everything begun from it, and a head only of a prodrome that holds
  nothing else. (Before 2026-10-04 `parents_of` left out the genesis, so
  every prodrome's heads held its genesis and the linearisation could put a
  change before it; no stored object's bytes or reading changed with it.)
- An object's **name** is `sha256(utf8(print(object)))` in lowercase hex.
  The file `objects/<name>.py` holds exactly that print, and a loader
  re-hashes the bytes before parsing them. `event_id(e) =
  sha256(utf8(print(e)))` names an EVENT apart from where it was written; it
  is never stored.
- **Every object has exactly one genesis.** A `Change`, a `Snapshot`, a
  `KeyAdded` or a `KeyRevoked` names it; a `Genesis` is its own; a `Signed`'s
  is its object's; a legacy object's is the `Sealed("", …)` root
  it rests on, or, where a legacy store already joined two roots, the least
  such root by name, whose one prodrome every legacy object is in. No `deps`
  or snapshot edge crosses geneses. So a store may hold several prodromes, a
  union of two stores of different geneses is DISJOINT, and every fold,
  register and entry factors into per-genesis ones, a todo keyed by
  `(genesis, todo)` (§6). A `Change` may depend on legacy writes, whose
  global ancestry overstates what it depends on; that is safe, since a
  later write can only settle more than the chain says, never less.
- **A write is durable.** Every file the store writes goes to a randomly
  named temp file in the directory it is written to, created exclusively
  (never a fixed `<name>.tmp`, which two writers of one object would
  share), and is synced to disk before it is renamed into place; the
  directory is then synced, once after a batch of placements (one append,
  one adoption). A directory the store makes (the root, `objects/`,
  `quarantine/`) is synced into its parent before anything is written into
  it, and a move into `quarantine/` syncs both directories. So a crash
  leaves either no file or the whole one, never an empty file under a name
  that promises content, and at worst a temp that `verify` reports.
- **A store reads the largest down-set it holds.** A history is a set of
  objects closed under parents, and a store's files need not be one: an
  object set aside (`quarantine`) or not yet arrived leaves what rests on
  it naming a parent the store lacks. The store's HISTORY is the INTERIOR
  of its objects, every object whose ancestors are all held; every read
  (the objects, the fold, `tips()`, an append's deps) reads it and nothing
  else. An object outside it is held, verified, and waits: no read sees it
  until what it rests on arrives, and then it joins as any arrival does.
  The interior is never more than the objects, idempotent and monotone, so
  it is a function of the object set (law 40); of a store closed under
  parents it is the store.
- **A read is total; a write is not.** A file of `objects/` that is no
  object (it fails its hash, it is not text, it does not parse at the
  schema the store is opened at) is held as UNREAD, with why, beside the
  objects, and read again at every look. What rests on it waits, as on an
  object not arrived, so every read answers the history of what verifies,
  and what it leaves out is named as a replica's reading names it (law 44):
  every name held or rested on that is no object, why, and the objects
  above it. A store and a replica holding the same files read the same
  (law 45). A WRITE refuses while anything is unread (every append, merge,
  snapshot and adoption, and the coordinated decision they begin with),
  naming the file: a write names the store's heads, and an unread file may
  be one, so a reading may leave a file out but a decision may not guess
  what it held.
- **Tips are derived.** `tips()` is the set of objects of the history no
  object names as a parent: a function of the object set alone, in any
  order it is listed. A
  store on disk is its `objects/` and nothing else, so two copies that each
  only added objects are merged by uniting the files — a `git merge` of two
  clones is this union and cannot conflict. A store under git needs
  `objects/** -text -diff` in the `.gitattributes` that governs it, so that
  no line-ending conversion or text merge rewrites the bytes a name is the
  hash of: a write to a store whose root holds no `.gitattributes` first
  writes one, durably, marking `objects/**` and `quarantine/**` `-text
  -diff`, and never rewrites one that is there. A store written before 0.9 also holds `HEAD` (one tip) and
  `refs/` (one file per head while there were several); nothing reads them,
  the objects derive exactly what they named (§9.17), and `verify` reports
  them as leftovers to delete.
- **`init(label)`** writes a prodrome's `Genesis`, once.
- **Appending writes a `Change`**, never a `Woven`, into the writer's
  genesis; a store of several geneses is written through a handle that
  names one.
  - If the genesis holds an object whose event prints byte-identically to
    `event`, `append(event)` writes nothing and answers the first such
    object in the linearisation, even one since superseded. Appending is
    IDEMPOTENT: replaying a delivery whose claim was overridden brings
    nothing back, and a host that means to assert it again, having seen the
    override, says something new — a new `at`, a new note.
  - Otherwise it writes `Change(genesis, deps, event)`. `deps` are the
    HEADS of `event`'s entity in the writer's genesis: every object the
    writer holds whose event names that entity, in any register or none,
    that no other such object descends from. They are an antichain, and
    what the writer SAW of the entity, as a git commit's parents are what
    its author had. Each register's frontier is beneath them, so a write
    still supersedes what it supersedes (§6.6), and nothing else. The heads
    are read STRUCTURALLY: under the policy where everything binds, and at
    no moment, because a name records what its writer held and never a
    reader's policy or moment. The first write to an entity has `deps` of
    `()`. (Before 2026-10-01 `deps` were only the frontiers of the
    registers `event` writes, so a writer's add and complete of one entity
    were recorded as concurrent; objects written then keep their bytes and
    read as they did.)
  - A write to an INFLATIONARY register (§6) whose value is not `≥` the
    reading it supersedes is refused before any object carries it, by the
    same structural reading. Of the todo's registers only the tendings are
    inflationary, and a `Tended` supersedes nothing, so no event of §4 is
    refused; a schema whose register supersedes and is inflationary is held
    to it (law 32).

  `deps` never leave the entity (one todo, under the todo schema): a store
  of changes is a disjoint union of per-entity DAGs, and changes to
  different entities are concurrent. A change's name is a function of its
  genesis, its event and what its writer held of its entity, and of nothing
  else: two replicas that agree about an entity write byte-identical
  changes to it, whatever else each holds. Two that disagree may each write
  one event over their own heads; those are TWINS, two objects with one `event_id`, each
  superseding only what its own deps reach, and a register reads them as
  one candidate (§6).
- **`snapshot()`** writes `Snapshot(genesis, tips, previous)`: the genesis's
  tips (never the genesis, once anything rests on it), and as `previous`
  the last snapshot among them; with nothing written
  since that one, it is the answer and nothing is written. Its closure is
  exactly what it attests: an object is in it iff it existed when the
  snapshot was written, because a name cannot be computed before what it
  hashes, and on a `previous` chain each closure contains the one before. A
  snapshot written before 2026-10-04 may name its genesis among its tips;
  it attests the same down-set. No fold reads a snapshot, and no change
  depends on one. When to write one is
  the host's call.
- **Linearisation** is Kahn's algorithm over parents with a min-heap on the
  name: deterministic, causal first, and arbitrary only between incomparable
  objects, which the name orders and no instant does. It orders an entity's
  `stream` (§6.7) and numbers objects for the ancestry index, and decides
  no value (law 25). A missing parent or a cycle is a refusal.
- `ancestors(x)` is the transitive parent closure; `concurrent(a, b)` holds
  when neither is an ancestor of the other.
- **`verify`** changes nothing and reports an object not hashing to its
  name, a missing parent, a cycle, a malformed `Woven`, a leftover `HEAD` or
  `refs/`, every entry of `objects/` that is not named `<name>.py` for a
  well-formed name (a temp an interrupted write left, or a stray) and every
  temp in the root as garbage, each by name — the reads pass over such an
  entry, and only `verify` speaks of it — a receipt for
  each file in `quarantine/`, which stands in for the missing parent that
  object would otherwise be reported as, and an event the
  policy does not `confirm` (§5) dated before any of its ancestors — a writer
  whose stamp the host forces cannot legitimately be dated behind what it was
  written on top of, where a backfill can; a change's ancestors are what its
  deps rest on, so it cannot be dated behind what it was written OVER. It
  also reports an object naming a genesis the store does not hold as a
  genesis (an edge to it is that finding, never also an edge between
  geneses), an edge between geneses, a dep another dep of the same change rests on, a dep that
  is not a write to its change's entity, an object a snapshot attests
  that the store lacks, a `Signed` that does not verify, and an object
  whose actor has a key in its prodrome and which no key of that actor's
  proves (§5's proof, every key object counted). There is no unreachable object and no stale
  head to report: every object is a tip or beneath one, and no tip rests on
  another.
- **`fsck`** is `verify` after QUARANTINE: every file of `objects/` whose
  bytes do not hash to its name is moved, under the lock and durably, to
  `quarantine/<name>.py`, or, where other bytes already lie there,
  `quarantine/<name>.<sha256 of its bytes>.py`, so no set-aside bytes are
  ever overwritten or deleted and the same bytes set aside twice are one
  file; the report names each (the receipt). A file that hashes to
  its name stays (it is the object, even one this reader cannot parse), and
  a stray is reported, never moved. Such a file blocks writes until its
  owner decides what it is: a newer schema's object is read by opening the
  store at that schema, and a print no schema parses is moved out of
  `objects/` by hand. A read leaves a file failing its hash
  out, and says so; a write refuses rather than guess what it held, and
  its refusal names the object and this operation (`prodrome fsck`). No
  read takes anything in `quarantine/` for part of the history.
  Set aside, the store reads the largest
  down-set it holds: the history without the object AND WITHOUT
  EVERYTHING RESTING ON IT, which is exactly what a replica that never
  received the object holds (law 40). `tips()` answers (the object's
  parents may be tips again), and `verify` reports the receipt until the
  object is restored from a replica and the quarantined file deleted. An
  append made meanwhile rests on the heads of that history; once the
  object is back, what rested on it rejoins, concurrent with the append.
- **`adopt(source, tip)`** copies in everything `tip` rests on that the store
  lacks, its genesis among it, verifying all of it
  before writing any, and writing parents before children, since an object
  belongs to the store as soon as its file exists. Placement falls out of the
  derivation: a tip already contained changes nothing; a tip containing every
  tip fast-forwards; otherwise it is a second tip. The source may be another
  store or a map of prints that arrived over a wire (`adopt_objects`); only
  where the bytes are read from differs. A change's bytes say nothing of
  which store holds it, so moving its file between two stores of one genesis
  changes no name (law 29).
- **`merge(parents)`** writes a `Woven` over them, every legacy tip by
  default: a legacy store's join, which a store of changes never needs.
- **A replica** is any set of objects closed under parents that reads (its
  objects, their fold, its tips), appends and receives: a store on disk, a
  store in memory, or an overlay. `receive(seeds)` takes in what `seeds`
  rest on that it lacks, verifying every object as `adopt` does before it
  takes any, parents first. An append decides on what the replica holds by
  the rules above, so two replicas holding the same objects write the same
  bytes, wherever each is held.
- **`accept(from, to, chosen)`** is `to.receive(chosen)` with `from`'s
  prints: the join restricted to what `chosen` rest on. Where `chosen` is
  closed under parents over `to`, `to` reads as it did plus exactly
  `chosen` (law 39). `sync`, an overlay's flush and `adopt` are it, with
  everything, all of the overlay's own objects and one tip chosen.
- **`sync(from, to)`** is `to.receive(from.tips())`: `to ∪ from`, the join.
  It reads no register. It is idempotent and order-free, and a sync cut
  short leaves `to` closed under parents and is completed by the next
  (law 35).
- **An overlay** over a replica writes to memory and reads `own ∪ base`:
  the base's objects and fold, with its own objects folded over them. It
  writes nothing to the base until flushed, and its flush is `sync(overlay,
  base)`.
- **A frontier token** names a frontier, a set of tips, in 64
  characters: the sha256 of the tips' names in sorted order, each followed
  by a newline. A pull asks a store of record for the objects that a
  replica lacks. The store of record reads the pull's token, sends some
  objects, and records the tips of what the replica then holds under their
  token. The replica names that token in its next pull. The store of
  record keeps a bounded number of the most recently recorded frontiers of
  each store. It reads a token as the closure of the tips recorded under
  it when it holds every one of those tips. It reads any other token, one
  it never recorded, let go or forgot, or one that names a tip it no
  longer holds, as unknown, and answers that pull as one from a replica
  that holds nothing (law 47).
- **A decision is the store of record's.** A question no monotone reading
  answers ("is it still standing, so I may perform?") is asked of a store on
  disk under its lock, and the record of its answer appended and synced
  before the lock is let go and the effect performed. A store in memory and
  an overlay hold no lock another writer takes and do not write through, so
  no decision is asked of one; monotone writes need no lock, and every
  replica appends them.
- **A history held in a register** is held by its heads: an event field of
  names, printed as a tuple sorted and distinct, whose register's type says
  which schema's objects they name (design §6.3). THE OBJECT MODEL GAINS
  NOTHING: no constructor and no field of an envelope is new, the inner
  objects are objects of their own schema in a replica of it, and their
  names are global. A NEST of such histories flattens into one history
  keyed by path, each object at the path of the register that held its
  history and then its entity's key, under the name it had: no object is
  renamed and no dep rewritten. An object's parents are of its own level,
  so a held history's head, or a parent within it, that its level lacks is
  a missing object; causality crosses levels only where a write names the
  inner heads it saw.

## 4. Events

A store is opened at a **schema**: its event vocabulary, the entity each
event is about, the registers each event writes (§6), and which events a
policy is asked about (§5). A schema is the STORE'S type and no object
carries it, so `Genesis(label, nonce)` is frozen as it was; a stored print
is parsed against the envelopes' vocabulary (§3) and then the schema's, and
an event the schema does not name is refused at that boundary. Every event
says about what, when its writer thought it was (data, §1) and who.

The REFERENCE schema is the **todo schema**, below: every conformance vector
was taken at it, and every legacy envelope (`Sealed`, `Woven`) belongs to
it. Kinds and fields, in order; all shipped; `""` means absent.

- `Created(todo, at, actor, text, note)`
- `Completed(todo, at, actor, note)`, `Cancelled(…)`, `Reopened(…)`
- `Tended(todo, at, actor, note)` — care taken on a todo that is never done,
  a pass at a recurring chore. It changes no state, spec or content: §6.1
  folds it into the tendings, and §7.3's `Recur` reads them.
- `SpecRevised(todo, at, actor, spec, note)`
- **The record kind**: `KIND(todo, at, actor, <the host's fields>)`.

The six kinds above are the TODO SCHEMA's: their fields are its semantics,
and they are frozen here. Each is about the todo it names. A record is a
todo's CONTENT, and content is a deployment's — so a host **payload**
supplies

- `KIND`, the constructor name, and the record's fields and their declared
  order, which together are the stored bytes and are frozen the same way;
- the VOCABULARY of the constructors those fields nest, which is §2's
  whitelist's other half;
- the parse and the print of those fields, the parse refusing what a smart
  constructor refuses;
- and the two readings §6 takes from a record and the only two: the spec it
  carries (§6.2, §6.4) and its checklist length (§6.4).

Nothing else about a record is read anywhere in this document. The REFERENCE
PAYLOAD — the one `conformance/*.py` was taken with, and the one every
`Authored(...)` in those vectors round-trips under — is `Authored(todo, at,
actor, kind, created, body, spec, rationale, category, waiting_on, detail,
source, subtodos, notes, note)`: `body` and `detail` are `MarkupSource`,
`waiting_on` is `StringSource`, `rationale` a tuple of strings, `source` a
`Source(sender, subject, date, hash, thread_id, message_id)` or `None`,
`subtodos` a tuple of `SubTodo(body, done)`, `notes` a tuple of `Note(on,
lines)` with `on` a site literal. `MarkupSource` and `StringSource` are opaque
text in that host's two rendering languages; the database stores and prints
them and never interprets them.

## 5. Standing

The database does not decide whom to believe. It asks the HOST, about one
object carrying an event at a time, and the answer is a `Standing`:

- **`Binds`** — the folds take it. It writes its registers and it is what the
  store believes.
- **`Claims`** — the folds refuse it. It is stored and it is SHOWN, beside the
  answer that stands, and it changes no confirmed reading.

A **policy** is that function: `standing(object, event) : Standing`, of the
object (its name, and the event it carries) and of a READING of the history
it is in, the proof below, and nothing else — not of the log's order, not
of the position, not of the moment. A policy that reads nothing of the
history is the same at every one; a reader asks a policy that does at the
history it reads (`at(history)`), and a store asks its own at every
`verify`. Every fold in §6 takes one, §6.7 takes one, and §3's `verify`
takes one; nothing in the database reads an actor name to decide anything.
A policy is asked of every object carrying an event. The SCHEMA says which
of its events are about standing (§4): the reference policy binds any other
whoever wrote it, and a policy that requires signatures asks of every one.
The todo schema asks about every kind but the record.

A policy answers one further question, whose default is the reading above:
`confirms(event)`, "is this the host's own word", which is `standing(event) ==
Binds` unless the policy says otherwise. Two readers ask it and neither is a
fold — §6.7's `confidence` field, and §3's clock rule — and a policy that folds a
writer's events while still marking them as that writer's is why it is asked
separately. `Claims` implies not `confirms`.

**The reference policy** — the one `conformance/*.py`'s `untrusted` fields
name, and the one this repository's vectors were taken under — is a set of
actor names, over any schema. An event `Claims` when its actor is on the set
and the schema asks about it — under the todo schema, a lifecycle, `Tended`
or `SpecRevised` event; everything else `Binds`, a
content record included, because writing content is what such a writer is for
and a fold that hid its writes would be an outage that reports success. It
`confirms` an event exactly when the actor is not on the set — so a content
record from a named actor binds and is still shown as that actor's. The empty
set is the policy under which everything binds and nothing is a claim.

**Naming is structural, belief is not.** A change's `deps` (§3) are read
under the policy where everything binds, because a name records what its
writer held. A reading's registers still fold only what its policy binds
(§6): a claim neither joins a confirmed frontier nor removes a write from
one, whatever its deps, so an untrusted change cannot settle a conflict
between trusted writes in the confirmed reading. A LATER binding write that
descends from it does, since its writer held everything the claim names.
Each reading has its own candidates — the confirmed reading's are the
binding writes, and a register none of whose writes binds is unwritten
there; the claimed reading's are all of them — so a claim never moves a
confirmed price (law 11), and one more urgent than the confirmed answer is
SHOWN (§6.7). Twins differ in standing only where a policy reads the proof:
a signature covers an object's name, so it proves the twin it signs and not
another placement of the same event over other deps, which would supersede
what its actor never saw.

**Proof: who wrote an object, read from the history.** An object's `actor`
is a field its writer chose; the history can prove it. A `Signed` (§3)
PROVES the object it signs when its signature verifies, its key is
registered to that object's actor in that object's prodrome by a
`KeyAdded` that counts, and every `KeyRevoked` of that key for that actor
that counts has it among its ancestors. Which key objects count is the
reader's: EVERY one for `verify` (the structural reading, as deps are
read), and under a policy that requires signatures, those a signature by
one of its ROOT keys that verifies covers. The proof of a history is the
set of objects it proves; it is a function of the object set, never of
the order the objects arrived in (law 41).

So a revocation keeps exactly the signatures its writer stood behind, those
beneath its deps, and takes away every other signature by its key: one made
after it, beside it, or before it and unseen by its writer. Nothing but the
objects says which came first, and a revocation that named nothing takes
every signature away. A key revoked stays revoked, since adding it again
writes the same `KeyAdded`. A signature need not come after its key's
registration: a device signs offline and is registered when next seen. A
key registers or revokes no key; only a root does, because a key that could
would make what a revocation means depend on which of two revocations came
first.

**The policy that requires signatures**, over any policy `P`, with a set
of ROOT keys: an object by actor `A` binds only where the proof of its
history, key objects counted where a root signed them, proves it, and then
as `P` says; it is the host's own word where it is proven and `P` confirms
it. An object it does not prove CLAIMS, whatever its kind, a content record
included, and is stored and shown as every claim is. It is the meet of `P`
and "proven", so it composes with any policy, and under it the reading is
`P`'s with exactly the unproven objects' events claims, whatever order the
objects arrived in. A history whose actors have no keys reads under any
policy that does not require signatures exactly as it did, and adding
signatures and keys to a history changes no reading under such a policy
(law 41).

**Both readings stay in the database** (§6.7): the CONFIRMED one, under the
host's policy, and the CLAIMED one, under the policy where everything binds. A
store that kept only the filtered reading could not show a claim at all, and
showing one is the whole point of storing it.

## 6. Folds

**A schema's registers.** An entity's REGISTERS are a product, one
component per register the schema names, each a join-semilattice, so the
product is one and folding a stream into it is a monoid action (6). The
schema routes each event to the registers it writes. A register a write
SUPERSEDES in holds its FRONTIER, the writes to it no later write to it
descends from; one that only accumulates is a grow-only set, joined by
union. Each register has a TYPE: its values, a partial order on them, and
the value a write gives it. The order decides no succession; the register's
reading is the MAXIMAL values of its frontier's values, the free completion
of the order: one is a value, and more is a conflict. A type may declare its
register INFLATIONARY, every write `≥` the reading it supersedes; §3's
append refuses a write that is not, before any object exists, and the
reading of such a register is then a homomorphism (law 32). A schema MAY
also carry a PRICE: the §7 term each candidate of an entity's reading prices
as, the reading priced as `Least` over them, their meet (law 34); for the
todo, a candidate is a world (4). An entity whose schema has no price has
none at all, which is not `Absent`.

**A schema may be declared.** A schema is a value a store is opened at, and
no object carries it. Beside a schema written as code, one may arrive as
TEXT, in the §2 grammar under the schema language's own closed vocabulary:
`Schema(key, events, registers)`, each `Event(name, fields)` a constructor
whose `Field(name, type)`s are in printed order, a type one of `Text()`,
`Integer()`, `Instant()`, `Reference()` (a §3 name), `Enum(alternatives)`
and `List(item)`, and each `Register(name, order, inflationary, writes)`
ordered by one of `Discrete()`, `Total()` (over an integer),
`Machine(covers)` (the reflexive and transitive closure of its `(lower,
upper)` pairs over an enum's alternatives) and `Inclusion()` (a list read as
a set), written by `Write(event, field)`s. It is ADMITTED only when, in this
order: it parses; it is its own canonical print, every set (events and
registers by name, alternatives, covers, writes) sorted, so its name is the
hash of its text as an object's is; its names are unique, and no event's is
an envelope's or the grammar's own; every event has the key field, a text;
every event has `at`, an instant, and `actor`, a text; every register is
written, by existing fields of one type, the type its order reads; every
machine's covers are acyclic; and every inflationary register's order has a
bottom. A refusal names the first law that fails. An admitted schema's
events print as their constructor with every field in declared order, its
registers are read as every register is (the maximal values of the
frontier under the declared order), and an inflationary one is held to §3's
refusal. A declared schema carries no price.

Every fold is a projection of one entity's registers at a moment `t` under a
§5 POLICY. A write joins its register when it is dated at or before `t` and
the policy binds it — an event the schema does not ask about joins whoever
wrote it — and what it supersedes is ancestry alone: no clock and no
linearisation picks a winner. Every fold factors per genesis (§3), an entity
keyed by `(genesis, key)`; in a store of one genesis that key reads as the
entity's.

**The todo schema's registers** are its state, spec and content, which a
write supersedes in, and its tendings. The orders of the first three are
discrete, a value being the event that wrote it, so a reading is its
CANDIDATES, the distinct events of its frontier, exactly as before, and
twins (§3) are one candidate; the tendings are a grow-only set, ordered by
inclusion. None is inflationary: a `Reopened` after a `Completed` goes
back, which is why a todo's conflicts are shown and priced. Its valuation is
FPL, below. A content record joins whoever wrote it.

1. `env(t, policy)`, the environment, in two halves. `outcomes : TodoId →
   set of candidate bindings`: the candidates of the todo's state register,
   each `Completed(at)` or `Cancelled(at)`, with "open" a candidate where a
   `Reopened` is one; a todo whose one candidate is open is unbound.
   `tended : TodoId → set of instants`: the instant of every binding
   `Tended`, a grow-only set folded by union, so nothing removes a tending
   and no order among them matters. Only events the policy says `Binds`
   write either half.
2. `specs(t, policy)`: the candidate specs of each todo's spec register,
   written by a record's payload (whoever wrote it) or a `SpecRevised` the
   policy binds.
3. `content(t)`: the candidate records of each todo's content register,
   whoever wrote them — no policy parameter: content renders, and §6.7 marks
   the row instead.
4. `flatten(t, policy) : TodoId → Term`: one function per todo. Its head is
   `checklist(first spec ever, first checklist length ever)`, from the writes
   no other write to their register precedes; at every moment a spec, a
   checklist length or the bound state changed there is a `Piece(at, term)`:
   a flat 1.0 while resolved, else `checklist(spec in force, items in
   force)`; assembled by `mk_piecewise`. "Spec" and "items" are the
   payload's two readings (§4) and the whole of what a record contributes. A
   `Tended` puts no piece: it is care and not a transition, and the terms
   that read tendings read them from the environment (§7.3), so no stored
   function changes meaning. The head extends to −∞: a todo's function is
   total over time, and before anything was recorded about a half its unit
   is the earliest recorded demand of that half. A completion recorded
   before its record therefore stands against the demand the record says it
   had. Consequence: the prefix law (§9.2) is exact for every moment after a
   todo's first spec and first checklist are recorded; a late first half
   re-heads the curve before it. Absent when there was never a spec or a
   checklist. `checklist(own, n)` is `own` when `n == 0`; `Conj(n ×
   Flat(0.5))` when `own` is `None`; else `OffsetBy(own, Conj(…))`.

   **A conflict prices as its most urgent world.** A WORLD of todo `x` picks
   one candidate from each of `x`'s written state, spec and content
   registers; its log is the log less the other candidates of those
   registers. The term in each piece is `Least` (§7) over the worlds in force
   at that piece's instant, each world's term as above, so one world is the
   term above byte for byte. Low is urgent, so a conflict rises and never
   sinks, and nothing picks a winner. Taking the most urgent spec and the
   most urgent state separately could build a combination no writer wrote; a
   world is always a reading some writer's view contains. The worlds are the
   product of the three candidate counts, and a conflict is shown and
   settled by the next write, so they are few.
5. `history(policy)`: the environment as a function of time. `history.at(t)`
   is the registers folded at `t`, which change only at the instants writes
   are dated; the tendings, a grow-only set, are their own history. No
   causal order enters it. It is a SCHEDULE: a value before any write, and
   from each instant a write is dated the registers folded there, its value
   at `t` that of the last such instant at or before `t`. Every fold at a
   moment above is this schedule, under its policy, read at `t`, and
   `flatten`'s pieces are it priced.
6. **Registers** over nodes `(name, parents, event, genesis)`, under any
   schema; for the todo's, one register
   per `(genesis, kind ∈ {state, spec, content}, todo)` holds its frontier,
   the writes to it that no later write to it descends from. A `Tended`
   writes no register: it joins the tendings, a grow-only set kept beside the
   frontiers, so concurrent tendings are their union and never a conflict.
   `extend(state, nodes)` is a monoid action. SUPERSESSION IS PER REGISTER:
   a write supersedes exactly the writes TO ITS OWN REGISTERS that it
   descends from, through any path; descending from a write in another
   register supersedes nothing there, and a conflict is two or more writes
   to one register none of which descends from another (law 37). A write
   that descends from a conflict settles it, and nothing else does: a merge
   settles nothing.
   Ancestry is indexed within each `(genesis, todo)` for changes, whose deps
   never leave a todo, and over every legacy object for legacy objects, so
   only a mixed store pays for both. `conflicts_of` names each register whose
   frontier holds more than one write.
7. **The entry**, under a schema with a valuation; here the todo's.
   `entries(nodes, t, policy) : [Entry]`, one row per todo
   any event mentions, per genesis and by id. It is the composition of the
   folds and §7, stated once so every consumer performs it once. With
   `confirmed` the registers under the policy: `outcome` is the todo's
   candidate bindings, one member where nothing conflicts; `claim` is the
   candidate bindings under everything-binds, where their kinds differ from
   `outcome`'s, absent where they agree; `spec = flatten(…)[todo]`, `Absent`
   (§7) where the todo has none, and `value` the fulfillment at `t` of
   `link(spec, specs)` (§7.2) under `confirmed`'s environment, where `specs`
   is `flatten(…)` over the todo's prodrome with `Absent` for every other
   todo it mentions — so `value` is a number, `absent` where it reads `∅`,
   or, where `spec` does not link, the `LinkError`, and under a conflict it
   is the least over worlds; `content` is the names of the candidate content
   records, sorted, and never the records — a consumer that wants the
   payload looks the object up, because what a record MEANS is the host's;
   `conflicts = conflicts_of(confirmed)[todo]`; `stream` is every node whose
   event names the todo, in causal order — the one reading the linearisation
   orders, and since deps never leave a todo, Kahn's algorithm over the
   todo's own objects. CAUSALITY FIRST: a change rests on every write to
   its entity its writer held (§3), so a writer's own writes to one entity
   stream in the order it wrote them, whatever registers they touch and
   whatever instants they carry (law 36), and a write that reached a writer
   before it wrote streams before its write. The name breaks a tie only
   between writes that truly were concurrent; no instant ever does, so a
   host never stamps instants apart to order its own writes. A todo whose
   events are all dated after `t` is still a row, open and `absent`: `t` asks what is believed, not what exists. A
   LIST of entries is in one order, defined here and by no host: ascending
   in value, ties by `(genesis, id)`, then every row with no number, `absent`
   or not linking, by `(genesis, id)`.
   `confidence` says whether the
   answer is the confirmed reading whole: a claim refused, a candidate
   content record the policy does not `confirm`, or both. Checked against `conformance/view/*.py` — SEEDED, not
   taken from the reference like the rest of `conformance/`: random logs
   drawn from this crate's own generator (`core/tests/common/mod.rs`'s
   `a_log`, the one `core/tests/all/fold_laws.rs`'s properties draw from too)
   under a fixed seed, folded at several instants under two reference
   policies. Their `untrusted` fields are read through the reference policy;
   the stored bytes and every expected answer are unchanged.
   `core/examples/generate_view_vectors.rs` regenerates them by hand; nothing
   under `cargo test` or CI ever does.

## 7. FPL

`Term` is the fixed point of `TermF a`:

```
Flat(value) | Decay(start, end, end_date, lead_up, start_date?) | Curve(points)
| Conj(terms, p) | Offset(delta, a) | Gate(gate, body) | Shift(delta, a)
| Within(window, p, a) | Importance(w, a) | After(event, anchor, term, pending, needs?)
| Recur(todo, anchor, term, pending) | Periodic(period, anchor, term)
| Piecewise(head, pieces) | OffsetBy(delta, term) | Least(terms) | Ref(todo)
| RefIn(store, entity) | Absent
```

Semantics `⟦t⟧(now, env) ∈ [0, 1] ∪ {∅}`. `∅` is NO VALUE: a note, a
reference, a proposal nobody has priced has no claim on attention, and `∅` is
not a number — an implementation's value type keeps it apart (the reference's
is `Option<f64>`). It is the identity of composition: it neither raises nor
lowers anything it is composed with.

- `Absent`: `∅` at every instant. It has no field; it prints `Absent()`.

- `Flat`: `value`. `Decay`: 1.0 before `start_date`; 0.98 before `end_date −
  lead_up`; linear from `start` to `end` across the window; `end` after.
  `Curve`: linear between points, clamped outside.
- `Conj`: the power mean, with exponent `p`, of the values of the members
  that have one, each clamped to at least 0.001; `p = 0` is the geometric
  mean; `∅` when no member has a value; empty is 0.5 (`Conj([])` has no
  member to be absent, and keeps its stored meaning).
  `Offset`: `x(1 − |δ|) + max(0, δ)`. `Gate`: `max(1 − ⟦gate⟧, ⟦body⟧)`; an
  absent gate is no gate, `⟦body⟧`, and an absent body is `∅`.
  `Shift`: `⟦a⟧(now + δ)`. `Within`: the power mean of those of the 65
  samples over `[now, now + window]` that have a value, `∅` when none does.
  `Importance`: `⟦a⟧^w`. `Offset`, `Shift` and `Importance` take `∅` to `∅`.
- `Least`: the minimum of the members that have a value, `∅` when none does,
  so `Absent` is its identity, as for `Conj`. It has at least one member,
  and `Least([t])` is `t`. It is commutative, associative and idempotent: a
  semilattice. It is how a conflict prices (§6.4), since min cannot be
  written with the other constructors — `Conj`'s `p → −∞` limit is min, but
  `inf` is not storable, and `Gate` is a max.
- `After`: unbound, `⟦pending⟧(now)`; `Completed(done)`, `⟦term⟧(now − (done
  − anchor))`; `Cancelled`, 1.0, whatever `term` reads. A binding counts only
  when `done <= now`. `pending` may be `Absent`, as `Recur`'s may. Where the
  environment holds several candidate bindings of `event` (§6.1), `After`
  reads the minimum over them of the reading under each.
- `Recur`: with `s = last_tended(env, todo, now)`, the latest tending of
  `todo` at or before `now`: none, `⟦pending⟧(now)`; else `⟦term⟧(now − (s −
  anchor))` — `After`'s slide, from the last pass (§7.3).
- `Periodic`: `⟦term⟧(anchor + ((now − anchor) mod period))`, the remainder
  Euclidean (non-negative), in microseconds.
- `Piecewise`: the piece in force, the last with `at <= now`, else the head
  — which may be `Absent`: priced from a date, or unpriced after one.
  `OffsetBy`: `offset(⟦term⟧, ⟦delta⟧)`; an absent delta is no offset,
  `⟦term⟧`, and an absent term is `∅`. So `checklist(own, n)` with an
  absent `own` reads as the checklist alone.
- `Ref(todo)` and `RefIn(store, entity)`: no reading of their own. Each is
  a variable, bound by `link` (§7.2); the semantics above are defined on
  CLOSED terms only.
- **Normal form** (`mk_piecewise`): no pieces means the head; nested pieces
  are spliced; no adjacent equal pieces; instants strictly increasing.
  `Absent` is a leaf and `normalize` leaves it where it is. `normalize`
  pushes `Conj`, `Least`, `Offset`, `Gate`, `Importance` and `OffsetBy`
  under `Piecewise` over the merged partition, translates instants under
  `Shift`, and leaves `Within`, `After`, `Recur` and `Periodic` in place: a
  window, a lookup and a fold of time onto one cycle do not commute with a
  partition.
- **Explain** is a decoration, `Cofree TermF (value, notes)`: the term's shape
  with each node's fulfillment at the moment its parent used it — `∅` where
  the node has none — plus notes (`Conj`: certifies, and shares over the
  members that have a value, an absent member's 0, both omitted where the
  conjunction is `∅`; `Within`: peak instant and share over the samples that
  have a value, omitted where none does, the subterm then explained at `now`;
  `After`:
  bound; `Recur`: bound — `pending` or `tended` — and, when tended, the last
  tending and the hours since it; `Periodic`: the start of the cycle in
  force; `Piecewise`: since, pieces). A serialization of one is the term's own
  shape carrying those two at every node.
- **Breakpoints**: the slope changes and jumps of the exact fragment (`Flat`,
  `Decay`, `Curve`, `Absent`, `Piecewise` of exact parts, `Offset`, `Shift`,
  `Least` of exact parts, constant composites). `Absent` is exact and has
  none; a `Least`'s include the instants where two of its members cross. A series with a knot
  at each and a second knot before each jump is the curve; a knot is `∅`
  where the term has no value, and no line is drawn to or from one. Other
  terms are sampled and say so.
- **Next change** (`observe::next_change(term, now, env, observation)`): a
  view shows a value at a precision, so what it draws is a function of the
  OBSERVED value. An `Observation` is a quantisation: `[0, 1]` in `n ≥ 1`
  equal steps, a value read as the step it rounds to (`down`, `nearest`
  with a half read upward, or `up`), and `∅` read as itself; it is
  monotone, so each step observes an interval and `observation ∘ ⟦t⟧` is a
  step function of time. The answer is `(at, exact)`: the first instant
  after `now`, on the microsecond grid, at which the observed value
  differs from its value at `now`, or none for never. EXACT on the exact
  fragment: affine between its breakpoints, so the step inside each
  stretch is found by bisection against the evaluator itself, and constant
  past the last. CONSERVATIVE elsewhere: the first instant at which an
  enclosure of the term over `[now, t]` (each atom's range, monotone
  composites on their children's bounds, a `Within` over its widened span,
  `After` and `Recur` split where a binding or tending comes into force,
  `Periodic` folded onto one cycle) stops being one observed value. Never
  later than the true change, possibly earlier; an enclosure over all time
  that is one value proves never, which is exact. `exact` is a fact about
  the behaviour, not the term's syntax. On the exact fragment the observed
  value from `now` on is itself a schedule, a knot at every step, and the
  answer is its first knot after `now` whose value differs. The environment is a snapshot, read as
  evaluation reads it: a binding or tending dated after `now` comes into
  force at its instant.
- A term has ONE serialization and it is §2's literal print. A JavaScript
  boundary may carry a JSON shape of the same term — the reference one is
  `prodrome-wasm-exports`'s `json` module, lowercase kind tags and spans in
  hours — but that is a boundary's business and not the database's, exactly
  as a rendering is (§1).

### 7.1 The chain compiler

`After` and `Recur` (§7.3) are the terms that read history. Every other
constructor is a function of `now` and its subterms, so a term with neither
anywhere in it can be evaluated against no environment at all. The compiler
is that erasure: given a term and the environment as of one instant, it
answers a term with the same reading and no `After` or `Recur` left.

```
compile(term, env) : Compiled
compile_chain(functions, env) : (todo → Compiled) | ChainError::Cycle(path)
```

`Compiled` is the term the compiler produced together with the LINKS it
resolved. Its readers — `term`, `fulfillment(now)`, `explain(now)`, `notes` —
take no environment, and that is the whole claim: a compiled term cannot
consult one, because the constructors that would have been compiled away.

**Per link.** With `After(e, anchor, term, pending, needs)` and what the
snapshot says about `e`:

- UNBOUND — `compile(pending)`. `bound` answers `None` at every instant, so
  the link is its pending branch and nothing else.
- `Completed(τ)` — `Piecewise(compile(pending), [(τ, Shift(anchor − τ,
  compile(term)))])`. Before τ the link is unbound; from τ the body slides by
  the slippage, and sliding `now` by a constant IS a `Shift`.
- `Cancelled(τ)` — `Piecewise(compile(pending), [(τ, Offset(1.0,
  compile(term)))])`. `x·(1 − |1|) + max(0, 1) = 1` for every `x`: the moot
  constant is a graded offset at δ = 1, and writing it as one keeps the mooted
  demand in the tree where a reader can still see what was dropped.
- A CONFLICT, several candidate bindings of `e` (§6.1) — `Least` of the link
  compiled under each candidate, so the compiled term reads what §7's
  `After` reads and law 13 holds as stated.

So each link contributes ONE graded offset δ ∈ [0, 1] — its MOOT GRADE, 1
where the upstream was cancelled and 0 everywhere else — applied with §7's
corrected form; δ = 0 is elided because `offset(x, 0) = x`, and so is a zero
`Shift`. An offset of `∅` is `∅`, and a cancelled upstream is moot whatever
the body reads, so a body holding an `Absent` is written `Gate(compile(term),
Flat(1.0))` instead: `max(1 − x, 1) = 1`, an absent gate is no gate, and the
dropped demand stays in the tree. The result is `normalize`d, so a chain (A
needs B needs C) is ONE
schedule at the root over the merged partition of the links' instants, and not
three freeze quantifiers the evaluator re-enters at every sample.

**Per recurrence.** `Recur(todo, anchor, term, pending)` with the snapshot's
tendings `s₁ < … < sₙ` of `todo` compiles to `Piecewise(compile(pending),
[(sᵢ, Shift(anchor − sᵢ, compile(term)))])`: `Completed(τ)`'s case once per
pass, exact because `last_tended` picks the latest tending at or before `now`
and a piece at `sᵢ` is in force from `sᵢ` until the next. A recurrence is not
a link: it contributes no note and no graph edge.

**`needs` is the COMPILER's field, not the evaluator's.** §7's semantics have
never read it and this does not change that: consuming it as a VALUE would
move a reading, and the law below forbids that. It is the link's declared LEAD
TIME, and the compiler is the first reader holding it beside the upstream's
actual instant — so a resolved link reports `ready = τ + needs`, the earliest
moment the link's own demand could be met, in its note. A host schedules by
it. The evaluator still does not read it.

**Cancellation is reported, never silent.** Every link becomes a note on the
compiled term — `pending`; `completed` with its slippage and its `ready`;
`moot` with its instant — and `explain` puts the moot ones at the root under
`moot`. §7's "a Cancelled upstream prices as moot and MUST be surfaced by
reporting" is a requirement on the REPORTING, and compiling is where it is
cheapest to honour: a 1.0 from a cancellation is indistinguishable from a 1.0
from a demand met, and the note is the only thing that tells them apart.

**Cycles are a value.** A single term is a tree and cannot cycle.
`compile_chain` is handed a MAP of todo to function whose links draw a graph
over those todos, and a cycle in that graph is a modelling error the compiler
is the first reader to see whole. It is refused as `ChainError::Cycle(path)`,
carrying the path so the host can name the loop, and never as a partial answer.

**This is an OPTIMISATION and not a semantics change.** Its law is §9.13,
`fulfillment(compile(t, env).term, now, ∅) == fulfillment(t, now, env)`. No
stored byte moves, no vector moves, and a host that never compiles reads
exactly what it read before.

**The cost.** The interpreter consults the environment once per `After` node
per evaluation, and `Within` is 65 evaluations of its subterm — so one link
under a window costs 65 lookups, and a chain of `d` links under one costs
65·d, at every query. The compiler pays one walk of the term (the resolution,
then `normalize`'s merge of the partition) ONCE, and every evaluation
afterwards pays none, because there is no `After` left to pay for.

WHAT THAT IS AND IS NOT WORTH, measured rather than assumed: on a chain of
three links under one window, 2000 evaluations, the lookups go from 390 000 to
0 for a 19 µs compile — and the WALL CLOCK moves about 7% in a release build,
because a lookup in a three-entry map is cheap. The saving being claimed is not
the microseconds. It is that the environment leaves the query path entirely: a
compiled term is a term (§7), so it can be cached, stored, shipped over a wire
and evaluated somewhere that holds no history at all, and the chain it came
from is one schedule a reader can see rather than a nest to re-enter.

### 7.2 References and linking

`Ref(todo)` is "the fulfillment of todo `todo`": a subtodo's parent, a group,
any todo whose demand is composed from other todos', or an unpriced object
that serves one. `todo` obeys `TodoId`'s rule and the constructor refuses
anything else. It is a LEAF of `TermF` and a
VARIABLE: a term holding one is OPEN, and evaluation — `fulfillment`,
`explain`, series knots, the compiler — is defined on CLOSED terms only, those
with no `Ref` anywhere. `Closed` is that type, so evaluating an open term is
a type error and not a wrong number.

```
link(term, specs : todo → Term) : Closed | LinkError
```

`link` substitutes every `Ref(x)` with `specs[x]`, linked in turn: bind for
the free monad over `TermF`, with todo ids as its variables. A rebuilt
`Piecewise` goes back through `mk_piecewise`, so a spec that is a schedule,
landing in a piece, is spliced like any nested schedule. A closed term is its
own link, untouched. `specs` is each todo's OWN function; for a store it is
`flatten`'s (§6.4) over every todo of the term's prodrome — an unqualified
`Ref(todo)` names that todo in its own genesis (§3) — and `Absent` for a known
todo with none — a record whose `spec` is `None`, or a todo only ever
created — so `Ref(x)` means x's whole function, its revisions and its
lifecycle, a completed child reads 1.0 from its completion, and a reference
to an unpriced todo links and reads `∅`, which a conjunction passes over.

**Refusals are values.** `LinkError::Unknown(x)` where `specs` holds no `x`:
for a store, an id its prodrome has never seen.
`LinkError::Cycle(path)` where the references loop: the substitution keeps the
path of references it is expanding, and meeting one already on it is the
cycle, reported with that path, its first todo repeated at the end. So `link`
is total and never loops. A cycle is refused where it is REACHED: specs that
loop elsewhere do not stop a term that never names them from linking.

**A qualified reference.** `RefIn(store, entity)` is "the fulfillment of
`entity` in the prodrome `store` names": a term in one store priced by an
entity in another, such as a proposal priced by the todo it serves. `store`
is a genesis's name or a name the host declares for one, and obeys
`TodoId`'s rule, which a genesis's 64 lowercase hex does; `entity` is a key
as a nest's path holds it, not empty and holding no `/`. It is the same
variable as `Ref` with a qualifier: `Ref(todo)` is `todo` at home.

```
link_in(term, specs, stores) : Closed | LinkError
stores = { genesis → (specs, env) }, names : name → genesis
```

`stores` is a SET OF PRODROMES, the host's environment, passed in (a
schema never reads another store): each member by its genesis, its
entities' own functions and the environment its terms read. `link` is
`link_in` with no stores. A `RefIn(s, x)` binds to `x`'s function in the
member `s` names (a declared name's genesis, else `s` read as a genesis),
linked THERE, so an unqualified `Ref` inside it names that member's own
entity, and compiled against that member's environment (§7.1), so no
`After` or `Recur` of it reads the history of the term it lands in. By law
13 it reads, at every instant and under any environment of the term it
lands in, what `x` reads in its own prodrome. A store the set does not
hold, or an entity its member does not, links to `Absent` and reads `∅`:
what a store does not know of another prices no claim, and once a sync
brings it, the reference reads it. Every unqualified `Ref` means exactly
what it meant, and links alike under any `stores`. A cycle is refused
across members as within one; a variable of another member is named
`genesis/entity` in a refusal.

No stored byte moves: `Ref` is a new constructor (§1), and a store without one
reads exactly what it read before. Nor does `RefIn`, a new constructor
beside it: a term without one prints, links and reads as before. Nor does `Absent`, a new constructor too: a
record whose `spec` is `None` reads as `Absent` at the todo, so an old store
reads as before except that a `Ref` to an unpriced todo, refused before `∅`
existed, now links.

### 7.3 Recurrence

A recurring todo is never done: vinegar the toothbrush every two months, pay
the rent on the first. Recording a pass as `Completed` would close it, so a
pass is a `Tended` (§4), which leaves the todo's state alone and joins the
environment's TENDINGS — per todo, a grow-only set of instants (§6.1). Two
replicas' tendings merge by union and never conflict (§6.6).

```
last_tended(env, todo, now) : Instant | None
```

is the latest tending of `todo` at or before `now`, under `bound`'s guard: a
pass recorded later never rewrites an earlier moment. Two terms read time
cyclically:

- `Recur(todo, anchor, term, pending)` repeats from the LAST PASS. `term` is
  authored against `anchor` and re-anchored to the last tending exactly as
  `After` re-anchors to a completion: a pass at `s` reads `term` at `now − (s
  − anchor)`, and no pass yet reads `pending`. `todo` obeys `TodoId`'s rule.
  The toothbrush is

  ```
  Recur(todo='toothbrushvinegar', anchor=datetime(2026, 8, 12, 0, 0, 0),
        term=Decay(start=0.98, end=0.3, end_date=datetime(2026, 10, 11, 0, 0, 0),
                   lead_up=timedelta(days=60), start_date=None),
        pending=Flat(value=0.3))
  ```

  — a decay from 0.98 on the day of a pass to 0.3 sixty days on, restarted by
  every pass, and 0.3 while it was never done.
- `Periodic(period, anchor, term)` repeats ON THE CALENDAR, whatever is done:
  `term` on `[anchor, anchor + period)`, and every instant, before `anchor`
  too, folded onto that cycle by a Euclidean remainder. `period` is positive.
  Rent and quarterly taxes are this.

Neither stores a curve and a `Tended` creates no piece, so `flatten` (§6.4)
gains no case and no stored object changes meaning: this is evolution by
adding kinds (§1). Both are temporal, so `normalize` leaves them in place;
`Recur` reads history, so the compiler resolves it (§7.1); neither is in the
exact fragment of the breakpoints, so a series over one is sampled.

A caution on reading the future, which `After` shares: a pass BEFORE the
anchor reads `term` later than `now`, so a `term` that itself reads history —
a nested `Recur` or `After` — reads it there. `Recur`'s own lookup never
looks past `now`; its body is a function of time like any other.

No stored byte moves: `Tended`, `Recur` and `Periodic` are new constructors
(§1), and a store without them reads exactly what it read before.

## 8. Types an implementation must have

`Hash` (64 hex), `TodoId`, `Actor`, `Name`; `Schema`, §4's store type as a
parameter — an event vocabulary with its parse and print, the key, instant
and actor of each event, the registers it writes, which events a policy is
asked about, and an entity's registers as a product — carried by value and
never as an existential, since one store holds one schema, its vocabulary
a value the store is opened at (a unit for a schema written as code, its
declaration for a declared one, §6), and beside it,
each only where the schema has it, `Price`, the term each candidate of a
reading prices as (§6), `History: Price`, the terms of each moment of a
price that changes with its history (§6.4), `Bind`, what a `Ref` reads of an
entity in its store (§6.1, §7.2), and `Row`, what a row shows and when it is
provisional (§6.7); a register TYPE, its
values, their `Order` and the value a write gives it; `Payload`, the todo
schema's record kind as a parameter — a constructor name, a field order, a
vocabulary, a parse, a print, `spec` and a checklist length;
`Standing = Binds | Claims` and
`Policy`, §5's standing as a parameter — one function of an event, carried by
value like the schema and for the same reason, since one store reads under one
policy; `Envelope = Genesis | Change | Snapshot | Sealed | Woven` over a
schema's events, each object's genesis beside it (§3);
`TodoEvent`, the todo schema's events; `Term` as `Fix TermF`, `Absent` among its leaves; a value as
`[0, 1] ∪ {∅}`, `∅` never a number (`Option`); `Closed` (§7.2), a term with
no `Ref`, which is what every evaluator takes, and `LinkError = Unknown |
Cycle`; `Stores`, a set of prodromes as qualified references read it, each
member's functions and environment by its genesis, and the names a host
declares (§7.2);
`Env`, the environment, each todo's candidate outcomes beside the grow-only
tendings (§6.1, §7.3);
`Explanation = Cofree TermF Annotation`; `Compiled` (§7.1), a term with every
`After` and `Recur` resolved beside the `Link`s it resolved, whose readers
take no environment because a compiled term cannot consult one, and
`ChainError`, whose
`Cycle` carries the path; `Frontier`, an ordered set of writes, empty for
an unwritten register, and its candidates; `Order`, a partial order on a
register's values (`Discrete` by default, `Total`, inclusion on a set), and
`Inflationary`, a register type's declaration that a write only grows its
reading; `Node`, an object at the place its DAG gives it, which only a
DAG makes, since an object's prodrome is a function of the set it is in;
`Folded`; `Replica`, what every store is (§3), and `sync`, generic over
two of them; `Decision`, the store of record locked, the only value a
coordinated decision is asked of and one only a store on disk makes;
`Breaks`; `Entry`
(§6.7) over a schema with a valuation, keyed by genesis and key, whose
reading is the schema's (a todo's outcome is a candidate set), whose
function is always
there (`Absent` where the todo has none), whose value is a number, `∅` or a
`LinkError`, and whose `Confidence` is a sum with no "provisional for no
reason" inhabitant. Smart constructors validate; records are data; engine
code never calls a raw constructor.

## 9. Laws

Against `conformance/*.py` and on generated inputs:

1. `print ∘ parse` is the identity on every stored object, and `hash(print)`
   is its name. Records included: every `KIND(...)` print in the vectors
   round-trips byte for byte under the payload they were taken with.
2. **Prefix.** Appending a later event changes no earlier moment's reading,
   except the head's unit (§6.4): a todo's first spec or first checklist,
   recorded late, re-heads its curve before it. With both halves recorded,
   exact.
3. Independent events commute; a uniform shift of every `at` changes no
   reading.
4. `history.at(t) == env(t)`: by definition, since both are the registers
   folded at `t` (§6.5).
5. `mk_piecewise` is a normal form: unit, join, idempotent, no adjacent
   repeats. `normalize` preserves every reading, leaves nothing buried, and is
   idempotent.
6. The folds are the register projections (§6), so there are not two
   implementations to compare; conflicts are exactly the registers both
   branches wrote; a merge settles nothing; a descending write settles.
7. Interpolation between series knots equals evaluation on the exact
   fragment: a line between two numbers, `∅` between two `∅`s, and a pair
   with one absent end only ever the second before a jump.
8. Fulfillment within 1e-9 of the reference on every vector; hashes, prints,
   linearisations, frontiers and `verify` findings exactly.
9. **The view is the composition** (§6.7): on any DAG and at any moment,
   every field of every entry equals the fold it is named by, and the rows
   are exactly the todos the events mention. Against `conformance/view/*.py`
   (seeded, §6.7) on linear chains and against `core/tests/all/fold_laws.rs`'s
   property on random DAGs.

Laws 10–12 QUANTIFY OVER THE POLICY (§5). They are what makes "the host
decides" honest: the database is a two-reading fold with a parameter, and these
say the parameter cannot do anything but select.

10. **The two readings are one under the policy that binds everything.** With
    every event binding, the claimed reading and the confirmed reading are
    equal at every moment: every fold answers the same, no entry carries a
    claim, and no entry is provisional. The empty reference roster is that
    policy and answers alike.
11. **A claiming event never moves the confirmed reading.** Append an event the
    policy only lets `Claim`, at any instant and about any todo, and every
    confirmed answer at every moment is the answer it was — the environment,
    the specs, the content, the functions, the whole history. It is stored and
    it is shown; it is not folded.
12. **Standing selects events, not positions.** Because `standing` is a
    function of the object and of a reading of the object set, never of a
    position, folding under a policy equals folding the sub-log of the
    events it binds under the policy that binds everything — and that stays
    true under any permutation of the log, since filtering commutes with
    reordering. Which events count is a function of the object SET (law 41
    for a policy that reads the proof); law 25 is the other half, that no
    linear extension of the DAG reads differently.

Law 13 quantifies over the ENVIRONMENT, and it is what makes §7.1's compiler an
optimisation rather than a second evaluator.

13. **Compilation preserves every reading** (§7.1). For every term and every
    environment, `fulfillment(compile(t, env).term, now, ∅)` equals
    `fulfillment(t, now, env)` to 1e-9 at every instant — the compiled side
    read against the EMPTY environment, which is the statement's teeth: it
    answers the same while unable to look anything up, because `After` and
    `Recur` are the only constructors that read the environment and compiling
    leaves neither. On `core/tests/all/fpl_laws.rs`'s own generators, the
    `a_term()` and `an_env()` §9.5 is checked over, tendings included; and
    pinned by every §9.8 vector, which the compiler does not touch, and every
    `conformance/recur.py` sample, which it reads the same.

Law 14 quantifies over the SPECS, and it is what makes §7.2's `link` a
substitution and nothing more.

14. **Linking is bind** (§7.2). A closed term links to itself against any
    specs. `fulfillment(link(Ref(x), specs), now)` equals
    `fulfillment(link(specs[x], specs), now)` at every instant. Linking
    against specs that still hold references reads the same as linking the
    specs first and the term against the closed ones. The linked term is in
    `mk_piecewise`'s normal form. A loop is refused with a path that closes
    on itself, every step of it a reference its spec holds, and is never
    evaluated. `Ref` round-trips through print and parse like every other
    constructor (law 1). On `core/tests/all/fpl_laws.rs`'s generators, over
    acyclic specs; and against `conformance/link.py`, written BY HAND from
    these laws — exact linked prints, readings to 1e-9, an unknown todo and a
    cycle refused. Law 18 says what a reference to an unpriced todo links
    to.

Laws 15 and 16 are §7.3's: care is a set, and recurrence reads it as of now.

15. **A tending is care, not state** (§4, §6). A `Tended` changes no
    outcome, spec, content record, function or register at any moment, and
    every entry keeps its state, its function and its content — an open todo
    stays open. The tendings are a grow-only set: `history.at(t)` holds the
    ones dated at or before `t` (law 4), a merge of two histories folds to
    the union of their tendings, and a tending writes no register, so never
    a conflict.
    `last_tended(env, x, now)` is the latest tending at or before `now`, and
    a tending dated later changes nothing it answers. On
    `core/tests/all/fold_laws.rs`'s generators, which draw `Tended` among the
    other kinds, and `core/tests/all/fpl_laws.rs`'s.
16. **Recurrence reads time as §7.3 says.** With the last tending of `x` at
    `s`, `Recur(x, anchor, term, pending)` at `s + d` equals `term` at
    `anchor + d`; with no tending at or before `now` it equals `pending` at
    `now`; and a tending of `x` dated after `now` changes nothing its own
    lookup reads at `now`. `Periodic(period, anchor, term)` at `now + period`
    equals itself at `now`, and on `[anchor, anchor + period)` equals `term`.
    Both round-trip through print and parse (law 1), and the smart
    constructors refuse a `Recur` whose `todo` is not a `TodoId` and a
    `Periodic` whose `period` is not positive. On `core/tests/all/fpl_laws.rs`'s
    generators; and against `conformance/recur.py`, written BY HAND from
    these laws — `Tended` prints folded under the reference policy, a claimed
    pass among them, and exact term prints with readings to 1e-9.

Law 17 is §3's: the tips are the objects, so a union of stores is a union of
files.

17. **The tips are derived.** (a) The tips of a union of two stores are the
    tips of either side that the union does not name as a parent, and equal
    the tips of the union's object set; two stores' files copied into one
    directory derive exactly them. (b) Two writers who each append to a copy
    of one store, and whose object directories are then united (a `git
    merge`), read, fold and view exactly as the Prodrome's own replica merge
    of the two stores (`adopt` of the other's tips). (c)
    Derivation does not depend on the order objects are listed or files were
    written. And NO STORED BYTE MOVES: for every `conformance/dag.py` store,
    laid out with the `HEAD` and `refs/` its writer left, the derived tips are
    exactly the heads those files named. On `core/tests/all/tips.rs`'s random
    DAGs, pure and on disk, which also check that the tips cover the store and
    none rests on another; `core/tests/all/fold_laws.rs`'s two-writer property;
    and `core/tests/all/dag.rs`.

Law 18 is §7's empty term: `∅` is an identity, and nothing mistakes it for a
number.

18. **`Absent` is the identity of composition** (§7). (a) `Conj(ts ++
    [Absent], p) = Conj(ts, p)` whenever some member of `ts` has a value, bit
    for bit, and `∅` with it when none does; `Conj([])` is 0.5 and
    `Conj([Absent])` is `∅`. An absent gate or offset is none; an absent
    body or term, and `Offset`, `Shift`, `Importance`, `Within` or
    `Periodic` of `Absent`, is `∅`. (b) A term with no `Absent` evaluates
    exactly as it did before `Absent` existed: a value in [0, 1] at every
    instant, and every vector of laws 8, 13, 14 and 16 unmoved. (c) Against
    a store, `link(Ref(x))` is `Absent` where `x` is known and has no
    function, `x`'s function linked where it has one, and `Unknown(x)` where
    no event names `x`; every entry's value is its function so linked and
    read.
    (d) `explain` and the series carry `∅` where a node has no value — the
    root reads what `fulfillment` reads, every `Absent` node is `∅`, an
    absent member's share is 0 — and `Absent` is exact, with no breakpoints,
    so law 7 holds over it. Laws 5, 13 and 14 are checked over generators
    that draw `Absent` among their leaves too. On `core/tests/all/fpl_laws.rs`'s
    and
    `core/tests/all/fold_laws.rs`'s generators; and against
    `conformance/absent/*.py` — `fpl.py`, `series.py` and `view.py` in the
    shapes of `conformance/fpl.py`, `conformance/series.py` and
    `conformance/view/*.py` — SEEDED like the view vectors (§6.7): drawn from
    those same generators under a fixed seed, and regenerated only by hand,
    by `core/examples/generate_absent_vectors.rs`. The frozen view vectors
    (law 9) changed exactly where this law says they must: their unpriced
    rows now read `value='absent'` and `spec='Absent()'`.

Laws 19–29 are §3's change identity and §6's candidates. Each names the
property test under `core/tests/all/` that checks it, on the generators the
laws above draw from, extended to diverging writers, replays, conflicts and
several geneses. `conformance/change.py`, written BY HAND from laws 19–21,
23, 24 and 28 like `link.py` (its written objects derived again under law
36's deps, every read unchanged), pins exact prints and names, a replay that
writes nothing, a twin pair, a conflict priced as its most urgent world, two
geneses united, a snapshot chain and a mixed store over a
`conformance/dag.py` one (`change_vectors.rs`).

19. **A name is its genesis, event and view.** `append(event)` writes a
    function of its genesis, `event`, and the structural heads of `event`'s
    entity. Adding objects of other entities, which do not carry `event`
    (law 20), changes no name. `change.rs::name_ignores_other_entities`.
20. **Append is idempotent.** Appending an event whose print the genesis
    holds writes nothing and answers the first object carrying it. Replaying
    any sequence of appends onto the store they produced leaves its object
    set unchanged, the replay after a rejection included (§3).
    `change.rs::replay_is_a_no_op`,
    `change.rs::replay_after_rejection_writes_nothing`.
21. **Independence is structural.** Every dep is a write to its change's
    todo, so `deps` never leave a todo, changes to different todos are
    concurrent, and a store written todo by todo is the store written in
    time order. `change.rs::deps_name_one_todo`,
    `registers.rs::deps_name_one_todo`.
22. **No stored byte moves.** Every `conformance/dag.py` store, and every
    generated store of `Sealed` and `Woven`, derives, linearises and
    verifies exactly as before, and reads exactly as before wherever no
    register has two candidates. `dag.py`'s `env` changed exactly where law
    24 says it must: at a todo whose state is in conflict, it is the
    candidate set. The existing `dag.rs`, `tips.rs`, `fold_laws.rs` and
    vector suites.
23. **Geneses are disjoint.** Every object has one genesis, and no edge
    crosses geneses. The files of two prodromes united verify clean, and
    fold, and view, to the union of the per-genesis folds keyed by
    `(genesis, todo)`. `change.rs::no_edge_crosses_geneses`.
24. **A conflict prices as its most urgent candidate.** With one candidate
    per register, every reading is as it was, bit for bit. Otherwise an
    entry's value is the least over its worlds. Agreeing twins are one
    candidate. `fold_laws.rs::one_candidate_reads_as_before`,
    `fold_laws.rs::conflict_is_its_most_urgent_world`,
    `registers.rs::agreeing_twins_are_one_candidate`.
25. **The linearisation decides no value.** Folding any linear extension of
    the DAG gives the same entries; only `stream`'s order may differ.
    `fold_laws.rs::any_linear_extension_folds_alike`.
26. **`Least` is a semilattice with `Absent` as identity.** It is
    commutative, associative and idempotent, and `Least([t]) = t`.
    `normalize` and `compile` preserve its reading (laws 5, 13), and it
    round-trips (law 1). `fpl_laws.rs::least_is_a_semilattice`, with `Least`
    drawn by `a_term()` so laws 1, 5, 13 and 14 cover it.
27. **A claim settles nothing it is not trusted to.** A claiming change,
    whatever its deps, leaves every confirmed frontier as it was, and the
    confirmed price under a conflict is the least over binding candidates
    only. `fold_laws.rs::claim_never_settles`.
28. **A snapshot attests its closure.** Its name changes if any object in
    its closure does, `verify` reports an object it attests that the store
    lacks, and on a `previous` chain each closure contains the one before.
    `change.rs::snapshot_commits_to_closure`,
    `change.rs::snapshot_chain_grows`.
29. **Placement is not identity.** Moving a change's file between two stores
    of one genesis changes no name. A change re-proposed over the same
    heads is the same object, and over moved heads it has the same
    `event_id`. Checked against two plain stores, a base and a store staged
    over it: `change.rs::accept_is_a_same_name_move`,
    `change.rs::reproposal_is_recognised`.

Laws 30–32 are §6's register types (`docs/design-register-types.md`, its
laws 2 to 4), on generated values and DAGs, with a state machine in the
tests beside the vocabulary's orders. That §4's registers read byte for
byte as before is law 22 and every vector suite, unchanged.

30. **An order is a partial order.** Each value type's `≤` is reflexive,
    antisymmetric and transitive.
    `order_laws.rs::every_order_is_a_partial_order`.
31. **A reading is the completion.** A register reads the maximal values of
    its frontier's values, each once, and one value exactly when one is
    greatest; under the discrete order on events that is the candidates.
    `order_laws.rs::a_reading_is_the_maximal_values`,
    `order_laws.rs::the_discrete_reading_is_the_candidates`,
    `order_laws.rs::a_note_reading_is_the_candidates`,
    `bodies.rs::a_flag_reads_its_greatest_body`.
32. **An inflationary reading is a homomorphism.** An append to an
    inflationary register whose value is not `≥` the reading it supersedes
    is refused, and over the histories so written `read(h₁ ∪ h₂) =
    max(read(h₁) ∪ read(h₂))`. Through the store's own append, at a schema
    with an inflationary register, the refusal writes nothing.
    `order_laws.rs::an_inflationary_reading_is_a_homomorphism`,
    `review.rs::the_append_refuses_a_move_back`,
    `review.rs::the_reading_of_a_union_is_the_join_of_the_readings`.

Law 33 is the design's law 1, over three schemas: the todo schema, a
second one in the tests (`review.rs`), a machine priced by a flat term per
phase and nothing else, and a third (`note.rs`), an application schema at
the generic event of `schema::headed` with one register, so the law is
seen to ask nothing of a schema. The
design's law 5 is law 34, over any `Price`, and law 24 is it for the todo
through the entry; its law 6 is law 22.

33. **A reading is a function of the object set.** Every parents-first
    permutation of a history, every duplication of its objects, and every
    partition into two replicas folded apart and then merged read the same,
    register by register, under any schema. And the route agrees with its
    names: one write joins exactly the registers its schema says it writes,
    and each register type values exactly those writes.
    `schema_laws.rs::a_todo_reading_is_a_function_of_the_object_set`,
    `schema_laws.rs::a_review_reading_is_a_function_of_the_object_set`,
    `schema_laws.rs::a_note_reading_is_a_function_of_the_object_set`,
    `schema_laws.rs::a_todo_write_joins_the_registers_it_names`,
    `schema_laws.rs::a_review_write_joins_the_registers_it_names`,
    `schema_laws.rs::a_note_write_joins_the_registers_it_names`,
    `bodies.rs::a_flag_reading_is_a_function_of_the_object_set`,
    `bodies.rs::a_flag_write_joins_the_registers_it_names`.
34. **A conflict prices as `Least` over its candidates.** Under any schema
    with a `Price`, a reading whose candidates price nothing has no price,
    one priced candidate is the price written as it, and otherwise the
    price's value at every instant is the least of the candidates' values,
    `∅` the top: below each, and attained. A todo's moment with nothing
    first written and no head is its reading's terms.
    `schema_laws.rs::a_todo_conflict_prices_as_the_least_of_its_candidates`,
    `schema_laws.rs::a_review_conflict_prices_as_the_least_of_its_candidates`.

The design's law 7 is law 35: replicas, wherever each is held.

35. **Sync is the join.** Between any two replicas, on disk or in memory, a
    sync whose wire is cut takes nothing; a sync of a down-set and then a
    whole one leaves the receiver reading what a fresh store reads of the
    union of both directories, a second sync takes nothing, and two
    replicas synced into a third in either order read the same. A store in
    memory answers every append of any history as a store on disk does,
    byte for byte. An overlay reads and appends as a store given its own
    objects and its base's, its base reads as one given only what reached
    it, and its flush leaves the base reading the union.
    `replica.rs::sync_is_the_join`,
    `replica.rs::a_store_in_memory_appends_what_a_disk_store_appends`,
    `replica.rs::an_overlay_reads_as_its_union_until_it_is_flushed`.

Law 40 is §3's reading of a store that is not closed under parents, over
two writers' changes and over legacy chains, with any objects removed.

40. **A store reads the largest down-set it holds.** The interior of a set
    of objects is exactly the objects no removed object is beneath; it is
    never more than the set, idempotent, monotone, meet-preserving and the
    whole of a history. A store holding any subset of a history, read cold
    or by a handle that held more, reads its objects, its fold and its tips
    as a store given only that subset's interior. And quarantine is that
    reading: an object damaged on disk is set aside by `fsck`, named in its
    report and kept byte for byte, and the store reads the history without
    it and everything resting on it.
    `down_set.rs::the_interior_is_the_largest_down_set`,
    `down_set.rs::a_store_reads_its_largest_down_set`,
    `down_set.rs::quarantine_is_the_down_set_without_what_rests_on_it`.

Laws 36 and 37 are §3's deps and §6.6's supersession, on two writers whose
replicas in memory sync in between and whose events are dated anywhere.

36. **A change rests on what its writer saw.** Each append descends from
    every write to its entity its writer held, in any register, and from no
    other: happens-before within an entity is what the deps say, for one
    writer and across writers.
    So a writer's own successive writes to one entity stream in the order
    it wrote them, whatever their registers and instants.
    `causal.rs::a_change_rests_on_what_its_writer_saw`,
    `causal.rs::a_note_change_rests_on_what_its_writer_saw`,
    `causal.rs::an_add_then_a_complete_stream_in_the_order_written`.
37. **Supersession is per register.** Each register's frontier is its
    writes that no other write TO IT descends from: a write descending from
    a write in another register supersedes nothing there.
    `causal.rs::a_change_rests_on_what_its_writer_saw`.

Law 38 is §7's next change, over generated terms, environments, instants
and observations.

38. **A next change is never late.** The observed value is constant on
    `[now, at)`; where the answer is exact it differs at `at`; it is never
    later than the first change an hourly sampling finds; and it is a
    function of its arguments, so every instant of an exact step answers
    that step's end.
    `observe.rs::every_term_is_constant_until_its_answer`,
    `observe.rs::an_exact_term_answers_its_next_step`,
    `observe.rs::the_answer_is_never_later_than_a_sampled_change`,
    `observe.rs::every_instant_of_a_step_answers_its_end`.

Law 39 is the design's law 10: nests of histories, over generated nests of
two and three levels.

39. **A nest flattens to a history.** Histories keyed by path form a monad:
    `join ∘ unit = id`, `join ∘ fmap unit = id`, `join ∘ join = join ∘ fmap
    join`, `fmap` a functor and `join` natural, `bind` is `join ∘ fmap`;
    reading the joined history at a path where no key begins another is
    reading the history held there; and a join restricted to chosen values
    reads as the parent plus exactly them, as `accept` of a down-set of an
    overlay's objects leaves its base reading as one given exactly those.
    Over histories held in registers, the join holds every object of every
    level once under its name, reading it at a held register's path reads
    as the inner replica does, a pointer reads as the union of its maximal
    writes' heads, an outer write's past reaches an inner object only
    through heads it or a write beneath it named, and the memoised fold of
    the join's algebra is the join and recomputes after a deep write
    exactly the spine it moved.
    `nest.rs::unit_is_a_unit_of_join`, `nest.rs::join_is_associative`,
    `nest.rs::fmap_is_a_functor_and_join_is_natural`,
    `nest.rs::bind_is_the_monads`, `nest.rs::reading_commutes_with_join`,
    `nest.rs::a_restricted_join_reads_as_the_parent_plus_the_chosen`,
    `replica.rs::accepting_chosen_objects_reads_as_the_parent_plus_them`,
    `nest_histories.rs::three_levels_flatten_and_read_as_their_parts`,
    `nest_histories.rs::concurrent_pointers_read_as_the_union_of_their_histories`,
    `nest_histories.rs::the_memoised_fold_recomputes_only_the_spine_a_write_moved`.

Law 40 is a schema declared as data (§6), against its twin written as
code and on generated declarations.

40. **A declared schema is its interpretation.** A declared schema
    equivalent to one written as code prints every event alike, so holds
    the same objects under the same names, and reads every register's
    frontier and maximal writes alike, at every moment, over every
    generated history; through a store it writes, refuses and answers as a
    twin alike, and syncs as any replica. Each declared order is a partial
    order over generated values. A lawful declaration is admitted in its
    canonical print, which parses and prints back the same and is named by
    its hash; a declaration with one defect is refused under exactly the
    law the defect breaks.
    `declared.rs::a_declared_schema_reads_every_history_as_its_rust_twin`,
    `declared.rs::a_declared_store_appends_as_its_rust_twin`,
    `declared.rs::a_declared_reading_is_a_function_of_the_object_set`,
    `declared.rs::every_declared_order_is_a_partial_order`,
    `declared.rs::a_lawful_schema_is_admitted_in_its_canonical_print`,
    `declared.rs::a_broken_schema_is_refused_under_the_law_it_breaks`,
    `nest_histories.rs::a_nest_of_declared_proposals_flattens_and_reads_as_its_parts`.

Law 41 is a set of prodromes (§3, §7.2), over generated sets of todo
prodromes, each a replica of its own genesis.

41. **A set of prodromes is a prodrome.** A set is a nest keyed by
    genesis: its join holds every object once and reads at each member's
    genesis as that member reads alone, whatever the other members hold
    (disjointness); the set a synced replica holds is the union of the sets
    synced into it, and a set of sets flattens associatively, in its
    history and its content name. Its reading is the product of its
    members': a qualified reference reads, through the set, what it reads
    through its member alone and what that member's view prices the entity
    at, by genesis or by a declared name, and under any environment of the
    term it lands in; one to a store or an entity the set lacks reads `∅`;
    a term with no qualified reference links alike under any set; a view
    prices a qualified reference to another prodrome of its DAG as that
    prodrome's view prices the entity; and a
    proposal priced by the todo it serves, through the set, prices as that
    todo's own view does and moves exactly when that todo's history does.
    `sets.rs::each_member_reads_in_the_set_as_alone`,
    `sets.rs::a_set_of_sets_is_associative`,
    `sets.rs::a_set_reads_as_the_product_of_its_members`,
    `sets.rs::a_qualified_ref_reads_what_its_member_reads`,
    `sets.rs::a_qualified_ref_to_what_the_set_lacks_is_absent`,
    `sets.rs::an_unqualified_term_links_alike_under_any_set`,
    `sets.rs::a_view_prices_a_qualified_ref_through_its_replicas_set`,
    `sets.rs::a_proposal_prices_as_the_todo_it_serves`.

Law 42 is who wrote an object (§3's signatures and keys, §5's proof),
over generated histories of writes, keys added and revoked by a root or
by nobody, and honest, unregistered and forged signatures of any object.

42. **A proof is a reading of the history.** The objects a history proves,
    with keys read structurally or under roots, are a function of its
    object set: a replica that received them one at a time in any order,
    or two halves joined by sync, proves what the whole proves. A
    signature proves only for its key's actor, only where it verifies,
    and only beneath every revocation of its key that counts. Under a
    policy that does not require signatures, signatures and keys change no
    entry of any reading. Under one that does, an object the history does
    not prove claims (one that no signature that verifies covers among
    them) and every other stands as the wrapped policy says, so the reading
    is the wrapped policy's with exactly those claims, through replicas
    that received the objects in any order; and a history whose every
    write is signed by its actor's registered key reads as the wrapped
    policy reads it, with no finding.
    `signatures.rs::a_proof_is_a_function_of_the_object_set`,
    `signatures.rs::signatures_change_no_reading_under_a_policy_that_does_not_require_them`,
    `signatures.rs::under_proven_an_unproven_object_claims_in_any_arrival_order`,
    `signatures.rs::a_history_signed_throughout_reads_as_the_wrapped_policy`,
    `sign.rs::a_registered_key_proves_what_it_signs_for_its_actor_only`,
    `sign.rs::a_forged_signature_proves_nothing`,
    `sign.rs::a_revocation_keeps_exactly_what_it_rests_on`,
    `sign.rs::under_roots_a_key_counts_where_a_root_signed_it`.

Law 43 is what an object rests on (§3), over generated histories of
several prodromes holding changes, snapshots, keys added and revoked, and
signatures, with the order stated from the objects' fields apart from the
implementation's `parents_of`.

43. **Every reader reads one causal order.** An object rests on the edges
    it names and the genesis it names. The tips of a history, of each of
    its prodromes and of a store in memory that received its objects in
    any order are exactly the objects nothing rests on, so a genesis is a
    head only when nothing is begun from it; the linearisation and a
    replica's parents-first walk are linear extensions of the order; the
    closure of an object is its down-set; and the history a set lacking a
    genesis holds is everything that does not rest on it. A snapshot that
    names its genesis among its tips verifies and attests what one naming
    only its heads attests.
    `rests_on.rs::the_heads_are_what_nothing_rests_on`,
    `rests_on.rs::every_order_puts_what_an_object_rests_on_first`,
    `rests_on.rs::a_closure_is_the_down_set_and_a_genesis_bears_its_prodrome`,
    `rests_on.rs::a_genesis_is_beneath_its_first_change`,
    `rests_on.rs::a_snapshot_naming_its_genesis_as_a_tip_attests_as_it_did`.

Law 44 is law 40 read in a replica that is not a store, over generated
histories of the declared proposal schema read at a newer schema under
which every `Tagged` fails to parse, as a store migrated away from does.

44. **A reading is total over what a replica holds.** What a set of
    prints leaves out of its history is every name it holds or its
    objects rest on that is no object there, a print that is not one or a
    parent no print is, each with exactly the objects resting on it; the
    history is the objects none of those names. The wasm exports read
    that history: adding unparseable prints, and what rests on them, to a
    held set changes no reading of the objects that do not rest on them;
    every reading answers what it left out, naming exactly the unparseable
    prints and everything above them; and a store on disk holding the same
    bytes reports as unread exactly those prints, in the same words.
    `down_set.rs::the_interior_is_the_largest_down_set`,
    `wasm/exports/src/unread.rs::an_unreadable_print_changes_no_reading_of_what_does_not_rest_on_it`,
    `unread.rs::what_is_left_out_is_exactly_the_unreadable_and_what_rests_on_them`,
    `unread.rs::a_store_and_a_page_name_the_same_unreadable_objects`.

Law 45 is law 44 read in the store of record, over generated histories of
the declared proposal schema read at it or at a newer schema, with any
files damaged in place, removed, or joined by prints that are no object.

45. **The store of record reads totally, as a replica does.** A store on
    disk holding a set of files, read by a fresh handle or by one that
    held other files first, holds exactly what a replica handed the same
    prints holds: the same history (the interior of the prints), the same
    objects waiting outside it and the same unread prints, so the same
    fold, heads, events and ancestry, and the same `excluded`. A write
    refuses exactly when a print is unread, naming the first.
    `core/tests/all/unread.rs::a_store_reads_as_a_replica_holding_its_files`,
    `down_set.rs::quarantine_is_the_down_set_without_what_rests_on_it`,
    `memory.rs::a_file_changed_under_a_held_name_is_read_again`.

Law 46 is a digest (design §6.5), over generated sets of lines, each
resting on up to two before it and given in any order and any number of
times, and generated summaries of any cost on any share of the nodes,
under budgets from none to unbounded. It reads no object: a digest is a
reading of lines a host holds, with summaries the host records. A line's
cost, parents and measure are functions of its name.

46. **A digest's view is a decaying cut of a tree that only grows.** The
    tree over a set of lines is a function of the set, its leaves the
    lines in causal order, concurrent ones by name, and a cycle refused; a
    line appended renames only nodes holding the last line, and one
    arriving late a logarithmic number. A view's parts tile the log in
    order and cost what their lines and summaries cost. A closing is a
    settled node with, leftward, the node at its level over each lower
    part before it that decay forces beside it, admissible when all are
    summarised and every leftward prefix of it costs less than the parts
    it replaces, so a forced node that would not lower the cost closes
    only on the start's account. A view never costs more than its budget
    or its lines, whichever is more, and fits its budget unless no closing
    is admissible; it is greedy, and does not seek a fit that needs a
    coarser node to the left; what it says is waiting is pending, and
    writing what it names, round after round, ends with the view in its
    budget or with every node it could close summarised and no closing
    admissible. Its parts' levels never rise toward the present. Zooming
    every part down to the lines reads the log back. An appended line only
    coarsens the past, every part before it lying within a part after it
    and none renamed, and, with lines of one cost `b` and summaries of one
    cost `s ≤ b`, from a view that fitted with nothing waiting, the new
    parts are at most `1 + ⌈b / s⌉` stretches of adjacent parts at one
    level, the line and one per closing. `pending` is exactly the settled
    nodes with no summary whose children are lines or summarised. The
    measure folded through `memo::fold` is the plain one, and
    `digest::read` is a sound key: reads of one name answer alike, and a
    summary recorded or a line's cost changed moves it.
    `memo_balance.rs::an_append_renames_only_the_spine`,
    `digest.rs::a_view_tiles_the_log`,
    `digest.rs::a_view_fits_its_budget_unless_no_closing_lowers_it`,
    `digest.rs::a_summary_longer_than_its_lines_never_closes`,
    `digest.rs::a_closing_decay_forces_closes_with_it`,
    `digest.rs::a_dear_start_does_not_close_on_a_forced_nodes_account`,
    `digest.rs::following_waiting_fits_the_budget_or_nothing_lowers`,
    `digest.rs::a_view_decays`,
    `digest.rs::zooming_every_part_reads_the_log`,
    `digest.rs::a_digest_is_a_function_of_the_set_of_lines`,
    `digest.rs::a_cycle_is_refused`,
    `digest.rs::pending_is_what_can_be_summarised`,
    `digest.rs::an_appended_line_only_coarsens_the_past`,
    `digest.rs::a_late_line_renames_a_logarithmic_spine`,
    `digest.rs::a_digest_read_through_memo_is_the_plain_one`.

Law 47 is a pull that names its replica by a frontier token (§3), over
two writers' histories over one shared history. A store of record keeps
one, two or three frontiers and answers two replicas and another writer.
The replicas write, post, and pull one piece at a time, and the pieces of
one pull interleave with every other step. The store of record can
forget every frontier it recorded and can go back to what it held after
an earlier post, and a replica can go back to an earlier state of its own.

47. **A pull by a frontier token is the pull from its tips.** A replica
    follows two rules. It names a token only after it takes the whole
    piece that named it. It names nothing once it holds less than its
    token's frontier, for example after it goes back to an earlier state.
    Each piece of each pull, and each post, records one frontier. The
    store of record holds a token when it recorded the token and neither
    let it go nor forgot it since. It lets a token go when the kept number
    of other distinct frontiers of the store are recorded after the token
    was last recorded.
    (a) An answer reaches exactly the closure of its token's tips when the
    store of record holds the token, the tips held under it are the tips
    the token names, and the store holds every one of those tips. It
    reaches nothing otherwise. The answer lacks the other objects, in the
    store's linear order (§3: Kahn's algorithm over parents with a
    min-heap on the name). A piece sends a prefix of them.
    (b) Each frontier that an answer records is the tips of what the
    answer reaches joined to what it sends, or, for a post, to what the
    replica posted and the objects those rest on. So every frontier names
    only objects that the replica holds, and no answer leaves out an
    object that the replica lacks (safe).
    (c) A piece falls back to nothing when, after its token was last
    recorded, the store of record forgot its frontiers, the store went
    back to an earlier state that lacks one of its token's tips, or the
    store of record recorded the kept number of other distinct frontiers
    of the store. A pull with no other step between its pieces falls back
    at most once and ends, with the replica holding every object of the
    store (complete). In general, a pull ends only while fewer than the
    kept number of other distinct frontiers are recorded between each two
    of its pieces, and while the store gains fewer objects between two
    pieces than a piece sends. Two cases do not end: with a kept number
    of 3, a writer that posts 3 times between each two pieces lets every
    token go, so every piece falls back and sends the same first objects;
    and with pieces of one object, a writer that posts one object between
    each two pieces keeps the number of lacking objects the same.
    (d) A pull from a known token whose closure is every object of the
    store sends nothing and records the same token (quiet).
    (e) A store of record that stops between two writes of a record keeps
    only tokens that read as unknown or as the tips they name. It removes
    the tokens it lets go, then writes the list of the tokens it keeps,
    and writes the new tips last, so it never keeps tips that the list
    does not name.
    `frontier.rs::a_pull_by_frontier_token_is_the_pull_from_its_tips`,
    `frontier.rs::a_record_stopped_at_any_write_reads_as_unknown_or_as_recorded`,
    `frontier.rs::a_record_whose_tips_are_not_its_token_s_reads_as_unknown`,
    `frontier.rs::a_pull_whose_token_is_let_go_between_its_pieces_does_not_end`,
    `frontier.rs::a_pull_whose_store_gains_a_piece_between_its_pieces_does_not_end`.

## 10. Non-goals

A clock in the merge; a second evaluator; a rendering as a source of truth;
multi-tenancy; a hosted service.
