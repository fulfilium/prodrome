// SEVERAL STORES, ONE LIST. This file turns the objects that the page
// fetched from each address into the data that one page typesets:
//
// 1. CHECK each address's objects with the core (`Todos.tips`). The core
//    leaves out every object whose bytes do not hash to its name, every
//    object that does not parse, and everything that rests on one of them
//    (SPEC §3, law 40). The page keeps only what the core keeps, and says
//    what it left out. No check of a name or of bytes is written here.
// 2. GROUP the addresses by the prodromes they hold. A store is named by its
//    genesis (a legacy chain by its least root, as `tips` names it), so two
//    addresses that hold one genesis are one store, and the store holds the
//    union of their objects, as a merge does.
// 3. READ each store on its own (view.ts), and read the union of all the
//    stores once with the core's `entries`. Law 41 says that each store
//    reads in the union as it reads alone, and that a qualified reference
//    reads the entity's price in its own store. So the union gives the one
//    list order (SPEC §6.7) and every price, a wait across stores included,
//    and no row is sorted or priced here.

import { baseOf, labels, printAddress, type Address, type Pin, type Route } from "./address";
import type { Fetched, Lister, Listing } from "./fetch";
import { read, type Core, type Entry, type ObjectIn, type Target, type View } from "./view";
import { iso, type Naive } from "./time";

/** One address of a store, as the page loads it. The site's own store has no address. */
export interface Source {
  label: string;
  /** The URL under which the store's `objects/` lies, with its `/`. */
  base: string;
  pin: Pin;
  /** Where the names of the store's objects come from. */
  lister: Lister;
  /** The address as the link wrote it, or words for the site's own store. */
  shown: string;
}

/** The site's own store, which a page with no store in its address shows. */
export const SITE: Source = {
  label: "roadmap",
  base: "",
  pin: { kind: "any" },
  lister: { kind: "site" },
  shown: "this site's store",
};

/** An object that the core left out, why, and what rests on it. */
export interface Left {
  name: string;
  why: string;
  resting: string[];
}

/** What the page tells a reader about one address. */
export interface Notice {
  label: string;
  shown: string;
  lines: string[];
}

/** One address, checked: the objects the core kept and the prodromes they hold, or why none are shown. */
export type Checked =
  | { kind: "kept"; source: Source; objects: ObjectIn[]; heads: string[]; notice: Notice }
  | { kind: "refused"; source: Source; notice: Notice };

/** A store: the addresses that hold its prodromes, and the union of their kept objects. */
interface Store {
  label: string;
  sources: Source[];
  objects: Map<string, ObjectIn>;
  heads: Set<string>;
}

interface Tips {
  heads: Record<string, string[]>;
  excluded: Left[];
}

/** A row of the core's `Todos.entries`: an entity of one prodrome, and its price. */
interface Row {
  genesis: string | null;
  todo: string;
  value: Entry["value"];
  unlinked: string | null;
}

/** A member of the set of prodromes, as `Todos.prices` answers it. */
interface Member {
  genesis: string | null;
  env: unknown;
  functions: Record<string, unknown>;
}

/**
 * Another item that an item waits on, or that waits on it: one the page
 * shows, one of a store the page has not loaded, or one that a loaded store
 * does not hold. The core prices the last two as `∅` (SPEC §7.2).
 */
export type Wait =
  | { kind: "loaded"; label: string; href: string }
  | { kind: "elsewhere"; label: string }
  | { kind: "unheld"; label: string };

/** An §6.7 row as a page shows it: its key in the page, its store, and its waits. */
export interface Shown extends Entry {
  key: string;
  store: string;
  waits: Wait[];
  waitedBy: Wait[];
}

/** What `typst/lib.typ` reads for a page of one or several stores; see typst/README.md. */
export interface Page extends Omit<View, "entries"> {
  entries: Shown[];
  /** The labels of the stores shown, in the link's order. */
  stores: string[];
  /** What follows `#/todo/<key>` in a link to an item: the stores' part of the address. */
  query: string;
  /** Where the history of the focused item links an object's file. */
  objects: string;
  title: string;
}

