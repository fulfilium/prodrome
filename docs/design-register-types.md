# Register types: the history is free, a schema is its reading

Status: design, 2026-10-01; stages 1 to 3 and 7 to 13 built (§8). Successor to
`design-change-identity.md`, whose frontier this names as the universal
part.

## 0. Why

The database holds one vocabulary. §4 says it outright: "the six kinds above
are the DATABASE's", and §6 reads them into four fixed registers (state,
spec, content, tendings), with FPL as the only valuation. A host whose
entities are not todos fakes being one. Three fakes are visible in the one
host that exists:

- **An inbox of proposals.** A proposal is a state machine (standing, then
  sent, failed, may-have-gone, checked, dismissed, superseded). Stored as a
  todo, its outcome is JSON in a `Completed`'s note, a dismissal is a
  `Cancelled`, an attempt is a `Tended`, and a proposal that serves nothing
  carries an FPL price of `Absent`: a valuation the type does not have.
- **A conversation.** A transcript is a grow-only set of lines in causal
  order. Stored as a todo, every line is a content record on one register,
  and "owed an answer" is derived by hand.
- **A page.** A card's "pending" flag was a state machine nothing tied to the
  store. A lost answer left it pending forever, though the store held the
  outcome.

Each fake is the same missing abstraction: what a register IS, given by its
type, with the database owning only what every type shares.

## 1. The history is the free join-semilattice

A replica holds a set of content-addressed objects closed under their parents
and deps (§3): a DOWN-SET of the causal order. Merging two replicas is set
union. Union is associative, commutative and idempotent, so:

- every replica that has seen the same objects is the same replica
  (convergence), whatever the order and however often each arrived;
- adopting an object twice is adopting it once, by identity, because its name
  is its bytes;
- a duplicate request is one object (`Change` replays write nothing, §3).

These hold for every type, because no type is consulted. Nothing above this
layer re-proves them. This is the universal construction: the free
join-semilattice on the objects a host writes, with union as its join.

## 2. A register reads its frontier

A schema routes each event to the registers it writes, with a value for
each. A register's reading at a moment, under a policy (§5), is its
FRONTIER: the binding writes no other binding write to it descends from.
Frontiers ordered by causal domination form a lattice, and reading the
frontier commutes with union:

    frontier(h₁ ∪ h₂) = frontier(h₁) ⊔ frontier(h₂)

So every register gets a join-semilattice for free, and a frontier with more
than one write is not an error: it is an element of that lattice with more
than one maximal write. This is today's `fold::Frontier`, and today's
`Register` trait already says the rest: a fold is a semilattice of writes.

**Causal order decides succession; nothing else does.** A write supersedes
exactly the writes it descends from. No clock, no name order and no value
order ever decides that one write replaced another. §3 and §6 stay true.

## 3. A type supplies an order; the join is free

What a register's type supplies is a PARTIAL ORDER on its values, `≤`. It
decides nothing about succession (§2). It decides only which concurrent
values are redundant: the register's reading is the set of MAXIMAL values
among its frontier's values,

    read(r) = max≤ { value(w) : w ∈ frontier(r) }

which is the free join-semilattice completion of the order (finite
antichains of the poset, ordered by domination). One maximal value is a
VALUE. More is a CONFLICT, shown whole as today. The type never writes a
join; the completion is the join.

- **Discrete order** (`a ≤ b` iff `a = b`). Every distinct value of the
  frontier is a candidate: exactly today's `candidates()`, twins counted
  once. The todo's state, spec and content registers are discrete.
- **Total order.** The reading is always one value, the greatest. A register
  that may only grow (a version counter, a high-water mark) is total.
- **A state machine** ordered by "further along". A proposal's machine is
  `standing < failed < sent`, `standing < unknown < checked`,
  `standing < dismissed`, `standing < superseded`. A retry that sent after a
  failed attempt reads `sent` (failed ⊔ sent = sent, since failed < sent).
  `sent` and `dismissed` are incomparable, and that is the one real conflict:
  one device sent while another dismissed. The order puts the conflict where
  it is true and nowhere else.
- **Sets under inclusion.** The grow-only set (the tendings, a transcript's
  lines) is the order `⊆` with union as its completion's join.

**Convergence needs no law from the type.** The reading is a function of the
object set (§1), for any partial order. A type that supplies a wrong order
reads wrongly; it cannot make two replicas disagree.

### 3.1 Inflationary registers

A type MAY declare its register INFLATIONARY: every write's value is `≥` the
values of the writes it supersedes. When it is, the reading itself is a
semilattice homomorphism,

    read(h₁ ∪ h₂) = read(h₁) ⊔ read(h₂)

and the register can be read incrementally, coordination-free (§6). The
declaration is not trusted. The append path enforces it: an append whose
value is not `≥` the reading it supersedes is refused, by the store's smart
constructor, before any object exists.

The proposal machine is inflationary. The todo's state register is not
(`Reopened` after `Completed` goes back), which is precisely why todos need
shown conflicts and a valuation of them (§4), and why the reading of a todo's
state is never incremental in the strong sense. Nothing is lost by that: the
frontier is still free.

## 4. A valuation is optional

A schema MAY value its entities: a map from an entity's reading to an FPL
`Term`, its price (§7). For a register in conflict, the valuation of the
antichain is `Least` over its members' valuations: the meet in the
fulfillment order, "price a conflict as its most urgent world". That is
today's rule, now stated as what it is: the valuation's own meet, applied to
an antichain. It is not the register's join.

An entity whose schema has no valuation has NO price. It is not `Absent`.
`Absent` stays a value of FPL meaning "this todo prices no opinion"; it
stops standing in for "this type has no price".

