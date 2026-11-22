// SEVERAL STORES, ONE LIST. This file turns the objects that the page
// fetched from each address into the data that one page typesets:
//
// 1. CHECK each address's objects with the core (`Todos.tips`). The core
//    leaves out every object whose bytes do not hash to its name, every
//    object that does not parse, and everything that rests on one of them
//    (SPEC §3, law 40). The page keeps only what the core keeps, and says
//    what it left out. No check of a name or of bytes is written here.
// 2. GROUP the addresses by the prodromes they hold. A prodrome is named by
//    its genesis (a legacy chain by its least root, as `tips` names it), so
//    addresses that share a prodrome, directly or through a third address,
//    are one store. The store holds the union of their objects, as a merge
//    does, and the page says that it joined them.
// 3. SPLIT a store that holds several prodromes into one part for each, so
//    that two prodromes' items never share a row, whatever their ids.
// 4. READ each part on its own (view.ts), and read the union of all the
//    parts once with the core's `entries`. Law 41 says that each part reads
//    in the union as it reads alone, and that a qualified reference reads
//    the entity's price in its own prodrome. So the union gives the one
//    list order (SPEC §6.7) and every price, a wait across stores included,
//    and no row is sorted or priced here.
//
// A store that the core cannot read is left out with a notice, and the
// others are shown: each step holds its own failures.

import { baseOf, labels, printAddress, type Address, type Pin, type Route } from "./address";
import type { Fetched, Lister, Listing } from "./fetch";
import type { Texts } from "./typst";
import { read, type Core, type Entry, type ObjectIn, type Replica, type Target, type View } from "./view";
import { iso, type Naive } from "./time";