/** The objects of one address, checked by the core: what it keeps, and what it leaves out and why. */
export function check(core: Core, source: Source, listing: Listing, fetched: Fetched[]): Checked {
  const lines: string[] = [];
  const notice = { label: source.label, shown: source.shown, lines };
  if (listing.kind === "failed") {
    lines.push(listing.why);
    return { kind: "refused", source, notice };
  }
  lines.push(...listing.notes);
  const held = fetched.flatMap((f) => (f.kind === "held" ? [f.object] : []));
  const missing = fetched.flatMap((f) => (f.kind === "missing" ? [f] : []));
  for (const m of missing) lines.push(`the listing names ${m.hash.slice(0, 12)}, which the host does not serve (${m.why})`);
  const replica = new core.Todos(JSON.stringify(held));
  let tips: Tips;
  try {
    tips = JSON.parse(replica.tips()) as Tips;
  } finally {
    replica.free();
  }
  const out = new Set<string>();
  for (const left of tips.excluded) {
    out.add(left.name);
    left.resting.forEach((name) => out.add(name));
    const n = left.resting.length;
    const resting = n === 0 ? "" : `, and ${n} ${n === 1 ? "object that rests" : "objects that rest"} on it ${n === 1 ? "is" : "are"} left out with it`;
    lines.push(`object ${left.name.slice(0, 12)} is left out: ${left.why}${resting}`);
  }
  const heads = Object.keys(tips.heads);
  if (source.pin.kind === "genesis" && !heads.includes(source.pin.genesis)) {
    lines.push(
      `the link expects the genesis ${source.pin.genesis.slice(0, 12)}, and this store holds ` +
        (heads.length === 0 ? "none" : heads.map((h) => h.slice(0, 12)).join(", ")),
    );
    return { kind: "refused", source, notice };
  }
  const objects = held.filter((o) => !out.has(o.hash));
  return { kind: "kept", source, objects, heads, notice };
}

/**
 * The stores the checked addresses hold, in the link's order. An address
 * that shares a prodrome with an earlier store joins that store; a store
 * keeps the union of its addresses' objects, so where two copies differ,
 * the store holds every object either copy holds.
 */
function group(checked: Extract<Checked, { kind: "kept" }>[]): Store[] {
  const stores: Store[] = [];
  for (const one of checked) {
    const same = stores.find((store) => one.heads.some((head) => store.heads.has(head)));
    const store: Store = same ?? { label: one.source.label, sources: [], objects: new Map(), heads: new Set() };
    if (same === undefined) stores.push(store);
    store.sources.push(one.source);
    one.heads.forEach((head) => store.heads.add(head));
    for (const object of one.objects) store.objects.set(object.hash, object);
  }
  return stores;
}

function members(core: Core, objects: ObjectIn[], at: string): Member[] {
  const replica = new core.Todos(JSON.stringify(objects));
  try {
    return (JSON.parse(replica.prices(at, "[]")) as { prices: Member[] }).prices;
  } finally {
    replica.free();
  }
}

/** The page's link to an item: its key, and the stores' part of the address. */
export function hrefOf(key: string, query: string): string {
  return `#/todo/${key}${query}`;
}

/**
 * The data of one page of the stores, read at `now`: every row of every
 * store, in the core's one list order. `route` says which item the page is
 * about, if any. With one store a row's key is its todo id, as on a page
 * of one store; with several it is `<label>/<todo>`, so two stores' items
 * never share a key, whatever their ids.
 */
