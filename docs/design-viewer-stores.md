# Design: one viewer for every public store

This note was written before stage 1 was built, and stage 1 was built to
it in the same pull request. For how to use the viewer, see
[`viewer.md`](viewer.md). For the semantics, see `SPEC.md`.

## The problem

The viewer is a static page. It is built with one store beside it, and it
shows that store. A person who keeps a store in a public repository must
build and publish a site of their own to see the store in a browser.

This change lets one hosted viewer show any public store. The page's own
address names where each store is. The page fetches each store, checks
each object with the core, reads the stores, and shows them. With several
addresses, the page shows one list of all their items, most urgent first.
Each row names its store. An item of one store that waits on an item of
another store is shown waiting, and the two items link to each other.

## What the library already has

A program names an item of another store with a qualified reference. A
term `RefIn(store, entity)` reads the price of the item `entity` in the
prodrome whose genesis is `store` (SPEC §7.2). A prodrome is the set of
objects that rest on one `Genesis` object. A genesis is the first object of
a store, and its name names the store. `fpl::link_in` binds such a
reference in a set of prodromes. `view::entries` binds it in the set of
prodromes of the objects that it reads, so a reading of the union of two
stores prices a reference from one to the other. Law 41 states that each
store reads in the union as it reads alone (SPEC §9).

So the library needed no new rule. Two findings follow:

1. **The WebAssembly module did not export `link_in`.** The viewer draws an
   item's thirty days and explains its price from a term that it links
   itself, with `link`. With `link`, a qualified reference reads `∅`, so the
   page would draw a curve that disagrees with the row's price. Stage 1
   adds the export `link_in(term, specs, stores)`. Its `stores` is the
   `prices` answer of the stores, `[{genesis, env, functions}]`, so a page
   passes that answer through and builds no shape of its own.
2. **This repository's roadmap store has no genesis.** It is a legacy
   chain: each object is `Sealed(prev=…)`. No qualified reference can name
   an item in it. The core names a legacy chain by its least root, so the
   viewer uses that name where it needs to know the store, and it can show
   at most one legacy chain in one list (see section 4).

**The rule for a wait.** A todo that waits on another todo has a spec that
refers to it: `Ref(x)` in its own store, or `RefIn(g, x)` in the store
whose genesis is `g` (docs/guide.md, section 7). The waiting todo's price
is computed from the price of `x`. With a bare reference, the two prices
are equal, so the waiting todo is exactly as urgent as `x`. The library
has no rule that changes the price of `x` because something waits on it.
The viewer applies the same rule across stores, because the core computes
every price, a wait across stores included. The page sorts nothing and
prices nothing.

## 1. A store's address

A person writes one of two forms:

- **A base URL**, under which `objects/` lies:
  `https://example.org/stores/team/`. The scheme is `https`. The URL holds
  no user, query or fragment. A build for tests
  (`VIEWER_LOOPBACK=1 viewer/build.sh`) also accepts plain `http` from
  `localhost` and `127.0.0.1`; the hosted page does not.
- **A repository on GitHub**: `gh:owner/repository[@ref][/dir…]`. The page
  turns it into `https://raw.githubusercontent.com/owner/repository/ref/dir/`.
  With no `@ref`, the page asks for `HEAD`, the repository's default
  branch. A ref may hold `/` (`feature/x`), but no part of it may be
  empty, `.` or `..`, and no directory may be `.` or `..`, so the URL that
  the page fetches stays in the repository that the address names. Other
  git hosts use the base URL form.

The addresses go in the fragment of the page's address, after a `?`. A
browser never sends the fragment to a host, so the viewer's host never
learns which stores a person looks at. The grammar is:

```
fragment = route [ "?" store *( "&" store ) ]
route    = "/" | "/in/" label | "/todo/" [ label "/" ] todo
store    = [ label "=" ] where [ "!" genesis ]
where    = "gh:" owner "/" repo [ "@" ref ] *( "/" segment )
         | "https://" host path | "http://" local-host path
label    = 1*32 of a-z 0-9 _ -
genesis  = 64 lowercase hex
```

- **A label** names a store in the page. A link may give one; otherwise the
  page derives one from the last directory, else the repository's name or
  the host. A derived label that another store already has gets a suffix,
  `-2`, `-3`. If a link gives two stores the same label, the page refuses
  the second one.
- **A pin** (`!genesis`) says which store the link expects. If the store
  does not hold that genesis (or, for a legacy chain, that least root), the
  page refuses the store and says why. A pin checks only that the address
  holds that genesis. It does not check that the address holds nothing
  else, or that it holds every object of that store: anyone can write
  objects that rest on a genesis, and a listing can leave objects out. A
  pin is 64 hex characters, because a shorter prefix can be found by a
  hostile host with enough work.
