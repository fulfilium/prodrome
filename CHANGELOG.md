# Changelog

All notable changes to this project are documented here. The format is
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) for its
API. THE STORED BYTES ARE A SEPARATE PROMISE and a stricter one: a shipped
constructor's fields never change, in any release.

## [Unreleased]

### Added

- **The viewer shows any public store named in its address, and several
  stores as one list (docs/design-viewer-stores.md).** A link names each
  store after `#/?`: a base URL under which `objects/` lies, or
  `gh:owner/repository[@ref][/dir]`, each with an optional label and an
  optional pinned genesis. The page reads a store's `index.json`, or
  GitHub's tree listing where there is none, and keeps only the objects
  that the core keeps. It names each object that it leaves out, and why.
  The list of several stores is the core's list over the union of their
  objects: each row names its store, items of two stores never merge, and
  an item that waits on an item of another loaded store reads that item's
  price and links to it. With no store in its address, the page shows the
  site's own store as before. The page sends no address to its own host.
  The page names every address above the list, caches only objects that
  the core checked, and bounds what one address costs
  (docs/design-viewer-stores.md, section 6). The sanitiser of typeset HTML
  also drops styles, forms, SVG animations, `name`, and every link of an
  SVG `a` that an HTML link may not have.
- **prodrome-wasm exports `link_in(term, specs, stores)`** (SPEC §7.2, law
  41). It links a term as `link` does, and binds each qualified reference
  in `stores`, the `prices` answer of a set of stores. No stored byte
  changes.

- **A store says when it is opened whether other programs may change it
  (SPEC §3, "The doors a store has"; laws 84 to 86).** `Doors::Shared`, the
  default, keeps the rule that a decision lists `objects/`. A store opened
  with `Doors::Alone` lets a decision trust the mark of the last hold, so
  an append on a handle that has read the store takes 5.8 ms in place of
  121 ms, and a post of 256 objects 30 ms in place of 150 ms, on a
  generated store of 50,000 objects. An object that arrives
  by a door such a store said it lacks is reported by `verify` and read by
  a fresh handle. A hold in which a write failed keeps no mark, so the next
  decision lists and removes the temps; the mark is drawn from the
  system's random source, so two forked processes never draw the same
  one; and a change of a handle's doors forgets its mark. The example
  `store_doors` measures a store opened either way. No stored byte
  changes.
- **The mark of the last hold lets a warm handle read without a listing
  (SPEC §3, "The mark of the last hold"; laws 40, 45 and 81).** Every hold
  of a store's lock writes a fresh random mark into `root/.writes`. A
  handle that has read the store reads that file, and lists `objects/`
  only when the mark is not the one it keeps. A read on such a handle of a
  store of 50,000 objects takes 0.08 ms in place of 130 ms. A change by a
  door that the store does not own is read at the next hold of the lock;
  after such a door, a host calls `record_heads()`. Every process that
  writes a store moves to this version together: an older binary holds the
  lock and writes no mark, so its writes are a door to a newer one. A new
  store's `.gitignore` also holds the line `/.writes`; the host of an older
  store adds that line, and `verify` reports a `.heads` or `.writes` that
  the `.gitignore` does not name (`Finding::Unignored`). No stored object
  changes.