export function readStores(
  core: Core,
  checked: Checked[],
  now: Naive,
  route: Route,
  query: string,
): { page: Page; notices: Notice[] } {
  const at = iso(now);
  const notices = checked.map((c) => c.notice).filter((n) => n.lines.length > 0);
  const kept = checked.flatMap((c) => (c.kind === "kept" ? [c] : []));
  const grouped = group(kept);

  // One legacy chain at most: the core reads every legacy object as one
  // prodrome with no genesis, so the items of two such stores would merge.
  const stores: { store: Store; members: Member[] }[] = [];
  let legacy: string | null = null;
  for (const store of grouped) {
    const read = members(core, [...store.objects.values()], at);
    if (read.some((m) => m.genesis === null)) {
      if (legacy !== null) {
        notices.push({
          label: store.label,
          shown: store.sources.map((s) => s.shown).join(", "),
          lines: [`this store and ${legacy} are both legacy chains with no genesis, which one list cannot tell apart; open it alone`],
        });
        continue;
      }
      legacy = store.label;
    }
    const twin = stores.find((s) => s.store.label === store.label);
    if (twin !== undefined) {
      notices.push({
        label: store.label,
        shown: store.sources.map((s) => s.shown).join(", "),
        lines: [`another store of the link has the label ${store.label}; give this one its own label`],
      });
      continue;
    }
    stores.push({ store, members: read });
  }

  const several = stores.length > 1;
  const keyOf = (label: string, todo: string) => (several ? `${label}/${todo}` : todo);
  const byGenesis = new Map<string | null, string>();
  for (const { store, members: ms } of stores) for (const m of ms) byGenesis.set(m.genesis, store.label);
  const set = JSON.stringify(stores.flatMap((s) => s.members));

  const union = stores.flatMap((s) => [...s.store.objects.values()]);
  const replica = new core.Todos(JSON.stringify(union));
  let rows: Row[];
  try {
    rows = (JSON.parse(replica.entries(at, "[]")) as { entries: Row[] }).entries;
  } finally {
    replica.free();
  }

  // The store of the item the page is about: the one the route names, or
  // the first store, in the core's order, that holds the todo.
  const focus = route.page === "item" ? route : null;
  const holder = (todo: string) => byGenesis.get(rows.find((r) => r.todo === todo && byGenesis.has(r.genesis))?.genesis ?? null);
  const focusLabel = focus === null ? null : focus.within.kind === "store" ? focus.within.label : (holder(focus.todo) ?? null);

  const page: Page = {
    at,
    entries: [],
    order: [],
    records: {},
    created: {},
    marks: {},
    explain: {},
    history: {},
    focus: focus === null ? null : focusLabel === null ? focus.todo : keyOf(focusLabel, focus.todo),
    stores: stores.map((s) => s.store.label),
    query,
    objects: "objects/",
    title: several ? "Stores" : stores.length === 1 && stores[0].store.sources[0].lister.kind === "where" ? stores[0].store.label : "Roadmap",
  };

  const shown = new Map<string, Shown>();
  const refers = new Map<string, Target[]>();
  for (const { store } of stores) {
    const label = store.label;
    const own = focusLabel === label && focus !== null ? focus.todo : null;
    const reading = read(core, [...store.objects.values()], now, own, set);
    const view = reading.view;
    if (own !== null) page.objects = `${store.sources[0].base}objects/`;
    Object.assign(page.records, view.records);
    for (const [todo, value] of Object.entries(view.created)) page.created[keyOf(label, todo)] = value;
    for (const [todo, value] of Object.entries(view.marks)) page.marks[keyOf(label, todo)] = value;
    for (const [todo, value] of Object.entries(view.explain)) page.explain[keyOf(label, todo)] = value;
    for (const [todo, value] of Object.entries(view.history)) page.history[keyOf(label, todo)] = value;
    for (const entry of view.entries) {
      const key = keyOf(label, entry.todo);
      if (!shown.has(key)) shown.set(key, { ...entry, key, store: label, waits: [], waitedBy: [] });
      refers.set(key, reading.targets[entry.todo] ?? []);
    }
  }

  // The core's order and prices, over the union: each row back to its store.
  for (const row of rows) {
    const label = byGenesis.get(row.genesis);
    if (label === undefined) continue;
    const entry = shown.get(keyOf(label, row.todo));
    if (entry === undefined) continue;
    entry.value = row.value;
    entry.unlinked = row.unlinked;
    page.order.push(entry.key);
    page.entries.push(entry);
  }

  // The waits, each linked where its store is loaded, both ways.
  for (const entry of page.entries) {
    for (const target of refers.get(entry.key) ?? []) {
      const key =
        target.kind === "home"
          ? keyOf(entry.store, target.todo)
          : byGenesis.has(target.store)
            ? keyOf(byGenesis.get(target.store) as string, target.entity)
            : null;
      const other = key === null ? undefined : shown.get(key);
      if (other === undefined) {
        // A reference home to a todo the store lacks does not link, and the
        // row's `unlinked` says so; only a store not loaded is said here.
        if (target.kind === "other") {
          const store = byGenesis.get(target.store);
          entry.waits.push(
            store === undefined
              ? { kind: "elsewhere", label: `${target.store.slice(0, 12)}…/${target.entity}` }
              : { kind: "unheld", label: keyOf(store, target.entity) },
          );
        }
        continue;
      }
      entry.waits.push({ kind: "loaded", label: several ? other.key : other.todo, href: hrefOf(other.key, query) });
      other.waitedBy.push({ kind: "loaded", label: several ? entry.key : entry.todo, href: hrefOf(entry.key, query) });
    }
  }
  return { page, notices };
}

/** The sources a fragment's addresses name, each with its label, in order. */
export function sources(addresses: Address[]): Source[] {
  const names = labels(addresses);
  return addresses.map((address, at) => ({
    label: names[at],
    base: baseOf(address.where),
    pin: address.pin,
    lister: { kind: "where", where: address.where },
    shown: printAddress(address),
  }));
}
