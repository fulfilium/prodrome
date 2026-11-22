// prodrome-typst — layouts for a Prodrome read as Typst.
//
// AN OPTIONAL EXTRA. The Prodrome's core never assumes an item's text is
// Typst; this package is for a host whose items do, or that wants its list
// and its marks typeset. It reads DATA — the JSON the core's WebAssembly
// module answers (`entries`, `explain`, a todo's `stream`), with the marks'
// samples beside it — and computes nothing about fulfillment itself: every
// number it draws was produced by the one evaluator (SPEC §1).
//
// ITS LOOK IS THE DESIGN SYSTEM'S (`@local/typst-design`): the fulfillment
// scale, the pie and the trace, and the words for a value are imported, not
// drawn here. What is the Prodrome's is laying out a store's reading.
//
// The data (see README.md for the whole shape):
//
//   (at: "2026-09-27T12:00:00",   the instant everything was read at
//    entries: (..),               prodrome-wasm `entries(..).entries`
//    order: (..),                 prodrome-wasm `entries(..).order`
//    records: (..),               prodrome-wasm `entries(..).records`
//    marks: ("<todo>": (back: 15, ahead: 15, values: (..))),
//    explain: ("<todo>": tree),   prodrome-wasm `explain`, optional
//    history: ("<todo>": (..)))   the todo's events, optional
//
// Every layout works in paged export (PDF, PNG, SVG) and in HTML export
// (`--features html`), where the marks become inline SVG through
// `html.frame`; and in HTML from a library without `html` (typst-wasm's
// restricted project), where the marks are left out and their words stay.

#import "@local/typst-design:0.1.0" as design
#import design: percent, state-of

// --- instants ----------------------------------------------------------------

/// A naive instant's digits (`YYYY-MM-DD`, then `THH:MM:SS` or ` HH:MM:SS`
/// and any fraction) as a datetime; the fraction is dropped.
#let _instant(iso) = if iso.len() == 10 {
  datetime(year: int(iso.slice(0, 4)), month: int(iso.slice(5, 7)), day: int(iso.slice(8, 10)))
} else {
  datetime(
    year: int(iso.slice(0, 4)),
    month: int(iso.slice(5, 7)),
    day: int(iso.slice(8, 10)),
    hour: int(iso.slice(11, 13)),
    minute: int(iso.slice(14, 16)),
    second: int(iso.slice(17, 19)),
  )
}

/// An instant as a reader says it: "Sep 27, 2026, 12:00", or "Sep 27, 2026"
/// for a date alone.
#let when(iso) = if iso.len() == 10 {
  _instant(iso).display("[month repr:short] [day padding:none], [year]")
} else {
  _instant(iso).display("[month repr:short] [day padding:none], [year], [hour]:[minute]")
}


// --- targets ----------------------------------------------------------------

/// Whether the library spells HTML elements. A world for markup whose author
/// is not trusted (typst-wasm's `Project.restricted()`) has no `html` module
/// under any name, and still exports HTML: text, and the elements Typst
/// makes of it.
#let _elements = "html" in dictionary(std)

/// THE ONE COMBINATOR OVER TARGETS: content as a target holds it. `paged` on
/// a page; in HTML, `html(std.html)`, given the module that spells elements;
/// in HTML from a library without one, `text`. Each is `paged` unless said.
#let _on(paged, html: auto, text: auto) = context if target() != "html" { paged } else if _elements {
  if html == auto { paged } else { html(std.html) }
} else if text == auto { paged } else { text }

/// Drawn content: itself on a page, an inline SVG in HTML, sized in `em` of
/// an 11pt text, and nothing where HTML cannot hold a drawing. Inside the
/// frame the target is a page, so a mark drawn there never asks for `html`.
#let _frame(body) = _on(body, html: html => box(html.frame(body)), text: none)

/// A block a stylesheet can find by class in HTML; a plain block otherwise.
#let _div(class, body) = _on(block(body), html: html => html.elem("div", attrs: (class: class), body))

#let _span(class, body) = _on(body, html: html => html.elem("span", attrs: (class: class), body))

/// A record's text: Typst markup when the host says its items hold Typst,
/// verbatim otherwise.
#let _text(body, markup) = if body == none or body == "" { [] } else if markup {
  eval(body, mode: "markup")
} else { body }

// --- the look -----------------------------------------------------------------