- **A large batch of placements costs one sync of its bytes (SPEC §3, "A
  write is durable", laws 82 and 83).** A batch of 256 prints or more, on
  Linux 5.8 or later and on ext4, XFS, btrfs or ZFS, writes every temp
  file first, makes all of them durable with one `syncfs` through a handle
  opened before the first byte, renames them parents first, and syncs
  `objects/` once. A post of 2,560 objects takes 347 ms in place of
  1,180 ms on a generated store of 50,000. Every other batch syncs each
  file, as before. A staging or a sync that fails removes its temps, and
  the next hold of the lock removes the temps of a write that stopped. No
  stored object changes.
- **The file store reads, keeps and makes husks (SPEC §3, laws 71 to
  75, 77, 78 and 80).** A husk is a `Change` whose event has each sealed part
  replaced by the name of its key, kept under the name of the whole
  change. A schema that has sealed parts implements `store::Husking` and
  answers the new defaulted method `Schema::husk` with
  `HuskObject::of_other`. At such a schema, a print that does not hash to
  its name, from its keeper of record, reads as the husk that it holds or
  as tampered; at every other schema such a print is tampered, as before.
  `EventStore::husk(names)` replaces each named whole that holds a sealed
  part with its husk, under the store's lock, and never takes a whole
  back. `Dag::husks` names the husks that a DAG holds. `dag::decode` now
  takes a `store::Sender` and answers `store::Stored`, the whole or the
  husk. A husk is exactly one print: any other print that parses to the
  same object is tampered (law 78). The stored format gains the husk; no
  stored constructor changes, and `conformance/husk.py` holds the new
  vectors.
- **`store::Sender`, whose word a receipt takes for a husk.** A replica
  reads a print that does not hash to its name as a husk only from its
  keeper of record (`Sender::Keeper`); from a stranger
  (`Sender::Stranger`) such a print is tampered. The party that writes a
  store's directory speaks as its keeper (SPEC §3). The doors that take
  it are listed under "Changed".
- **`Secret::sign_whole`, which signs only an object held whole (law 80).**
  It refuses a name the history holds as a husk, or does not hold: a
  signature of a husk's name would prove a whole the signer never hashed.
  The wasm export `sign` signs through it.
- **`Proof::standing`, and `sign::Proved`.** A signature proves only the
  bytes that hash to its name. For a husk held under that name it vouches
  instead (§5, law 77): `standing` answers `Proven`, `Vouched` or
  `Unsigned`. `Proven` binds a husk that the proof vouches for, and
  `verify` does not report one as unsigned.
- **A run of appends (law 76).** `EventStore::append_all` and
  `Decision::append_all` decide a sequence of events over one look and
  place every new object at the end, with one directory sync, and stop at
  the first refusal with `Stopped`, which names the events placed before
  it. An append is a run of one, and so are the writes of `init`,
  `snapshot` and `merge` and the placement of a receipt. A placement that
  fails leaves the store's memory as a cold read of its files (law 79).
  10,000 appends as one run cost one look in place of 10,000.
- **`codec::assert_pinned`, a pin of a schema's stored names.** A test
  gives the constructors as text, one `Name(field, …)` on each line, and
  the function fails unless the declared constructors spell that text. The
  reference payload, `Heads`, the note schema, the two tag schemas and the
  test schemas of this repository pin their constructors with it.
- **A derived sum's variant can hold one record.** A variant whose one
  unnamed field is a record, as `Email(Draft)`, prints and reads as the
  record's call, with the record's constructor and fields. Two variants
  whose constructors have one name are refused when the program compiles.
  `codec::distinct` is the check.
- **`TodoId` is a `Literal`, as `Actor` is.** Its literal is its text, and
  a read checks the text with `TodoId::new`. A host's newtype over a
  `TodoId` derives its codec.
- **`#[literal(crate = path)]`, the path of the core in a derived impl.**
  A program that imports `prodrome-core` under another name gives that
  name, and needs no `extern crate` alias. The default is `::prodrome`.
  The option applies to a struct or an enum.
- **The script machine opens to its host (law 68).**
  `laws::script::run_with` reads the claimed reading under the host's
  policy, and calls the host's check after every step with each actor and
  what its replica holds, read-only, so that an application checks its own
  invariants on the same scripts. `laws::script::run` is `run_with` with
  the policy `Everything` and no check. `laws::Registers`, the reading that
  the laws compare, is public, and `laws::script::long_run` runs many
  scripts and returns their `Rate`, scripts and steps per second.
  `docs/script-law.md` shows the three with an example.

- **`#[derive(Literal)]`, in the new crate `prodrome-derive`.** The core
  re-exports it as `prodrome::literal::codec::Literal`. It writes the
  `Literal` impl of a struct with named fields (one constructor), an enum
  (one constructor for each variant) or a newtype, from the type's
  declaration. The options `rename`, `default`, `skip`, `with` and `new`
  keep a stored name, read a missing field as its default, leave a field
  out, give a field its own codec, and send the fields to a smart
  constructor. A struct that marks one field `#[literal(sum)]` has one
  constructor for each variant of that field's enum, with the struct's
  other fields first, as an event whose kinds share leading fields does.
  `Literal::CONSTRUCTORS` gives each derived type's
  constructors as data, so that a test can pin them. The reference payload
  and `Actor` derive their codec. Their prints and their reads, refusals
  and messages included, do not change, and no stored constructor changes.
  `docs/design-literal-derive.md` gives the routes that were weighed and
  the measurements.
- **`laws::script`, the script law (SPEC law 68).** A machine drives three
  replicas (in memory, an overlay, or on disk) through a random script of
  steps: appends with and without faults, partial and whole syncs in any
  order, refused receipts, key revocations (a key revoked more than once
  among them), reads at instants and through a cache, readings recorded at
  down-sets, snapshots, forgets and flushes. After every step it checks
  every invariant of law 68. It is generic over a schema, behind the
  `laws` feature, and `docs/script-law.md` shows an application how to run
  it. The core runs it over the todo schema and the tag set's two schemas,
  and a nightly run calls a longer run. `snapshot::attest` is the store's
  rule for what a snapshot attests, which `EventStore::snapshot` now calls.
  No stored constructor changes.

- **`prodrome-github` completes the roadmap items that a merged pull request
  names.** A pull request's description names each item that it completes
  in a line `Completes: <id>`. For a `pull_request` payload,
  `prodrome-github` checks that the store holds every named item. When the
  pull request has merged, it writes one `Completed` for each item, as
  `github-mirror`, at the instant of the merge, with the note `completed by
  pull request #N`, and a replay writes nothing. The new workflow
  `completes.yml` runs the check on every change to a pull request against
  `main`, and records the completions on every push to `main`, for the
  pushed commit's pull request and the recent merges. `pages.yml` publishes
  the viewer again when a `completes` run finishes. No stored constructor
  and no core API changes.

- **`Dag::heads`, the heads of a history derived from a DAG (SPEC law 65).**
  It answers the DAG's kept tips and the count of its objects. A host
  that places its own files derives its record of heads with it, and no
  longer builds a whole `Held` (with a fold over every object) to read
  the tips and the count. `Replica::heads` and the derived path of
  `EventStore::heads` call it, so the crate has one derivation. The
  WebAssembly fixture `core/tests/fixtures/dag-sizes` stays at 89,664
  bytes after `wasm-opt -Oz`.
- **`dag::Reach` and `Dag::reach`: a set of a DAG's objects as positions in
  its linear order (SPEC law 64).** A DAG keeps an index of its order
  beside the order: each object's parents as positions, and a search from
  a name to its position. A reach holds one bit for each object, so its
  closure, what lies outside it and its tips are walks over integers.
  `Frontiers::answer` and its records read a frontier's reach this way,
  for any token. On the new `frontier_pull` example (a generated DAG of
  30,000 objects, pulled in pieces of 1,000), a pull in the store's order
  takes about 80 to 110 ms in place of about 520 to 610 ms, and a pull in a
  host's own order with `record_with` takes about 75 ms in place of about
  840 to 1,050 ms. The answer to the last piece takes about 0.4 ms in place
  of about 21 ms. No answer's bytes change.
- **A store on disk records its heads: `EventStore::heads`,
  `EventStore::record_heads`, `Replica::heads` and `dag::Heads` (SPEC §3,
  laws 60 to 63).** The store keeps a file, `root/.heads`, that holds the
  tips of its history and the count of its objects. `heads()` reads that
  file, and one `stat` for each recorded tip, in place of a listing of
  `objects/`. A write removes the record, durably, before it places
  objects, and writes it once when it releases the lock, so no stop of a
  write leaves a served record that is not the derived heads. The record
  is derived and local, and the store writes a `.gitignore` with the line
  `/.heads` where the root has none. No object and no stored constructor
  changes. On the new `store_heads` example (a generated store of 30,000
  objects), `heads()` takes about 0.2 to 0.4 ms, and `tips()` takes about
  70 ms on a handle that has read the store and about 340 ms on a new
  handle in a new process. An append takes 66 to 73 ms, against 59 to
  71 ms before the change.
- **A cache held in a file: `memo::Holder`, `memo::File` and `memo::Front`
  (SPEC §6, laws 51 to 58).** `Holder` is the one interface of a cache's
  pairs. `memo::Memory` holds them in memory, and `File` holds them in one
  file, so a new process reads what an earlier process computed. `Cache`
  is a holder under an eviction policy, `Cache::held` puts a policy over
  any holder, and `Front` reads a holder through a cache in memory.
  `memo::fold` runs over any holder. `Cache::forget` drops a set of keys
  and erases their values from the file. SPEC §6 gives the layout of a
  cache file. A cache file is a new kind of file, and no object of a store
  changes. On the new `memo_file` example (a generated store of 40,000
  objects and a cache file of 5.6 MB), a new process opens the warm file
  in about 25 ms and folds the store's tree with one hit. Without a cache
  file, the same fold takes about 85 ms. After one object is appended, the
  fold computes 10 to 14 of the tree's 50,000 nodes.
- **`sign::Checks` and `Proof::through`, a table of signature checks
  (SPEC law 49).** `Checks` keeps whether each `Signed` object verifies,
  under the object's content name, at most its capacity of them, the least
  recently used dropped first. It computes that name from the `Signed`
  object it is given, so no caller can make a check of one object answer
  for another. `Proof::through(dag, registrar, &checks)`
  answers what `Proof::of` answers, and a history read again checks only
  the signatures that the table does not hold.
- **`digest::Tree::cover`, the nodes that tile a span of lines (SPEC law
  50).** Over any `Measure`, it answers the largest nodes that lie within
  the span, in order, or `None` for an empty span or one past the last
  line.

### Changed

- **The smart constructors refuse a span that does not fit in 64 bits of
  microseconds and an instant outside the years 1 to 9999 (SPEC §7, "The
  time axis").** `mk_shift`, `mk_within`, `mk_decay`, `mk_after` and
  `mk_periodic` refuse the span, and `mk_decay`, `mk_curve`, `mk_after`,
  `mk_recur`, `mk_periodic` and `mk_piecewise` refuse the instant. The
  literal reader already refused both, so no stored term changes. A term
  that these constructors refuse could not be printed. The constructors
  already returned a `Result`, so no signature changes. `us_of`, a hidden
  function, returns an `i128` where it returned an `i64`.
  `Schedule::shift` drops a knot that falls after the last instant of the
  axis, and turns a knot before the first instant into the head. The new
  public items are the constants `FIRST` and `LAST`, and the new hidden
  items are `advance`, `retreat` and `delta_of_us`.

- **`digest::view` takes two waters and the cost of a line (issue #104,
  SPEC laws 88 to 91).** The signature is
  `view(tree, line, summarized, waters)`. `line(node)` gives what a line
  costs in the view, and `Waters::new(high, low)` gives a high water and a
  low water at or under it. When a line's arrival puts the view over the
  high water, the view closes nodes until it costs at most the low water
  or no closing is admissible. Between two such merges, an arrival adds
  one part at the end of the view and changes no other part, so a reader
  that caches the view's text as a prefix keeps it. `digest::read` takes
  the same `line` argument, so that it stays a sound key. To keep the
  earlier behaviour, pass `Node::bytes` as `line` and `Waters::at(budget)`
  as the waters: `view(tree, Node::bytes, summarized, Waters::at(budget))`
  answers what `view(tree, summarized, budget)` answered, and
  `read(tree, Node::bytes, summarized)` the same name as
  `read(tree, summarized)`. No stored byte changes.
- **The readings of a schema's history are the crate `prodrome-reading`,
  and `prodrome-core` is a facade (issue #92, section 13 of
  `docs/design-core-face.md`).** A program that reads a todo's price and
  its row in a list can depend on `prodrome-data`, `prodrome-memo`,
  `prodrome-dag`, `prodrome-term`, `prodrome-fpl`, `prodrome-todo` and
  `prodrome-reading`: 63,808 tokens by `scripts/face.py --lines`, in place
  of the 114,465 tokens of `prodrome-core` and the crates below it. The
  new crate reads no store. `prodrome-core` holds only the law kit, behind
  the feature `laws`, and re-exports every other module at its old path,
  so a program that depends on `prodrome-core` changes nothing. No
  behaviour, stored byte, canonical print, conformance vector or law
  changes, and no item is renamed. Two hidden items are narrowed: `after`
  and `ratio` of `prodrome-term` became private. A program that moves to the
  new crate changes these paths:
  - `prodrome::fold` becomes `prodrome_reading::fold`, which holds
    `readings`, `env`, `specs`, `content`, `flatten`, `price`,
    `link_specs`, `member` and `stores`, and re-exports
    `prodrome_dag::fold` and `Kind`, `Registers` and `Write` of
    `prodrome_todo::todo`;
  - `prodrome::schema` becomes `prodrome_reading::schema`, which holds
    `Price`, `History`, `Bind` and `Row`, and re-exports
    `prodrome_data::schema`;
  - `prodrome::todo` becomes `prodrome_reading::todo`, which holds
    `Reading` and the impls of the four traits for `TodoEvent`, and
    re-exports `prodrome_todo::todo`;
  - `prodrome::view` becomes `prodrome_reading::view`;
  - `payload::spec_field` is `prodrome_todo::payload::spec_field`.

  `after` and `ratio`, which were public and `#[doc(hidden)]` in
  `prodrome_term::fpl`, are private to `prodrome-fpl`, because only the
  evaluator calls them.

- **The terms, the evaluator and the todo schema's events are crates of
  their own: `prodrome-term`, `prodrome-fpl` and `prodrome-todo` (issue
  #92, section 12 of `docs/design-core-face.md`).** A program that builds,
  prints and parses a term can depend on `prodrome-data` and
  `prodrome-term` alone: 26,827 tokens by `scripts/face.py --lines`, in
  place of the 113,095 tokens of `prodrome-core` and the crates below it.
  A program that also writes a todo's events adds `prodrome-todo` (31,870
  tokens), and a program that evaluates a term adds `prodrome-fpl` (32,532
  tokens with the data and term crates). Neither `prodrome-term` nor
  `prodrome-todo` depends on the evaluator. `prodrome-core` depends on the
  three crates and re-exports each moved item at its old path, so a
  program that depends on `prodrome-core` changes nothing. No behaviour,
  stored byte, canonical print, conformance vector or law changes, and no
  item is renamed or narrowed. A program that moves to the new crates
  changes these paths:
  - `prodrome::term` becomes `prodrome_term::term`, and
    `prodrome::schedule` becomes `prodrome_term::schedule`;
  - the smart constructors (`mk_flat`, `mk_decay`, `mk_curve`, `mk_conj`
    and the other `mk_*`, `least_of`, `checklist`), the spans (`Delta`,
    `div_delta`, `total_seconds`, `delta_from_hours`, `hours`),
    `print_term`, `parse_term`, `PRIORITY_POWER`, and `Env`, `Outcome` and
    `Candidates` move from `prodrome::fpl` to `prodrome_term::fpl`, which
    also re-exports `Instant`, `instant_of`, `datetime_of`, `iso`,
    `parse_iso` and `FplError` of `prodrome_data::instant`;
  - the rest of `prodrome::fpl` (the evaluator: `Closed`, `fulfillment`,
    `explained`, `link`, `link_in`, `Stores` and the others),
    `prodrome::chain`, `prodrome::breaks` and `prodrome::observe` become
    `prodrome_fpl::fpl`, `prodrome_fpl::chain`, `prodrome_fpl::breaks` and
    `prodrome_fpl::observe`. `prodrome_fpl::fpl` re-exports
    `prodrome_term::fpl`, so it holds every item of `prodrome::fpl`;
  - `prodrome::event`, `prodrome::payload` and `prodrome::reference`
    become `prodrome_todo::event`, `prodrome_todo::payload` and
    `prodrome_todo::reference`. The feature `reference` of `prodrome-todo`
    is on by default, and the feature `reference` of `prodrome-core` turns
    it on;
  - `TodoVocabulary`, `TODO_SIGNATURES`, `State`, `Spec` and `Content` of
    `prodrome::todo`, and `Registers`, `Kind` and `Write` of
    `prodrome::fold`, become items of `prodrome_todo::todo`, because the
    impl of `Schema` for `TodoEvent` must be in the crate of `TodoEvent`.
    `todo::Reading`, the readings of a todo and the impls of `Price`,
    `History`, `Row` and `Bind` stay in `prodrome-core`;
  - `prodrome::nest::Segment` is also `prodrome_data::segment::Segment`,
    because `mk_ref_in` checks a name by its rule. `prodrome_nest`
    re-exports it.

  The core no longer uses `chrono` in its library, so `chrono` is a
  dev-dependency of `prodrome-core`. So `prodrome-core` no longer turns on
  chrono's feature `clock` for a program that depends on it. A program
  that calls `chrono::Utc::now` or `chrono::Local::now`, and got the
  feature only through the core, turns it on in its own dependency on
  `chrono`.

  Two items that were private to the core are public and
  `#[doc(hidden)]` in `prodrome-term`, because `prodrome-fpl` and the
  readings call them and `pub(crate)` does not reach across crates:
  `prodrome_term::fpl::piecewise`, which puts a schedule of terms in normal
  form, and `prodrome_term::fpl::us_of`, a span in whole microseconds.
  Their docs start with "For the evaluator" and carry no promise between
  versions.

- **The store, the tag sets, the nests and the digest are crates of their
  own: `prodrome-store`, `prodrome-tags`, `prodrome-nest` and
  `prodrome-digest` (issue #92, section 11 of
  `docs/design-core-face.md`).** A program that opens a store, appends to
  it and reads it under a policy can depend on `prodrome-data`,
  `prodrome-memo`, `prodrome-dag` and `prodrome-store` alone: 63,091
  tokens by `scripts/face.py --lines`, in place of the 112,003 tokens of
  `prodrome-core` and the crates below it. `prodrome-core` depends on the
  four crates and re-exports each moved item at its old path, so a
  program that depends on `prodrome-core` changes nothing. No behaviour,
  stored byte, conformance vector or law changes, and no item is renamed
  or narrowed. A program that moves to the new crates changes these
  paths:
  - `prodrome::store` becomes `prodrome_store::store`, and
    `prodrome::snapshot` becomes `prodrome_store::snapshot`, which holds
    `attest` and `Attestation` and re-exports `prodrome_data::snapshot`.
    The crate depends on `prodrome-data` and `prodrome-dag`. The husk
    types and `Sender`, which `prodrome_store::store` re-exports, are in
    `prodrome_data::husk`;
  - `prodrome::tags` becomes `prodrome_tags`, whole, on `prodrome-data`
    and `prodrome-dag`;
  - `prodrome::nest` becomes `prodrome_nest`, whole, on `prodrome-data`,
    `prodrome-memo` and `prodrome-dag`;
  - `prodrome::digest` becomes `prodrome_digest`, whole, on
    `prodrome-data` and `prodrome-memo`;
  - `prodrome::event::Lifecycle` becomes `prodrome_data::event::Lifecycle`,
    because it holds only data. The other records of the todo schema stay
    in `prodrome-core`.

  The unit tests and the doc tests of the store drive it with the todo
  schema, so `prodrome-store` has `prodrome-core` as a dev-dependency.

- **The caches, the readings and the declared schemas of a store are
  their own crates, `prodrome-memo`, `prodrome-dag` and
  `prodrome-declared` (issue #92, section 10 of
  `docs/design-core-face.md`).** A program that reads the registers of an
  entity from its events under a policy can depend on `prodrome-data`,
  `prodrome-memo` and `prodrome-dag` alone. Their faces are 19,654, 7,697
  and 14,424 tokens by `scripts/face.py --lines`, 41,775 in all, in place
  of the 92,204 tokens of `prodrome-core` and the 19,231 of
  `prodrome-data` that such a program read before. `prodrome-core`
  depends on the three crates and re-exports each moved item at its old
  path, so a program that depends on `prodrome-core` changes nothing. No
  behaviour, stored byte, conformance vector or law changes, and no item
  is renamed or narrowed. A program that moves to the new crates changes
  these paths:
  - `prodrome::memo` becomes `prodrome_memo`, whole. The crate depends on
    `prodrome-data` alone;
  - `prodrome::declared` becomes `prodrome_declared`, whole. The crate
    depends on `prodrome-data` alone. A store reads a declaration through
    `EventStore::at`, which stays in `prodrome-core`;
  - `prodrome::dag`, `prodrome::registers` and `prodrome::sign` become
    `prodrome_dag::dag`, `prodrome_dag::registers` and
    `prodrome_dag::sign`. `prodrome_dag::sign` re-exports the object types
    of `prodrome_data::sign`, and `prodrome_dag::registers` re-exports
    `prodrome_data::stamp::Stamp`;
  - `prodrome::fold::read` becomes `prodrome_dag::fold::read`.
    `prodrome_dag::fold` re-exports the register algebra of
    `prodrome_data::fold`, with `Product` and `fold::sole`. The readings of
    a todo, such as `readings`, `env` and `flatten`, stay in
    `prodrome-core`;
  - `prodrome::policy::{Policy, Proven}` become `prodrome_dag::policy::…`.
    `Standing`, `Everything` and `Untrusted` become
    `prodrome_data::policy::…`, because they name only an `Actor`, and
    `prodrome_dag::policy` re-exports them. The trait `Policy` names a
    `Dag` in `Policy::at`, so it is in `prodrome-dag`, with its impls for
    the three types.

  The items of `prodrome-dag` that only `prodrome-core` uses are public
  and `#[doc(hidden)]`, with no promise between versions, and
  `prodrome-core` requires the exact version of each new crate. Of these,
  `Dag::husk` lets the kept tips and order go where a husk rests on other
  names than the object it replaces, `Folded::rewrite` passes over a stamp
  whose instant differs, and `Reach::insert` passes over a position past
  the order. The doc comments of `prodrome-declared` are shorter by the
  rule of stage 1, and its module example is the test
  `declared/tests/admit.rs`.
- **The conversions between a stored `Datetime` and an instant are in
  `prodrome-data` (issue #92, section 9 of `docs/design-core-face.md`).**
  The type `Instant`, the functions `instant_of`, `datetime_of`, `iso` and
  `parse_iso`, and the error `FplError`, which `parse_iso` returns, are in
  the new module `prodrome_data::instant`. `prodrome-core` re-exports each
  of them at its old path in `prodrome::fpl`, so a program that depends on
  `prodrome-core` changes nothing. A program that reads and writes the
  instants of events can depend on `prodrome-data` alone, and changes
  `prodrome::fpl::` to `prodrome_data::instant::` in these paths. The doc
  comments of `prodrome-data` are shorter, by the rule of stage 1, and its
  root doc lists what a program needs to build, stamp, print, parse and
  hash events. The face of `prodrome-data` is 19,231 tokens by
  `scripts/face.py --lines data`, in place of 22,291, and 11,669 tokens by
  the rustdoc measure, in place of 14,083. This change narrows no item. No
  behaviour, stored byte, conformance vector or law changes.
- **The data of a store is its own crate, `prodrome-data` (issue #92,
  stage 3 of `docs/design-core-face.md`).** A program that only builds,
  prints, parses and hashes events can depend on `prodrome-data` alone.
  Its public face is 22,260 tokens by `scripts/face.py --lines data`, and
  14,083 tokens by the rustdoc measure, in place of the 113,539 and
  52,026 tokens of `prodrome-core` that such a program read before. It
  depends on chrono, ed25519-dalek, memchr, sha2, thiserror and
  prodrome-derive. `prodrome-core` depends on it and re-exports each moved
  item at its old path, so a program that depends on `prodrome-core`
  changes nothing, except for the four items in the last item of this
  list. No behaviour, stored byte, conformance vector or law changes.
  A program that moves to `prodrome-data` changes `prodrome::` to
  `prodrome_data::` in these paths:
  - `literal`, `genesis`, `change` and `topo`, whole;
  - `event`: the names (`Hash`, `TodoId`, `Actor`, `Name`), `Envelope`,
    the envelope constructors, `ENVELOPE_SIGNATURES`, `EventVocabulary`,
    and the prints, parses and hashes. The todo schema's events
    (`TodoEvent`, `Created`, `Lifecycle`, `SpecRevised`, `Authored` and
    their `mk_*`) stay in `prodrome-core`;
  - `payload`: the field reads, such as `string_field` and `required`.
    `Payload`, `RECORD_PREFIX`, `record_signature` and `spec_field` stay;
  - `schema`: `Schema` and `schema::headed`. `Price`, `History`, `Bind`
    and `Row` stay;
  - `snapshot`: `Snapshot` and `mk_snapshot`. `attest` and `Attestation`
    stay;
  - `sign`: `PublicKey`, `Signature`, `Signed`, `KeyAdded`, `KeyRevoked`
    and `mk_key_revoked`. `Secret`, `Registrar`, `Proved`, `Proof` and
    `Checks` stay;
  - `fold`: the register algebra (`Product`, `Register`, `RegisterType`,
    `GrowSet`, `Order`, `Discrete`, `Inflationary`, `Total`, `grows`,
    `maximal`, `Frontier`, `Climb` and `fold::sole`). The folds and the
    readings stay;
  - `store::{Hashed, Husk, HuskObject, Husking, NotAHusk, Other, Sender,
    Stored}` become `prodrome_data::husk::…`, and `registers::Stamp`
    becomes `prodrome_data::stamp::Stamp`.

  `#[derive(Literal)]` still writes `::prodrome::…` paths. A program with
  `prodrome-data` alone writes `#[literal(crate = prodrome_data)]` on each
  derived type.

  Four items that only the library uses leave the public face, with no
  deprecation, because the version is below 1.0. `literal::print_float`
  is no longer public; `print_literal` prints a `Value::Float`. The
  macro `newtype_str!` is `#[doc(hidden)]` at
  `prodrome_data::newtype_str!`, and `prodrome-core` no longer exports it.
  `BitSet` is `#[doc(hidden)]` at `prodrome_data::bitset::BitSet`, and
  `prodrome-core` no longer exports it at `registers::BitSet`. `topo::Topo`
  is `#[doc(hidden)]`. Nineteen other items that only `prodrome-core` uses
  are public in `prodrome-data` and `#[doc(hidden)]`, because `pub(crate)`
  does not reach across crates; `docs/design-core-face.md`, section 8,
  lists them. A program can also reach a hidden item through a path of
  the facade, such as `prodrome::event::rests_on`. A hidden item carries
  no promise at either path, and `prodrome-core` requires the exact
  version of `prodrome-data`.
- **The viewer shows every item's text as written, and runs none of it as
  Typst.** An item's text comes from whoever can write to the store, and
  the issue mirror writes each issue's title, which anyone may write. No
  store can say yet who may write markup.
- **The doc comments of the core's public items are shorter (issue
  #92).** Each doc comment now states the item's contract in one
  sentence, keeps the invariants, hazards, outside facts and law numbers
  that the code does not show, and cites the section of `SPEC.md` for the
  rest. The public face of `prodrome-core` is the signatures and doc
  comments of its public items. Measured with `scripts/face.py`, it falls
  from 67,722 to 51,886 tokens, and in the line measure of
  `scripts/face.py --lines` from 131,100 to 113,381 tokens. No code,
  name, signature or stored byte changes. `docs/design-core-face.md`
  proposes the next stages: narrowing what is public, and dividing the
  core into crates.
- **Every door that takes prints from another party takes a `Sender`
  (SPEC §3).** A host whose schema has no husks passes
  `Sender::Stranger`, and reads as before:
  - `Replica::receive(seeds, print_of)` is now
    `receive(seeds, print_of, sender)`; an implementation of `Replica`
    takes the argument too.
  - `accept(from, to, chosen)` is now `accept(from, to, chosen, sender)`.
  - `sync(from, to)` is now `sync(from, to, sender)`.
  - `EventStore::adopt(source, digest)` is now
    `adopt(source, digest, sender)`.
  - `EventStore::adopt_objects(objects, digest)` is now
    `adopt_objects(objects, digest, sender)`.
  - `Dag::from_prints(schema, prints)` is now
    `from_prints(schema, prints, sender)`.
  - `dag::decode(schema, name, bytes)` is now
    `decode(schema, name, bytes, sender)`, and answers `store::Stored`
    (`Whole` or `Husk`) in place of the `Envelope`; `Stored::into_object`
    gives the object.
  - The wasm exports' `Replica::of`, `Replica::at` and
    `Replica::declared` take the sender as their last argument. A class's
    JS constructor reads as a stranger's, and `fromKeeper` reads as the
    keeper's. A `verify` row gains `husk`.
- **`Replica::kept` replaces `Replica::print` as the required method.**
  `kept(name)` answers the print a replica keeps under a name, the whole
  or its husk, and `accept` sends it. `print` is now a provided method
  that answers only bytes whose hash is the name. A host that implements
  `Replica` renames its `print` to `kept`; with no husks the two answer
  the same.
- **`Fields` refuses in the words of a derived read (SPEC law 69).**
  `Fields::of`, `Fields::among` and `Fields::get` are built on
  `codec::call`, `codec::call_of` and `codec::read`, the reads that a
  derived impl makes, so a hand-written read and a derived one word each
  refusal alike. `Fields::get` reads a field with `Literal::from_field`.
  `no_arm(sum, name)` takes the name of the sum's type and words its
  refusal as a derived sum does: `Reject is not one of Verdict's
  constructors`. `Fields::among` takes the name of the sum too, and
  `schema::headed::Body` gains `NAME`, the name of the body's type, which
  `Event::from_value` passes to it. What each read accepts does not change:
  a property compares the old `Fields` and the new one, and four readers of
  this repository as they were and as they are, over generated values and
  changed ones. Only the messages change, and no stored form changes.
  Every refusal of a field names the field: the default
  `Literal::from_field` leads a refusal of `from_value` with the field's
  context (`Heads.count: expected a natural number …`), and so does a
  derived newtype for its smart constructor's refusal. A refusal inside a
  nested record reads as a path (`Thread.first: Review.remark must be a
  string, got …`). `ProdromeError::within` leads a refusal with a context
  and keeps its kind. `dag::Heads` derives its codec. A breaking change to `Fields::among`,
  `no_arm` and `Body`.
- **An inflationary register reads the greatest of all its writes (SPEC
  §6, law 32).** The fold read every register from its frontier, so a
  write below the reading, which the append refuses and a receipt takes as
  a change whose deps name the higher write, lowered the reading. An
  inflationary register is now held by `fold::Climb`, which keeps its
  frontier and the writes that no other write outranks, and its reading is
  the maximal values of all its writes. The reading is a function of the
  object set, and it never falls as objects arrive. `receive` still takes
  every valid object, and `append` still refuses a write below its
  reading. In a history that appends wrote the reading is the frontier's,
  write for write, so no honest reading changes, and no stored byte and no
  conformance vector changes. `verify` reports a write below what it rests
  on (`Finding::Below`). A host's fixtures that were built by hand, as
  changes with chosen deps, can show this finding at their next `verify`:
  the review fixture of the wasm exports wrote `Closed` over `Review`, which
  the review order does not allow, and its close now rests on nothing.
  `verify` checks each write against the product of its nearest writes
  beneath it, so a chain costs one join for each write, and a schema with no
  inflationary register (`Product::climbing` is empty) costs nothing. `Product::frontier_mut` becomes
  `Product::join_to`, `Product::join` gets a default, `Product::climbed`
  states law 32 between two readings, `Frontier::grows` moves to
  `Climb::grows`, and `fold::sole::Sole` chooses how its register is held
  (`Sole::Held`, `fold::sole::Hold`) instead of overriding `Sole::grows`.
  The script law (law 68) writes a write below its reading as a fault
  (`laws::script::Writing::Below`), checks that no reading of an
  inflationary register falls, and runs over the proposal schema.
  `docs/design-inflationary-reading.md` gives the reasons and the costs.
- **`Payload` requires `Record`, and loses `KIND`, `FIELDS`, `fields` and
  `from_fields`.** `Record::CONSTRUCTOR` gives the record kind's name and
  fields, `Record::values` the values, and `Record::from_call` the read. A
  host derives `Literal` for its payload to get all four. `Literal` gains
  `from_field`, a read that names the field's context, and the payload
  helpers (`string_field` and the others) are reads of it. `mk_note`,
  `mk_subtodo` and `mk_todo` take any string type for their text. No print
  and no refusal message changes.
- **The printable set of a string's print is the one of Unicode 16.0.0, in
  every build (SPEC §2, law 67).** This is a clarification, and it changes
  the name of no valid object. The printer writes a printable character
  outside ASCII as itself and any other one as an escape, so the set
  decides the print and the name of each object that holds a string. The
  printer asked the crate `unicode-general-category` at any version 1.x
  for each category, so a version with the data of a later Unicode would
  print a newly assigned character as itself, and would give the object
  another name. The set is now a table in the core,
  `literal::printable`, that `scripts/printable.py` generates from the
  `UnicodeData.txt` of Unicode 16.0.0. SPEC §2 names the version and
  states the SHA-256 of the set. A test compares the table with version
  1.1.0 of the crate for every character, and names each code point that
  differs. The function `literal::is_printable` moves to
  `literal::printable::is_printable`. Unicode 18.0.0 assigns 17,810 code
  points that are unassigned in 16.0.0, and each of them prints as an
  escape. The WebAssembly module has a new export `printable`, which
  answers the Unicode version and the digest. The module
  `prodrome_bg.wasm` falls from 1,418,987 to 1,375,397 bytes, because the
  table holds only the printable set and not every category. No stored
  constructor changes.

- **`verify` finds a redundant dep with one walk over the linear order
  (SPEC law 66).** The rule reports a dep of a change that another dep of
  the same change rests on. It built the closure of each dep's parents, so
  a store of long histories cost time that grows with the square of a
  history's length. Where the DAG has a linear order, a change with one
  dep now costs no walk, and a change with more deps costs one walk down
  the positions of the order, from the deps to the least of them. Where
  the DAG has no linear order (a missing parent or a cycle), the rule
  builds the closures as before. The findings do not change for any
  store. On the new `verify_cost` example, a generated chain of 40,000
  changes verifies in about 170 ms in place of about 930 s.
- **`Reached::Known` holds a `&Reach`, `Reached` has a lifetime, and
  `Lacking::reached` returns it by value.** A host that read the reach as
  a `BTreeSet<Hash>` reads it through `Reach::len`, `Reach::contains` and
  `Reach::iter`, or collects `iter()`. The reach borrows the DAG that the
  answer reads, as `Lacking` already does.
- **The facts that a `Dag` keeps are compiled once, not once for each
  schema.** The tips, the linear order and the order's index are functions
  of the names and their parents alone, so their code no longer depends on
  the schema. On the new fixture `core/tests/fixtures/dag-sizes` (a
  program that uses `Dag` at three schemas, built for
  `wasm32-unknown-unknown` and run through `wasm-opt -Oz`), the module is
  89,664 bytes, against 103,838 bytes before the linear order was kept and
  105,009 bytes after.
- **`verify` reports a stale record of heads (`Finding::StaleHeads`).** A
  host that merges stores by their files (a union or copy of object files,
  a same-name move from a staging store, or a `git merge` or checkout of
  the directory) changes `objects/` without the store. After such a step,
  `verify` reports that the record of heads is older than the objects,
  until a write, a `decide`, a call of `EventStore::record_heads` or a
  deletion of `.heads`. A host that never calls `heads()` needs nothing
  else for correctness, because `tips()` and every decision derive. A host
  that expects a clean `verify` after such a step calls `record_heads` or
  deletes `.heads`. `Decision` now writes the record when it is dropped.
- **A DAG keeps its linear order, and `Dag::linearise` returns a borrowed
  slice (SPEC law 59).** A `Dag` derives its order on the first ask and
  keeps it until its objects change, as it keeps its tips.
  `Dag::linearise` returns `Result<&[Hash], ProdromeError>`, and a caller
  that needs to own the list calls `to_vec`. So a second answer of
  `Frontiers::answer` on a DAG that did not change orders no object, and
  `Dag::nodes`, `Dag::verify` and `EventStore::snapshot` read the kept
  order too. The kept order costs about 120 bytes for each object. On the
  new `frontier_answer` example (a generated DAG of 30,000 objects), a
  second answer from nothing takes about 3.6 ms instead of 32 ms. No
  stored byte and no answer changes.
- **`memo::Lookup` holds owned values, and `Cache::keys`, `Cache::pairs`,
  `Cache::findings` and `Cache::join` work over any holder.** A value read
  from a file is decoded, so a lookup owns it: `Lookup::Hit(V)` and
  `Lookup::Finding(Vec<V>)`. `Cache::join` takes `&mut impl Holder<V>`,
  `Cache::pairs` and `Cache::findings` take `&mut self` and answer a `Vec`,
  and `Cache::keys` answers an iterator of owned keys.
  `Cache<V, E>` is `Cache<V, E, Memory<V>>`, so code that names a cache in
  memory does not change. `Evict` has a new method `learn`, with a default.
- **The check of the examples' pictures compares pixels, not bytes.**
  `checks.prodrome-typst` compared each committed picture of an example
  with a fresh render byte for byte. The rasteriser does not give the same
  bytes on every machine: two builds of the one derivation differed in a
  few pixels (a normalised RMSE of 0.0004), so the check failed on any
  machine that rendered again and passed only where the binary cache held
  the first render. It now passes when the RMSE between the two pictures
  is at most 0.002, which three wholly changed pixels exceed, and fails
  when the two have different sizes.
- **`digest::view` no longer takes time quadratic in the log's length.**
  The view tries a closing with two queries of a segment tree that it keeps
  for each level of the tree. A segment tree is a binary tree over a
  sequence that answers a range's sum and its largest prefix sum in
  logarithmic time. The view keeps the nodes that may close and are not yet
  refused, so it does not scan every open node after each line. The view's
  answer is unchanged, and the new law `the_view_is_the_reference_view`
  checks it against a copy of the earlier view. On the new `digest_view`
  example (15,900 lines, a 16 KiB budget), one view at 3,000 of 3,938
  summaries goes from about 2.9 s to about 6 ms. With every node
  summarised, it goes from about 4.3 ms to about 5.5 ms.
- **A cold read of a store is several times faster.** On the new
  `store_read` example (23,901 objects, 151 MB), a cold `EventStore::dag`
  goes from about 2.95 s to about 0.69 s on four cores, from 3.07 s to
  1.32 s on two, and from 3.14 s to 1.79 s on one, on an x86 machine
  without the SHA instructions. The literal parser reads bytes and copies
  a string's text between escapes in one piece, with `memchr`, a new
  dependency of the core: it parses about 3x faster on one thread. A store
  reads, hashes and decodes its files, and takes their metadata, on every
  core that `std::thread::available_parallelism` reports, and on one
  thread where it reports none, as in the browser. The history is built
  over places instead of names, a memory that held nothing derives its
  tips in one pass, and an object name (`Hash`) compares by the 32 bytes
  of its digest, held in place beside its text. Every object reads to the
  same value and the same name, and every refusal has the same words: the
  tests of `literal_parser.rs` compare the parser with a frozen copy of
  the one it replaced, and a new law in `events.rs` checks that a name
  orders as its text does. Without the SHA instructions, the SHA-256 of
  every file bounds the read: reading and hashing the files alone takes
  about 0.40 s on four cores of that machine.
- **A pull that lacks nothing walks no object (SPEC law 47 (d)).** When
  the tips recorded under a pull's token are the DAG's own tips,
  `store::frontier::Frontiers::answer` answers the new
  `Reached::Everything`: no closure is built, the objects are not put in
  their linear order, and `Lacking::record` records the same tips with the
  count of every object. Before, each such pull walked and ordered the
  whole store. `Dag::tips` is derived on its first ask and kept until the
  DAG's objects change, so a store of record that keeps its DAG answers it
  without a pass over the objects. A breaking change to `Reached`. Test
  `frontier.rs::a_token_at_the_store_s_tips_reaches_everything_and_lacks_nothing`,
  which also asks the same token over a DAG with one more object and is
  answered `Known`; the law's model accepts `Everything` exactly where the
  token's closure is every object.

### Fixed

- **The evaluator no longer stops on a time shift past the calendar
  (SPEC §7, "The time axis"; law 92).** Evaluation panicked where it added
  a span to an instant and the sum left the range of the calendar type. A
  `Shift` of 100 million days did this, and so did a long `Within` window,
  a `Periodic` period, a `Decay` lead-up, and the search of `next_change`.
  A program that folded a store that held such a term stopped. Now every
  instant that evaluation computes is on the time axis, and a sum that
  would leave the axis stops at the end that it passes. A reading inside
  the axis keeps its value, its print and its vectors. The same change
  removes three more panics: the count of microseconds of a span of more
  than 292,000 years, the rounding of `div_delta` for a divisor of more
  than 2⁶², and `delta_from_hours` for an hour count of more than 2⁶³
  microseconds.

- **A list shows a todo that content records alone make (SPEC §6.7, law
  87).** A todo whose first event is an `Authored` record, with no
  `Created`, was missing from the open list of `prodrome list` and of the
  viewer, although its own page read it as open. Both lists asked for the
  todo's first `Created`. A §6.7 row now carries `begun`, the least instant
  of the events that name its todo, of any kind, and `Entry::exists_at(t)`
  is the one test of existence. The new field is a breaking change for code
  that builds a `view::Entry`. The command line's `list` and `show`, and
  the viewer's list, read it. The viewer's closed list keeps the same rows
  that exist. prodrome-wasm's row (`entries`, and `Todos.entries`) gains
  the key `begun`; the `created` map keeps its meaning and serves only as
  the body of a todo with no record. No stored byte changes.
- **The pull request checks verify the roadmap store after a write.** The
  `.gitignore` finding of `verify` (law 81) needs a `.writes` file, which a
  fresh checkout lacks, so the check of each pull request passed while the
  issue mirror, which writes first, failed. The check now verifies the
  `roadmap-data` store as it is checked out, and again after a copy of it
  takes a write. A run of the issue mirror, the pricing step or the
  completions that fails says so in a comment on its issue or pull request.

- **The append asks the schema's refusal of an entity's first write (SPEC
  §3, law 70).** `registers::deps_for` returned no deps for an entity that
  the writer held no write of, before it asked `Product::grows`. So an
  append took a first write that the schema's refusal refuses on the empty
  reading, and `verify` then found the write `Below`. A schema whose
  refusal is its order admits every value on the empty reading, so no
  schema of this repository showed the fault. `deps_for` now reads the
  entity's stream, the empty stream for an entity with no write, and asks
  `grows` of it in every case. Every append goes through `deps_for`: a
  store's, an overlay's, a store in memory's, the wasm exports' and
  `prodrome-github`'s. `Folded::stream` answers the stream of one entity,
  and five readers that looked it up by hand use it. A receipt still takes
  such a write, and `verify` still reports it. Law 70 states that the
  append refuses exactly what `verify` finds. The floored review schema of
  the tests (`core/tests/schemas/floored.rs`) refuses a merge that does
  not stand on a review, and the law's tests and the script machine (law
  68) run over it. No stored form changes.

### Added

- **The wasm exports read a schema whose key has several fields.** A row
  names its entity by every field of its key, each under its name, so
  `schema!` takes a schema such as `tags::membership`, whose key is a pair
  of a target and a tag. `Json::key_field` is now `Json::key_fields`, and
  the new `Json::key_values` gives the key's values as JSON; the trait no
  longer asks that a key be a string. The exports implement `Json` for
  the two `tags` schemas: a membership row names its target and its tag,
  and reads its `presence` as `"on"` or `"off"`, and an alias row reads
  its `alias` as the tag the name stands for, or `null`.
  `tags::membership::Membership::presence` is public again. The JSON of
  every other schema does not change. A breaking change to `Json` under
  this file's rule, for the exports crate alone. Tests in
  `wasm/exports/src/tags.rs`, over reviews tagged with labels.
- **The law kit (`prodrome::laws`), behind the `laws` feature, which is off
  by default.** The laws of SPEC §9 that ask nothing of a schema are
  functions an application runs over its own schemas and stores: `printed`
  (law 1: every print parses, prints back byte for byte and hashes to its
  name), `free`, `routed` and `valued` (law 33: a reading is a function of
  the object set, folded in any parents-first order a `Shuffle` gives, with
  objects folded twice, and split into two replicas; a write folds to its
  entity and joins exactly the registers it names; a register type values
  exactly those writes), and `synced` (law 35's join clauses, between
  replicas of any two kinds: a cut sync takes nothing, a down-set then the
  whole reads as the union, a second sync takes nothing, and either order
  into a third replica reads the same). `history` builds a history,
  `holding` a store in memory that holds one, `a_shuffle` draws a
  `Shuffle`, and `Shuffle::halves` splits a history into two replicas.
  Each law answers `Ok` or the law it found `Broken`, naming the first
  entity and register that differ with both sides. The feature adds the
  optional dependency `proptest`. The crate's tests enable it through a
  dev-dependency on the crate itself, and run the kit over the todo, review
  and note schemas (`schema_laws.rs`, `kit.rs`, `replica.rs`), with a
  known-broken input for each law. A MINOR change under this file's rule:
  new API only, and no stored byte, vector or export moves.
- **`tags`, mergeable tag sets (SPEC law 48).** A host implements
  `tags::Tagging`: the type of a target, the type of a tag (which can be
  the same type), and the field name of each. A compile-time check
  refuses two equal names, and the names `at`, `actor` and `to`. The
  membership schema prints `Added(<target>, <tag>, at, actor)` and
  `Removed(…)`, and its pair's register reads the greatest presence of
  its frontier under `Off < On`: an observed-remove set where an add
  wins. The alias schema prints `Aliased(<tag>, at, actor, to)` and
  `Unaliased(<tag>, at, actor)`, and refuses a tag as an alias of
  itself. Both schemas use `fold::sole::Bodies` for their one register.
  `TagSet::of` reads one prodrome of each store. It follows each name's
  chosen alias, reads a ring and every chain into it as the ring's least
  name, lists each `Disagreement`, and returns the events for `add`,
  `remove` and `rename`. A MINOR change under this file's rule: new API
  only. Tests in `core/tests/all/tags.rs`, over notes tagged with
  labels.
- **`fold::sole::Bodies<B>`, the register type whose value is an
  `Event<B>`'s body under the body's own `Order`.** A one-register schema
  whose reading is its bodies (a flag set off or on, a discrete label)
  writes no register type of its own: `One<Bodies<B>>` is its product. Two
  writes with equal bodies are one value, named by the least write, and an
  event whose body writes no register joins nothing. `Order` holds for a
  reference to an ordered value, as for the value. A MINOR change under
  this file's rule: new API only. Tests in `core/tests/all/bodies.rs`, a
  flag schema with an ordered body and a view that writes nothing, over
  laws 31 and 33.
- `store::frontier`, a frontier token for a pull over a wire (SPEC §3,
  law 47). `Token::of` names a set of tips by the sha256 of their sorted
  names, one on each line. `Frontiers::answer` reads a pull's token against
  a store's objects and answers a `Lacking`: what the token reaches, or
  `Reached::Unknown` when the store of record does not hold the token or
  the store no longer holds one of its tips, and the objects outside it in
  linear order. `Lacking::record` and `Lacking::record_with` record the
  tips of what the replica then holds, at most the kept number for each
  store, through `Records`, an effect that the host supplies. No stored
  byte changes.

### Changed

- The repository moved to the `fulfilium` organization: it is
  `github.com/fulfilium/prodrome`, and the viewer is at
  `https://fulfilium.github.io/prodrome/`. The links, the package metadata and
  the issue mirror's comments name the new home.

The Prodrome in the browser, a read-only viewer of this crate's roadmap
folded and typeset on the reader's machine, and GitHub issues mirrored onto
that roadmap. Typst and GitHub enter the repository only as optional extras
beside the core; `core/` and `cli/` depend on neither, and no stored byte
changes.

And `Absent`, the empty term (`docs/design-empty-term.md`): an object with no
claim on attention is priced by a term whose value is `∅`, so FPL is total
and an unpriced todo stops being a special case in every reader. A MINOR
change under this file's rule: no stored object changes, a term without
`Absent` reads exactly what it read, and the API breaks where a value was an
`f64`.

And the object store is durable and its fsck honest, which are PATCH fixes
under this file's rule: no stored byte changes, and a healthy store reads and
verifies exactly as it did.

And change identity (`docs/design-change-identity.md`, SPEC §3): a change is
named by its prodrome, its event and the writes it supersedes, never by
where it was written, so appending is idempotent, changes to different todos
are independent, and a conflict prices as its most urgent candidate. A MINOR
change under this file's rule: `Genesis`, `Change`, `Snapshot` and `Least`
are new constructors and no stored object changes; a legacy store derives,
linearises and verifies as it did, and reads as it did wherever no register
holds two candidates. The API breaks where a register had a winner.

And register types, stage 1 (`docs/design-register-types.md`): a register's
values are partially ordered and its reading is their maximal values. A
MINOR change under this file's rule: the todo's registers are discrete, so
every reading and every stored byte is as it was.

And register types, stage 2: a store is opened at a SCHEMA, the type of its
events, and the store, the fold, `verify` and the view are generic over one;
the todo schema is the reference one. A MINOR change under this file's rule:
no stored byte and no object name moves, and every vector reads as it did.
The API breaks wherever a type was parameterised by the payload.

And register types, stage 3: prodrome-wasm's exports are generic over a
schema, and a host builds ONE module of all its schemas with a macro, from a new
library crate. A
MINOR change under this file's rule: no stored byte moves, and every export
the reference module had answers byte for byte as it did.

And a schema's price is what the design says a valuation is: `Price`, the
term each candidate of a reading prices as, the reading priced as `Least`
over them. What the todo alone has, a price that changes with its history,
what a `Ref` reads of an entity, and what a list row shows, are `History`,
`Bind` and `Row`, each implemented only by a schema that has it. Unreleased
API only: `schema::Valuation` and the wasm's `PricedJson` never shipped in a
release, and every vector and export reads byte for byte as it did.

And a store remembers what it has read (`docs/design-register-types.md`
§6.1, stage 7's in-memory replica): a handle verifies each object once and
holds it with the fold of them all, so an append and a read cost the objects
new since it last looked and not the store. At 3,500 objects (17.6 MB) a
warm append falls from about 235 ms to about 15 ms and a warm read from
about 120 ms to about 8 ms (`core/examples/store_cost.rs`). A MINOR change
under this file's rule: no stored byte moves, every vector reads as it did,
and a store read through its memory reads exactly as one read cold. The API
breaks where `dag()` answered a copy.

And replicas (`docs/design-register-types.md` §6.1, stage 7): a store on
disk, a store in memory and an overlay are one `Replica`, `sync` is the
join of any two, and a coordinated decision is a type only the store on
disk has. A MINOR change under this file's rule: no stored byte moves,
every vector and export reads as it did, and the reference module's
classes gain a method.

And a change rests on what its writer saw (SPEC §3, laws 36 and 37): its
deps are its entity's heads, every write to it in any register that no
other descends from, where they were the frontiers of the registers it
writes. One writer's add and complete of one entity are no longer recorded
as concurrent, so the stream orders them as written and no instant has to.
A MINOR change under this file's rule: no shipped field changes and no
stored object moves or reads differently; an append over a history writes
a different object than before wherever its entity has a head outside the
registers it writes. A reader before this one reports such a dep as
writing no register its change writes.

And caches (`docs/design-register-types.md` §6.2, stage 8): a cache is a
tabulation of a pure function, a store of (key, value) pairs joined by
union, and hierarchical caching is one memoised fold over a
content-addressed tree of any shape. A MINOR change under this file's
rule: a new module, and no stored byte, vector or export moves.

And nesting (`docs/design-register-types.md` §6.3, stage 9, SPEC law 39):
a register may hold a history by its heads, a nest of such histories
flattens into one history keyed by path, renaming no object, and every way
one replica takes chosen objects from another is one restricted join. A
MINOR change under this file's rule: new API only, no stored byte moves,
the object model gains no constructor, and every vector and export reads
as it did.

And time's one type (`docs/design-register-types.md` §6.2): a behaviour
that is piecewise constant is a `Schedule<A>`, a step function with its
algebra law-tested, and what reimplemented one (a `Piecewise`'s pieces, a
register's history, the wire's environment over time, an observed value)
is one or a function of one, with one fold over time where there were two.
A MINOR change under this file's rule: no stored byte, vector or export
answer moves; the API breaks where a `Piecewise` layer had two fields.

And a change rests on its genesis (SPEC §3, law 43): what an object rests
on is the edges it names and the genesis it names, so a genesis is no
longer a head forever beside the changes begun from it, and no
linearisation puts a change before its genesis. A MINOR change under this
file's rule: no shipped field changes and no stored object moves or reads
differently; `snapshot()` over a prodrome holding anything writes a
different object than before, since its tips no longer hold the genesis,
and `conformance/change.py`'s snapshot chain is derived again for it.

And a reading is total over what a replica holds (SPEC law 44): a print
that does not parse at the schema a page reads at leaves out itself and
everything resting on it, as a store's largest down-set does (law 40), and
every reading of the wasm exports' classes answers what it left out instead
of refusing. A MINOR change under this file's rule: no stored byte moves; a
reading that refused now answers, with one key more; the reference module's
free functions answer and refuse as they did.

And the store of record reads totally, as a replica does (SPEC law 45): a
file of `objects/` that is no object is left out of every read of an
`EventStore` with what rests on it, and named in the `Dag` the read answers,
where one such file used to refuse every read. Writes still refuse while
anything is unread, naming the file. A MINOR change under this file's rule:
no stored byte moves, a healthy store reads, writes and verifies exactly as
it did, and a read that refused now answers.

And a digest (`docs/design-register-types.md` §6.5, SPEC law 46): a long
log of lines read through summaries, as a tree of the lines' names, a view
that tiles the log under a byte budget, and zoom. A MINOR change under
this file's rule: a new module, and no stored byte, vector or export
moves. `memo::balance` builds a different tree of a sequence than before
(every element at one depth, every chunk off the path to the last element
holding two or more), so a cache filled by a fold of a balanced sequence
misses once.

### Added

- **A generic event type for application schemas (`schema::headed`), and
  a product for a schema with one register (`fold::sole`).** `Event<B>`
  holds a key, `at`, `actor` and a body `B`, and implements `Schema`. It
  prints as `Constructor(<key fields>, at, actor, <body fields>)` through
  `literal::codec`. The trait `Body` gives the rest of the schema: the key
  type, the registers, the event constructors, the registers that a body
  writes, whether a policy is asked about it, the check of a key and a body
  together (`Body::admits`, with a typed `Refusal`), the print of the body
  and the parse of a whole event. `Event::new` asks `Body::admits` and is
  the only way to build an event, so every event that exists passed the
  check; a body whose `Refusal` is `Infallible` also gets the total
  `Event::of`. `KeyFields` gives the fields of a key, and every `Literal`
  key has one field. `Constructors<B>` is the vocabulary of `Event<B>`.
  `fold::sole::One<R>` is the product of a schema whose events write one
  register, `Only`. Its parameter is the register's type: a
  `RegisterType` that also implements `Sole`, which names the schema and
  can refuse an append that falls below the reading. `One` reads the
  frontier under that type, and joins a write only where the type gives
  the write a value, so an event that writes no register supersedes
  nothing. `Whole<E>` is the register type whose value is the whole event,
  so its reading is the frontier's candidates. A MINOR change under this
  file's rule: new API only, and no stored byte, vector or export moves.
  A note schema at `Event`, with a title that writes the register and a
  view that writes none, is the example of `schema::headed`'s module
  documentation (`core/src/schema/headed/example.rs`, inside every build's
  source), and the tests include the same file. Law 31 (a register type's
  reading) and laws 33 and 36 (schema laws) run over it, and
  `core/tests/all/note.rs` checks its print, its parse, the refusal of an
  empty name, a conflict that a third title settles across two replicas,
  and a view that supersedes no title.
- **A codec between Rust values and literals (`literal::codec`).** The
  trait `Literal` gives a type one stored form that reads back exactly,
  with `to_value` and `from_value`. `record` and `headed` build the call of
  a `Constructor` (a name and its field order), and `Fields` reads the call
  back by field name. `arm` and `no_arm` help a reader of a sum. The module
  implements `Literal` for `String`, `bool`, `u32`, `u64`, `usize`,
  `Datetime`, `chrono::NaiveDate` (as `YYYY-MM-DD`), `Option`, `Vec` and
  `BTreeSet` (a sorted tuple, each member once), and `event` implements it
  for `Actor` and `Hash`. The macro `text_literal!` implements it for a
  type whose literal is its text, read back through the type's
  `TryFrom<String>`. The macro `word_literal!` implements it for a type
  with a finite set of values, each printed as a word, and reads a word
  back from the list of all the type's values. `literal::Table` holds a list of `Constructor`s, which
  is the same type as before. A MINOR change under this file's rule: new
  API only, and no stored byte, vector or export moves. Tests in
  `core/tests/all/codec.rs`.
- **A store's reads are total (SPEC law 45).** `EventStore::tips`, `dag`,
  `folded`, `events`, `ancestors`, `concurrent` and `Replica::held` answer
  the history of the files that verify: a file failing its hash, not text
  or not parsing at the store's schema is held as unread, what rests on it
  waits, and the `Dag` answered names it in `excluded()` exactly as a
  replica handed the same prints does. A `Dag` holds the objects waiting
  outside its history, so `interior()` keeps every print and `excluded()`
  and `verify()` answer of it what they answer of the prints. The store
  reads what it lacks by `Dag::from_prints`, the replica's own function,
  and `verify` and `fsck`'s quarantine are views of the same look into an
  empty memory. `EventStore::decide`, and every write, refuses while
  anything is unread, with `Dag::whole`, which now borrows and names every
  print it refuses with. Laws in `core/tests/all/unread.rs`. The command
  line's `list` and `show` print what their reading left out on the
  standard error, each name with why and how many objects rest on it
  (`Outcome::left_out`).
- **A digest (design §6.5, SPEC law 46):** `digest`. `tree(leaves)` over
  a set of lines given in any order, each a `Leaf` with a name, the lines
  it rests on, a cost and a `Measure` (a monoid, `()` for none), ordered
  by `topo::linear`, a cycle refused as `Cycle`; a `Node`'s name, span,
  level, bytes, measure and children, and `Tree::settled` for a node no
  appended line renames. `pending`, the settled nodes whose summary can be
  written now; `view(tree, summarized, budget)`, the parts tiling the log,
  what they cost and what is `waiting` to be summarised, each closing,
  with what decay forces beside it, lowering the cost; `zoom`, a part's children; `Part::handle`,
  `first+lines`; `read`, a sound key for a cached view; `Measured`, the
  measure as a `memo::Algebra`. Laws in `core/tests/all/digest.rs`;
  `core/examples/digest_prefix.rs` measures what an append keeps of the
  printed view.
- **`topo::linear`:** the crate's one causal order, Kahn's walk with ties
  by key, which `Dag::linearise` now is and the digest orders its lines by.
- **`memo::balance` builds a tree that only grows:** a run closes at a
  boundary once it holds two, and a level's last run as it is, so an
  element appended renames only chunks holding the last element
  (`memo_balance.rs::an_append_renames_only_the_spine`).
- **What a history leaves out (SPEC law 44).** `Dag::excluded`: every
  name a set of prints holds or its objects rest on that is no object
  there, why (`Unread`, or `None` for a parent no print is), and the
  objects resting on it, by `Dag::above`, the up-set dual of `closure`.
  The interior is exactly the objects no `resting` names. `Dag::verify`'s
  unread and broken findings are read off it, unchanged. `Dag` implements
  `Extend`, the union with more objects. In the wasm exports, a class's
  history is the interior of what it was sent, and `tips`, `since`,
  `readings`, `proven`, `entries`, `prices` and `next_changes` each answer
  `excluded: [{name, why, resting}]` beside what they read (`why` is
  `"failed to parse: …"`, the parse's refusal); `Read::whole` refuses for
  an answer with no room for it. Laws in `core/tests/all/down_set.rs` and
  `wasm/exports/src/unread.rs`, over `common/draw.rs`, the history
  generator the two crates now share.
- **Schedules (design §6.2):** `schedule::Schedule<A>`, a head and knots at
  strictly increasing instants, meaning the function whose value at `t` is
  the last knot at or before `t` (`at`, `since`, `last`). `new` refuses
  knots out of order (`schedule::Unordered`) and `of` builds one from a
  map. `map` and `traverse`, `zip` and `sequence` with `constant`, `+` and
  `Sum` where the value is a monoid, `join` for a schedule of schedules,
  `shift`, `normal` and `next_change`. `fold::readings`, an entity's
  registers as a schedule, of which `fold::read` is the value at an
  instant. `observe::observed`, the observed value of an exact term from an
  instant on, as a schedule. Laws in `core/tests/all/schedule.rs`,
  `fold_laws.rs` (a reading at a moment is the readings at it) and
  `observe.rs` (the observed schedule is the observed value).
And device signatures (`docs/design-register-types.md` §6.4, stage 12,
SPEC §3, §5 and law 42): who wrote an object can be proven. A signature is
an object of its own naming what it signs, so no object's bytes or name
change and an object gains signatures at any time; a key belongs to an
actor because an object says so; and a policy may require the proof. A
MINOR change under this file's rule: `Signed`, `KeyAdded` and `KeyRevoked`
are new constructors, no stored object changes, and a history whose actors
have no keys reads, verifies and exports exactly as it did under every
policy that does not require signatures. The API breaks where a policy
answered about an event: it answers about an object.

### Added

- **Signatures and keys as objects (SPEC §3):** `sign`. `Signed(object,
  key, signature)`, Ed25519 (`ed25519-dalek`, strict verification) over
  `prodrome object ` and the object's name, its one parent the object it
  signs; `KeyAdded(genesis, actor, key)`, with no parents;
  `KeyRevoked(genesis, deps, actor, key)`, keeping the signatures beneath
  its deps. `sign::Secret`, a device key from the seed its host keeps,
  signing deterministically; `sign::PublicKey`, `sign::Signature`.
- **The proof (SPEC §5, law 42):** `sign::Proof::of(dag, registrar)`, the
  objects a key registered to their actor signed, beneath every revocation
  of it, a function of the object set; `sign::Registrar::Anyone` (every
  key object, as `verify` reads them) or `Roots`.
- **The policy that requires signatures (SPEC §5):** `policy::Proven<P>`,
  the meet of any policy and the proof, its keys registered by root keys,
  made at a history (`Proven::new(policy, roots, history)`) and read at
  another by `Policy::at`; `Standing::and`. An unproven object claims, a content record included.
- **`verify` reports** a `Signed` that does not verify
  (`Finding::Forged`) and an object whose actor has a key and which no key
  of that actor's proves (`Finding::Unsigned`).
- **The wasm exports:** every schema's class gains `sign(secret, object)`,
  sealing a held object's signature into the page's replica and answering
  `{hash, objects}` as `append` does; `proven(roots)`; and the static
  `public_key(secret)`. Every reading's policy argument also takes
  `{untrusted, roots}`; an array of actor names reads as it did.

### Changed (breaking)

- **A policy answers about an object:** `Policy::standing(object, event)`
  and `Policy::confirms(object, event)` take the object's name, and a
  policy is asked of every object carrying an event, where the readers
  asked only of events the schema `asks` about. `Untrusted` and
  `Everything` answer exactly as they did; a host's own policy binds what
  the schema does not ask about by checking `asks` itself, as `Untrusted`
  does. `Policy::at(history)`, the policy at a history (`None` for one
  that reads nothing of a history), which the store's `verify` asks.
- **`dag::Finding`** gains `Forged` and `Unsigned`.

And a schema as data (`docs/design-register-types.md` §5 and §6.0, stage
10, SPEC law 40): a schema may arrive as text at run time, is admitted only
when its laws hold, and is read by the code a Rust schema is read by. A
MINOR change under this file's rule: no stored byte moves, the object model
gains no constructor, no object carries its schema, and every vector and
export reads as it did. The API breaks where a schema's vocabulary was a
type with a `Default`: it is now a value a store is opened at.

And a set of prodromes is a prodrome (`docs/design-register-types.md`
§6.3.1, SPEC §7.2 and law 41): a qualified reference, `RefIn(store,
entity)`, prices a term in one store by an entity in another, through a set
of prodromes the host passes. A MINOR change under this file's rule: `RefIn`
is a new term constructor, so no stored byte moves, and every unqualified
`Ref` links and reads as it did. The API breaks where `TermF::Ref` had one
field.

### Added

- **A qualified reference (SPEC §7.2):** `fpl::mk_ref_in` and the term
  `RefIn(store, entity)`, the same variable as `Ref` with a qualifier:
  `TermF::Ref { store, entity }`, `store` none for an unqualified one, which
  prints as `Ref(todo)` did. `fpl::Stores`, a set of prodromes as qualified
  references read it (`fpl::Member`, each member's functions and
  environment, by genesis, and `Stores::named` for a host's names), and
  `fpl::link_in`, of which `link` is the case with no stores. A qualified
  reference is linked in its member and compiled against that member's
  environment, so it reads what its entity reads at home; a store or an
  entity the set lacks reads `∅`. Its JSON kind is `refIn`. Laws in
  `core/tests/all/sets.rs`.
- **A set of prodromes (design §6.3.1, SPEC law 41):** `nest::Set`, a
  replica's prodromes as a nest keyed by genesis, each read by a member
  level, whose join puts every object at `genesis/key`; `Dag::genesis_of`,
  the genesis of an object's prodrome. `fold::member`, one prodrome as a
  term elsewhere reads it, and `fold::stores`, a fold's prodromes as an
  `fpl::Stores`; `view::entries` prices through `fold::member`, and links
  each entry through the set of its DAG's prodromes, so a qualified
  reference to a sibling prodrome prices as that prodrome's view does. Laws in
  `core/tests/all/sets.rs`: disjointness, a set of sets is associative, and
  the set's reading is the product of its members'.
- **The inbox, priced by what it serves (design §0, §6.3.1):** a test
  schema, `core/tests/schemas/inbox.rs`, whose proposal's `Price` is a
  qualified reference to the todo it serves; read through a set of it and
  two todo prodromes, its price is that todo's and moves exactly when that
  todo's history does (`sets.rs`).

- **A schema as data (design §5, §6.0, stage 10, SPEC law 40):** `declared`.
  `declared::Form`, a schema in the schema language, the objects' own §2
  literal grammar under a closed vocabulary: event constructors with typed
  fields (text, integer, instant, reference, a closed enum, a list), the
  key, and registers with an order (`Discrete`, `Total`, a `Machine` given
  by its covering relation, `Inclusion`), inflationary or not, and the
  fields that write them; `print` is canonical, every set sorted.
  `declared::Declaration::admit(text)`, the only way to a declaration,
  asks eight laws in order and refuses with the first that fails
  (`declared::Law`, `declared::Refusal`): grammar, canonical (the text is
  its own print, so its name is its hash), unique names, the key, a stamp
  on every event, typed routes, every order a partial order (a machine's
  covers acyclic) and a bottom under every inflationary register.
  `declared::Declared`, an event of a declared schema, implements `Schema`
  with the declaration as its vocabulary; its registers are `Frontier`s
  read under the declared order (`declared::Valued`, the core's own
  orders interpreted) and an inflationary one climbs by `fold::grows`.
  Laws in `core/tests/all/declared.rs`: a differential against the
  proposal schema written in Rust (`core/tests/schemas/proposal.rs`) over
  generated histories and through stores, law 1 and law 2 over the
  declared schema, a lawful generated schema admitted and a broken one
  refused under the law it breaks; `nest_histories.rs` nests it.
- **`EventStore::at`, `MemoryStore::at`, `Replica::schema`:** a store
  opened at a schema value; an overlay parses at its base's.
- **`literal::Signature::Owned`:** a field order a vocabulary holds as
  owned names, bound by the function that binds a static one.
- **The wasm exports over a declared schema:** `schema!(Name, declared)`, a
  class `new Name(schema, objects)` that admits the schema first, with
  `declaration()`; the reference module exports it as `Declared`.

- **Nesting (design §6.3, stage 9, SPEC law 39):** `nest`. `nest::Path`,
  the free monoid on `nest::Segment`s, and `nest::History<T>`, a set of
  `(path, value)` pairs that is a monad (`unit`, `fmap`, `join`, `bind`),
  read at a path by `at` and joined with chosen values of an inner history
  by `accept`. `nest::Heads<I>`, a register value holding a history of the
  schema `I` by its heads, ordered by inclusion; `nest::Nests`, a schema's
  held registers; `nest::pointer`, a held register's reading, the union of
  its maximal writes' heads. `nest::Nest`, a tree of content-addressed
  histories read from each level's replica by `nest::Leaf` and
  `nest::Holding`: `flatten` (the join), `below` and `past` (happens-before
  across levels), and a `memo::Tree`, so `memo::fold` folds it.
  `store::accept(from, to, chosen)`, the restricted join for replicas;
  `sync`, `EventStore::adopt` and `Overlay::flush` are it, and
  `Overlay::accept` takes a batch object by object. Laws in
  `core/tests/all/nest.rs`, `nest_histories.rs` and `replica.rs`.
- **Caches (design §6.2, stage 8, law 8):** `memo`, depending on the hash
  type alone. `memo::Cache`, pairs keyed by `memo::Key` (a function's
  name and its argument's), each key a register in the flat order:
  `get` answers a miss, a hit or a `Finding` (two values, held and never
  chosen), `insert` and `join` (union) answer the findings they make,
  and any entry may be removed. `memo::Evict` decides what is dropped;
  `Keep` and a bounded `Lru` ship. `memo::fold`, the memoised
  catamorphism over any `memo::Tree` (a content name and children) by any
  `memo::Algebra` (a name and a step), memoised at every node by
  (algebra, node) and answering a `memo::Report`: per node and by depth,
  what hit, what was computed and which keys held two values.
  `memo::name`, the Merkle name a node must have, covering its children's.
  `memo::balance`, a long sequence as a balanced tree of the host's own
  measured chunks, its shape a function of the elements alone. Laws in
  `core/tests/all/memo.rs`, `memo_fold.rs` and `memo_balance.rs`.
And a behaviour names its next observed change
(`docs/design-register-types.md` §6.2, SPEC §7 and law 38): a view reads a
fulfillment through an `Observation`, and `next_change` answers when what it
shows next changes, exactly on the exact fragment and never late elsewhere.
A MINOR change under this file's rule: new API only, no stored byte moves,
and every vector and export reads as it did.

### Added

- **The next observed change (design §6.2, SPEC §7 and law 38):**
  `observe::Observation`, a quantisation of `[0, 1] ∪ {∅}` (`levels` equal
  steps, `Rounding::{Down, Nearest, Up}`, `Observation::PERCENT`), and
  `observe::next_change(term, now, env, observation)`, answering
  `NextChange { at, exact }`: the first microsecond after `now` at which the
  observed value differs, `None` for never. Exact on the exact fragment,
  each atom's closed form cutting time into affine stretches bisected
  against the evaluator; conservative elsewhere, by an enclosure of the term
  over `[now, t]`, never later than the true change. The wasm exports carry
  it: `next_change(term, now, env, observation)` in the reference module,
  and `next_changes(at, untrusted, observation)` per row on a priced
  schema's class, an observation crossing as `{levels, rounding}`.

- **Replicas (design §6.1, stage 7, SPEC §3 and law 35):** `store::Replica`,
  what every store is: `held` (its objects, fold and tips of one look),
  `tips`, `print`, `append` and `receive`, which takes in what some seeds
  rest on, every object verified before any is taken, parents first.
  `store::sync(from, to)`, one function over any two. `store::MemoryStore`,
  a store in memory that appends by the decision an `EventStore` makes
  (`Memory::change`), byte for byte. `store::Overlay`, which writes to
  memory, reads `own ∪ base` from the base's memory with its own objects
  admitted over it, and is flushed by `sync`. `store::Decision`, from
  `EventStore::decide`: the lock held and the memory level with the
  directory, reads as of the lock and an append written through; an
  overlay and a store in memory have none (compile-fail doctests). A
  borrowed replica is a replica.
- **The wasm exports append:** each schema's class has `append(event,
  genesis)`, sealing an event's print into the page's replica as a `Change`
  over its own fold and answering `{hash, objects}`, the objects
  (`[{hash, text}]`) to send the host, none for a twin.
- **A store's memory (design §6.1, stage 7):** `EventStore` holds every
  object it has read, each verified once (rehashed from its file, or sealed
  by its own append), their tips and their fold. A read or an append lists
  `objects/` and reads only the names the memory lacks; a held file is read
  again only when its `stat` no longer matches what was seen when it was
  verified (or it changed too recently for `stat` to tell, git's racily
  clean), and `verify` still rehashes everything. An append decides its
  twin check and its deps on the memory under the store's lock, after the
  look that brings the memory level with the directory, so another
  writer's objects are always in it. `EventStore::folded` answers the fold
  (`fold(&dag.nodes_across_gaps()?)`), kept by insertion. Laws, in
  `core/tests/all/memory.rs`: any interleaving of appends by this handle,
  another and a fresh one, a third replica's files arriving in no causal
  order, deletions and reads reads as a fresh handle does (objects, fold,
  readings, heads); every append writes the bytes a fresh handle's would;
  and an adoption through the memory is idempotent, order-free, completes
  when interrupted, and reads as the union of the two directories.
- **`Folded::insert`:** an object placed where the linearisation of the
  union puts it, so the fold is an action of the object SET: in any
  parents-first order, inserting is `fold` of the whole. It holds where
  every object rests only on objects of its own scope (`Folded::local`,
  true of every store an append wrote); elsewhere it answers
  `registers::Elsewhere` and changes nothing. `Folded::push` is the monoid
  action's step `extend` was made of, and `BitSet::open` makes room for a
  position.
- `core/examples/store_cost.rs`: an append and a read, cold and warm,
  against a store of a host's size.

- **`prodrome-wasm-exports`, the exports generic over a schema** (a new
  library crate, `wasm/exports/`): `Replica` reads a set of objects once at
  a schema, and `verify`, `tips` (each prodrome's heads by its genesis),
  `since` (what a replica holding some tips lacks), `readings` (each
  register's maximal writes, for any schema), and, for a schema with a
  `Row`, a `History` and a `Bind`, `entries` and `prices` answer over it.
  `Json` is what a schema says to cross the boundary (the field its events
  name an entity by, its registers' names, its values' JSON) and `RowJson`
  its row's reading's JSON; the todo schema has both, at any payload.
  `prodrome_wasm_exports::schema!(Name = Schema)`, with `, priced` for a
  schema with those three, instantiates them as a JS class per schema in one
  module. The
  term codec (`json`) and the wire shapes (`wire`) moved into it.
  prodrome-wasm, the reference module, instantiates `Todos` with it, beside
  its exports, which answer as they did.
- **`fold::Product::reading(register)`:** a register's reading by name, its
  frontier's maximal writes under its type's order, so a reader holding
  only `Schema::REGISTERS` reads any schema. SPEC law 33's test compares it
  too. The tests' review schema is `core/tests/schemas/review.rs`, which
  prodrome-wasm's tests instantiate through `schema!` over a small store.

- **`schema::Schema` (SPEC §4, §6, §8):** what a store's events are and what
  each writes: the vocabulary (`Vocabulary`, `to_value`, `from_value`), the
  `key`, `at` and `actor` of an event, the registers it supersedes in
  (`Register`, `REGISTERS`, `writes`), an entity's registers as a
  `fold::Product`, and which events a policy is asked about (`asks`). The
  event type is the schema; `TodoEvent<P>` is the todo schema, in the new
  `todo` module with its vocabulary `TodoVocabulary`, its register types
  `State`, `Spec` and `Content`, and its row's `todo::Reading`.
- **`fold::Product` and `fold::RegisterType`:** an entity's registers as a
  product of semilattices joined by the schema's route, with each superseding
  register's frontier by name, its conflicts, and the append's refusal
  (`grows`); a register type names its values, their order and the value a
  write gives it. `Frontier::reading` reads a frontier under one, and
  `Frontier::grows` refuses a write below an inflationary one's reading.
  `fold::read` is the one fold, over any schema.
- **`schema::Price`:** the optional price of a schema's entities, the term
  each candidate of a reading prices as (`terms`), the reading priced as
  `Least` over them, their meet in the fulfillment order (`fold::price`). An
  entity whose schema has none has no price, which is not `Absent`.
- **`schema::History`, `schema::Bind` and `schema::Row`**, each what only a
  schema that has it implements, the todo's at any payload: `History: Price`
  is a price that changes with the entity's history, the terms of each
  `moment` that `fold::flatten` makes a piece of §6.4's function; `Bind` is
  what FPL's `Ref`s read of an entity in this store (`fold::env`), its key a
  string; `Row` is what a §6.7 row shows (`Reading`, `reading`) and when it
  is provisional (`disputes`, `unconfirmed`). `view::entries` asks for all
  three; a `Ref` to an entity in another store is a host's environment,
  passed in, never a schema's method.
- `event::ENVELOPE_SIGNATURES` and `todo::TODO_SIGNATURES`, the two halves of
  what `EVENT_SIGNATURES` was.
- SPEC law 33, a reading is a function of the object set, over the todo
  schema and a review schema in the tests (`core/tests/all/review.rs`,
  `schema_laws.rs`), which also drive law 32 through the store's append.
- SPEC law 34, a conflict prices as `Least` over its candidates' terms,
  stated over `Price` and run over the todo schema and the review schema,
  which is priced by a flat term per phase.

- **`fold::Order` (SPEC §6):** a partial order on a register's values, with
  `Discrete` (the default), `Total` and inclusion on a set as instances;
  `fold::maximal` and `Frontier::read` are the completion, a register's
  reading as the maximal values of its frontier. `fold::Inflationary`
  declares a register that may only grow, and `fold::grows` is the refusal an
  append of a write that goes back meets; no todo register supersedes a write
  it could refuse, so the store calls it from stage 2's schemas. SPEC laws
  30–32 and their evidence, `core/tests/all/order_laws.rs`.

- **`Absent`, a new leaf of `TermF` (SPEC §7):** `∅` at every instant,
  printed `Absent()`, built by `fpl::mk_absent`. `∅` is the identity of
  composition: `Conj` and `Within` mean over what has a value (`∅` when
  nothing does; `Conj([])` is still 0.5), an absent gate or `OffsetBy` delta
  is none, and every other operator passes `∅` through. `After`'s and
  `Recur`'s `pending` may be absent; a cancelled `After` is still 1.0.
- **A `Ref` to a known todo with no spec links to `Absent` (SPEC §7.2)**,
  where it was refused; `LinkError::Unknown` is now only an id the store
  has never seen, and says so. `fold::link_specs(functions, known)` takes the
  todos the store knows.
- **One list order (SPEC §6.7):** `view::list_order`, ascending in value,
  ties by id, then every row with no number, `absent` or not linking, by id.
  `prodrome list` sorts by it; prodrome-wasm's `entries` answers it as
  `order`; the Typst package lists by it (`listed`, which replaces
  `by-fulfillment`) and sorts nothing itself.
- prodrome-wasm: `fold` answers `functions`, every todo the objects mention
  with its function or `absent`, which is what `link` binds against; the JSON
  codec has `{"kind": "absent"}`.
- `fpl::holds_absent`. The chain compiler writes a cancelled link over a body
  holding an `Absent` as `Gate(body, Flat(1.0))`, since an offset of `∅` is
  `∅`; an `Absent`-free body is still `Offset(1.0, body)`.
- **SPEC law 18** and its evidence: `core/tests/fpl_laws.rs` (an absent
  member changes no conjunction, a term without `Absent` has a value
  everywhere, `∅` composes as §7 says, `explain` carries it and gives it no
  share, an exact term's knots are its curve), whose generators, moved to
  `core/tests/common/terms.rs`, now draw `Absent` so laws 5, 13 and 14 cover
  it too; `core/tests/fold_laws.rs` (a `Ref` links to `Absent` for a known
  todo with no function, and every row reads that link).
  `conformance/absent/{fpl,series,view}.py` are NEW and SEEDED like the view
  vectors, by `core/examples/generate_absent_vectors.rs`, run by hand.

- **`typst-wasm/`**, Typst 0.15.1 compiled to WebAssembly
  (`nix build .#prodrome-typst-wasm`), in a cargo workspace of its own so the
  core's lock file never sees it. Exports `compile_html(source, files)` (the
  HTML document or diagnostics with spans, as a value, never a throw),
  `highlight`, `complete`, `hover` and `add_font`; every offset is UTF-16.
  The module is 22.0 MB, 6.3 MB with brotli.
- **`typst-wasm/` for views**: `Project`, a world kept between compilations
  as typst-cli's watch mode keeps one. A host sets each view module on its
  own (`set(key, text)`, `remove(key)`): a file set again unchanged keeps its
  parse and its hash, and an edited one is `Source::replace`d, so only the
  edit reparses. The state is each compilation's own argument
  (`compile(main, state)`, read as `/state.json`), never one JSON object of
  every file. `Project.restricted()` compiles markup whose author is not
  trusted: its library has no `html` module and no `plugin`, under any name,
  `eval` and `std` included, and completion there offers neither.
  `Project.complete` and `Project.hover` answer over a file of the project.
  `compile_html`, `highlight`, and `complete` and `hover` (typst-ide) are
  each a cargo feature, all on by default, so `prodrome-typst-wasm` is the
  viewer's module as before; `nix build .#prodrome-typst-wasm-views` is
  `Project` and `highlight`, and `.#prodrome-typst-wasm-editor` adds
  typst-ide. Each writes `web/typst_bg.wasm.br` (brotli, quality 11) beside
  the wasm, for a host to serve precompressed.
  `checks.prodrome-typst-wasm-bench` times a 250-item page and a keystroke,
  old export against new (`typst-wasm/bench/views.mjs`), and
  `checks.prodrome-typst-wasm-tests` runs clippy, pedantic, over every
  feature set. What each feature costs, after `wasm-opt -Os`:

  | module (`nix build .#…`)    | features                | raw         | brotli     |
  | ---------------------------- | ----------------------- | ----------- | ---------- |
  | `prodrome-typst-wasm`        | oneshot, highlight, ide | 22 064 360  | 6 266 825  |
  | `prodrome-typst-wasm-editor` | highlight, ide          | 22 054 373  | 6 268 139  |
  | `prodrome-typst-wasm-views`  | highlight               | 21 824 480  | 6 209 471  |

  Feature by feature, in bytes: `ide` 230 600 raw and 66 000 brotli,
  `oneshot` 10 000 and 3 900, `highlight` 2 400 and nothing measurable, the
  parser being the compiler's own. So an editor costs its host 1% more if it
  loads `-editor` instead of `-views`, and a second, lazily loaded module
  would cost it 6.3 MB more.

  Little, because link-time optimisation already drops what no export
  reaches: the module is the compiler and the data its dependencies embed,
  which no feature of this crate can leave out.
- **`typst/`**, the `prodrome-typst` package: the roadmap list ordered by
  fulfillment, an item's page, and two marks: a value's pie beside its
  percentage, and a todo's thirty days (fifteen back, fifteen ahead) as one
  thick line, the past faded, in a 0–100% frame. Its look is the design
  system `typst-design` (a flake input, `@local/typst-design`): the
  fulfillment scale, the pie, the trace and the words for a value are
  imported from it, and the layouts are set in its faces, type scale,
  tones, capsule rules and diamond separators; the package keeps only the
  laying out of a store's reading. Problem, Watch and Fine are words only.
  Instants read as dates, and an item's
  price in words ("42% now, falling from 55% to 5% by Sep 29, 17:00, over 3
  days"), with its explanation's parts beneath it. It reads the core's JSON
  and computes nothing about fulfillment. `checks.prodrome-typst` compiles
  its examples to PDF and HTML, and its laws (`tests/laws.typ`), and
  compares the examples' committed pictures with a fresh render;
  `checks.prodrome-typst-restricted` compiles them in typst-wasm's
  restricted project, whose library has no `html` and no `plugin`.
- **`viewer/`**, the static web app (`nix build .#prodrome-viewer`), with
  routes `#/` and `#/todo/<id>`, a Typst editor demo on the item page, and a
  service worker that caches the shell per build and the objects forever. It
  never scrolls sideways at a phone's width, and its smoke test checks the
  list against `prodrome list`'s order at 420px.
  `viewer/assemble.sh` puts any store's objects beside it. It bundles the
  whole Typst package path (`prodrome-typst` and `typst-design`), and its
  stylesheet's colours and faces are read from the design system by
  `viewer/tokens.typ` at build time.
- **`.github/workflows/pages.yml`** deploys it on pushes to `main`, by hand,
  and after `roadmap-data`'s `verify` succeeds on a push (`workflow_run`: a
  push trigger for that orphan branch would never fire from `main`).
- `prodrome-wasm`: `entries` also answers `created`, each todo's first
  `Created` instant and last `Created` text, the body `prodrome list` shows
  for a todo with no record. Additive; the rows are unchanged.
- **`prodrome-github PAYLOAD.json`**, in its own crate `github/`, apart from
  the core and its CLI: one GitHub webhook delivery, `issues` or
  `issue_comment`, applied to a store. An opened issue becomes item
  `gh-<number>` (a `Created` and a record with the title as its body, the
  issue's URL as its `source.message_id`, category `issue` and a flat 80%); a
  retitle is a content revision with no spec; closed is `Completed` and
  reopened is `Reopened`. A `/price` comment by an OWNER, MEMBER or
  COLLABORATOR is a `SpecRevised` written as the commenter: `/price 40`,
  `/price 30 --deadline 2026-10-15 [--end N] [--lead-up DAYS]`, `/price ref
  gh-7`, or `/price accept`, which re-issues `pricing-bot`'s latest proposal.
  Anyone else's `/price` is ignored before it is parsed. `--proposal FILE`
  records a pricing reply's `/price` line as a claim by `pricing-bot`, dated no
  earlier than what its deps rest on, so it keeps §3's clock rule for a
  reader who names the bot untrusted. Deterministic and offline: every
  instant comes off the payload, and a delivery already applied appends
  nothing. Pinned by fixture payloads and two properties — replaying what was
  applied changes nothing, and close then reopen folds to open.
- **`.github/workflows/mirror.yml`** runs that verb on every issue event and
  `/price` comment and pushes the result to `roadmap-data`, welcoming each new
  issue once with its item link.
- **`.github/workflows/price.yml`**: Claude, with no tools and one turn,
  proposes a price for each new or edited issue; a job without the model
  records it as a `pricing-bot` claim and posts it.
- `docs/github-agent.md`: the flow and its trust boundaries.
- `Price::DEFAULT_START`, `DEFAULT_END` and `DEFAULT_LEAD_UP_DAYS` in
  `prodrome-cli`, so `--deadline` and `/price … --deadline` write one decay.
- **Three objects (SPEC §3):** `Genesis(label, nonce)` begins a prodrome and
  its name is the prodrome's identity; `Change(genesis, deps, event)` is an
  event over the frontiers of the registers it writes, so its deps never
  leave its todo; `Snapshot(genesis, tips, previous)` attests everything its
  tips rest on, chained to the snapshot before. Modules `genesis`, `change`
  and `snapshot`, and `event::event_id`, an event's name apart from where it
  was written.
- **`EventStore::init(label)`** writes a store's `Genesis`, once;
  **`snapshot()`** writes a `Snapshot` of the genesis's tips, or answers the
  last one when nothing is new; `in_genesis(genesis)` names the prodrome a
  handle writes into, for a store that holds several. `prodrome init` writes
  the genesis, and **`prodrome snapshot`** is new.
- **`Least(terms)`, a new node of `TermF` (SPEC §7):** the minimum of the
  members that have a value, `Absent` its identity; exact, its breakpoints
  including the crossings; `fpl::mk_least` and `least_of`, and
  `{"kind": "least"}` in prodrome-wasm's JSON.
- **`dag::Dag`**, the DAG as a value: tips, closure, linearisation, the
  geneses and each object's, the nodes the registers read, and `verify` as
  `dag::Finding`s. `registers::deps_for` is what `append` depends on.
- `verify` reports five new findings, each by name: an object naming a
  genesis the store does not hold, an edge between geneses, a dep another
  dep rests on, a dep writing no register its change's event writes, and an
  object a snapshot attests that the store lacks.
- **SPEC laws 19–29** and their evidence: `core/tests/all/change.rs` (a name
  is its genesis, event and view; replaying writes nothing; deps name one
  todo; geneses are disjoint; a snapshot attests its closure; placement is
  not identity, over two plain stores), `fold_laws.rs` (one candidate reads
  as before, a conflict is its most urgent world, any linear extension folds
  alike, a claim settles nothing), `fpl_laws.rs` (`Least` is a
  semilattice), and `conformance/change.py`, NEW and written by hand.

### Changed (breaking)

- **A `Piecewise` holds a schedule:** `TermF::Piecewise(Schedule<A>)`,
  where it had `head` and `pieces`; `fpl::in_force` is gone, being
  `Schedule::since`. `mk_piecewise` refuses what it refused, with the same
  message. The wasm `History` (`series_knots`'s argument) refuses a todo's
  bindings out of order, which it read wrongly before; `fold`'s own
  `history` is always in order.
- **`EventStore::fsck` sets aside every file failing its hash**, then
  verifies, where `EventStore::quarantine(name)` set aside one named file;
  `prodrome fsck` replaces `prodrome quarantine <name>`, and a read's
  refusal names it. `verify` (and `prodrome verify`) still changes nothing.
  Unreleased API only: `quarantine` never shipped in a release.

- **`Dag::nodes_across_gaps` is gone**: a set of objects missing a parent
  reads as its `Dag::interior()`, whose `nodes()` never refuses one, and
  `EventStore::dag` answers that interior (SPEC §3, law 40).

- **Only a `Dag` makes a `registers::Node`** (SPEC §3, §8): `Node::of` is
  gone and the fields are private, read by `name()`, `parents()`,
  `event()`, `genesis()` and `into_event()`. An object's prodrome is a
  function of the set it is in, since a `Change` naming a legacy root is in
  the legacy prodrome and nothing in its bytes says so; a node made of one
  envelope put such a change in a prodrome of its own. A caller folds
  `Dag::nodes()`, and a test that built nodes by hand builds the objects
  and collects them into a `Dag`. Two compile-fail doctests pin it.

- **A schema's vocabulary is a value (design §5, stage 10):**
  `Schema::Vocabulary` asks `Clone + Debug + Send + Sync` and no longer
  `Default`; `Schema::from_value`, `parse_event`, `parse_envelope`,
  `Envelope::from_value`, `Change::from_call`, `dag::decode`,
  `Dag::from_prints` and `memory::receive` take the schema they parse at;
  `Schema::REGISTERS` is `Schema::registers(vocabulary)`, and
  `fold::Product` gains `registers`, which `conflicts` reads.
  `EventStore::new`, `MemoryStore::default` and the wasm's `Replica::of`
  stay, for a schema whose vocabulary has a `Default`. The wasm's `Json`
  names its key field and registers off the schema value (`Json::KEY` is
  `Json::key_field`, `Json::register` takes the schema).
- **A change's deps are its entity's heads (SPEC §3):** `registers::deps_for`
  answers every write to the entity, in any register, that no other write
  to it descends from (`registers::heads`), so the store on disk, the store
  in memory, the overlay, a `Decision` and the wasm `append` all write
  them. Supersession is unchanged and per register (§6.6): a write still
  supersedes exactly the writes to its own registers it descends from.
  `verify` accepts a dep in any register of the change's entity, and
  reports one of another entity (`Finding::Foreign`, where
  `Finding::Unwritten` reported a dep in no register the change writes).
  `conformance/change.py`'s written objects and answers are derived again
  under the rule; its bases, verify rows and prices are as they were. A
  host changes nothing but what it expects of new objects' deps; one that
  stamped its own writes instants apart to order them may stop.

- **`EventStore::dag` answers `Arc<Dag<E>>`**, the memory's, where it
  answered a copy; a caller that needs an owned `Dag` clones it.
  `EventStore<E, Pol>` asks `E: Schema` of its type. `EventStore::load`
  rereads one file and no longer feeds anything. `adopt` and
  `adopt_objects` refuse a store holding a file that does not verify, as
  every read of it does; quarantining the file first, then adopting the
  object back, is the repair.

- **`fold::Product` has a required method, `reading`**: a product names its
  registers' types, which only it knows. An implementation answers each
  register's `Frontier::reading` under that register's type.

- **A store's type parameter is its schema, the event type, where it was the
  payload.** `EventStore<P, Pol>` is `EventStore<TodoEvent<P>, Pol>`, and so
  `Dag`, `Envelope`, `Change`, `registers::{Node, Stamp, Folded, Prodrome}`,
  `fold::Frontier`, `decode`, `parse_envelope`, `parse_event` and
  `EventVocabulary`. `Policy<P>` is `Policy<E: Schema>`, and asks about
  `&E`: a call through a blanket policy (`Untrusted`, `Everything`) on an
  `Arc<E>` or a `&&E` names the event, `policy.standing(&*stamp.event)`.
- `Folded::todos` is `Folded::entities`. `registers::deps_for` answers a
  `Result`, refusing a write an inflationary register would refuse.
- `fold::Registers::read(stream, at, policy)` is `fold::read(stream, at,
  policy)`; `frontier` and `conflicts` are `fold::Product` methods, so a
  caller imports the trait. `fold::env` takes any `Bind`, `fold::flatten`
  any `History`, and `fold::link_specs` any key that reads as a string.
- **`view::Entry` is `Entry<E: Row>`:** `todo` is `key`; `outcome` and
  `content` are the todo `reading`'s fields, `entry.reading.outcome` and
  `entry.reading.content`, read by `Entry::outcome()` and
  `Entry::content()`; `claim` is the claimed `todo::Reading`;
  `conflicts` is keyed by the schema's register names (`fold::Kind` for a
  todo); `list_order` is generic.
- `EVENT_SIGNATURES` is gone: `event::ENVELOPE_SIGNATURES` and
  `todo::TODO_SIGNATURES` are its halves.

- **A value is `Option<f64>`**, `None` being `∅`, so it cannot be read as a
  number: `fpl::fulfillment`, `Compiled::fulfillment`, `Explanation::value`
  and `breaks::Knot::value`. prodrome-wasm's `fulfillment` answers
  `undefined` for `∅`, and a knot or an explanation node `null`.
- **Every `view::Entry` has a function:** `Priced` is `Price` and
  `Entry::priced: Option<Priced>` is `Entry::price: Price`, whose `spec` is
  `Absent` where the todo has no spec and no checklist and whose `value` is
  `Result<Option<f64>, LinkError>`. `Entry::spec` answers `&Term` and
  `Entry::value` a number, `∅` or the `LinkError`.
- On the wire an entry's `value` is a number, `"absent"`, or `null` where it
  does not link, and its `spec` is always a print. `prodrome show` prints
  `price absent` and the spec `Absent()`; `prodrome list` shows `∅` in the
  price column (`—` still marks a function that does not link).
- The Typst package reads `order` from its data, and draws `"absent"` and
  `null` values as unpriced; the viewer marks every todo the store knows.
- The frozen view vectors change in exactly their unpriced rows, which now
  read `value='absent'` and `spec='Absent()'`.
- **`prodrome::term` holds the functor:** `Term`, `TermF`, `CurvePoint` and
  `normalize` move there from `fpl`. `TermF::traverse` is the one function
  that names every field; `map` borrows the layer, `as_ref` joins
  `children`, and `transpose` is gone. `Term::cata`, `try_cata`, `para` and
  `any` fold a term, `breaks::Breaks` is a monoid whose `Default` is its
  identity (`exact: true`), and `breaks::constant` is gone.

- **`append(event)` writes a `Change` (SPEC §3)**, and nothing when the
  prodrome already holds an event that prints the same, answering the first
  object carrying it; its `parents` argument and the sealing on every tip
  are gone. A store needs a genesis to append to: `init` one, or a legacy
  store's root is its genesis. `merge` stays, for a legacy store's tips.
- **A register reads as its candidates (SPEC §6):** the distinct events of
  its frontier, twins one. `Folded::chosen`, `chosen_of` and the
  linearisation's pick are gone, and so are the sequential folds
  (`chronological`, `env_at`, `specs_at`, `authored_at`), `fold::Binding`,
  `fold::Env` and `evaluation_env`: `fold::env`, `specs`, `content` and
  `flatten` project a todo's registers at an instant, keyed by `(genesis,
  todo)`, and answer `fpl::Env`, whose outcomes are `fpl::Candidates`.
  `After` reads the least of its candidates, and `flatten` writes `Least`
  over a conflict's worlds.
- **`view::Entry`** carries its `genesis`, its `outcome` and `claim` as
  candidate sets, and `content` as the candidate records' names;
  `list_order` breaks ties by `(genesis, id)`.
- `store::tips_of` and `linearise` are `Dag` methods; `registers::nodes_of`
  is `Dag::nodes`; `read_dag`, `read_dag_named`, `read_dag_at`,
  `objects_from` and `read_chain` are gone.
- prodrome-wasm: an outcome in conflict crosses as the array of its
  candidates, `null` for open, in `fold`'s `env` and in an entry (`state`
  joined by `|`). `fold`'s `history.bindings` is each todo's state register
  read at every instant the reading changes, ascending, where it was every
  binding write in chain order; `series_knots` reads either. Over the pinned
  DAG, no knot moves. `verify_objects`' `genesis` lists `Genesis` objects
  and legacy roots, never a change with no deps. No export is added or
  removed.
- prodrome-github leaves a replay to `append` rather than filtering the
  prints the store holds, and dates a claim past what its deps rest on
  rather than past every event in the store.
- `conformance/dag.py`: the `env` of seeds 9, 27 and 34, whose state is in
  conflict, reads as the candidate set (law 22). Nothing else moves.

### Changed

- **The README is short, and a new guide explains the model.** The README
  now contains a description of Prodrome, its properties, the installation
  steps, one example, and links. The other sections moved to `docs/`:
  - `docs/guide.md` first explains the model: how Prodrome computes the
    state of every todo at a chosen time from the full history of events.
    Then it explains pricing, the Rust API, the files on disk, and
    WebAssembly.
  - `docs/development.md` explains the tests, the CI, the conformance
    vectors, and the layout of the repository.
  - The roadmap and the viewer each have their own page, `docs/roadmap.md`
    and `docs/viewer.md`.
  - `docs/README.md` lists all of these pages.

  This change does not change any code, law, vector, or stored byte.
- **CI: the pull request loop is about a minute (README, Testing).** A
  pull request's `cargo` job runs fmt, clippy and the doctests (`lint`)
  beside the workspace's other tests (`test`), with the flake's toolchain,
  each in a `target/` restored from its run on main, and recompiles only
  the crates whose sources it changed: freshness
  is decided by the git blob names of what an artefact was built from
  (`.github/scripts/target-sources.sh`), not by checkout times. Each
  property law runs at most `PRODROME_MAX_CASES` cases, 32 on a pull
  request, through one rule every law's configuration now goes through
  (`core/tests/common/cases.rs`); main and a new nightly run take every
  law's full count, the nightly with fresh seeds. `nix flake check` stays
  the gate on main and runs beside the loop on a pull request. No law,
  vector or stored byte changes.

### Fixed

- **A change rests on its genesis (SPEC §3, law 43).** `parents_of`, the
  one definition of what an object rests on, left out the genesis a
  change, a snapshot or a key object names, so every `Genesis`-rooted
  store's heads held its genesis, `ancestors` and `closure` stopped short
  of it, and the linearisation could put a change before it. It now
  answers the genesis beside the edges, and every reader moved with it:
  `Dag::tips` and `tips_in`, `linearise`, `closure`, `interior`,
  `ancestors`, a store's memory and `receive` (whose second definition,
  parents plus genesis, is gone), `verify`'s missing parent, and the wasm
  exports' `tips`, `since` and `verify`. The fold's incremental insert
  counts a scope's genesis as inside it, since it orders nothing within
  one; `verify` reports an edge to a stranger genesis once, as the
  stranger. `Snapshot::parents`, a snapshot's edges without its genesis,
  is gone: `parents_of` is the one answer. Laws:
  `core/tests/all/rests_on.rs`.
- **`fsck` quarantines what fails its hash and loses no byte (SPEC §3,
  law 40).** One damaged file stopped every read until someone named it to
  `quarantine`; `fsck` now finds and moves every such file, durably and
  under the lock, names each in its report, and never overwrites one set
  aside before (a second file under one name lands beside it). The store
  then reads the history without the object and everything resting on it.
  Law: `quarantine_is_the_down_set_without_what_rests_on_it` in
  `core/tests/all/down_set.rs`; `prodrome-cli`'s verbs.
- **A store reads the largest down-set it holds (SPEC §3, law 40).** A
  store missing an object (set aside, or not arrived) read every other
  object across the gap: an object resting on the missing one was folded,
  was a tip, and was a head an append wrote over, though no replica that
  lacks the object could hold it, and `events()` and the CLI's reads
  refused outright on the missing parent. The store's history is now the
  interior of its objects, `Dag::interior`, every object whose ancestors
  are all held; one outside it waits, verified, and joins when what it
  rests on arrives. `Dag::nodes_across_gaps` is gone. Laws:
  `core/tests/all/down_set.rs`.
- **Every file the store writes is written durably (SPEC §3).** One write
  serves them all: a randomly named temp in the target's own directory,
  synced, renamed, and the directory synced. A directory the store makes
  (the root, `objects/`, `quarantine/`) was not synced into its parent, so
  a crash could lose a fresh store's `objects/` with every object synced
  inside it; each is now synced into its parent as it is made.
- **The store writes its own `.gitattributes` (SPEC §3, README).** The
  attribute that keeps git from rewriting an object's bytes was documented
  and only this repository carried it, so a store elsewhere in a repository
  was unprotected. A write to a store with no `.gitattributes` in its root
  writes one, `objects/** -text -diff` and `quarantine/** -text -diff`, and
  one that is there is never rewritten. `verify` names a temp the root's
  write left (`Finding::Garbage` is now a path within the store). Tests:
  `the_gitattributes_is_written_durably_and_never_rewritten`,
  `a_directory_is_made_with_its_ancestors` and, now asserting the store
  reads as it did, `a_crash_before_the_rename_leaves_only_a_temp`, in
  `core/src/store.rs`.
- **A mixed store reads each todo as one stream.** A caller folding a store
  of legacy objects and changes object by object (`Node::of`) split a todo
  whose changes name the legacy root into two prodromes; nodes now come
  only from the DAG, which places every such change with the `Sealed`s of
  its todo. Test: `a_mixed_store_folds_a_todo_as_one_stream` in
  `core/tests/all/registers.rs`.
- **A handle sees a file change under a name it has read (SPEC §3).** The
  parent index a handle kept said what it gave up: an object tampered with
  after the handle verified it was not noticed by `tips()`. The memory keeps
  each file's `stat` instead, and a file that no longer looks as it did is
  read and verified again before anything is read from memory. Test:
  `a_file_changed_under_a_held_name_is_read_again`.
- **A write is durable (SPEC §3).** An object was written to `<name>.tmp`
  and renamed with no sync, so a power loss could leave a zero-length file
  under a name that promises content. It is now written to a randomly named
  temp file created exclusively, synced before the rename, and `objects/` is
  synced once per append or adoption. Law: a crash between the write and the
  rename leaves no object, only a temp that `verify` reports
  (`a_crash_before_the_rename_leaves_only_a_temp` in `core/src/store.rs`).
- **`verify` sees everything in `objects/` (SPEC §3).** It looked at `.py`
  files only, so a temp an interrupted write left, or any other stray, was
  never reported. Every entry that is not `<name>.py` for a well-formed name
  is now a finding, by name; the reads pass over such entries rather than
  refusing a `.py` whose stem is not a name. Law: `verify_names_every_stray`
  in `core/src/store.rs`.
- **One damaged object no longer stops every write (SPEC §3).** `tips()`
  refused a store holding a file that fails its hash, and `append` asks
  `tips()`, so one such file stopped the store with no way out. It still
  refuses rather than guess, and now names the object and the fix:
  `EventStore::quarantine(name)`, and `prodrome quarantine <name>`, move a
  file failing its hash from `objects/` to `quarantine/` (a file that hashes
  to its name is refused), after which the store answers and `verify`
  reports a receipt for the quarantined file until the object is restored.
  Test: `after_quarantine_the_store_answers_and_verify_holds_the_receipt` in
  `core/src/store.rs`, and `prodrome-cli`'s verbs.
- **Git must not rewrite object bytes (SPEC §3, README).** A store under git
  needs `objects/** -text -diff` in its `.gitattributes`, or a line-ending
  conversion leaves files that no longer hash to their names; documented,
  and added to this repository's own `.gitattributes`.
- `breaks::series_knots` puts a knot a second before a jump that falls
  exactly on the window's end, as it does for every other jump, instead of
  drawing a ramp across it. No frozen series moves.

### Unchanged

- **The stored bytes.** `Absent` is a new constructor; a record whose `spec`
  is `None` reads as `Absent` at the todo, and `flatten` is unchanged, so an
  old store reads as before except that a `Ref` to an unpriced todo now
  links. Every reference vector answers what it answered.
- `Genesis`, `Change`, `Snapshot` and `Least` are new constructors, and
  `Sealed` and `Woven` are read as ever. A store with none of the new
  objects derives, linearises and verifies exactly as it did, and reads as
  it did wherever no register holds two candidates.
- Every object written before a change's deps became its entity's heads
  keeps its deps and reads exactly as it did: `conformance/dag.py`, every
  other vector, `wasm/snapshots/dag-1.txt` and
  `wasm/conformance/term-json.json` are unchanged. A replay of an event the
  prodrome holds still writes nothing: the twin check compares the event,
  not the deps.

## [0.9.0] - 2026-09-27

A MINOR version under this file's rule: no stored object
changes, every vector answers what it answered, and the API breaks where the
heads used to be. A store's tips are DERIVED from its objects (SPEC §3), so a
store is its `objects/` and nothing else, and a `git merge` of two clones of a
store is the Prodrome's own union with nothing that can conflict.

### Changed (breaking)

- **Tips are derived (SPEC §3):** the objects no object names as a parent.
  `store::tips_of` is the pure function; `EventStore::tips` applies it to the
  store and is now fallible (`Result<BTreeSet<Hash>, _>`), since it reads
  every object it has not verified.
- **`HEAD` and `refs/` are gone, not cached.** Nothing needed them: every read
  already loaded the objects, the objects imply exactly the heads the files
  named (law 17, checked on every `conformance/dag.py` store laid out as the
  0.8 writer left it), and a cache is a second record of the heads that can
  disagree with the first — which in a git-tracked store is also the one file
  two clones can conflict on, the thing this change exists to remove. Old
  stores READ unchanged; the store never writes or deletes those files, and
  `verify` reports each as a leftover, so dropping them (`git rm HEAD refs`)
  is an explicit act. This repository's own `roadmap/HEAD` is removed; the
  tip derived is the tip it named.
- `EventStore::tip` is removed. "The" tip of a store with two is not a
  question with an answer; callers ask `tips`.
- `append(event, None)` seals on EVERY tip: none is genesis, one is a
  `Sealed` byte for byte as before, several is a `Woven` carrying the event.
  In 0.8 it sealed on `HEAD` alone and left the other head standing; now the
  next write after a union settles it. `merge(None, _)` joins every tip, as
  before.
- `verify` no longer reports an unreachable object or a stale head (neither
  can exist: every object is a tip or beneath one), nor anything about HEAD
  and `refs/` agreeing; it reports a leftover `HEAD` or `refs/`. Every
  missing parent is reported, not only the first a walk reached. An object
  that nothing names — once an "orphan" — is a tip.
- `adopt`/`adopt_objects` verify every object before writing any, and write
  parents before children, because an object is in the store the moment its
  file is: a refused adoption leaves nothing behind, and a crash mid-adoption
  leaves a store closed under parents. There is no placement step left.
- `prodrome-wasm`: `verify_objects(objects)` derives the tips from the
  objects it was handed (the `tips` argument is gone) and answers them.

### Added

- `EventStore::objects`, every object by name, reverified — the one read the
  others are taken from; `read_dag` is one pass over the files.
- **A parent index** in each `EventStore` handle (shared by its clones): name
  → parents for every object it has verified, so `tips` is a directory
  listing plus the objects it has not seen. A memo of a pure function (a
  name is the hash of its bytes), so it needs no invalidation, and a deleted
  file is simply no longer listed. Measured in release, on a synthetic store
  of 1000 objects (366 KB): 39 ms to derive by loading everything, 2.8 ms
  warm through the index, against 2.6 µs to read 0.8's `HEAD`; 0.6 ms for all
  40 `dag.py` stores. The one thing it gives up, stated and test-pinned: a
  handle that verified an object before it was tampered with keeps its
  parents for `tips`. Every read of the objects, and `verify`, rehash every
  file.
- **SPEC law 17** and its evidence: `core/tests/tips.rs` (a union's tips
  compose, and two stores' files in one directory derive them; the listing
  order does not matter; the tips cover the store and none rests on another,
  on random DAGs, pure and on disk); `core/tests/fold_laws.rs` (two writers'
  unioned directories read, fold and view as their `adopt`-merge, and the
  next append weaves what a merge would); `core/tests/dag.rs` (derived tips
  equal the old `HEAD`/`refs/` on every conformance store). The vectors are
  unchanged.

## [0.8.0] — 2026-09-25

A MINOR version: recurrence, as one new event kind and two new terms. No
stored byte changes and every existing vector answers what it answered; the
environment's type grows a second half, and every caller follows it.

### Added

- **`Tended(todo, at, actor, note)`, a new event kind (SPEC §4):** a pass at
  a todo that is never done. It changes no state, spec or content, and it puts
  no piece in `flatten`. `event::mk_tended`; the reference policy lets a
  roster actor's tending only claim, like a completion.
- **The tendings (SPEC §6.1):** per todo, the instant of every binding
  `Tended`, a grow-only set folded by union. `env_at`, `history` and the
  registers carry it; a tending writes no register, so concurrent tendings
  are their union and never a conflict. `fpl::last_tended(env, todo, now)` is
  the latest tending at or before `now`.
- **`Recur(todo, anchor, term, pending)` (SPEC §7.3):** `term`, authored
  against `anchor`, re-anchored to the last tending of `todo` as `After` is to
  a completion; `pending` before the first. `mk_recur` refuses a `todo`
  `TodoId` would refuse. Its explanation notes `bound` (`pending` or
  `tended`), and when tended, `tended` and `agoHours`.
- **`Periodic(period, anchor, term)` (SPEC §7.3):** `term` read at `anchor +
  ((now − anchor) mod period)`, the remainder Euclidean, so moments before
  `anchor` repeat too. `mk_periodic` refuses a period that is not positive.
  Its explanation notes `cycleStart`.
- `fpl::phase(period, anchor, now)`, the instant a `Periodic` reads.
- The chain compiler resolves a `Recur` into the schedule its tendings make:
  pending before the first, and from each tending the body slid by its
  slippage. §9.13 now quantifies over tendings too.
- **SPEC laws 15 and 16** and their evidence: `core/tests/fold_laws.rs` draws
  `Tended` among the generated kinds and checks a tending changes no state,
  spec, content, function, register or entry, and that tendings merge by
  union; `core/tests/fpl_laws.rs` checks `last_tended` reads as of now,
  `Recur` re-anchors, waits and ignores later tendings, `Periodic` repeats,
  and both round-trip and refuse what they must. `conformance/recur.py` is
  NEW and written BY HAND: `Tended` prints folded under the reference policy,
  a claimed pass among them, and readings to 1e-9. `Recurs` and `RecurCase`
  join the vector vocabulary.
- `prodrome-wasm`: `lifecycle` spells `Tended`; the JSON codec has `recur`
  (`todo`, `anchor`, `term`, `pending`) and `periodic` (`periodHours`,
  `anchor`, `term`).

### Changed (breaking)

- `fold::Env` and `fpl::Env` are structs, `outcomes` beside `tended`
  (`TodoId`/`String` → `BTreeSet<Instant>`), where they were maps of
  outcomes. A caller that read the map reads `env.outcomes`; `Env::new()` is
  still the empty environment. `fold::History` holds the tendings too and
  answers them from `tended()`.
- `TodoEvent` has a `Tended` arm; an exhaustive match gains one.
- The wasm environment crosses as `{"outcomes": {…}, "tended": {"<todo>":
  ["<iso>", …]}}`, both in `fold`'s `env` and in what `fulfillment`,
  `explain` and `compile` read; `fold`'s `history` is `{"bindings": {…},
  "tended": {…}}`, which `series_knots` reads. A half left out is empty, so
  `{}` is still the empty environment; any other key, the old bare map
  included, is refused.

### Unchanged

- **The stored bytes**, and every existing conformance vector: `Tended`,
  `Recur` and `Periodic` are new constructors, and a store without them reads
  exactly what it read before. `flatten` gains no case.

## [0.7.0] — 2026-09-25

A MINOR version: a new constructor, and the evaluator's signatures move so
that an unbound reference is a type error. No stored byte changes, and every
existing vector answers what it answered.

### Added

- **`Ref(todo)`, a new FPL leaf (SPEC §7.2):** the fulfillment of another
  todo, for subtodos and groups whose demand is composed from other todos'.
  It prints as `Ref(todo='<id>')`; `mk_ref` and the parse refuse an id
  `TodoId` would refuse. A term holding one is OPEN.
- **`fpl::link(term, specs) -> Result<Closed, LinkError>`:** every `Ref(x)`
  replaced by `specs[x]`, recursively — bind for the free monad over `TermF`.
  A rebuilt `Piecewise` goes back through the normal form. Refusals are
  values: `LinkError::Unknown(id)`, and `LinkError::Cycle(path)`, found on
  the path being expanded, so `link` is total and never loops.
- `fpl::Closed`, a term with no `Ref`: `Closed::of(term) -> Option<Closed>`,
  `term()`, `into_term()`, `normalize()`. `TermF::transpose`, the traversal
  in `Result` that `link` is written with. `From<LinkError> for
  ProdromeError`.
- `fold::link_specs(&flatten(…))`: a chain's functions as `link` reads them,
  so `Ref(x)` means x's whole function, its lifecycle included.
- `Entry::unlinked() -> Option<&LinkError>`.
- **SPEC law 14** and its evidence: `core/tests/fpl_laws.rs` checks a closed
  term links to itself, a reference reads what its spec reads, linking
  commutes with substitution order, the linked term is in normal form, a
  loop is refused with a real cycle, and `Ref` round-trips print and parse.
  `conformance/link.py` is NEW and written BY HAND from those laws: exact
  linked prints, readings to 1e-9, an unknown todo and a cycle refused. Its
  names, `Links`, `LinkCase` and `Refused`, join the vector vocabulary.
- `prodrome-wasm` exports `link(term, specs)`, answering the closed term in
  the JSON shape (`{"kind": "ref", "todo": …}` is the new tag). `json_entry`
  grows one key, `unlinked`.

### Changed (breaking)

- `fpl::fulfillment` and `fpl::explained` take `&Closed`, as do
  `breaks::series_knots` and `chain::compile`; `chain::compile_chain` and
  `chain::chain_order` take `&BTreeMap<String, Closed>`, and
  `Compiled::term()`/`into_term()` answer a `Closed`. A caller holding a
  `Term` with no reference wraps it with `Closed::of`; one holding a
  function from `flatten` links it with `link(term, &link_specs(&functions))`.
- `view::Priced::value` is `Result<f64, LinkError>`: the entry's value is its
  function LINKED against every todo's function (§6.7). `Entry::value()`
  still answers an `Option<f64>`, `None` where the function does not link.
- The wasm exports that evaluate refuse a term that still holds a `ref`.

### Unchanged

- **The stored bytes**, and every existing conformance vector: `Ref` is a new
  constructor, and a store with none reads exactly what it read before.

## [0.6.0] — 2026-09-10

A VERSION because the core's public API grew: `prodrome::chain` is a module a
reader can depend on. NOTHING ELSE MOVED — no stored byte changes, no vector
changes, no reading changes, and the interpreted path is the one it was.

### Added

- **`chain`, the chain compiler (SPEC §7.1).** `After` is the ONE term that
  reads history; every other constructor is a function of `now` and its
  subterms. `chain::compile(term, env)` erases it — the term comes back with
  every `After` resolved against the snapshot and `normalize`d, so a chain of
  dependencies is ONE schedule at the root instead of a freeze quantifier the
  evaluator re-enters at every sample. `Compiled`'s readers — `term`,
  `fulfillment(now)`, `explain(now)`, `notes`, `links`, `moot` — take NO
  ENVIRONMENT, which is the claim: a compiled term cannot consult one, because
  the only constructor that would is gone.
- EACH LINK IS ONE GRADED OFFSET δ ∈ [0, 1], applied with §7's corrected form.
  `x·(1 − |1|) + max(0, 1) = 1` for every `x`, so a CANCELLED upstream is
  `Offset(1.0, …)` — the moot constant written as the offset it is, with the
  demand that was dropped still in the tree where a reader can see it. δ = 0 is
  the identity and is elided, as is a zero `Shift`. A completed upstream is a
  `Piecewise` whose piece slides the body by the slippage; an unbound one is
  its pending branch and nothing else.
- A cancellation is REPORTED and never silent (§7): every link is a `Link` on
  the compiled term, `moot()` names the cancelled ones, and `explain` carries
  them at the root under `moot`. A 1.0 from a cancellation is indistinguishable
  from a 1.0 from a demand met, and the note is the only thing that tells them
  apart.
- `chain_order` and `compile_chain` take the chain as a MAP of todo to
  function — `fold::flatten`'s answer — and read the graph its links draw over
  those todos. A cycle is refused as a VALUE, `ChainError::Cycle(path)`, the
  path naming the loop; a link onto something outside the map is not an edge,
  because the snapshot answers it.
- **`After.needs` is CONSUMED, by the compiler and not by the evaluator.** §7's
  semantics have never read it and this does not start: consuming it as a value
  would move a reading. It is the link's declared LEAD TIME, and the compiler
  is the first reader holding it beside the upstream's actual instant, so a
  resolved link reports `ready = at + needs`. A host schedules by it.
- **SPEC §9.13**, the law that makes the above an OPTIMISATION and not a second
  semantics: `fulfillment(compile(t, env).term, now, ∅)` equals
  `fulfillment(t, now, env)` to 1e-9 at every instant, the compiled side read
  against the EMPTY environment. `core/tests/fpl_laws.rs` quantifies it over
  the same `a_term()` and `an_env()` §9.5 runs on, with two more properties
  beside it — no `After` survives, and an environment that answers differently
  about everything changes nothing.
- `core/tests/chain.rs` pins the cases: what each link compiles to, as an exact
  print; the cancellation report; that `needs` moves no number; the cycle and
  its message; and THE COST, as a test rather than as a claim — a chain of
  three links under one `Within` costs up to 65·3 = 195 environment lookups per
  interpreted evaluation and none at all compiled, asserted on the counting env
  in both its readings (the `After` nodes that could ask, and an environment
  that would show if one did) and never on the clock.
  MEASURED AND STATED AS MEASURED: 390 000 lookups become 0 for a 19 µs
  compile, and the wall clock moves about 7% in release, because a lookup in a
  three-entry map is cheap. What the compiler buys is that the environment
  leaves the query path — a compiled term can be cached, stored, shipped and
  evaluated where no history exists — and not a constant factor.
- `prodrome-wasm` gains ONE export, `compile(term, env)`, returning the
  compiled term's canonical print beside its links. The other exports are
  unchanged in shape and in answer.

### Unchanged

- **The stored bytes**, and every conformance vector. The compiler adds no
  constructor, reads no new field and changes no existing term's value; §9.8's
  150 terms answer exactly what they answered, which is the frozen pin under
  §9.13's property.
- **The interpreted path.** `fpl::fulfillment`, `fpl::explained` and
  `breaks::series_knots` are untouched, and a host that never compiles reads
  what it read before. Compiling is an optimisation a caller opts into.
- The wasm wire, apart from the one export added above.

## [0.5.0] — 2026-09-10

A VERSION AND NOT AN `Unreleased`: a new crate ships in the workspace and
`packages.default` changes, which are things a reader can depend on, and both
belong under a number rather than under a heading that never settles. Nothing
about the format moved.

### Added

- **`prodrome-cli`**, the `prodrome` binary, over `prodrome-core` with the
  reference payload (`reference::Todo`) and the reference policy
  (`policy::Untrusted`). The verbs: `init`, `add`, `done`, `cancel`, `reopen`,
  `revise`, `list`, `show`, `verify`, `weave`. `--store DIR` defaults to
  `./roadmap` when that directory exists and to `.` otherwise; `--actor`
  defaults to `$PRODROME_ACTOR`, then `$USER`; `--untrusted NAME,…` (or
  `$PRODROME_UNTRUSTED`) is the READER's standing policy, so one invocation
  shows the confirmed reading and the claimed one at once.
- IT IS A HOST, AND IT IS WHERE THE CLOCK IS. The core has none — an event's
  `at` is data (§4) and a fold is a query at an instant the caller names (§6) —
  so `--at` names one and the machine's clock is only what a caller who did not
  falls back to. On a write `--at` is the instant STAMPED ON the event, which
  is how a todo that became true yesterday says so.
- A price is stated as `--priority N`, a constant, or as `--deadline
  YYYY-MM-DD` with `--start`, `--end` and `--lead-up-days`, a `Decay` onto
  17:00 that day. The scale is fulfillment as a percentage, so LOW IS URGENT
  and there is no second scale to invert.
- `weave` is `EventStore::merge(None, None)`: every head into one `Woven`
  carrying no event, which is what settles a store two branches both appended
  to. No actor and no note, because a merge asserts structure and not a fact
  about a todo (§3).
- **`roadmap/`**: this crate's own todos, as a store of objects, seeded by the
  verbs above and committed like any other directory. The historical items
  carry the instants their releases were actually cut, so `prodrome list --at`
  answers about the past with what was believed then. It is the one example
  that cannot go stale: CI runs `prodrome verify --store roadmap` and
  `prodrome list --store roadmap`, so a pull request that appends to it proves
  its objects rehash and that the store still folds.
- THE STORE HOLDS NO POLICY and CI names no allowlist — 0.3's point, applied to
  this repository. Objects are content-addressed and never rewritten, so a pull
  request's diff is exactly the objects it adds, the objects are reviewed like
  code, and merging one is a maintainer's act. README's *Contributing* says it
  in those words.
- `nix build .#prodrome-cli` builds and tests the binary, and it is a
  `nix flake check` check. `packages.default` is now the binary rather than the
  wasm; `nix build .#prodrome-wasm` is unchanged. `roadmap/` is deliberately
  NOT in the flake's fileset: it is data the binary reads, not source it is
  built from, so appending a todo rebuilds nothing.

### Unchanged

- **The stored bytes.** No constructor, field or print changed; every object
  written by 0.1.0 through 0.4.0 parses, prints back byte for byte and hashes
  to the same name. `roadmap/` is written by the same printer that wrote the
  vectors.
- **The wasm wire**, and every conformance vector.
- `prodrome-core`'s API. The binary is a consumer of it and added nothing to
  it: `EventStore`, the folds, `view::entries` and `policy::Untrusted` are what
  the verbs are made of.

## [0.4.0] — 2026-09-10

### Changed

- **The vectors are literals** (SPEC §2). `conformance/` held 1.9 MB of JSON
  in a crate whose claim is that there is ONE reading of its data: one closed
  grammar, one printer, one parser. Evidence written in a second format was a
  second format to keep in step. `conformance/*.py` is that evidence in the
  crate's own grammar, read by the crate's own parser against the VECTOR
  VOCABULARY — a second closed whitelist, disjoint from a store's, so a vector
  name is refused in an object and an object's kind is refused in a vector.
  Every stored object, event and term inside one is a STRING holding its
  canonical print, because those exact bytes are what a vector is evidence of
  (§9.1); instants are `datetime(...)`, and the mappings the JSON keyed by
  todo id or object name are tuples of a named constructor.
- A CHECKED MIGRATION, not a refresh: the vectors are frozen and have no
  generator left to re-run, so the converting commit read the JSON, built the
  literal, printed it, parsed it back, asserted the two values equal, mapped
  the literal back to JSON and asserted it equalled what it started from —
  exactly, no tolerance, every key and every float — for all 400 vectors, and
  ran that under `cargo test`. The counts are unchanged: 120 logs, 40 DAGs,
  150 terms, 60 windows, 30 + 30 view cases.
- **The JSON codec leaves the core.** `fpl::to_json`, `fpl::from_json`,
  `fpl::explanation_json` and `fpl::explain` are `prodrome-wasm`'s
  `json` module now — JSON is JavaScript's literal grammar and the wasm crate
  is the JavaScript boundary. The core keeps `fpl::explained`: the decoration
  is §7's, only its JSON was not. SPEC §7 no longer names `to_json` as part of
  the contract.
- `prodrome-core` depends on neither `serde` nor `serde_json`, as a dependency
  or a dev-dependency. Nothing in the crate parses a second format.
- `core/examples/generate_view_vectors.rs` emits literals, laid out one case
  per line so a diff is readable.

### Added

- `wasm/conformance/term-json.json`: the `json` and `explain` halves of what
  was `conformance/fpl.json`, unchanged, for the same 150 terms — the JSON
  boundary's evidence, in the crate where the JSON is produced.
  `wasm/src/json.rs`'s test replays all of them, and `wasm/src/wire.rs` gains
  the first test of `json_entry`'s ten keys.
- `core/tests/common/vectors.rs`: the vector vocabulary and the accessors a
  suite reads a case with. `common::entry_value` (an `Entry` as the vector
  grammar's `Row`) and `common::explanation_value` (an explanation's
  DECORATION — kind, value, notes — beside the term's own print) replace the
  copy of `json_entry` the core kept in step with the wasm crate by hand.

### Unchanged

- **The stored bytes.** No constructor, field or print changed; every object
  written by 0.1.0, 0.2.0 and 0.3.0 parses, prints back byte for byte and
  hashes to the same name. The format of the EVIDENCE changed; the format of
  the DATA did not.
- **The wasm wire.** Every exported function's arguments and answers are the
  shapes they were — `term_json`, `explain`, `series_knots`, `entries`,
  `fold`, `registers` included, down to the lowercase kind tags, the ISO
  instants and the spans in hours.
- **Every expected answer**, to the digit: what `conformance/*.py` says is
  what `conformance/*.json` said.


## [0.3.0] — 2026-09-09

### Changed

- **Standing is the host's** (SPEC §5, rewritten). The core carried one
  deployment's trust policy: a set of untrusted actor names, plus a
  kind-dependent rule (their lifecycle events did not bind, their content
  records did), plus a `verify` clock rule keyed on the same set. None of that
  is a fact about a DAG of events — it is the same move 0.2 made for the
  record. The database now asks the HOST, one event at a time, and gets a
  `policy::Standing`: `Binds` (the folds take it) or `Claims` (stored, shown,
  never folded). Nothing in the core reads an actor name to decide anything.
- `policy::Policy<P>` is that question as a trait. One required method,
  `standing(&self, event: &TodoEvent<P>) -> Standing`; one provided method,
  `confirms(&self, event) -> bool` ("is this the host's own word"), whose
  default is `standing(event) == Binds`. Not sealed: a host implements it.
- `fold::Untrusted` is now `policy::Untrusted`, the REFERENCE policy — the
  rule `conformance/` was taken under, in the core rather than behind the
  `reference` feature, because it is a policy over the core's own event kinds
  and needs no payload. `Untrusted::none()`, `::of` and `::actors` are
  unchanged; `Untrusted::binds` is gone (ask `standing`). `policy::Everything`
  is new: the policy under which every event binds, which is what §6.7's
  CLAIMED reading is taken under.
- Every fold takes `&impl Policy<P>` where it took `&Untrusted`:
  `fold::{env_at, specs_at, flatten, history}`, `registers::{extend, fold}`,
  `view::entries`. `EventStore<P>` is now `EventStore<P, Pol = Untrusted>` and
  holds the policy by value, so `EventStore::<Todo>::new(root, policy)` takes
  an `Untrusted` where it took a `BTreeSet<Actor>`; `EventStore::policy()`
  hands it back.
- `event::binds(event, &BTreeSet<Actor>)` is gone. It was §5 hardcoded; the
  rule is `Untrusted`'s `Policy` impl now.
- `view::Standing` is `view::Confidence` and `Entry::standing` is
  `Entry::confidence` — the name went to the event-level sum, which is what
  §5 is about. `Provisional` and `Confidence::{of, is_provisional}` are
  unchanged, and `Provisional::Content` now means "the winning content record
  is one the policy does not `confirm`" rather than "its actor is untrusted".
- §3's `verify` states its clock rule through the policy: an event the policy
  does not `confirm`, dated behind any of its ancestors. Same rule, same
  findings, same wording — the reference policy `confirms` exactly the actors
  off its roster, whatever the kind, which is why it overrides the default.
- SPEC §5 is rewritten as STANDING; §3, §6.1–6.5, §6.7 and §8 quantify over
  the policy.

### Added

- SPEC §9.10–9.12, the laws that make "the host decides" honest, as proptests
  over generated logs in `core/tests/fold_laws.rs`:
  - **9.10** under the policy that binds everything, the claimed and confirmed
    readings are equal — every fold answers alike, no entry carries a claim,
    no entry is provisional (`the_two_readings_agree_under_a_policy_that_binds_everything`);
  - **9.11** a claiming event never moves the confirmed reading — append one
    and every confirmed answer at every moment is the answer it was
    (`a_claiming_event_never_moves_the_confirmed_reading`);
  - **9.12** standing selects events, not positions — folding under a policy
    is folding the sub-log of the events it binds, under any permutation of
    the log (`standing_selects_events_and_not_positions`).
- `conformance/view/*.json` (SPEC §6.7): `view::entries` had no frozen
  vectors — they were scrubbed when this crate went public, taken as they
  were on a private chain. SEEDED instead: random logs from this crate's own
  generator (`core/tests/common/mod.rs`'s `a_log`, the same one
  `core/tests/fold_laws.rs`'s properties draw from), under a fixed seed,
  folded at several instants under two reference policies (rostering
  `"triage"` and, inverted, `"bassel"`). `core/tests/conformance_view.rs`
  replays them; `core/examples/generate_view_vectors.rs` is the only thing
  that regenerates them, run by hand, never by `cargo test` or CI.

### Unchanged

- **The stored bytes.** No constructor, field or print changed; every object
  written by 0.1.0 and 0.2.0 parses, prints back byte for byte and hashes to
  the same name.
- **The wasm wire.** `fold`, `registers` and `entries` still take `untrusted`
  as a JSON array of actor names; `wire::parse_untrusted` turns that array
  into the reference policy, which is the rule the argument always meant.
  Every exported function's arguments and answers are the shapes they were,
  `entries`' `unconfirmed` included.
- **Every conformance vector**, byte for byte. `conformance/*.json`'s
  `untrusted` fields are read through the reference policy; the prints,
  hashes, linearisations, frontiers, fulfillments, entries and `verify`
  findings are the ones the vectors held.

## [0.2.0] — 2026-09-09

### Changed

- **A record's fields are the host's** (SPEC §4). `Authored` used to name one
  deployment's schema — `kind`, `created`, `body`, `spec`, `rationale`,
  `category`, `waiting_on`, `detail`, `source`, `subtodos`, `notes`, `note` —
  inside the database. The record kind is now `KIND(todo, at, actor, <the
  host's fields>)`, where a `payload::Payload` supplies the constructor name,
  the fields and their order, the vocabulary those fields nest, their parse and
  print, and the only two readings the folds take: the spec a record carries
  and its checklist length.
- Every type that carries an event is generic over that payload:
  `Authored<P>`, `TodoEvent<P>`, `Envelope<P>`, `EventStore<P>`,
  `registers::{Node, Write, Frontier, Folded}<P>`, and the folds, registers and
  `view::entries` that read them. A type parameter and never a `dyn`: one store
  is parsed against one closed vocabulary, and that vocabulary is the core's
  names union the payload's.
- `literal::Signature` borrows its field order from the vocabulary that
  answered (`Signature<'a>`), so a vocabulary assembled at runtime can lend
  one. `Table::find` keeps the `'static` a static table has.
- `wasm`: a record crosses the boundary as its payload's own `fields()`, keyed
  by the names it stores them under, each literal mapped to JSON by the obvious
  rules. Two keys inside `content`/`records` changed shape: `created` and
  `source.date` are full ISO instants rather than `%Y-%m-%d`, and a record's
  `spec` arrives in the literal shape rather than `fpl::to_json`'s. `todo`,
  `notes`, and a `kind` tag on nested calls are new.

### Added

- `payload::Payload`, and the field readers a host's `from_fields` needs
  (`string_field`, `datetime_field`, `tuple_or_empty`, `spec_field`, …), which
  are the ones the core's own kinds are parsed with.
- `prodrome::reference`, behind the default `reference` feature: the payload
  `conformance/*.json` was taken with, `Todo`, with `Source`, `SubTodo`,
  `Note`, `NoteSite`, `MarkupSource` and `StringSource`. A worked example of
  the trait, and the fixture the conformance suites drive. Turn it off with
  `default-features = false`.
- SPEC §9.1 now says the round trip explicitly for the generic path, and
  `tests/folds.rs` checks it: every `Authored(...)` print in the vectors parses
  under the reference payload and prints back byte for byte.

### Removed

- From `prodrome::event`: `MarkupSource`, `StringSource`, `Source`, `SubTodo`,
  `Note`, `NoteSite`, `mk_authored`, `mk_source`, `mk_note`, `mk_subtodo`, and
  the record's entry in `EVENT_SIGNATURES`. All of it is in
  `prodrome::reference`, unchanged. `mk_record` is the core's constructor for a
  record now: the three fields it owns, and a payload.

### Unchanged

- The stored bytes. Every object written by 0.1.0 parses, prints back byte for
  byte and hashes to the same name under the reference payload.
- The literal grammar, `Hash`, `TodoId`, `Actor`, `Name`, the lifecycle kinds,
  `SpecRevised`, FPL, the folds' semantics, the registers' semantics, and every
  conformance vector.

## [0.1.0]

- The first public release: the literal grammar (§2), objects and the DAG (§3),
  events (§4), trust (§5), the folds and registers (§6), FPL (§7), and the
  conformance vectors the laws in §9 are checked against.
