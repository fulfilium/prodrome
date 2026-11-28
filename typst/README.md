# prodrome-typst

A Typst package that lays out a Prodrome: the roadmap list ordered by
fulfillment, and one item's page with its price and its thirty days, in the
look of the design system it imports.

| The list | An item |
| --- | --- |
| ![The roadmap example](examples/roadmap.png) | ![The item example](examples/item.png) |

**An optional extra.** The Prodrome's core (`core/`, `cli/`) never depends on
Typst and never assumes an item's text is Typst. This package is for a host
whose items hold Typst, or that simply wants its roadmap typeset. The browser
viewer in `viewer/` is one such host, and an example of how to use it.

**It computes nothing about fulfillment.** Every number it draws was answered
by the core (compiled to WebAssembly, or any other implementation of SPEC.md).

**Its look is the design system's.** The package imports
[`typst-design`](https://github.com/bmabsout/typst-design)
(`@local/typst-design:0.1.0`) for everything that is a look rather than a
layout: the fulfillment scale (`fulfillment-color(v)`, continuous, low is
urgent, readable on paper and by red–green colour-blind readers), the pie,
the trace, the words for a value (`state-of`: Problem below 0.5, Watch below
0.7, Fine) and `percent`. `∅`, or a todo that does not link, has no value
and no colour of the scale; it reads "unpriced". The design system's own
laws hold its scale and marks; `tests/laws.typ` holds what the layouts rely
on of them, and the layouts themselves.

## Use

```typst
#import "@local/prodrome-typst:0.1.0": roadmap, item, price, trace-of

#let data = json("data.json")
#roadmap(data)                         // the list, links to #/todo/<id>
#item(data, "ship-the-viewer")         // one item's page
#price(0.42)                           // a value's pie and percentage
#trace-of(data, "ship-the-viewer")     // its thirty days
```

Every layout works in paged export (PDF, PNG, SVG), in HTML export
(`typst compile --features html --format html`), where the marks become inline
SVG, and in HTML from a library without `html` (typst-wasm's
`Project.restricted()`, for markup whose author is not trusted), where the
marks are left out and their words stay. The examples compile every way, with
the design system on the package path beside this package:

```console
$ mkdir -p pkgs/local/prodrome-typst pkgs/local/typst-design
$ ln -s "$PWD/typst" pkgs/local/prodrome-typst/0.1.0
$ git clone https://github.com/bmabsout/typst-design pkgs/local/typst-design/0.1.0
$ typst compile --package-path pkgs typst/examples/roadmap.typ
$ typst compile --package-path pkgs --features html --format html typst/examples/item.typ
```

`nix flake check` does the same (`checks.prodrome-typst`, with the design
system the flake pins), compiles `tests/laws.typ`, whose asserts are the
package's laws, and compiles the examples in typst-wasm's restricted project
(`checks.prodrome-typst-restricted`). `examples/roadmap.png` and
`examples/item.png` are the examples rendered, and the check renders them
again (`nix build .#prodrome-typst-examples`) and fails if they differ.

## The data

One JSON object, in the shapes prodrome-wasm answers, with the marks'
samples beside them. `examples/data.json` is a real one, made from a small
store by the viewer's own `viewer/src/view.ts`.

| Key       | What                                                              | From                            |
| --------- | ----------------------------------------------------------------- | ------------------------------- |
| `at`      | the instant everything was read at, ISO                           | `entries(..).at`                |
| `entries` | one row per todo (SPEC §6.7); a row's `begun` is the instant of its first event of any kind, and the list shows a row only where `begun` is at or before `at` | `entries(..).entries` |
| `order`   | the rows' todo ids in the core's list order                       | `entries(..).order`             |
| `records` | the records the rows name, by object name                        | `entries(..).records`           |
| `created` | per todo, its first `Created`'s `at` and its last one's `text`, the body of a todo with no record | `entries(..).created` |
| `marks`   | per todo, `back` and `ahead` (days) and `values`, evenly spaced from `back` days before `at` to `ahead` days after it, both ends included, one of them at `at`; `null` where it reads `∅` | `fulfillment`, `series_knots` |
| `explain` | per todo, the explanation tree (optional)                         | `explain`                       |
| `history` | per todo, its events: `at`, `kind`, `actor`, `hash` (optional)    | `fold(..).stream`               |
| `focus`   | the key of the todo a page is about, or `null`                    | the host                        |

A page of several stores keys each row by `<store>/<todo>`, so two stores'
items never share a key. Then `order`, `created`, `marks`, `explain`,
`history` and `focus` use that key, and each entry has these optional
fields. A page of one store may leave them out, and a row's key is then
its todo id:

| Field      | What                                                                  |
| ---------- | --------------------------------------------------------------------- |
| `key`      | the row's key                                                         |
| `store`    | the label of the row's store                                          |
| `waits`    | the items its function refers to: `{kind, label, href}`, `kind` `loaded` (with `href`), `elsewhere` (a store not loaded) or `unheld` (its store does not hold it) |
| `waitedBy` | the items whose functions refer to it, in the same shape              |

## What it exports

- `roadmap(data, title:, href:, markup:)`: the open items that exist at `at`,
  in the core's list order (ascending value, then every item with no number,
  ties by id: the order `prodrome list` prints), then the closed.
- `item(data, todo, markup:, objects:, back:, back-title:)`: a link back to
  the list at `back`, which shows `back-title` (`Roadmap` by default), then body, price and state, the
  thirty days, what it waits on and what waits on it, detail, the price in words ("42% now, falling from 55% to 5%
  by Sep 29, 17:00, over 3 days") with the explanation's parts beneath it,
  and the history, each event dated and linked to its object file.
- `price(value)`: the design's pie and the percentage, or "unpriced".
- `trace-of(data, todo)`: the todo's thirty days, `data.marks` drawn as the
  design's trace around `data.at`; the past faded, now a thin ink line and a
  dot, dated at the ends, a week either side, and now.
- `when` (an instant as a reader says it), `explanation` (a tree of prices
  in words, every kind of term said with percentages, spans and dates and
  never a field's name), `listed`, and, from the design system, `state-of`
  and `percent`.

`markup: true` reads an item's body and detail as Typst markup; the default
shows them verbatim, because nothing promises a store's text is Typst. In HTML
export each piece carries a class (`roadmap`, `price`, `price unpriced`, `marks`,
`explanation`, …) for a stylesheet to find.

The layouts are set in the design system's look: its serif in ink, headings
in its display face on its type scale, toned as its jobs sample the identity
ramp; a capsule rule under each title (an `hr class="capsule"` in HTML, for a
stylesheet to draw); the facts, the crumbs and a closed item's state joined
by its diamond; tables as rows on its quiet rules, under sans capitals.
