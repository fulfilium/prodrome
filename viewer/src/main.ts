// The Prodrome in the browser: a READ-ONLY viewer of any public store, and an
// example of how a host uses the Prodrome.
//
// The page's address names the stores to show (address.ts). With none, the
// page shows the store that the site serves beside it: `objects/<name>.py`,
// each an object's canonical print, and `index.json`, the list of names.
// For each store, the page reads a listing of its objects, fetches each
// object by its name (fetch.ts), keeps only the objects that the core
// compiled to WebAssembly (prodrome-wasm) checks against their names, and
// reads every store with the core (stores.ts). Typst compiled to WebAssembly
// (typst-wasm) and the `prodrome-typst` package typeset the result. Nothing
// is computed on a server, and nothing is written anywhere. The page sends
// the addresses to no host but the stores' own: they are in the fragment,
// which a browser never sends.
//
// ROUTES ARE A CONTRACT: `#/` is the list and `#/todo/<id>` an item. Other
// tools link to them (the issue mirror links `#/todo/gh-<number>`). A link
// to the stores adds `?<stores>` to either, and `#/in/<label>` lists one store.

import initProdrome, * as prodrome from "prodrome-wasm";
import initTypst, * as typstWasm from "typst-wasm";
import { parseFragment, printStores, type Address, type Read, type Route } from "./address";
import { editor } from "./editor";
import { fetchObjects, list } from "./fetch";
import { check, hrefOf, readStores, SITE, sources, type Checked, type Notice, type Page, type Source } from "./stores";
import { wall, type Naive } from "./time";
import { body, compilePage, itemDocument, listDocument, scratchDocument, type Typst } from "./typst";
import type { Entry } from "./view";

declare const __BUILD__: string;

/**
 * The fonts the site bundles for Typst and the page — Source Serif, the
 * design system's serif, and Libertinus behind it, OFL.
 */
const FONTS = [
  "LibertinusSerif-Regular.otf",
  "LibertinusSerif-Italic.otf",
  "LibertinusSerif-Bold.otf",
  "LibertinusMath-Regular.otf",
  "SourceSerif4-Regular.otf",
  "SourceSerif4-It.otf",
  "SourceSerif4-Bold.otf",
];

/** An instant as a reader says it; a naive instant's digits read as UTC (see time.ts). */
const READABLE = new Intl.DateTimeFormat("en-US", {
  dateStyle: "medium",
  timeStyle: "short",
  hourCycle: "h23",
  timeZone: "UTC",
});

const page = document.getElementById("page") as HTMLElement;
const status = document.getElementById("status") as HTMLElement;
const storesNav = document.getElementById("stores") as HTMLElement;
const notices = document.getElementById("notices") as HTMLElement;
const home = document.querySelector("header .home") as HTMLAnchorElement;

function say(text: string) {
  status.textContent = text;
}

/** The stores a page's address asks for: its stores, or with none, the site's own store. */
function asked(read: Read): Source[] {
  return read.stores.length === 0 ? [SITE] : sources(storesOf(read));
}

/** Every store of the address, fetched and checked by the core: one value per address. */
async function load(stores: Source[]): Promise<Checked[]> {
  return Promise.all(
    stores.map(async (source) => {
      const listing = await list(source.lister, source.base);
      const fetched = listing.kind === "listed" ? await fetchObjects(source.base, listing.names) : [];
      return check(prodrome, source, listing, fetched);
    }),
  );
}

async function loadTypst(): Promise<Typst> {
  await initTypst({ module_or_path: `wasm/typst_bg.wasm?v=${__BUILD__}` });
  const fonts = await Promise.all(
    FONTS.map(async (name) => {
      const response = await fetch(`fonts/${name}`);
      return response.ok ? new Uint8Array(await response.arrayBuffer()) : null;
    }),
  );
  for (const font of fonts) if (font) typstWasm.add_font(font);
  return typstWasm;
}

/** A row's value as the list prints it: a percentage, `∅` for absent, `—` unlinked. */
function percent(value: Entry["value"]): string {
  if (value === "absent") return "∅";
  if (value === null) return "—";
  return `${Math.round(value * 100)}%`;
}

/**
 * Before Typst has arrived, or if it never does: the list as plain HTML, the
 * rows `prodrome list` prints (open, and created by now) in the core's order.
 */
function plain(view: Page): HTMLElement {
  const list = document.createElement("ol");
  const rows = new Map(view.entries.map((e) => [e.key, e]));
  const existed = (key: string) => {
    const first = view.created[key];
    return first !== undefined && first.at <= view.at;
  };
  for (const key of view.order) {
    const entry = rows.get(key);
    if (entry === undefined || entry.state !== "open" || !existed(key)) continue;
    const li = document.createElement("li");
    const a = Object.assign(document.createElement("a"), { href: hrefOf(key, view.query) });
    a.textContent = key;
    li.append(`${percent(entry.value)} `, a, ` ${view.created[key]?.text ?? ""}`);
    list.append(li);
  }
  return list;
}

