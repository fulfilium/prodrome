# The viewer: Prodrome in the browser

The site <https://fulfilium.github.io/prodrome/> shows this crate's
[roadmap](roadmap.md), and any public store whose address is in the page's
own address. The site is read-only and static. The page downloads the
objects of a store, as they are on disk, and the Prodrome core compiled to
WebAssembly checks each object against its name. The page computes each
item's current value in the browser, with the same core. Then it lays out
the result with Typst, also compiled to WebAssembly. No server computes
anything. [`design-viewer-stores.md`](design-viewer-stores.md) gives the
design and its measurements.

## Show your own store

Write a link to the viewer with your store's address after `#/?`. You need
no build and no site of your own.

- A store in a public repository on GitHub, in the directory `roadmap` of
  the branch `roadmap-data`:

  ```
  https://fulfilium.github.io/prodrome/#/?gh:owner/repository@roadmap-data/roadmap
  ```

  Leave out `@branch` for the default branch, and `/directory` for a store
  at the top of the repository.
- A store on any static host that sends `Access-Control-Allow-Origin`,
  under the URL whose `objects/` holds the objects:

  ```
  https://fulfilium.github.io/prodrome/#/?https://example.org/stores/team/
  ```

- Several stores in one list, separated by `&`, each with an optional label
  before `=`:

  ```
  https://fulfilium.github.io/prodrome/#/?docs=gh:owner/docs&app=gh:owner/app@main/roadmap
  ```

The list then holds the items of all the stores, most urgent first, and
each row names its store as `<label>/<id>`. Above the list, the page links
each store's own list. If an item of one store waits on an item of another,
the row says so and links to that item, and that item's page links back.
An item waits on an item of another store when its spec is
`RefIn(store='<the other store's genesis>', entity='<id>')` (SPEC §7.2).

A link can pin a store's genesis: add `!` and the 64 hex characters of the
name of the store's first object. The page then refuses an address that
does not hold that genesis. A pin does not check that the address holds
nothing else, or every object of the store: anyone can write objects that
rest on a genesis, and a listing can leave objects out.

The page shows the text of a store that this site does not serve as
written. It runs no text of such a store as Typst. Above the list, the
page names every store and the whole address it was loaded from.

The page needs the names of the store's objects. It reads them from
`index.json` beside `objects/`, a JSON array of the names. For a store on
GitHub with no `index.json`, the page asks GitHub's API for a listing.
GitHub allows a visitor who is not signed in a small number of these
listings an hour. To avoid the limit, publish an `index.json`: the command
`sh viewer/assemble.sh` writes one, and so does

```console
$ (cd roadmap/objects && ls *.py | sed 's/\.py$//' | sort) \
    | jq -R . | jq -sc . > roadmap/index.json
```

Commit `index.json` again after each change to the store. A name that the
file lacks is an object that the page does not show. `prodrome verify`
does not read `index.json`, so it does not report a file that is out of
date.

The page sends no address and no object to the viewer's own host: the
addresses are in the part of the link after `#`, which a browser does not
send. It fetches each store's files from that store's host only.

If an object's bytes do not hash to its name, or the host does not serve an
object that the listing names, the page shows the rest of the store and
says which object it left out, and why.

- The page `#/` shows the list, with the most urgent item first. Each row
  shows its price as a small pie chart.
- The page `#/todo/<id>` shows one item: its text, its price, an
  explanation of the price, and its history. It also draws the item's value
  over thirty days, from fifteen days ago to fifteen days ahead, as one
  line.

The pie charts and the line use one continuous colour scale: red at 0,
then orange and yellow, and green at 1. The item page also has a small Typst
editor with syntax highlighting and completion. The editor is a
demonstration, and it saves nothing.

## The viewer is an optional example

`core/` and `cli/` never depend on Typst. No code in them assumes that the
text in a store is Typst. The Typst code is in three directories of its
own. A program whose items contain Typst can use them:

```
typst-wasm/   Typst 0.15.1 for the browser: HTML export, highlighting, completion,
              and a world that it keeps between compilations
              (its own cargo workspace and lock file, apart from the core's)
typst/        prodrome-typst, a Typst package: the list, one item, and the two charts
viewer/       the static web app, in TypeScript, built by nix with tsc and esbuild
```

The commands `list` and `show` print plain text. If they used Typst, `cli/`
would depend on Typst. To get a PDF, compile the package's layouts with the
`typst` binary. See [`typst/README.md`](../typst/README.md).

## Run the viewer locally

Run these commands with a checkout of `roadmap-data`:

```console
$ nix build .#prodrome-viewer -o result-viewer
$ git worktree add ../roadmap-data roadmap-data
$ sh viewer/assemble.sh result-viewer ../roadmap-data/roadmap site
$ python3 -m http.server -d site 8000     # then open http://localhost:8000/
```

`assemble.sh` works on any store, so you can use the same commands to show
your own store. If Playwright is installed, the command
`node viewer/test/smoke.mjs site <todo>` opens the list and one item in
headless Chromium and checks them. The command
`node viewer/test/stores.mjs APP viewer/test/stores` serves the three small
stores in `viewer/test/stores/` from a second local origin, and checks
pages of them: one list, the wait from one store to another, a changed
object, an object that the host does not serve, a pinned genesis, joined
addresses, two prodromes with one id, and text crafted to attack the page.
APP is a build that reads stores from this machine:
`VIEWER_LOOPBACK=1 sh viewer/build.sh APP …`, with the arguments that
`flake.nix` passes. The hosted build reads https alone. `node viewer/test/measure.mjs` measures what a
store costs to fetch (see the design note).

## GitHub Pages

The workflow `.github/workflows/pages.yml` runs the same build and assembly.
It runs on every push to `main`, and after every push to `roadmap-data`
that passes `verify`.