/// THE DESIGN SYSTEM'S LOOK over a layout: its serif in ink for the text,
/// its display face on the type scale for headings, toned as its jobs sample
/// the identity ramp (a title strong, a section medium), and structure from
/// its marks and space rather than boxes: a table is rows on quiet rules. Set
/// rules only, so in HTML every element stays the element it is, for a
/// stylesheet to dress in the same tokens.
#let _look(body) = {
  let tone(job) = design.ramps.maroon.sample(design.jobs.at(job))
  set text(font: design.faces.serif, fill: design.palette.ink)
  show raw: set text(font: design.faces.mono)
  show heading: set text(font: design.faces.display, weight: 600)
  show heading.where(level: 1): set text(size: design.scale(3), fill: tone("heading-1"))
  show heading.where(level: 2): set text(size: design.scale(1), fill: tone("heading-2"))
  set table(stroke: (_, y) => (bottom: 0.5pt + design.palette.rule), inset: (x: 0.3em, y: 0.45em))
  body
}

/// A capsule rule under a title, in HTML an `hr` a stylesheet draws.
#let _rule = _on(design.capsule-rule(), html: html => html.elem("hr", attrs: (class: "capsule")), text: none)

/// Items joined by the design's diamond, the only inline separator; framed
/// in HTML in a `span.diamond` a stylesheet spaces, and a middle dot where
/// HTML cannot hold a drawing.
#let _sep(..items) = design.sep(
  ..items.pos().filter(item => item != none),
  diamond: () => _on(
    design.diamond(),
    html: html => html.elem("span", attrs: (class: "diamond"), html.frame(design.diamond(spacing: 0pt))),
    text: [ · ],
  ),
)

/// A column's name: the design's sans capitals, muted.
#let _label(body) = design.label-text(body, fill: design.palette.ink-muted)

/// What is said beside a title, a step down the scale and muted.
#let _aside(class, body) = _div(class, text(size: design.scale(-1), fill: design.palette.ink-muted, body))

// --- the marks ----------------------------------------------------------------

/// A value as a list or a page shows it: the design's `price`, its pie and
/// its percentage, or a quiet "unpriced", found by class in HTML. Where HTML
/// cannot hold a drawing, its words alone.
#let price(value) = if value == none or value == "absent" {
  _span("price unpriced", design.price(value))
} else {
  _span("price", _on(design.price(value), text: strong(percent(value))))
}

// --- reading the data --------------------------------------------------------

/// A row's key: what `order`, `created`, `marks`, `explain` and `history`
/// name it by. A page of several stores keys a row `<store>/<todo>`, so two
/// stores' items never share a key; a page of one store keys it by its todo
/// id, and leaves `key` out.
#let _key(entry) = entry.at("key", default: entry.todo)

#let _record(data, entry) = if entry.content == none { none } else {
  data.at("records", default: (:)).at(entry.content, default: none)
}

#let _field(record, name) = if record == none { "" } else {
  let value = record.at(name, default: "")
  if value == none { "" } else { value }
}

/// What a todo asks for: its record's body, else its `Created`'s text — the
/// body `prodrome list` prints.
#let _body(data, entry) = {
  let body = _field(_record(data, entry), "body")
  if body != "" { body } else {
    data.at("created", default: (:)).at(_key(entry), default: (text: "")).text
  }
}

/// Whether the todo exists at the instant the data was read at: the core's
/// `begun`, the instant of the todo's first event of any kind, is at or
/// before it (SPEC §6.7). A row exists for every todo the store mentions,
/// the future's included, so the list keeps only the rows that pass.
#let _existed(data, entry) = entry.begun <= data.at

/// The rows in the core's list order (SPEC §6.7), which `prodrome list`
/// prints too: most urgent first, then every row with no number, by id. The
/// order is the core's, so this package never sorts by value itself.
#let listed(data) = data.order.map(key => data.entries.find(e => _key(e) == key))

/// The items a row waits on (`waits`), or that wait on it (`waitedBy`):
/// each a link where the page shows it (`kind` `loaded`), else its name and
/// why the page does not: its store is not loaded (`elsewhere`), or its
/// store does not hold it (`unheld`). Nothing for a row with none.
#let _waits(entry, field, words) = {
  let waits = entry.at(field, default: ())
  if waits.len() > 0 {
    _span("waits", [#words #waits.map(w => if w.kind == "loaded" { link(w.href, raw(w.label)) } else if w.kind == "elsewhere" [#raw(w.label), in a store not loaded] else [#raw(w.label), which its store does not hold]).join([, ])])
  }
}

/// THE THIRTY DAYS: a todo's marks (`data.marks`), drawn as the design's
/// `trace` around the instant the data was read at. Nothing for a todo with
/// no marks, or where HTML cannot hold a drawing.
#let trace-of(data, todo) = {
  let marks = data.at("marks", default: (:)).at(todo, default: none)
  if marks != none {
    _frame(design.trace(marks.values, at: _instant(data.at), back: marks.back, ahead: marks.ahead))
  }
}