/** One address of a store, as the page loads it. The site's own store has no address. */
export interface Source {
  label: string;
  /** The URL under which the store's `objects/` lies, with its `/`. */
  base: string;
  pin: Pin;
  /** Where the names of the store's objects come from: the site, or a link. */
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

/** What the page tells a reader about one address or store. */
export interface Notice {
  label: string;
  shown: string;
  lines: string[];
}

/** One address, checked: the objects the core kept and the heads of each prodrome they hold, or why none are shown. */
export type Checked =
  | { kind: "kept"; source: Source; objects: ObjectIn[]; heads: Record<string, string[]>; notice: Notice }
  | { kind: "refused"; source: Source; notice: Notice };

/** A store: the addresses that hold its prodromes, each with the names of its objects, and their union. */
interface Store {
  label: string;
  sources: { source: Source; names: Set<string> }[];
  objects: Map<string, ObjectIn>;
  /** Each prodrome's heads, by its genesis (a legacy chain's least root). */
  heads: Map<string, string[]>;
}

/** One prodrome of a store, as the page shows it: what a row of the list names as its store. */
interface Part {
  label: string;
  store: Store;
  objects: ObjectIn[];
}

/** A store as the bar above the page names it: its label, its addresses, and whether this site serves it. */
export interface Shelf {
  label: string;
  addresses: string[];
  from: Lister["kind"];
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

/** A core answer, or the text of its refusal: one store's failure stays that store's. */
function attempt<T>(run: () => T): { kind: "ok"; value: T } | { kind: "failed"; why: string } {
  try {
    return { kind: "ok", value: run() };
  } catch (error) {
    return { kind: "failed", why: error instanceof Error ? error.message : String(error) };
  }
}

/** A core question asked of a set of objects, the replica freed after. */
function ask<T>(core: Core, objects: ObjectIn[], question: (replica: Replica) => T): T {
  const replica = new core.Todos(JSON.stringify(objects));
  try {
    return question(replica);
  } finally {
    replica.free();
  }
}

const short = (name: string) => name.slice(0, 12);

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
  for (const m of missing) lines.push(`the listing names ${short(m.hash)}, which the page did not get: ${m.why}`);
  const answer = attempt(() => ask(core, held, (replica) => JSON.parse(replica.tips()) as Tips));
  if (answer.kind === "failed") {
    lines.push(`the core could not read this store: ${answer.why}`);
    return { kind: "refused", source, notice };
  }
  const tips = answer.value;
  const out = new Set<string>();
  for (const left of tips.excluded) {
    out.add(left.name);
    left.resting.forEach((name) => out.add(name));
    const n = left.resting.length;
    const resting = n === 0 ? "" : `, and ${n} ${n === 1 ? "object that rests" : "objects that rest"} on it ${n === 1 ? "is" : "are"} left out with it`;
    lines.push(`object ${short(left.name)} is left out: ${left.why}${resting}`);
  }
  const heads = Object.keys(tips.heads);
  if (source.pin.kind === "genesis" && !heads.includes(source.pin.genesis)) {
    lines.push(
      `the link expects the genesis ${short(source.pin.genesis)}, and this store holds ` +
        (heads.length === 0 ? "none" : heads.map(short).join(", ")),
    );
    return { kind: "refused", source, notice };
  }
  const objects = held.filter((o) => !out.has(o.hash));
  return { kind: "kept", source, objects, heads: tips.heads, notice };
}

/**
 * The stores the checked addresses hold, in the link's order. An address
 * joins every earlier store with which it shares a prodrome, and those
 * stores become one, so the grouping does not depend on the order of the
 * addresses. A store keeps the union of its addresses' objects, so where two
 * copies differ, the store holds every object either copy holds.
 */
function group(checked: Extract<Checked, { kind: "kept" }>[]): Store[] {
  let stores: Store[] = [];
  for (const one of checked) {
    const keys = Object.keys(one.heads);
    const joined = stores.filter((store) => keys.some((key) => store.heads.has(key)));
    const store: Store = joined[0] ?? { label: one.source.label, sources: [], objects: new Map(), heads: new Map() };
    for (const other of joined.slice(1)) {
      store.sources.push(...other.sources);
      other.objects.forEach((object, name) => store.objects.set(name, object));
      other.heads.forEach((heads, key) => store.heads.set(key, heads));
    }
    stores = stores.filter((s) => !joined.slice(1).includes(s));
    if (joined.length === 0) stores.push(store);
    store.sources.push({ source: one.source, names: new Set(one.objects.map((o) => o.hash)) });
    for (const object of one.objects) store.objects.set(object.hash, object);
    for (const key of keys) store.heads.set(key, [...new Set([...(store.heads.get(key) ?? []), ...one.heads[key]])]);
  }
  return stores;
}

/** What the page says of a store that several addresses hold: that it joined them, and what only one of them holds. */
function joinedNotice(store: Store): Notice | null {
  if (store.sources.length < 2) return null;
  const shown = store.sources.map((s) => s.source.shown);
  const lines = [`these ${shown.length} addresses hold the same prodrome, and the page shows them as one store: ${shown.join(", ")}`];
  for (const { source, names } of store.sources) {
    const only = [...names].filter((name) => store.sources.every((s) => s.names === names || !s.names.has(name)));
    if (only.length > 0) lines.push(`${only.length} of its objects come only from ${source.shown}: ${only.map(short).join(", ")}`);
  }
  return { label: store.label, shown: shown.join(", "), lines };
}

/**
 * A store's parts, one for each prodrome it holds. A part of a store with
 * one prodrome is the store. A part of a store with several is the objects
 * on which that prodrome's heads rest, as the core's `since` finds them,
 * labelled `<store>-<genesis's first 8 hex>`.
 */
function split(core: Core, store: Store): Part[] {
  const all = [...store.objects.values()];
  if (store.heads.size === 1) return [{ label: store.label, store, objects: all }];
  return [...store.heads].map(([key, heads]) => {
    const since = new Set(ask(core, all, (replica) => (JSON.parse(replica.since(JSON.stringify(heads))) as { since: string[] }).since));
    return { label: `${store.label}-${key.slice(0, 8)}`, store, objects: all.filter((o) => !since.has(o.hash)) };
  });
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
 * never share a key, whatever their ids. `texts` says how the page reads
 * the items' text: as Typst only when every store is this site's own.
 */
export function readStores(
  core: Core,
  checked: Checked[],
  now: Naive,
  route: Route,
  query: string,
): { page: Page; notices: Notice[]; shelves: Shelf[]; texts: Texts } {
  const at = iso(now);
  const notices = checked.map((c) => c.notice).filter((n) => n.lines.length > 0);
  const grouped = group(checked.flatMap((c) => (c.kind === "kept" ? [c] : [])));
  const say = (label: string, store: Store, line: string) =>
    notices.push({ label, shown: store.sources.map((s) => s.source.shown).join(", "), lines: [line] });

  const parts: { part: Part; members: Member[] }[] = [];
  let legacy: string | null = null;
  for (const store of grouped) {
    const joined = joinedNotice(store);
    if (joined !== null) notices.push(joined);
    const parted = attempt(() => split(core, store));
    if (parted.kind === "failed") {
      say(store.label, store, `the core could not read this store: ${parted.why}`);
      continue;
    }
    if (parted.value.length > 1) {
      say(store.label, store, `this store holds ${parted.value.length} prodromes, shown as ${parted.value.map((p) => p.label).join(", ")}`);
    }
    for (const part of parted.value) {
      const read = attempt(() => ask(core, part.objects, (replica) => (JSON.parse(replica.prices(at, "[]")) as { prices: Member[] }).prices));
      if (read.kind === "failed") {
        say(part.label, store, `the core could not read this store: ${read.why}`);
        continue;
      }
      // One legacy chain at most: the core reads every legacy object as one
      // prodrome with no genesis, so the items of two such stores would merge.
      if (read.value.some((m) => m.genesis === null)) {
        if (legacy !== null) {
          say(part.label, store, `this store and ${legacy} are both legacy chains with no genesis, which one list cannot tell apart; open it alone`);
          continue;
        }
        legacy = part.label;
      }
      if (parts.some((p) => p.part.label === part.label)) {
        say(part.label, store, `another store of the link has the label ${part.label}; give this one its own label`);
        continue;
      }
      parts.push({ part, members: read.value });
    }
  }

  const several = parts.length > 1;
  const keyOf = (label: string, todo: string) => (several ? `${label}/${todo}` : todo);
  const byGenesis = new Map<string | null, string>();
  for (const { part, members } of parts) for (const m of members) byGenesis.set(m.genesis, part.label);
  const set = JSON.stringify(parts.flatMap((p) => p.members));

  const union = parts.flatMap((p) => p.part.objects);
  const answer = attempt(() => ask(core, union, (replica) => (JSON.parse(replica.entries(at, "[]")) as { entries: Row[] }).entries));
  const rows = answer.kind === "ok" ? answer.value : [];
  if (answer.kind === "failed") notices.push({ label: "stores", shown: query, lines: [`the core could not read the stores together: ${answer.why}`] });

  // The part of the item the page is about: the one the route names, or
  // the first part, in the core's order, that holds the todo.
  const focus = route.page === "item" ? route : null;
  const holder = (todo: string) => byGenesis.get(rows.find((r) => r.todo === todo && byGenesis.has(r.genesis))?.genesis ?? null);
  const focusLabel = focus === null ? null : focus.within.kind === "store" ? focus.within.label : (holder(focus.todo) ?? null);
  const site = parts.every((p) => p.part.store.sources.every((s) => s.source.lister.kind === "site"));

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
    stores: parts.map((p) => p.part.label),
    query,
    objects: "objects/",
    title: several ? "Stores" : site ? "Roadmap" : (parts[0]?.part.label ?? "Stores"),
  };

  const shown = new Map<string, Shown>();
  const refers = new Map<string, Target[]>();
  for (const { part } of parts) {
    const label = part.label;
    const own = focusLabel === label && focus !== null ? focus.todo : null;
    const reading = attempt(() => read(core, part.objects, now, own, set));
    if (reading.kind === "failed") {
      say(label, part.store, `the core could not read this store: ${reading.why}`);
      continue;
    }
    const view = reading.value.view;
    if (own !== null) page.objects = `${part.store.sources[0].source.base}objects/`;
    Object.assign(page.records, view.records);
    for (const [todo, value] of Object.entries(view.created)) page.created[keyOf(label, todo)] = value;
    for (const [todo, value] of Object.entries(view.marks)) page.marks[keyOf(label, todo)] = value;
    for (const [todo, value] of Object.entries(view.explain)) page.explain[keyOf(label, todo)] = value;
    for (const [todo, value] of Object.entries(view.history)) page.history[keyOf(label, todo)] = value;
    for (const entry of view.entries) {
      const key = keyOf(label, entry.todo);
      if (!shown.has(key)) shown.set(key, { ...entry, key, store: label, waits: [], waitedBy: [] });
      refers.set(key, reading.value.targets[entry.todo] ?? []);
    }
  }

  // The core's order and prices, over the union: each row back to its part.
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
              ? { kind: "elsewhere", label: `${short(target.store)}…/${target.entity}` }
              : { kind: "unheld", label: keyOf(store, target.entity) },
          );
        }
        continue;
      }
      entry.waits.push({ kind: "loaded", label: several ? other.key : other.todo, href: hrefOf(other.key, query) });
      other.waitedBy.push({ kind: "loaded", label: several ? entry.key : entry.todo, href: hrefOf(entry.key, query) });
    }
  }

  const shelves = parts.map(({ part }) => ({
    label: part.label,
    addresses: part.store.sources.map((s) => s.source.shown),
    from: part.store.sources.every((s) => s.source.lister.kind === "site") ? ("site" as const) : ("where" as const),
  }));
  const texts: Texts = site ? { kind: "markup" } : { kind: "verbatim" };
  return { page, notices, shelves, texts };
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