**What landed.** A valuation is `schema::Price`, one method:

    fn terms(registers: &Self::Registers<'_>) -> Result<Vec<Term>, FplError>;

the term each candidate of the reading prices as, and `fold::price` is
`Least` over them, defined once. `Least` is the meet in the fulfillment
order: its value at every instant is the least of its members', with `∅` as
the top, so a reading none of whose candidates prices is `∅` and an
unpriced candidate never lowers a priced one (law 5). For the todo, a
candidate is a WORLD, one candidate from each register.

What only the todo has is in traits only the schemas that have it
implement, none of them required of a `Price` (§5):

- `History: Price`, a price that changes with the entity's history:
  `moment(now, first, head)`, the terms of one moment, a register unwritten
  yet read as first written and a world that prices nothing priced as the
  head. `fold::flatten` makes each moment a piece of SPEC §6.4's function, and a
  moment with nothing first written and no head is the reading's `terms`.
- `Bind`, what FPL's `Ref`s read of an entity in this store
  (`fold::env`), its key a string. A `Ref` to an entity in ANOTHER store is
  a host's environment, passed in, never a method of a schema.
- `Row`, what a list row shows (`Reading`) and when it is provisional
  (`disputes`, `unconfirmed`); `view::entries` asks for `Row + History +
  Bind`.

A schema whose price does not change with its history, that nothing
refers to, and whose store trusts every writer implements `Price` alone.

**Two operations, two laws.** The completion's join merges CONCURRENT VERSIONS
of one register: idempotent, since a value joined with itself is itself.
FPL's conjunction (`Conj`, the power mean, §7) AGGREGATES DIFFERENT ENTITIES:
a commutative operation that is deliberately not idempotent (a `Flat(1.0)`
member dilutes, law-tested). Calling both "the connective" would hide that
they obey different laws. A schema chooses its order; FPL's conjunction is
not a register's business.

## 5. A schema

A host declares a schema, and the database is generic over it:

- an EVENT VOCABULARY: closed constructors, their frozen fields and their
  nested vocabulary, parsed by §2's whitelist walker and printed canonically,
  exactly as a payload is today;
- an ENTITY KEY each event names (a todo, a proposal, a conversation);
- its REGISTERS, a product: each a name, a value type with its order, whether
  it is inflationary, and the route from each event to the registers it
  writes with what value;
- optionally a VALUATION (§4): `Price`, and where it has them, `History`,
  `Bind` and `Row`.

The registers of an entity are a product of semilattices, so they are a
semilattice; the fold of a stream is a monoid action (`extend`), as §6.6
already states. Standing (§5) is unchanged: a policy answers per event, the
confirmed reading folds what binds, the claimed reading folds everything,
and both stay in the database.

**The schema is the store's type, not a field of an object.** A host opens a
store at a schema, as it opens one at a payload today, and the schema is the
parse: an event the schema's vocabulary does not name is refused at the
boundary. No object carries its schema, so `Genesis(label, nonce)` stays
frozen. A host may say in a genesis's label what a prodrome is for.

**The todo schema is the reference schema.** §4's six kinds and the record
kind, routed into state (discrete), spec (discrete, valued by FPL), content
(discrete) and tendings (grow-only), with FPL's valuation and `Least`. Every
legacy envelope belongs to it. Every conformance vector reads byte for byte
as before: the generalisation must be invisible to the one schema that
exists.

**What landed: a schema as data** (stage 10, §8). The schema was always a
value in this design; a Rust schema is the case where the type says all of
it. So `Schema::Vocabulary` is a value a store is opened at
(`EventStore::at`, `MemoryStore::at`, `Replica::schema`), every parse takes
it, and a Rust schema's is a unit with a `Default`. A DECLARED schema is
another value of that slot, `declared::Declaration`, and its events,
`declared::Declared`, implement `Schema` by interpreting it. Its parts are
this section's list, as data:

- the vocabulary: constructors with typed fields in printed order, the
  types a closed set (text, integer, instant, a reference to an object, a
  closed enum, a list of those);
- the key, a text field every event has, and the stamp (`at`, `actor`)
  every event has because every event says when and who;
- the registers, each a name, an order from a closed vocabulary (discrete,
  total over an integer, a machine given by its covering relation, sets
  under inclusion), whether it is inflationary, and the route: which field
  of which event writes it, the one combinator;
- no valuation: a price stays Rust's (§4).

The text is the objects' own §2 grammar under the schema language's closed
vocabulary, not JSON: one parser, one printer and one whitelist for objects
and schemas, a canonical print by construction, and so a content name, the
hash of the text. A schema's sets print sorted, so its text is a function of
the schema and not of the order it was written in.

Nothing is read by a second path. A declared register is a `Frontier`, read
by `Frontier::read` under its declared order, which IS the core's
(`Discrete`, `Total`, the order on sets, and a machine's closure), so its
reading is the completion every register's is; an inflationary one climbs
by `fold::grows`, through a value type only admission's inflationary
registers construct. Stores, `sync`, `accept`, nesting and the memoised fold
are the code they were, at a type parameter they had not seen.



A page is a reading too: a fold over the replica's history ∪ the page's
INTENTS. An intent is a write the page has asked the box for and not yet seen
in the history. Intents form a grow-only set, and an intent leaves the view
once a history write SUBSUMES it (the box's record of the act). So a view is
a monotone function of two semilattices, and drawing it is idempotent.

A lost answer cannot freeze such a view: the pending state is an intent, and
the replica's next sync either holds the outcome (the intent is subsumed) or
does not (the intent is still visible, and asking again is safe because the
effect is memoised by its hash, §1).

**CALM names where coordination is needed.** Monotone readings, the readings
of inflationary registers and grow-only sets, can be computed
incrementally and need no coordination. A NON-MONOTONE question needs a
point of coordination: "is this proposal still standing, so I may send?"
is a negation over the history, and the host answers it under a lock, once.
The design predicts exactly the one lock the host found it needed, and every
new non-monotone question is a lock to name, never a race to discover.

What stays out of the store: events too frequent or too local to deserve an
object (a keystroke, a streaming answer's text). They are in-memory registers
of the same types, never content-addressed.

### 6.0 The whole application is a reading

Not only a card: the whole client is one function,

    screen(t) = view(reading(history ∪ intents), ui, t)

over exactly four sources, each a semilattice or a time:

- **history**: the objects synced from the host, a grow-only set (§1);
- **intents**: what the page asked for and the history does not yet hold, a
  grow-only set retracted by subsumption (§6);
- **ui**: the client's own registers (below);
- **t**: the clock. A price is already a behaviour, a function of time (§7):
  the list is the reading sampled at `t`, so a ticking clock re-sorts it and
  nothing refreshes.

**Navigation is a register.** The route is a register of the ui tier;
switching pages or opening an entity is an event that writes it, and the
screen is a different projection of readings the replica already holds:
nothing mounts or loads. History is a log of route events, so "back" moves a
pointer and reads again; a deep link is a route value printed. Selection,
focus, an open sheet, scroll and an unsent draft are registers of the same
kind, folded the same way, and a key binding or a palette entry is an event
mapped to an intent. A transition is a behaviour of time.

**Sync is chosen per register.** Every register is the same type whatever
tier it lives in, so whether it syncs is a property of the register, not of
the mechanism: a route stays on its device, a draft may follow its writer
from one device to another, and "what this device has open" may be a
per-device register another device follows. One writer per device means no
conflict within it; across devices, two opens are two candidates like any
register's.

**Effects are intents, all of them.** A monotone act (complete, edit, add) is
sealed into the client's replica and synced (§6.1); a non-monotone one (send
this message) is a request the host answers with a `Decision` under its lock
(§6.1). Either way the page shows it pending until the history subsumes it,
so no pending state is ever set by hand and none can be left behind.

**The host is the same shape.** Its background work is rules over registers:
"when a reading has property P, perform E under a `Decision`, and record the
outcome" (an answer owed in a conversation, a proposal standing and tapped, a
queue of new work). The rules that need a lock are exactly the non-monotone
ones, as CALM says.

What this removes: per-page loads (a page reads the replica), polling (sync
drives the reading), the class of races between a load and a reload (there
are no loads, only a set that grows), and every pending flag.

**A view is a document over a typed value, so views are data.** Once the
reading is one value of a declared schema (the module's `schema!` answers it),
`view` need not be code in the client's language: it can be a Typst function
of that value, compiled live by `typst-wasm` to HTML. Three things make that
safe whoever wrote the view:

- the compile's world holds only the view's modules and the state, as data
  (text in the state is a string, never markup, and nothing evaluates it);
- the output passes a tripwire that refuses a script, an inline handler or a
  `javascript:` link, so a view cannot act by itself;
- a control declares its intent as data (`data-intent`), and the client parses
  it at the boundary against a CLOSED vocabulary of intents; the non-monotone
  ones still go through a `Decision`.

So a view can do nothing a person's tap could not, and adding a page or a
popup is adding a module, not a build. Which party may write a module (the
application, its user, an agent working for them) is the application's
choice; the structure allows each and prescribes none. Keystroke-rate state
(text being typed, scroll, a stream) stays in client-owned islands, so a
keystroke never recompiles a view.

**State that is data too: what landed.** Views as data give an agent new
pages over the schemas that exist. The next step was an agent adding STATE
of its own: a new register type (its order, whether it is inflationary) or
a new store. Stage 10 (§5's "What landed") answers the store half without a
module: a schema arrives as text and admission is a law check rather than a
review, each law mechanical and named by the refusal when it fails:

- it parses, and it is its own canonical print, so it has a name;
- names are unique, the key exists on every event, every event is stamped;
- every route is typed: a register is written by fields that exist, of one
  type, the type its order reads;
- every order is a partial order: discrete, total and inclusion by
  construction, a machine because its covering relation is acyclic, so its
  transitive closure is antisymmetric;
- every inflationary register's order has a bottom, since its reading is a
  homomorphism of semilattices with units and the empty history reads as
  the least value.

Laws that cannot fail need no check: the reading is a function of the
object set because no declared order is consulted on succession (§2, §3),
and an inflationary register climbs because the append refuses a write that
does not (§3.1). What is still open is the other half, a register type
whose order is not in the closed vocabulary (code an agent ships: a wasm or
WASI module, the proof it carries, and what a module failing a law later
costs the stores built on it), and intents a module declares beyond the
closed vocabulary of §6.0. A declared schema is the closed vocabulary's
answer, and adding an order to it is adding a case with its law.

Found on the way: an inflationary register whose maximal values have no
upper bound keeps its conflict. The proposal machine's `sent` and
`dismissed` have none, so a write after both must climb above each and none
can; §9's "a later write that descends from both settles it" holds for a
register that is not inflationary, and for an inflationary one only where
the order has a join of the two. That is the order saying the conflict is
real and final; a host that wants it settled gives the machine a state
above both.

### 6.1 Replicas: memory and disk

A store in memory and a store on disk are not two kinds of thing. Each is a
set of objects closed under their parents (§1), so each is a REPLICA of the
same free semilattice, and moving state between them is the join:

    sync(from, to) = to ∪ from

SYNC adopts into `to` every object of `from` that `to` does not hold, each
verified on adoption as any received object is (it hashes to its name, its
parents are held). It is idempotent (an object adopted twice is one object),
order-free (union commutes), and safe to interrupt and repeat, since a
partial sync is a smaller union and the next one completes it. Nothing about
it is a type's business: it never reads a register.

An OVERLAY is a store that writes to one replica (memory) and reads the union
of that replica and another (disk): its readings are the readings of
`memory ∪ disk`, by §1, without copying either. Flushing an overlay is
`sync(memory, disk)`. The replicas that already exist are instances of it: a
browser's replica of a host's store (writes sealed locally, adopted by the
host), and an agent's staging area (a batch is an overlay over its target,
and accepting it is a sync of exactly the objects picked).

**Durability follows the CALM line (§6).** A MONOTONE write (a proposal
asked, a transcript's line, an edit, a page's intent) may stay in memory and
sync lazily: a crash before the sync loses that write and never a reading's
consistency, since the replica that survived is still a down-set. A
NON-MONOTONE decision may not: "is it still standing, so I may perform?" and
the record of the effect must be answered and written by the replica of
record, under its lock, before the effect, or a crash between them lets the
effect run twice. So an overlay is never the store a coordinated decision is
asked of: the type of a store that may answer one is the type of a store
that writes through, and an overlay does not have it.

A third tier never syncs: in-memory registers of the same types over events
too local or too frequent to be objects (a keystroke, a streaming answer).

**A reading is total over what a replica holds.** A replica in a page is
handed prints, and a print may not be an object at the schema the page
reads at: a constructor the schema has since changed, bytes that do not
hash to their name. The page's history is then what a store's is (SPEC law
40), the largest down-set of the objects, and the print is left out with
everything resting on it, which is the history of a replica that never
received it. Nothing is left out silently: what is left out is a function
of the prints, `Dag::excluded`, each name with why and the objects above
it, the one `verify` reports by, and every reading answers it beside what
it read (SPEC law 44). So adding an unreadable print, and what rests on
it, changes no reading of an object that does not rest on it.

The store of record is a replica like any other, so it reads the same way
(SPEC law 45). Its files are prints, read by the function a page reads
what it is handed by (`Dag::from_prints`); one that is no object is held
as unread in the same `Dag`, beside the objects waiting on it, so a store
and a replica holding the same files are one value and `excluded` answers
alike of both. What does not follow is a DECISION (above): it names the
store's heads and answers a negation over the history, and an unread file
may be a head or the very write that settles the question. A reading may
omit; a decision may not guess. So every write refuses while anything is
unread, naming the file, and `fsck` (for a file failing its hash) or a
store opened at the schema that parses it is the way on.

### 6.2 Caches are tabulations of pure functions

Once a view is a pure function of a reading (§6.0), every cache in the
application is one structure, and it is a prodrome of a particular type.

**A cache has no meaning of its own.** The meaning of `memo f` is `f`
(Elliott's denotational discipline: observing through the cache equals
observing the function). A function `a → b` is isomorphic to a table indexed
by `a`, so memoising is tabulating `f` and indexing the table, filled on
demand (Elliott's functional memo tries). Everything below follows from that
one equation.

**The cache is a partial function, ordered by information.** A finite cache
is an approximation of `f`, ordered by inclusion of graphs: the more pairs it
holds, the more of `f` it knows, and `f` itself is the top (Scott's order). So
a cache is a store whose register per key has the FLAT order (unknown, then
one value) and whose join is union. Because `f` is pure, the union of two
caches is always consistent: two replicas that computed `f(a)` hold the same
`b`. Two different values at one key are not a conflict to show but a
FINDING: `f` was not pure, or its key left out something it read.

**A key names the whole computation.** The key is the hash of the function
(its code and everything that code reads: the view's modules, the
typesetter's version, the fonts) together with the hash of its argument,
which is what a build system's derivation is. The argument is itself a
content-addressed value, so a sub-value has a name and equality is a
comparison of names.

**What it may do that no other store may.**

- An entry may be DELETED. Nothing rests on it (it has no deps, and nothing
  takes it as one), and anything dropped is recomputed on demand. Eviction
  moves the cache down the information order and leaves every meaning where
  it was.
- An entry is VERIFIED by recomputing it, as an object is verified by
  rehashing it, so an entry from an untrusted writer is a claim to check, and
  a trusted one is a shortcut (a binary cache's signature is the same choice).
- A cache stays out of a checkpoint. A checkpoint is the data and the code
  that reads it, and a cache is a function of the two.
- It syncs like any replica (§6.1): what the host computed reaches the
  client by union, and the other way.

**Hierarchical, always, and generically so.** Nothing names levels. Any
value a view reads is a tree (a fixed point of some functor), and made
content-addressed it is a Merkle tree, every subtree named by its bytes, as
the history itself is. A view over it is a fold, and the cache is ONE
combinator, the memoised catamorphism:

    memoCata alg (Fix f) = memo (name (Fix f)) (alg (fmap (memoCata alg) f))

the algebra memoised at every node, keyed by that node's name. So the cache
has the data's own shape, whatever the data is: a change recomputes the
spine from the changed subtree to the root and every other node is a hit,
with no change tracked and no level chosen. A cache only at the root is all
or nothing, and a cache only at the leaves rebuilds everything above them;
this is neither, and it is the same for a list, a page, a conversation or a
term. Where a node holds a long sequence, the sequence is itself a balanced
tree (a measured finger tree), so the spine through it is logarithmic. The
output is a tree of the same names (a keyed DOM, a node per name), so
placing a redraw also touches only the spine.

So a cost is EXPLAINED, not only measured: a redraw reports, per node and by
depth, what hit and what was recomputed, and its cost is the work along the
spines that moved. A number that does not follow from those counts is a
finding (a name that covers too much, so too much misses, or too little, so a
hit is wrong).

**Time is an input like any other, keyed by what is observed.** A cache keyed
by the instant never hits. But a view shows a behaviour's value at a
precision (a whole percent, a pie's drawn angle), so it is a function of the
OBSERVED value, `view ∘ observe ∘ v`, and is keyed by that. A behaviour that
is an explicit function of time (a fulfillment term) can also answer when its
observed value next changes (push-pull reactivity: a behaviour knows its next
discontinuity), so a client redraws exactly at those instants instead of on a
ticking clock.

LANDED for fulfillment terms. `observe::Observation` is the `observe`, a
value of its own rather than a constant in a view: `[0, 1]` in `n` equal
steps with a stated rounding, `∅` read as itself, monotone, so `observe ∘ v`
is a step function. `observe::next_change(term, now, env, observation)` is
its next step, `(at, exact)`, `at` none for never. On the exact fragment
(SPEC §7 breakpoints) it is the step itself: each atom's closed form (a
`Decay`'s window, a `Curve`'s points, a `Piecewise`'s knots, two lines'
crossing) cuts time into stretches where the term is affine, and the step
inside one is bisected against the evaluator, so it agrees with
`fulfillment` bit for bit. Elsewhere (`Within`, composites of moving parts,
`After`, `Recur`, `Periodic`) it is CONSERVATIVE: the first instant an
enclosure of the term over `[now, t]` stops being one observed value, never
later than the step and early only where the enclosure is loose (members
moving in opposite directions). New history is not time: it arrives by sync,
a new input, and the answer is asked again. The wasm exports carry it:
`next_change` beside `fulfillment` in the reference module, and
`next_changes` per row on a priced schema's class.

**Time's one type is a schedule.** A behaviour that is piecewise constant
is a `schedule::Schedule<A>`: a head and knots at strictly increasing
instants, MEANING the function whose value at `t` is the last knot at or
before `t` (`at`). Its algebra is stated as that meaning and law-tested:
`map` (a functor), `zip` and `sequence` (an applicative, two step functions
combined pointwise over their merged knots, `constant` the unit), `+` where
the value is a monoid (`at` a homomorphism), `join` (the reader monad's
diagonal), `shift` (read later), `normal` (the quotient by meaning) and
`next_change` (the first knot after `t` whose value differs: the push
half). Its representation is the free one, a head and a finite map, and
`Schedule::of` builds it from that map, so it is a function of a set of
knots, whatever order they came in.

What were step functions written by hand are now it or functions of it. A
`Piecewise` term holds a `Schedule<Term>`: evaluation is `at`, the normal
form is `map opened . join . normal` (splicing was the join), and
`normalize` lifts a pointwise layer by `sequence` and a `Shift` by
`shift`. An entity's registers as a function of time are
`fold::readings`, ONE fold over time: `fold::read` is its value at an
instant (the scan keeping only its end), SPEC §6.4's function maps it to
prices, and §6.5's history maps it to outcomes; the wire's `History` holds
each todo's bindings as one. The observed value of an exact term is
`observe::observed`, a `Schedule<Option<u32>>` from `now` on, and on the
exact fragment `next_change` is its `Schedule::next_change`; the
conservative enclosure cuts a span by a schedule (a `Piecewise`'s own,
`After`'s binding, `Recur`'s last tending).

**What landed.** `prodrome::memo` (stage 8, §8), which depends on the hash
type and on nothing else of the crate.

- **The cache as a store.** `memo::Cache` holds (key, value) pairs, a
  `memo::Key` being a function's name and its argument's. A key reads as a
  miss, a hit or a FINDING (`Lookup::Finding`, every value it holds), so
  the cache is the free structure on its pairs, read as a partial function
  where it is one. `join` is union and answers each finding it makes;
  `insert` does the same for one pair. Any entry may be removed, and a
  `memo::Evict` policy decides which go: `Keep` drops none, `Lru` keeps a
  bounded number. It is not an `EventStore` and holds no objects, on
  purpose: an object is never deleted, is verified by rehashing and
  belongs in a checkpoint, an entry is none of these, and with no type in
  common no entry can be received into a history or an object evicted from
  one. It syncs as a replica does, by `join`, and a host sends
  `Cache::pairs`.
- **The memoised fold.** A `memo::Tree` is a node with a content name and
  children; a `memo::Algebra` has a name and a step from one layer (the
  node, its children's results) to a result. `memo::fold` runs the step
  bottom-up, memoised at every node by (the algebra's name, the node's
  name), and never enters a subtree whose top hits. Its `memo::Report`
  says, per node and by depth, what hit, what was computed and which keys
  held two values (computed, and neither used). `memo::name` is the name a
  node must have: its own bytes, length-prefixed, and its children's
  names in order, so a change renames exactly its spine.
- **A long sequence.** `memo::balance` builds a sequence into a balanced
  tree of the host's own chunk nodes, each named over its children and
  holding their measure. A run of nodes closes after a node whose name
  ends a chunk (one in four) once it holds two, or at sixteen, and a
  level's last run closes as it is, so every element is at one depth and
  every chunk off the path to the last element holds two to sixteen; the
  shape is a function of the elements alone: two replicas holding one
  sequence build one tree and share its cache, an edit renames a
  logarithmic spine, and an append renames only chunks on that path, so a
  closed run never changes.

No wasm export. The algebra is a host's code, so a fold in the module
would call back across the boundary at every node it computes, and what
the boundary would carry (a tree of JSON and a name per algebra) is the
host's to define. The first host is a client's view, in the host's own
language or in `typst-wasm`'s workspace, and an export is added when one
reads a cache through the module.

### 6.3 A prodrome of prodromes is a prodrome

A register's value may itself be a prodrome: a conversation inside the store
of conversations, a card's or a popup's local state inside the page's, an
agent's batch beside the chain it targets, a plugin's store inside its user's.
Histories nest, and a nest of histories FLATTENS into one history. That is a
monad, and it is the shape local state takes.

- **Unit.** An event is the history of one object.
- **Join**, `P (P e) → P e`. An outer history holds an inner one by its heads
  (a pointer, as a git tree holds a subtree by its name). Join replaces the
  pointer with the objects it names, each keyed by the PATH of the register
  that held it. Content names are global, so no inner object is renamed and
  no dep is rewritten; only the key gains a prefix.
- **Why the laws hold.** Keys are paths, and paths under concatenation are the
  free monoid; the objects form the finite down-closed powerset (§1), whose
  join is union of unions. Unit and associativity are those two structures'
  laws: flattening three levels gives the same history in either order, as
  paths `a/(b/c)` and `(a/b)/c` are one path.
- **Reading commutes with join.** Reading the flattened history at a path is
  reading the inner history it came from, so nothing nested reads
  differently once flattened, and a reader never needs to know which form it
  holds.
- **Bind** gives every value of a history its own sub-history: `h >>= f` is
  local state per entity (each todo's draft, each conversation's lines),
  flattened into one history keyed by entity.

**Local state is a nested prodrome.** A component of a client (a page, a card,
a popup, a view an agent wrote) owns a sub-history in the in-memory tier
(§6.1). It is read like any store, its registers sync or not as each declares
(§6.0), and it is undone and travelled through in time like the rest,
because after join it IS the rest. Nothing is kept beside the reading at any
level.

**Accepting is joining; discarding is not.** Taking a sub-history into its
parent is join restricted to the objects chosen: an agent's batch accepted
or rejected object by object, an overlay flushed to the replica of record
(§6.1), a plugin's store admitted after its laws pass, a draft committed.
Discarding an in-memory sub-history removes objects nothing outside rests on,
which is why it may.

**Causality crosses levels only through pointers.** An inner object's deps are
inner. An outer write that depends on inner state names the inner heads it
saw, as a change names its entity's heads (§3's change identity), so
happens-before between levels is exactly what the pointers say.

**And the rest composes.** A prodrome of prodromes is a tree of
content-addressed histories, so the memoised fold (§6.2) folds it with no
further mechanism, and a client built this way is Elm's architecture with
three changes: the model is the reading of a history that only grows (time
travel, undo and sync are not extras), views are data loaded at run time
(§6.0), and nested components compose by join instead of by hand-written
message plumbing.

**What landed.** `prodrome::nest` (stage 9, §8).

- **The monad.** `nest::Path` is the free monoid on segments, and
  `nest::History<T>`, a finite set of `(path, value)` pairs, is the
  powerset of its writer: `unit`, `fmap`, `join` (`{(p·q, v)}`), `bind` as
  `join ∘ fmap` (the writer's bind, whose function never sees the key),
  `at` (reading at a path, the values under it with the path stripped) and
  `accept`, the join of a parent and the chosen values of an inner history,
  defined as that join so it is no second mechanism.
- **A register whose value is a history.** `nest::Heads<I>` is a value
  holding a history of the schema `I` by its heads, a tuple of names printed
  sorted as a change's deps are. A schema declares such a register as it
  declares any other, by a `RegisterType` whose values are `&Heads<I>`, so
  the inner schema is the register's TYPE and never a field of an object,
  as the store's schema is never a field (§5); `nest::Nests` names a
  schema's held registers, gives each a path segment and reads the heads an
  event writes. Heads are ordered by inclusion, which is sound (heads within
  heads name a history within a history) and needs no inner object; the
  register's reading, `nest::pointer`, is the UNION of its maximal writes'
  heads, the inner history's own join, which the order cannot compute and
  does not pretend to. Not inflationary: a pointer moves to what its writer
  saw.
- **The nest and its join.** A `nest::Nest`, read from each level's
  replica (`Leaf`, `Holding`, one replica per schema holding every
  sub-history of that level, as one object database holds a repository's
  trees), is one level's objects, each at its entity's key, and the nest
  each pointer holds at `key/register`. A pointer holds everything ANY write
  to it named, as a commit reaches every tree its ancestors named, so the
  join is a function of the object sets. `Nest::flatten` is the join: every
  object at its path under the name it had. An object of a level rests only
  on objects of that level: a head or parent the level lacks is a missing
  object, never an edge to another level. `Nest::past` is happens-before
  across levels: an object's own level's ancestors, and through each
  pointer among them, what it named.
- **One mechanism for accepting.** `store::accept(from, to, chosen)` is the
  restricted join for replicas: `to` takes what `chosen` rest on, verified,
  renaming nothing. `sync` is it with every tip chosen, `Overlay::flush`
  with all of the overlay's own objects, and `EventStore::adopt` with one
  tip; `Overlay::accept` is an agent's batch accepted object by object.
  What moved: the receive each of those called directly is now reached
  through `accept`, and nothing else changed. An admitted plugin store has
  no code yet; it will be `accept` too.
- **The memoised fold.** `Nest` is a `memo::Tree`: a node is a level's
  history, its children the nests its pointers hold, its name
  `memo::name` over its keyed objects and their children's names. The join
  is `memo::fold` of one algebra, and nothing was added to `memo`.

### 6.3.1 A set of prodromes is a prodrome

A store already holds several prodromes, each begun by its own `Genesis`
and none resting on another's objects (SPEC §3: no edge crosses geneses).
That is a SET OF PRODROMES, and it is §6.3's nest keyed by genesis, not a
structure beside it.

- **Identity by genesis.** A member's key in the set is its genesis's
  name, which is global and content-addressed, so a member is never given
  a key by the set it is in: the same prodrome in two sets is one member,
  and its two replicas join by union like any replica (§6.1).
- **A set is a nest keyed by genesis.** It has no objects of its own; each
  member's history is held at its genesis, so `join` puts every object at
  `genesis/key`, renaming nothing.
- **Disjointness.** A member's reading is a function of its own objects:
  no other member's objects move it. Each object is in exactly one member,
  and reading the set's join at a genesis reads that member as it reads
  alone.
- **A set of sets is a set.** Its members carry their own keys, so a set
  of sets is a nest keyed at the root, and its join is the union of the
  sets: syncing two replicas is joining the sets they hold, each member by
  its genesis. Associativity is the nest's: a union of unions is one union.
- **The reading is the product.** The set's reading is each member's
  reading, by genesis. What a term elsewhere reads of a member is that
  member's functions and the environment its terms read (§4's `Bind` is
  this, for one store).

**A qualified Ref is a variable of the set.** `Bind` keeps a reference to
another store out of every schema: "a host's environment, passed in". The
set is that environment. `RefIn(store, entity)` names an entity of the
member `store` names, a genesis or a name the host maps to one, and it is
the same variable as `Ref(todo)` with a qualifier: an unqualified `Ref` is
the qualified one at home, so it keeps meaning what it meant and every
vector stands. A qualified reference binds to its entity's function linked
in its own member, so a `Ref` inside it names that member's entity, and
compiled against that member's environment (§7.1), so no `After` or
`Recur` of it reads the history of the term it lands in. It therefore
reads what the entity reads at home (SPEC law 13). A store the set does
not hold, or an entity its member does not, reads `∅`: what one replica
does not yet know of another has no claim on attention, and the next sync
that brings it makes the reference read it.

So a proposal in an inbox store is priced by the todo it serves, a
`RefIn` into the todo store, and its price moves when that todo's history
moves, and at no other time.

**What landed.**

- **The set as objects.** `nest::Set`, a `Level` over a replica's objects
  that reads each prodrome by a member level and holds it at its genesis
  (`Dag::genesis_of` says which); its nest's `flatten` is the join, its
  name covers its members'. Nothing was added to `Nest` but a nest with no
  objects of its own.
- **The set as a reading.** `fpl::Stores`, the product of members' readings
  as references read them (`fpl::Member`: functions and environment), with
  a host's names (`Stores::named`); `fold::member` reads one prodrome so,
  `fold::stores` the prodromes of the fold a replica holds (the legacy
  prodrome, which no `Genesis` begins, is no member), and `view::entries`
  prices each
  entry through `fold::member`, so the two cannot disagree, and links it
  through the set of its DAG's prodromes, so a replica of several reads as
  the set it is.
- **The qualified reference.** `fpl::mk_ref_in` and `fpl::link_in`, of
  which `link` is the case with no other stores.
- **The motivating use**, as a test: `core/tests/schemas/inbox.rs`, an
  inbox whose proposal's `Price` is a `RefIn` to the todo it serves, read
  through a set of it and two todo prodromes (`sets.rs`). A proposal that
  serves nothing has no price, which is not `Absent` (§4).
### 6.4 Who wrote an object is a reading

An object's `actor` is a field its writer chose, attested by nothing but
custody of the box. Once replicas write from phones and agents, custody is
not one place, and who wrote an object should be provable. Nothing above
needs to change for it to be: a signature is one more object, a key's owner
one more reading, and requiring a signature one more policy.

**A signature is an element, not an edit.** `Signed(object, key,
signature)` names what it signs by its content name and rests on it, its
one parent. The signed object's bytes and name never change, so signing is
adding an element to the free semilattice (§1), never rewriting one: an
object gains signatures at any time, from any number of keys, a device
signing what it wrote offline when it next comes online. Ed25519 is
deterministic, so one key signing one object writes one object byte for
byte, and signing twice is signing once, by identity. The signature covers
the object's NAME, so its deps and genesis too: a signature proves this
placement of an event, not the event wherever it is placed, since an
attacker re-placing a signed event over deps its writer never saw would
make it supersede what its writer never superseded. So twins, one event
over two placements, may differ in standing, which is the point.

**Keys are history.** `KeyAdded(genesis, actor, key)` says, in a prodrome,
that `key` speaks for `actor`; it rests on its genesis alone, so
registering a key is idempotent and a key may sign before its registration
arrives.
`KeyRevoked(genesis, deps, actor, key)` says it no longer does, EXCEPT for
the signatures beneath its deps, which name what its writer stands behind
as a change's deps name what its writer saw.

**The proof is a reading, and its meaning is a set.** Denotationally,

    proof(H) = { o ∈ H : ∃ s = Signed(o, k, σ) ∈ H.
                   σ verifies,
                   KeyAdded(g(o), actor(o), k) counts in H,
                   ∀ r = KeyRevoked(g(o), _, actor(o), k) counting in H.
                     s ∈ past(r) }

every clause a question about which objects exist and what each rests on,
so `proof` is a function of the object set (law 13). A revocation's effect
is the set of signatures it keeps, `past(r)`, and several revocations keep
the INTERSECTION of theirs: commutative, associative and idempotent, so
revocations commute with one another and with everything else, and no
order but the objects' own decides anything. A signature concurrent with a
revocation (made after, beside, or before it and never seen by its writer)
is not beneath it and does not prove: the conservative answer, and the one
the revoker can always widen by revoking again over what it trusts, or
narrow to nothing by revoking with no deps.

`proof` is not monotone: a revocation arriving takes proof away. That is
CALM's line (§6) again, not a defect: "is this object proven, so I may act
on it?" is a non-monotone question, asked under the store of record's
decision like every other (§6.1). The readings that fold it stay functions
of the object set, so replicas still agree once they hold the same objects.

**Who registers a key is the host's, as who is believed is.** A key object
COUNTS for a reader as its registrar says: every one, for `verify` (the
structural reading, as deps are read, §3), or, under a policy that requires
signatures, those a signature by one of the host's ROOT keys covers. A key
does not register keys. If it could, a revocation could revoke the key that
registered the key that revokes it, and what counts would depend on which
of two revocations is read first: a least fixed point through a negation,
which a set does not determine. Roots are outside the history for the same
reason a policy is: the database does not decide whom to believe.

**Requiring a signature is a meet.** A policy is now a function of an
object and of a reading of its history, `standing(H)(o)`, and a policy that
reads nothing of the history is the same at every one (§5's policies are).
`Proven<P>` is `proof ⊓ P` in the two-element lattice `Claims < Binds`:
one combinator over any policy, never a special case beside the others. An
unproven object CLAIMS whatever its kind, stored and shown as every claim
is; so under `Proven<P>` the reading is `P`'s with exactly the unproven
objects claims, and under a `P` that does not read the proof, signatures
change no reading at all. A `Proven` is a policy AT a history, made at one
and moved to another by `at`, so no reader holds one that read nothing.

**Where a key lives is the host's.** A device key is a seed the host keeps
(a phone in its own storage, a box in a file) and hands to the call that
signs; neither the core nor the wasm module holds one, and the core has no
source of randomness to draw one, as it has no clock.

**What a proof costs.** `Proof::of` verifies every signature it reads,
each time it is asked, so a reading under `Proven` costs a verification
per signature held. A signature's validity is a pure function of its
object's bytes, so it is a tabulation by §6.2 waiting to be kept: keyed by
the `Signed`'s name, never stale, droppable at will. Not built: no host
yet holds enough signatures to need it.

**What landed.** `prodrome::sign` (stage 12, §8): the three objects;
`Secret`, a device key from its seed; `Signed::verifies`, strict
verification; `Proof::of(dag, registrar)` with `Registrar::{Anyone,
Roots}`. `policy::Proven<P>` and `Standing::and`; `Policy::standing` and
`confirms` take the object's name, and `Policy::at` moves a policy to a
history (`None` for one that reads nothing of it). `verify` reports a
signature that does not verify and an object whose actor has a key and
which no key of its actor's proves. The wasm exports: `sign(secret,
object)` on every schema's class, sharing `append`'s write path; `proven`;
`public_key`; and `{untrusted, roots}` wherever a reading takes a policy.

### 6.5 A digest: a log read through summaries

A transcript or a journal is a grow-only set of lines in causal order
(§3's sets under inclusion), and a long one is more than a model can be
prompted with. A DIGEST reads it through summaries, and each piece of it is
something this document already has.

- **The tree is a reading.** The lines are a set, each naming the lines
  it rests on, and the tree's leaves are that set in the crate's one causal
  order (`topo::linear`, which `Dag::linearise` also is: parents first,
  concurrent lines by name). Over them, `memo::balance` (§6.2) names each
  chunk by `memo::name` over its children, carrying a monoid's measure of
  its lines. So the shape is a function of the set, not of when each line
  arrived or the order a caller gave: two replicas holding one log build one
  tree, a line given twice is the line once, a line arriving late renames a
  logarithmic spine, and an appended one renames only the path to the last
  line, so every node off that path is SETTLED and keeps its name for good.
  A position (`first+lines`) is a handle to show a reader, never an
  identity.
- **A summary is an object the caller records.** It is written by a model
  from the lines a node spans, so it is an observation, not a function of
  them, and never a cache entry: it belongs in a history beside the lines,
  keyed by the node's name, and two replicas that wrote two summaries of one
  node hold both. The digest reads only which nodes have one and how long,
  and `pending` names the settled nodes whose summary can be written now,
  a function of which exist, so summaries of different nodes commute. A
  node on the path to the last line is never offered: the next line renames
  it.
- **The view is a cache.** A view is a cut that tiles the log under a
  budget, a pure function of the tree as read under its lines' costs and its
  summaries (`digest::read`, a sound key) and the budget, so it is a
  tabulation by §6.2. It is built as the log was, line by line: while over
  budget, a CLOSING is made, settled nodes replacing the parts they span. A
  closing starts at one node and holds what DECAY forces beside it (the node
  at its level over each lower part before it, leftward), and it is
  admissible when all its nodes are summarised and every leftward prefix of
  it costs less than what it replaces. That is the one rule: a node closing
  over a child whose own summary would not lower the cost is the case with
  nothing forced, and a FORCED node whose summary would not lower the cost
  closes because the start, and each prefix up to it, makes up the
  difference; a start that would not lower the cost itself never closes on a
  forced node's account. Of the admissible closings, the one started at the
  most due node (age over size) is made. So a view never costs more than its
  lines and fits its budget unless no closing is admissible. It is GREEDY:
  decay forces nodes only at the start's level, so it does not seek a fit
  that needs a coarser node to the left, and may stop over its budget though
  one exists. A closed part is settled, never renamed and never reopened: an
  appended line only coarsens the past, and the printed prefix up to the
  first part it closes stays as a model's prompt cache saw it. Over ten
  thousand lines in a chain, an append keeps 95 to 98 percent of the printed
  view's bytes on average, and all of it in four appends of five or more
  (`core/examples/digest_prefix.rs`).
- **What waits is what to summarise.** A closing more due than the one
  made, or any when none can be, that lacks summaries puts in `waiting` the
  pending nodes at or under each node it lacks one for. A summariser that
  writes what `waiting` names, round after round, ends with the view in its
  budget or with nothing left that a summary could make lower. `zoom`
  opens a part, and the tree is lossless.
- **Decay is in levels, not lines.** A part is never at a lower level than
  a part after it, so a reader may infer that the past is shown at least as
  coarse as the present, level by level, and that a part at level `h` off
  the last path spans `2^h` to `16^h` lines. Not that an earlier part spans
  more lines than a later one: chunks hold two to sixteen, and decay in
  lines cannot hold beside the budget (an old line alone beside a sibling of
  three cannot close into a part of four without that part following one of
  a single line, and nothing older can close instead).

**What landed.** `prodrome::digest`, which, like `memo`, depends on the
hash type, `memo` and `topo` and nothing else of the crate; `memo::balance`
became the tree that only grows that it needed (every element at one depth,
every chunk off the last path holding two to sixteen, a closed run never
renamed), and `Dag`'s walk became `topo::linear`, which the digest orders
its lines by. SPEC law 46. No wasm export and no schema: the host that reads
a transcript through a digest is the first to say what a line and a summary
are as objects.

## 7. Laws

Each is a property test over generated histories, in the core's `tests/`:

1. **Free.** A reading is a function of the object set: every permutation,
   duplication and partition-then-merge of a history reads the same.
2. **Order.** Each value type's `≤` is reflexive, antisymmetric and
   transitive, over generated values.
3. **Completion.** A reading is the set of maximal values of the frontier's
   values; it is one value exactly when one is greatest.
4. **Inflationary.** For a register declared inflationary, an append whose
   value is not `≥` the reading it supersedes is refused, and
   `read(h₁ ∪ h₂) = read(h₁) ⊔ read(h₂)`.
5. **Valuation.** A conflict's price is `Least` over its candidates' prices.
6. **Reference.** Under the todo schema every existing vector reads
   unchanged (the successor of law 22).
7. **Sync.** `sync` is idempotent and order-free, an interrupted sync
   completes on the next, and a reading after `sync(a, b)` equals the
   reading of `a ∪ b`; an overlay reads as its union, and its flush leaves
   the replica of record reading that union.
8. **Memo.** Reading through a cache equals computing (`memo f = f` on
   generated arguments, with entries evicted at random between reads); a
   key's two values are reported, never chosen; a fold memoised at every
   node (`memoCata`) equals the plain fold, over generated trees of any
   shape; and a redraw's reported recomputes and hits are exactly the nodes
   whose names did and did not move.
9. **Next change.** For a behaviour that answers its next observed change at
   `t₁` after `t₀`, the observed value is constant on `[t₀, t₁)`; where the
   answer is exact it differs at `t₁`, and where it is conservative `t₁` is
   no later than the first instant it does. BUILT: SPEC law 38,
   `core/tests/all/observe.rs`.
10. **Nesting.** Over generated nested histories: `join ∘ unit = id` and
    `join ∘ fmap unit = id`; `join ∘ join = join ∘ fmap join`; reading the
    joined history at a path equals reading the inner history there; and a
    join restricted to chosen objects reads as the parent plus exactly those
    objects. BUILT: SPEC law 39, `core/tests/all/nest.rs` and
    `nest_histories.rs`.

11. **Declared.** A declared schema equivalent to a Rust one reads every
    generated history identically, register by register, and appends
    identically through a store; each declared order is a partial order;
    admission admits a lawful schema in its canonical print and refuses a
    generated broken one under the law it breaks. BUILT: SPEC law 40,
    `core/tests/all/declared.rs`.
12. **A set of prodromes.** Over generated sets of prodromes, each member
    a replica of its own genesis: a set's join reads each member at its
    genesis as it reads alone (disjointness); the set a synced replica
    holds is the union of the sets synced into it, associatively; a
    qualified reference reads, through the set, what its member alone
    prices its entity at, by genesis or by a declared name; one to a store
    or an entity the set lacks reads `∅`; an unqualified term links alike
    under any set; a view prices a qualified reference to a sibling
    prodrome as that prodrome's view does; and a proposal priced by the
    todo it serves moves
    exactly when that todo's history does. BUILT: SPEC law 41,
    `core/tests/all/sets.rs`.

13. **Signatures.** Over generated histories of writes, keys added and
    revoked by a root or by nobody, and honest, unregistered and forged
    signatures of any object: the proof is a function of the object set,
    through replicas that received the objects in any order or in two
    halves joined by sync; signatures change no reading under a policy that
    does not require them; under one that does, an unproven object claims
    and every other is as the wrapped policy says, in any arrival order;
    and a history signed throughout reads as the wrapped policy reads it.
    BUILT: SPEC law 42, `core/tests/all/signatures.rs`.

14. **Total readings.** What a set of prints leaves out of its history is
    a function of the prints, the same of the set and of its interior; a
    replica's every reading answers it beside what it read; and a store on
    disk, read cold or warm, holds exactly what a replica handed its files
    holds, its writes refusing exactly while a print is unread. BUILT: SPEC
    laws 44 and 45, `core/tests/all/down_set.rs`,
    `wasm/exports/src/unread.rs`, `core/tests/all/unread.rs`.

15. **A digest.** Over generated sets of lines and summaries: the tree is a
    function of the set; a view tiles the log, never costs more than its
    lines, fits its budget unless no admissible closing (with what decay
    forces beside it) remains, and decays in levels; what waits is pending,
    and following it ends in the budget or with nothing left to lower;
    zooming reads the log back; an appended line only coarsens the past and
    changes a bounded number of parts; the memoised measure is the plain one
    and `read` is a sound key.
    BUILT: SPEC law 46, `core/tests/all/digest.rs`.

## 8. Stages

In this repository, each one PR:

1. **Order and completion.** `Order` on values (a `Discrete<T>` default), the
   completion over `Frontier`, the inflationary refusal, laws 2 to 4. The
   todo registers are discrete; law 6 holds. BUILT: `fold::Order`,
   `fold::maximal` and `Frontier::read`, `fold::Inflationary` and
   `fold::grows` (called by the store from stage 2, when a schema has an
   inflationary register that supersedes); SPEC §6 and laws 30 to 32.
2. **Schema.** The trait (vocabulary, key, registers as a product, route,
   valuation), the todo schema as its reference instance, stores and folds
   generic over it, `view` over a schema with a valuation. Law 1 and law 6.
   BUILT: `schema::Schema`, implemented by the event type itself, so a
   schema's events are a closed sum and nothing else parses into one
   (`TodoEvent<P>` is the todo schema, in `todo`); an entity's registers as
   a `fold::Product` of `Frontier`s and grow-only sets, each read under a
   `fold::RegisterType`; a valuation, since narrowed to `schema::Price`, its
   price `Least` over a reading's candidates by construction (`fold::price`),
   beside `History`, `Bind` and `Row` for what only the todo has (§4);
   `EventStore`, `Dag`,
   `Envelope`, `Change`, the fold, `registers`, `verify`, `Policy` and
   `view::entries` generic over a schema, the append calling `grows` through
   `Product::grows`. A review schema in the tests, priced by a flat term per phase, runs
   law 1 beside the todo's and law 4 through the append (SPEC laws 32 and
   33); law 5 is SPEC law 34 over any `Price`, the review schema priced by a
   flat term per phase, and law 24 for the todo's entry; law 6 is law 22,
   every vector unchanged.
3. **The wasm.** The exports read a store at a schema. BUILT: the exports
   are generic over a schema (`prodrome-wasm-exports`, a library): `verify`, `tips`
   and each prodrome's heads, `since` a replica's tips, and `readings`, each
   register's maximal writes through `fold::Product::reading`, for any
   schema; `entries` and `prices` for one with a `Row`, a `History` and a
   `Bind`. A schema
   crosses the boundary by `Json` beside it (its key's field, its
   registers' names, its values' JSON, a function of the value) and, priced,
   `RowJson` (its row's reading's JSON); not a bound on `Schema`, since
   JSON is the boundary's and a schema read natively owes it nothing. A host
   says `prodrome_wasm_exports::schema!(Name = Schema)`, with `, priced`
   for those three, once per schema, each a JS class in ONE module, its own
   `cdylib` crate. `prodrome-wasm` is the reference module, built the same
   way: the todo schema at the reference payload, `Todos`, beside its old
   exports, byte for byte; the tests' review schema runs through the
   same macro over a small store.

In a host (the first one), after stage 2:

4. **The inbox as a proposal schema**, its machine ordered as in §3,
   inflationary, valued by what it serves; the existing objects migrated
   once, on a copy first, with every proposal's reading pinned.
5. **Conversations as a conversation schema** (a transcript as a grow-only
   set in causal order; owed as a reading).
6. **The whole client as a reading** (§6.0): every store a replica on the
   client, intents as one set, the ui (the route first) as registers, the
   clock as a source; pages as pure views with no loads; then the host's
   background work as rules over registers.

In this repository, after stage 3:

7. **Replicas.** `sync(from, to)` between any two stores at one schema, an
   in-memory store, and an OVERLAY that writes to memory and reads
   `memory ∪ disk`, with its flush. A store that answers a coordinated
   decision (a lock, a write-through append) is a type an overlay is not.
   Laws: sync is idempotent and order-free, an interrupted sync completes on
   the next, a reading after sync equals the reading of the union, and an
   overlay reads as its union. An agent's staging area is its first host
   use (a batch as an overlay over its target, accepted by a sync of the
   objects picked), replacing a copy of the target and a deletion.

   BUILT. First the in-memory replica behind a store: an
   `EventStore` holds every object it has read, each verified once, with
   their tips and their fold (`store::memory`), and each look at the disk
   replica is a sync into it of exactly the objects it lacks: a name
   listed and not held is read and verified, a held name no longer listed
   is forgotten, and a held file whose `stat` changed is read again. A
   file that is no object is held as unread, as a page holds a print it
   cannot parse (§6.1), and read again at every look. The
   fold follows by `registers::Folded::insert`, which puts an object where
   the linearisation of the union puts it, so the memory is the fold of
   the object set whatever order the objects arrive in, never rebuilt from
   the files. Laws, `core/tests/all/memory.rs`: a store read
   incrementally reads as the same objects read cold, and an append
   through the memory writes byte for byte what a cold one does.

   Then the replicas. `store::Replica` is what every store is: it reads
   (`Held`: its objects, their fold, its tips), appends, gives the print
   held under a name, and receives, verifying every object before it takes
   any, parents first (`memory::receive`, the walk every adoption shares).
   `store::sync(from, to)` is `to.receive(from.tips())`, one function for
   every pair. What an append writes is `Memory::change`, decided on what
   is held, so `store::MemoryStore`, objects and prints in memory, writes
   byte for byte what an `EventStore` holding the same objects writes.
   `store::Overlay` writes to memory and reads the base's memory (its
   objects and fold, shared) with its own objects admitted over it, taken
   again when the base changes; `Overlay::flush` is `sync(overlay, base)`.
   The coordinated decision is `store::Decision`, which only
   `EventStore::decide` makes: the lock held, the memory level with the
   directory, and an append written through. Two compile-fail doctests pin
   that an overlay and a store in memory have none. The wasm exports'
   classes gain `append(event, genesis)`, which seals an event into the
   page's replica through a `MemoryStore` and answers the object to send
   the host. Laws, `core/tests/all/replica.rs` (SPEC law 35): sync between
   disk and memory in all four pairings is idempotent and order-free,
   completes when cut short, and reads as the union; a store in memory
   appends as a disk store does over any history; an overlay reads as its
   union and its flush leaves the base reading it.
8. **Caches** (§6.2). A cache as a store of (key, value) pairs in the flat
   order per key, joined by union, a key's two values a finding; eviction
   by any policy, one bounded LRU shipped; and the memoised fold over a
   content-addressed tree of any shape, memoised at every node and
   reporting what hit and what was computed; a long sequence as a
   balanced, measured tree. Law 8.

   BUILT: `prodrome::memo` (§6.2's "What landed"). Laws,
   `core/tests/all/memo.rs`: a join is union (commutative, associative,
   idempotent), a key holding two values reads as a finding with both and
   never as either, an LRU cache holds its bound. `memo_fold.rs`, over
   lists, binary trees and rose trees and generated edits (a node
   relabelled, a subtree grafted): the memoised fold is the plain fold,
   two algebras sharing one cache, entries dropped at random between folds
   and under a small LRU; after an edit the fold computes exactly the spine
   from the edited subtrees to the root, each node once, and hits exactly at
   the top of each unchanged subtree beside it; with subtrees shared, it
   computes exactly the names it had not seen; two caches joined fold as
   either; a forged second value is reported and the node computed.
   `memo_balance.rs`: a balanced sequence folds to the sequence and its
   measures index it, its shape is its elements, and an edit computes a
   logarithmic number of nodes where a list computes all before it.

   BUILT LATER: a cache held in a file. `memo::Holder` is the one
   interface of the pairs, with `memo::Memory` and `memo::File` as its two
   interpreters, `Cache` as a holder under a policy, and `memo::Front` as
   a holder read through memory. `memo::fold` runs over any holder, and
   `Cache::forget` erases values from a file. SPEC §6 gives the file's
   layout, and SPEC laws 51 to 58 state the laws, in
   `core/tests/all/memo_file.rs` and `memo_entities.rs`.
9. **Nesting** (§6.3). A register whose value is a history held by its heads;
   `join` flattening a nest into one history keyed by path, `unit` and
   `bind`; a restricted join that accepts chosen objects (the shape an
   agent's batch, an overlay's flush and an admitted plugin store share);
   reading commuting with join. Law 10.

   BUILT: `prodrome::nest` and `store::accept` (§6.3's "What landed"). Laws,
   `core/tests/all/nest.rs`, over generated nests of two and three levels
   whose keys share prefixes: unit, associativity, the functor's laws and
   join's naturality, bind's laws and bind as `join ∘ fmap`, join only
   prefixing keys, reading at a path commuting with join, and the
   restricted join reading as the parent plus exactly the chosen values.
   `nest_histories.rs`, over histories held in registers of a test schema
   (`core/tests/schemas/holder.rs`, a shelf holding a history of any schema)
   at two levels over reviews and over todos and at three over shelves of
   reviews: the join holds every object of every replica once, under its
   name; reading it at a held register's path reads, through a replica that
   accepts exactly those objects, as the inner replica reads; a pointer
   reads the heads its last write saw, and two concurrent pointers the
   union; an outer write's past reaches an inner object only through heads
   it, or a write beneath it, named, and an inner object's past is inner; a
   pointer naming another level's object is missing; and the memoised fold
   of the join's algebra is the join, recomputing after a write two levels
   deep exactly the spine of three nests it moved. `replica.rs`: accepting a
   down-set of an overlay's objects leaves the store reading as one given
   exactly those, and the flush takes the rest.

10. **A schema as data** (§5, §6.0). A schema given at run time as text,
    admitted only when its laws hold, its events implementing the schema
    trait by interpreting it, so a store at it reads, appends, syncs, nests
    and memoises by the code a Rust schema's does; the wasm exports with a
    class over a schema given at construction. Law 11.

    BUILT: `prodrome::declared` (§5's "What landed"). The schema's vocabulary
    is a value a store is opened at (`EventStore::at`, `MemoryStore::at`,
    `Replica::schema`), and every parse takes it; `literal::Signature::Owned`
    lends a field order held as owned names. Laws, `core/tests/all/declared.rs`
    (SPEC law 40): the proposal schema written in Rust
    (`core/tests/schemas/proposal.rs`, design §3's machine beside a total, an
    inclusion and a discrete register) and as data print every generated
    event alike, so hold the same objects, and read every register alike at
    every moment; through stores every append is written, refused or
    answered as a twin alike, and a sync carries the declared store; law 1
    and law 2 hold over the declared schema; a generated lawful schema is
    admitted in its canonical print and a generated schema with one defect
    is refused under the law it breaks, for each of the eight.
    `nest_histories.rs` nests declared proposals under the shelf schema.
    The wasm: `schema!(Name, declared)`, `new Name(schema, objects)`, and
    the reference module's `Declared`.

11. **Sets of prodromes** (§6.3.1). A set as a nest keyed by genesis, its
    reading the product of its members', and the qualified reference that
    reads a member, resolved through the set a host passes. Law 12.

    BUILT: `nest::Set`, `fpl::Stores`, `fold::stores` and `fpl::link_in`
    (§6.3.1's "What landed"). Laws, `core/tests/all/sets.rs` (SPEC law 41).
12. **Signatures** (§6.4). A detached signature as an object, keys
    registered and revoked by objects, the proof a reading of them, and a
    policy that requires it, composed with any other; the wasm exports
    sign with a key the host holds and read what is proven. Law 12.

    BUILT: `prodrome::sign` and `policy::Proven` (§6.4's "What landed").
    Laws, `core/tests/all/signatures.rs` (SPEC law 42), and unit tests in
    `sign.rs` for each clause of the proof: a key proves for its actor only,
    a forged signature proves nothing and `verify` says so, a revocation
    keeps exactly what it rests on, and under roots a key counts only where
    a root signed it. `wasm/exports/src/review.rs`: a device signs what it
    wrote through the exports, and a policy requiring signatures reads it.

13. **A digest** (§6.5). A summary tree over a log of lines, which nodes
    can be summarised next, a budgeted view and zoom; law 15.

    BUILT: `prodrome::digest` (§6.5's "What landed").

## 9. Non-goals

- A last-writer-wins register. Time is data (§1): no value order is a clock.
- Operation-based types that need delivery order. Every type here reads a
  set; the DAG already carries causality.
- Merging values a type does not order. Incomparable concurrent values are a
  conflict, shown, and a later write that descends from both settles it
  (under an inflationary register, only a write at or above both, §6.0).