// --- the list ----------------------------------------------------------------

/// The roadmap: every open todo, most urgent first, each with its price and
/// a link to its page. `href` turns a row's key into that link. The closed
/// follow, in a shorter list. A row that waits on another says so, linked.
#let roadmap(
  data,
  title: [Roadmap],
  href: todo => "#/todo/" + todo,
  markup: false,
) = _look({
  let open = listed(data).filter(e => e.state == "open" and _existed(data, e))
  let closed = data.entries.filter(e => e.state != "open" and _existed(data, e)).sorted(key: _key)

  heading(level: 1, title)
  _rule
  _aside("summary", [#open.len() open on #when(data.at) — most urgent first.])

  _div("roadmap", table(
    columns: 2,
    table.header(_label[Price], _label[Item]),
    ..open
      .map(e => (
        price(e.value),
        [#link(href(_key(e)), raw(_key(e))) \ #_text(_body(data, e), markup)#_waits(e, "waits", [\ waits on])],
      ))
      .flatten(),
  ))

  if closed.len() > 0 {
    heading(level: 2)[Closed]
    list(..closed.map(e => _sep(
      link(href(_key(e)), raw(_key(e))),
      [#e.state],
      if e.at != "" [since #when(e.at)],
    )))
  }
})

// --- one item ----------------------------------------------------------------

/// An instant in a sentence, where the year goes without saying: "Sep 29,
/// 17:00".
#let _short(iso) = _instant(iso).display("[month repr:short] [day padding:none], [hour]:[minute]")

/// A span of hours in words: whole days as days, two days or more as about so
/// many days, anything shorter as hours.
#let _duration(hours) = {
  let h = calc.abs(hours)
  let unit(n, one) = str(n) + " " + one + if n == 1 { "" } else { "s" }
  if h > 0 and calc.rem(h, 24) == 0 { unit(int(h / 24), "day") } else if h >= 48 {
    "about " + unit(int(calc.round(h / 24)), "day")
  } else if calc.fract(h) == 0 { unit(int(h), "hour") } else {
    str(calc.round(h, digits: 1)) + " hours"
  }
}

/// How a power mean with exponent `p` weighs its members.
#let _mean(p) = if p == 1 { "the average" } else if p == 0 { "the geometric mean" } else if p < 1 {
  "a mean held down by the weakest"
} else { "a mean lifted by the strongest" }

/// What a node of the explanation says, in words, after its value: every kind
/// of term (SPEC §7) and the notes `explain` gives it. No field's name is
/// shown.
#let _says(node) = {
  let kind = node.kind
  if kind == "flat" [flat] else if kind == "decay" {
    [falling from #percent(node.start) to #percent(node.end) by #_short(node.endDate), over #_duration(node.leadUpHours)]
    if "startDate" in node [; 100% before #_short(node.startDate)]
  } else if kind == "curve" {
    let (first, last) = (node.points.first(), node.points.last())
    if node.points.len() == 1 [a curve held at #percent(first.value)] else [
      along a curve from #percent(first.value) on #_short(first.at) to #percent(last.value) on #_short(last.at)#if node.points.len() == 3 [, by way of one other date] else if node.points.len() > 3 [, by way of #(node.points.len() - 2) other dates]
    ]
  } else if kind == "conj" {
    let n = node.terms.len()
    if n == 0 [no parts, so neither urgent nor settled] else [
      #_mean(node.p) of #n #if n == 1 [part] else [parts]#if "certifies" in node [, so no part is below #percent(node.certifies)]
    ]
  } else if kind == "offset" {
    if node.delta > 0 [pulled #percent(node.delta) of the way up to 100%] else if node.delta < 0 [
      pulled #percent(-node.delta) of the way down to 0%
    ] else [its part, unchanged]
  } else if kind == "gate" [its own price, counted only as far as its gate is met] else if kind == "shift" {
    if node.deltaHours > 0 [as its part will read in #_duration(node.deltaHours)] else if node.deltaHours < 0 [
      as its part read #_duration(node.deltaHours) earlier
    ] else [its part, unshifted]
  } else if kind == "within" [
    #_mean(node.p) over the next #_duration(node.windowHours)#if "peakAt" in node [, weighed most at #_short(node.peakAt)]
  ] else if kind == "importance" {
    if node.w > 1 [its part made more urgent, to the power #node.w] else if node.w < 1 [
      its part made less urgent, to the power #node.w
    ] else [its part, at its own weight]
  } else if kind == "after" {
    if node.bound == "pending" [
      waiting on #raw(node.event)#if "needsHours" in node [, which needs #_duration(node.needsHours) once it is done]
    ] else if node.bound == "completed" [since #raw(node.event) was done, planned for #_short(node.anchor)] else [
      #raw(node.event) was cancelled, so this no longer waits on it
    ]
  } else if kind == "recur" {
    if node.bound == "pending" [a recurring #raw(node.todo), not done yet] else [
      a recurring #raw(node.todo), last done #_short(node.tended), #_duration(node.agoHours) ago
    ]
  } else if kind == "periodic" [
    repeating every #_duration(node.periodHours)\; this cycle began #_short(node.cycleStart)
  ] else if kind == "piecewise" {
    let prices = node.pieces + 1
    if node.since == "" [the first of #prices dated prices] else [
      the price set on #_short(node.since), #if prices == 2 [one of two dated prices] else [one of #prices dated prices]
    ]
  } else if kind == "offsetBy" [its own price, offset by another] else if kind == "ref" [
    as #raw(node.todo) is priced
  ] else if kind == "absent" [no claim on attention]
}

/// A node's parts, each with what it is to its parent.
#let _parts(node) = {
  let kind = node.kind
  if "terms" in node {
    let shares = node.at("shares", default: none)
    node.terms.enumerate().map(((i, part)) => (
      [part #(i + 1)#if shares != none [, #percent(shares.at(i)) of the weight]:],
      part,
    ))
  } else if kind == "gate" {
    (([gated on:], node.gate), ([its own:], node.body))
  } else if kind == "offsetBy" {
    (([its own:], node.term), ([offset by:], node.delta))
  } else if kind == "after" {
    (([once done:], node.term), ([until then:], node.pending))
  } else if kind == "recur" {
    (([after each pass:], node.term), ([until the first:], node.pending))
  } else if kind == "piecewise" {
    (([in force:], node.term),)
  } else if "term" in node {
    (([its part:], node.term),)
  } else { () }
}

/// An explanation tree (prodrome-wasm `explain`) in words: each node's value
/// and what it says ("42% now, falling from 55% to 5% by Sep 29, 17:00, over
/// 3 days"), its parts beneath it. A decoration of the term, never a second
/// reading of it. A node's `null` is always `∅`. `now` marks the root: a part
/// is read at the instant its parent used it, which need not be now.
#let explanation(node, now: true) = {
  [#price(node.value)#if now [ now], #_says(node)]
  let parts = _parts(node)
  if parts.len() > 0 {
    list(..parts.map(((label, part)) => [#emph(label) #explanation(part, now: false)]))
  }
}

/// One todo's page: its body, its price and its thirty days, what it waits
/// on and what waits on it, its detail, why it is worth what it is, and its
/// history. `todo` is the row's key. `objects` is where an object's file is
/// linked (`objects/<hash>.py` on the site).
#let item(
  data,
  todo,
  markup: false,
  objects: "objects/",
  back: "#/",
) = _look({
  let entry = data.entries.find(e => _key(e) == todo)
  if entry == none {
    heading(level: 1)[No item #raw(todo)]
    _rule
    [Nothing in this store mentions #raw(todo). #link(back)[Back to the list.]]
  } else {
    let record = _record(data, entry)
    let body = _body(data, entry)

    _aside("crumbs", _sep(link(back)[← Roadmap], raw(todo)))
    heading(level: 1, if body == "" { raw(todo) } else { _text(body, markup) })
    _rule

    _div("facts", _sep(
      price(entry.value),
      state-of(entry.value),
      entry.state,
      if entry.claimed != "" [claimed #entry.claimed],
      if entry.unconfirmed [unconfirmed],
      if entry.at not in (none, "") [since #when(entry.at)],
    ))
    _div("marks", trace-of(data, todo))
    _waits(entry, "waits", [Waits on])
    _waits(entry, "waitedBy", [Waited on by])

    let detail = _field(record, "detail")
    if detail != "" { _div("detail", _text(detail, markup)) }

    heading(level: 2)[Price]
    let tree = data.at("explain", default: (:)).at(todo, default: none)
    if tree != none { _div("explanation", explanation(tree)) }
    if entry.value == "absent" [Absent: this item has no value, which is not a zero.]
    if entry.unlinked != none [Not priced: #entry.unlinked]

    let events = data.at("history", default: (:)).at(todo, default: ())
    if events.len() > 0 {
      heading(level: 2)[History]
      _div("history", table(
        columns: 4,
        table.header(_label[When], _label[Event], _label[By], _label[Object]),
        ..events
          .map(e => (
            when(e.at),
            e.kind,
            e.actor,
            link(objects + e.hash + ".py", raw(e.hash.slice(0, 12))),
          ))
          .flatten(),
      ))
    }
  }
})
