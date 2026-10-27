# The viewer: Prodrome in the browser

The site <https://fulfilium.github.io/prodrome/> shows this crate's
[roadmap](roadmap.md). The site is read-only and static. The page downloads
the objects of the `roadmap-data` branch, as they are on disk. The page
computes each item's current value in the browser, with the Prodrome core
compiled to WebAssembly. Then it lays out the result with Typst, also
compiled to WebAssembly. No server computes anything.

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
headless Chromium and checks them.

## GitHub Pages

The workflow `.github/workflows/pages.yml` runs the same build and assembly.
It runs on every push to `main`, and after every push to `roadmap-data`
that passes `verify`.