- **Routes.** `#/` and `#/todo/<id>` keep their meaning, so every present
  link keeps working. `#/in/<label>` lists one store. With several stores,
  an item's route names its store: `#/todo/<label>/<id>`. A route with no
  store finds the first store, in the list's order, that holds the id.
- **Reserved characters.** In a component, every character outside
  `A–Z a–z 0–9 - . _ ~ : $ ' ( ) * + , ;` prints as `%XX`, byte by byte in
  UTF-8. So `% & = ! ? # @ /` and spaces never appear raw in a component.
  A ref with a `/` in it prints its `/` as `%2F`. A base URL's path is kept
  in one form: each segment decoded and then escaped by that rule.
- **Lengths.** A link names at most 8 stores. One store's text is at most
  1,024 characters, and the parser reads at most 8,192 characters. Longer
  addresses are refused with a reason, never cut.

`viewer/src/address.ts` holds the one parser and the one printer. Their
laws, checked over generated values by `viewer/test/address.test.ts` on
every build of the viewer:

1. For every fragment value `f`, `parse(print(f)) = f`.
2. For every text `t` that parses, `parse(print(parse(t))) = parse(t)`.
3. The parser never throws: a store's text that is not an address is a
   value that names the text and the reason.
4. The address printed is the address fetched: for every `gh:` text that
   parses, generated `.` and `..` attacks included, the URL that the page
   fetches is one that a browser does not rewrite, under the owner and
   repository that the address names.

## 2. How the page finds a store's objects

A plain file host cannot list a directory. Two ways remain:

- **(a) A manifest.** The store publishes a file that names its objects:
  either all of their names, sorted, or only the heads. The heads are the
  objects that no other object rests on. From the heads, the page can walk
  to every other object, because every object names the objects it rests
  on.
- **(b) The git host's listing.** GitHub lists a ref's files in one request
  (`GET /repos/{owner}/{repo}/git/trees/{ref}?recursive=1`). GitHub
  documents a limit of 60 such requests an hour for each address of a
  visitor who is not signed in, and it cuts a listing short above 100,000
  entries or 7 MB, and says so with `truncated`. This environment's proxy
  signs every request to GitHub, so these limits were not observed here.

**The measurements.** `viewer/test/measure.mjs` serves a store from a local
server that answers each request after 80 ms, as a distant host does. It
sends the requests of one round together, as a browser does over HTTP/2.
It stops when the core has checked the objects and answered the list. Each
way runs twice, and the second run counts.

| Store | Way | Rounds | Requests | Bytes | Time to the list |
| --- | --- | ---: | ---: | ---: | ---: |
| this repository's roadmap, 154 objects | all, by the index | 2 | 155 | 79,614 | 199 ms |
| this repository's roadmap, 154 objects | walk from the heads | 22 | 155 | 71,641 | 1,896 ms |
| generated, 500 items, 1,267 objects | all, by the index | 2 | 1,268 | 395,920 | 428 ms |
| generated, 500 items, 1,267 objects | walk from the heads | 5 | 1,268 | 344,531 | 612 ms |

The bytes are the bodies of the answers. The roadmap's index is 10,319
bytes, and its heads are 2,346 bytes (35 heads). The generated store has a
shallow history, so its walk takes few rounds, but its heads file is 40%
of the size of its index.

A list needs the state of every item, and that state rests on every
object. So for a list, a walk fetches every object too, and it costs one
round for each level of the history. A walk is better only for a page that
shows part of a store, for example one item, or a store too large to read
whole. Stage 1 fetches all objects by an index.

**The decision for stage 1:**

1. The page reads `<base>index.json`, a JSON array of names. Any host can
   serve it, and `viewer/assemble.sh` already writes it beside a published
   copy of a store.
2. For a `gh:` address with no `index.json`, the page uses GitHub's tree
   listing. This costs one request to the API for each load of the page.
   When the limit is met, the page says so, names the time at which the
   limit renews, and says that an `index.json` in the store needs no
   listing. When the listing is cut short, the page says so.
3. For a base URL with no `index.json`, the page says that a plain host
   cannot list `objects/`, and links to how to write one.

**Stage 2: a manifest written by the command line.** A verb such as
`prodrome manifest` would write `index.json` (all names, sorted) into the
store's directory, where the store is published. The file would be
committed beside `objects/`. Today `verify` does not read a file beside
`objects/`, so it accepts such a file and does not check it. Stage 2 would
change `verify` and SPEC §3 in the same pull request: `verify` would read
the manifest and report a name that it lists and the store lacks, and an
object that it does not list. Until then, a person writes the file with
the command in `viewer.md`. A manifest that is out of date costs a reader the objects it does
not list. It never makes the page show a wrong object, because the core
checks every object that the page fetches. Stage 1 does not write to the
`roadmap-data` branch.