/** The links to each store's own list, when the page shows several. */
function showStores(view: Page, route: Route) {
  storesNav.replaceChildren();
  storesNav.hidden = view.stores.length < 2;
  if (storesNav.hidden) return;
  const current = route.within.kind === "store" ? route.within.label : null;
  const link = (text: string, href: string, here: boolean) => {
    const a = Object.assign(document.createElement("a"), { href, textContent: text });
    if (here) a.setAttribute("aria-current", "page");
    return a;
  };
  storesNav.append(
    "Stores: ",
    link("all", `#/${view.query}`, current === null && route.page === "list"),
    ...view.stores.flatMap((label) => [" · ", link(label, `#/in/${label}${view.query}`, current === label)]),
  );
}

/** What the page left out of each store, and why. Nothing when every object was kept. */
function showNotices(found: Notice[]) {
  notices.replaceChildren();
  notices.hidden = found.length === 0;
  for (const notice of found) {
    const details = document.createElement("details");
    const summary = document.createElement("summary");
    const code = document.createElement("code");
    code.textContent = notice.shown;
    summary.append(`${notice.label} (`, code, `): ${notice.lines.length} ${notice.lines.length === 1 ? "problem" : "problems"}`);
    const items = document.createElement("ul");
    for (const line of notice.lines) items.append(Object.assign(document.createElement("li"), { textContent: line }));
    details.append(summary, items);
    notices.append(details);
  }
}

function show(nodes: Node, typeset: boolean) {
  const container = document.createElement("div");
  container.className = typeset ? "typst" : "plain";
  container.append(nodes);
  page.replaceChildren(container);
}

/** The page's view of one route: the list of one store keeps only that store's rows. */
function within(view: Page, route: Route): Page {
  if (route.page !== "list" || route.within.kind === "every") return view;
  const label = route.within.label;
  return { ...view, order: view.order.filter((key) => view.entries.find((e) => e.key === key)?.store === label) };
}

/** The stores' part of an address that parsed, refusals left out. */
function storesOf(read: Read): Address[] {
  return read.stores.flatMap((s) => (s.kind === "address" ? [s.address] : []));
}

/** The refused stores' texts, as notices. */
function refusals(read: Read): Notice[] {
  return read.stores.flatMap((s) =>
    s.kind === "refused" ? [{ label: "address", shown: s.text, lines: [`this is not a store's address: ${s.why}`] }] : [],
  );
}

async function main() {
  if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("sw.js").catch(() => undefined);
  }
  say("loading the core…");
  await initProdrome({ module_or_path: `wasm/prodrome_bg.wasm?v=${__BUILD__}` });
  const typst = loadTypst();

  // The stores the page has loaded, by the stores' part of the address as
  // it was read: a change of route alone fetches nothing again.
  let loadedFor: string | null = null;
  let checked: Checked[] = [];
  // THE ONE READ OF THE CLOCK, as in the command line: the core has none,
  // and a host names the instant it asks about. It is read again whenever
  // the stores are fetched again.
  let now: Naive = wall(new Date());

  let current = 0;
  const render = async () => {
    const ticket = ++current;
    const read = parseFragment(location.hash);
    const query = printStores(storesOf(read));
    const asking = JSON.stringify(read.stores);
    if (loadedFor !== asking) {
      const stores = asked(read);
      say(stores.length === 1 ? "fetching the objects…" : `fetching ${stores.length} stores…`);
      const fetched = await load(stores);
      if (ticket !== current) return;
      checked = fetched;
      loadedFor = asking;
      now = wall(new Date());
    }
    const at = read.route;
    const { page: view, notices: found } = readStores(prodrome, checked, now, at, query);
    home.href = `#/${query}`;
    showStores(view, at);
    showNotices([...refusals(read), ...found]);
    const objects = checked.reduce((n, c) => n + (c.kind === "kept" ? c.objects.length : 0), 0);
    const where = view.stores.length > 1 ? ` in ${view.stores.length} stores` : "";
    const readAt = `${objects} objects${where}, read ${READABLE.format(new Date(now))}`;
    const shown = within(view, at);
    if (at.page === "list") show(plain(shown), false);
    say(`${readAt} — loading Typst…`);

    const engine = await typst;
    if (ticket !== current) return;
    const compiled = compilePage(engine, at.page === "list" ? listDocument : itemDocument, shown);
    if (compiled.html === null) {
      const errors = compiled.diagnostics.map((d) => d.message).join("; ");
      say(`Typst could not typeset this page: ${errors}`);
      if (at.page === "list") return;
      show(plain(shown), false);
      return;
    }
    const content = document.createDocumentFragment();
    content.append(body(compiled.html));
    if (!compiled.markup) {
      const note = document.createElement("p");
      note.className = "note";
      note.textContent = "This text is not valid Typst, so it is shown as written.";
      content.append(note);
    }
    if (at.page === "item" && shown.entries.some((e) => e.key === shown.focus)) {
      content.append(editor(engine, shown, scratchDocument()));
    }
    show(content, true);
    document.title = at.page === "item" ? `${shown.focus ?? at.todo} — Prodrome` : `${shown.title} — Prodrome`;
    say(readAt);
    document.body.dataset.state = "typeset";
    window.scrollTo(0, 0);
  };

  window.addEventListener("hashchange", () => {
    document.body.dataset.state = "loading";
    render().catch(fail);
  });
  await render();
}

function fail(error: unknown) {
  say(`Something went wrong: ${error instanceof Error ? error.message : String(error)}`);
  document.body.dataset.state = "failed";
}

main().catch(fail);