## 3. Trust

The page trusts no host. A host, a mirror, a cache or a hostile server can
serve any bytes, and only the listing can change.

- **An object is kept only if the core keeps it.** For each address, the
  page passes the fetched objects to the core's `Todos` (prodrome-wasm) and
  reads `tips()`. The core leaves out every object whose bytes do not hash
  to its name, every object that does not parse, and every object that
  rests on one of those (SPEC §3, law 40). The page keeps exactly the rest.
  The page has no check of a name or of bytes in TypeScript.
- **One object fails.** The page shows the rest of the store, and a notice
  above the list names the object, says why the core left it out, and
  counts the objects that rest on it.
- **The listing names an object that the host does not serve.** The page
  names the object and the host's answer. The objects that rest on it are
  left out by the core as "not held".
- **A listing that leaves objects out** cannot be found from the objects.
  The page shows what the listing names. A pin of the genesis finds a
  listing that leaves out the whole store, but not one that leaves out the
  newest objects. Stage 2 can let a link pin the heads too.
- **A store's genesis is not the one the link pins.** The page refuses the
  store, names the genesis it expected and the ones it found, and shows the
  other stores.
- **Names in a listing** are read once, at the edge: a text that is not 64
  lowercase hex is not fetched, and the page counts it.
- **Bytes as the host sent them.** The page decodes an object as UTF-8 and
  keeps a leading byte-order mark, so the core hashes what the host sent.

## 4. Several stores, one list

- **Each prodrome is read on its own.** An address can hold several
  prodromes. The page splits a store into one part for each prodrome: the
  objects on which that prodrome's heads rest, as the core's `since` finds
  them, labelled `<store>-<first 8 hex of its genesis>`. The page reads each
  part's objects with the core's `entries` and `fold`, as it read one store
  before. The rows, the records, the text of each item and its history
  come from that reading. So two prodromes of one address that use one id
  give two rows. An object of one prodrome that rests on a legacy write is
  read with that write.
- **The list is the core's.** The page reads the union of all stores'
  kept objects once with `Todos.entries`. The core keys every row by its
  genesis and its id, prices a qualified reference through the set of
  prodromes in the union, and sorts the rows (SPEC §6.7: ascending value,
  ties by genesis and id, then the rows with no number). The page takes
  the order and each row's price from that answer. By law 41, a store's
  rows read the same in the union as alone, except that a reference to
  another loaded store now reads that store's item.
- **Items never merge.** With several stores, a row's key in the page is
  `<label>/<id>`, and every map of the page is keyed by it. Two stores
  with the same ids give two rows. Each row shows its key, so it names its
  store.
- **A filter by store.** Above the list, the page links each store's own
  list, `#/in/<label>`. That list keeps the core's order.
- **Waits across stores.** For each item, the page finds the references in
  its function (the JSON terms of `fold`). A `RefIn` to a genesis of a
  loaded store links to that item, and that item's page links back with
  "Waited on by". A `RefIn` to a store that is not loaded is shown as
  "waits on `<genesis>…/<id>`, in a store not loaded", and the core reads
  its price as `∅`. A `RefIn` to an item that a loaded store does not hold
  is shown as such, and also reads `∅`.
- **One store at two addresses.** Addresses that hold the same genesis (or
  least root), directly or through another address, are one store: an
  address joins every earlier store with which it shares a prodrome, and
  those stores become one, so the grouping does not depend on the order of
  the addresses. The store holds the union of their kept objects, as a
  merge does, so where the copies differ, the store holds every object
  that either copy holds. The first address names the store. The page says
  that it joined the addresses, and names the objects that come from only
  one of them.
- **One store's failure stays its own.** A store whose listing, objects or
  reading fails is a notice, and the page shows the other stores.
- **Legacy chains.** The core reads every legacy object as one prodrome
  with no genesis. So the page shows at most one legacy chain in a list,
  and refuses another one with a notice.

## 5. What it costs a host

- **Fetches from another origin.** The page fetches from the store's host,
  so the host must send `Access-Control-Allow-Origin`. These were checked
  with `curl -H "Origin: https://example.org"`:
  `raw.githubusercontent.com` sends `*` and `Cache-Control: max-age=300`,
  and it serves the ref `HEAD`; GitHub Pages sends `*` and `max-age=600`;
  `api.github.com` sends `*`. Other hosts were not checked.
- **The page's own policy.** The hosted page's Content Security Policy
  allows `connect-src` to any `https:` host; a build for tests adds this
  machine. Each fetch omits credentials (`credentials: "omit"`), so no
  cookie of the visitor goes to a store's host, and sends no `Referer`, so
  a store's host does not learn the page's address. A fetch follows a
  redirect: wherever the bytes come from, the core checks them against
  their name, and the policy allows only https.
- **Caching.** An object's name is the hash of its bytes, so an object
  never changes. From this site, the service worker keeps each object in a
  cache for good. From another host, it keeps nothing itself: the page
  puts an object in a cache only after the core has kept it, and at most
  20,000 such objects, and the worker serves an object of another host
  from that cache or from the network. So a host that serves wrong bytes
  once is not remembered, and a link cannot fill the browser's quota with
  unchecked bytes. The worker does not cache a listing of another host. A
  host's own cache can keep an old listing for some minutes (300 s for
  raw.githubusercontent.com). The page then shows the store as it was, and
  never a wrong object.
- **Bounds.** For one address, the page reads at most 20,000 objects, an
  object of at most 1 MiB, at most 64 MiB of objects, and a listing of at
  most 16 MiB. It sends at most 16 requests at once, and stops after 60 s.
  At each bound, the page says which bound the store met, and shows what
  it read.
- **Load.** A load of a store costs one listing and one request for each
  object that the browser does not hold. A second load costs the listing
  only.

## 6. What a link can do to a visitor

Anyone can write a link that makes this page load a store that they host.
The page keeps these promises to the person who opens such a link:

1. **No text of a store that a link names is run.** An item's text can be
   Typst markup, and markup is code: it can spell any HTML element, loop
   for ever, and read the data of every loaded store. So the page typesets
   the text of a store that this site does not serve as written, and says
   so. Only this site's own store, on a page that shows nothing else, has
   its text read as Typst. (In the viewer, an item's markup runs inside the
   package's `eval`, where `/data.json` names a file of the package, so it
   cannot read the data today; the rule does not rest on that.)
2. **The typeset part holds no script, style or form.** For every store,
   this site's own included, the page drops scripts, frames, plugins,
   forms and their controls, `<style>`, media, SVG animations and foreign
   objects; every handler `on…`; every `style` and `id` but Typst's own on
   its drawings; and every `name`, `ping`, `formaction`, `srcset`, `srcdoc`
   and `target`. A link must go to `#`, http(s) or this site. The page's
   policy forbids inline script as well.
3. **The page says where each store comes from.** Above the list, outside
   the typeset part, the page names every store with every address it was
   loaded from, whole, and marks each store that this site does not serve.
   A link's label cannot pass a store off as this site's own.
4. **Only checked objects are shown and kept.** See section 3 and the
   caching above.
5. **A link costs a known amount.** See the bounds above.
6. **Nothing goes to the viewer's own host.** The addresses are in the
   fragment, and every fetch goes to a store's host, with no credentials
   and no `Referer`.

`viewer/test/stores.mjs` checks each of these with a store, `gamma`, whose
items are crafted to attack the page: a style element that hides the
notices, a fixed layer with one link, a read of the data file, an endless
loop, and an element named `createElement`.

## 7. Writing, later

Stage 1 writes nothing. The smallest honest interaction for a store kept
in a public repository is this:

1. A person fills in a form on the page: a proposed item, a comment, or a
   changed price.
2. The page builds the new object with the core:
   `Todos.append(event, genesis)` already seals an event over the store's
   heads, and answers the object's name and print. The page needs each
   event's constructor through the WebAssembly module, as `lifecycle` gives
   for the lifecycle events. It writes no print in TypeScript.
3. The page hands the object to the person as a change to the repository:
   a file to add at `<dir>/objects/<name>.py`. On GitHub, a link to the
   web editor can carry the file's path and text, so the person commits it
   in a fork and opens a pull request.
4. The store's owner merges the pull request or not. `verify` runs on the
   pull request, as on any change to a store.

The event's actor is a claim, as every actor is (SPEC §5). A signature is
not needed for the owner to decide; later, the page can sign with a key
that the person holds (`Todos.sign`). The page must never write to any host
by itself, never hold a token, and never send an object anywhere except by
the person's own action.

## Stage 1

- `viewer/src/address.ts`: the grammar, its parser and printer.
- `viewer/src/fetch.ts`: the listing and the fetch of a store's objects.
- `viewer/src/stores.ts`: the check by the core, the grouping by genesis,
  the split into prodromes, and the one list.
- `wasm/src/reference.rs`: the export `link_in`.
- `typst/lib.typ`: a row's key, its store and its waits. Each new field is
  optional, so the package's examples render as before.
- The page with no store in its address shows the site's own store, as
  before.

The size of the viewer's script, built with the same tools before and
after: `app.js` grew from 138,105 to 159,812 bytes (39,921 to 47,802
bytes compressed with gzip -9), and `sw.js` from 1,561 to 1,743 bytes.
