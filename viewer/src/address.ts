// THE PAGE'S ADDRESS: which page to show, and which stores to show it of.
//
// Everything after `#` is the fragment, and a browser never sends the
// fragment to the viewer's own host. So the addresses of the stores go
// there, and the page reads them itself. See docs/design-viewer-stores.md
// for the grammar and its reasons. In short:
//
//   fragment = route [ "?" store *( "&" store ) ]
//   route    = "/" | "/in/" label | "/todo/" [ label "/" ] todo
//   store    = [ label "=" ] where [ "!" genesis ]
//   where    = "gh:" owner "/" repo [ "@" ref ] *( "/" segment )
//            | "https://" host path | "http://" local-host path
//
// `parseFragment` is the one parser and `printFragment` the one printer.
// The law: for every fragment value `f`, `parseFragment(printFragment(f))`
// is `f` (viewer/test/address.test.ts checks it over generated values).

/** The rule of a label: a short name for a store, chosen by the link's author or derived. */
const LABEL = /^[a-z0-9_-]{1,32}$/;
/** The rule of a todo id (SPEC §2, `TodoId`). */
const TODO = /^[a-z0-9_-]+$/;
/** A genesis's name: 64 lowercase hex. */
const GENESIS = /^[0-9a-f]{64}$/;
/** GitHub's rules for an owner and for a repository's name. */
const OWNER = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/;
const REPO = /^[A-Za-z0-9._-]{1,100}$/;
/** Hosts that may be read over plain http: only this machine, for tests. */
const LOCAL = new Set(["localhost", "127.0.0.1", "[::1]"]);

/** The most stores one link can name. */
export const MOST_STORES = 8;
/** The longest text one store may take in the fragment, escapes included. */
export const LONGEST_STORE = 1024;
/** The longest fragment the parser reads. */
export const LONGEST_FRAGMENT = 8192;

/** Which ref of a repository: the host's default branch, or a named one. */
export type Ref = { kind: "default" } | { kind: "named"; name: string };

/** Where a store's `objects/` directory is. */
export type Where =
  | { kind: "github"; owner: string; repo: string; ref: Ref; dir: string[] }
  /** `url` is absolute, with no query, fragment or user, and ends with `/`. */
  | { kind: "base"; url: string };

/** What a link expects of the store: anything, or that it holds one genesis. */
export type Pin = { kind: "any" } | { kind: "genesis"; genesis: string };

/** The label a link gives a store, or none, so that the page derives one. */
export type Label = { kind: "derived" } | { kind: "named"; name: string };

export interface Address {
  label: Label;
  where: Where;
  pin: Pin;
}

/** Which stores a page is about: all that are loaded, or the one with this label. */
export type Within = { kind: "every" } | { kind: "store"; label: string };

export type Route = { page: "list"; within: Within } | { page: "item"; within: Within; todo: string };

/** One store's text from the fragment: an address, or why it is not one. */
export type Parsed = { kind: "address"; address: Address } | { kind: "refused"; text: string; why: string };

export interface Fragment {
  route: Route;
  stores: Address[];
}

/** What `parseFragment` reads: the route and every store's text, each parsed or refused. */
export interface Read {
  route: Route;
  stores: Parsed[];
}

type Result<T> = { kind: "ok"; value: T } | { kind: "refused"; why: string };

const ok = <T>(value: T): Result<T> => ({ kind: "ok", value });
const refused = <T>(why: string): Result<T> => ({ kind: "refused", why });

// --- escapes ------------------------------------------------------------------

/**
 * The characters that a component prints as they are. Every other character
 * prints as `%XX`, byte by byte in UTF-8. The set leaves out the characters
 * that the grammar reserves: `% & = ! ? # @ /` and space.
 */
const PLAIN = /[A-Za-z0-9\-._~:$'()*+,;]/;

function escape(text: string): string {
  let out = "";
  for (const ch of text) {
    if (PLAIN.test(ch)) out += ch;
    else for (const byte of new TextEncoder().encode(ch)) out += `%${byte.toString(16).toUpperCase().padStart(2, "0")}`;
  }
  return out;
}

function unescape(text: string): Result<string> {
  try {
    return ok(decodeURIComponent(text));
  } catch {
    return refused(`${JSON.stringify(text)} has a broken % escape`);
  }
}

// --- one store -----------------------------------------------------------------

function parseRef(text: string): Result<Ref> {
  const name = unescape(text);
  if (name.kind === "refused") return name;
  if (name.value === "") return refused("the ref after @ is empty");
  return ok({ kind: "named", name: name.value });
}

function parseSegment(text: string): Result<string> {
  const segment = unescape(text);
  if (segment.kind === "refused") return segment;
  if (segment.value === "" || segment.value === "." || segment.value === "..") {
    return refused(`the directory segment ${JSON.stringify(segment.value)} is not allowed`);
  }
  if (segment.value.includes("/")) return refused("a directory segment holds no /");
  return ok(segment.value);
}

function parseGithub(text: string): Result<Where> {
  const [head, ...rest] = text.split("/");
  const owner = unescape(head);
  if (owner.kind === "refused") return owner;
  if (!OWNER.test(owner.value)) return refused(`${JSON.stringify(owner.value)} is not a GitHub owner`);
  if (rest.length === 0) return refused("gh: needs owner/repository");
  const [repoText, ...dirText] = rest;
  const at = repoText.indexOf("@");
  const repo = unescape(at < 0 ? repoText : repoText.slice(0, at));
  if (repo.kind === "refused") return repo;
  if (!REPO.test(repo.value) || repo.value === "." || repo.value === "..") {
    return refused(`${JSON.stringify(repo.value)} is not a repository name`);
  }
  let ref: Ref = { kind: "default" };
  if (at >= 0) {
    const named = parseRef(repoText.slice(at + 1));
    if (named.kind === "refused") return named;
    ref = named.value;
  }
  const dir: string[] = [];
  for (const segment of dirText) {
    const parsed = parseSegment(segment);
    if (parsed.kind === "refused") return parsed;
    dir.push(parsed.value);
  }
  return ok({ kind: "github", owner: owner.value, repo: repo.value, ref, dir });
}

/**
 * A path segment in the one form a base URL keeps: decoded, then escaped
 * by `escape`. So two spellings of one path are one address, and the
 * reserved characters of the grammar never appear raw in it.
 */
function canonicalSegment(segment: string): Result<string> {
  const raw = unescape(segment);
  return raw.kind === "refused" ? raw : ok(escape(raw.value));
}

function parseBase(text: string): Result<Where> {
  if (text.includes("?") || text.includes("#")) return refused("a base URL holds no query and no fragment");
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    return refused(`${JSON.stringify(text)} is not a URL`);
  }
  if (url.protocol !== "https:" && !(url.protocol === "http:" && LOCAL.has(url.hostname))) {
    return refused("a base URL is https (plain http only for this machine)");
  }
  if (url.username !== "" || url.password !== "") return refused("a base URL holds no user or password");
  const segments: string[] = [];
  for (const segment of url.pathname.split("/").slice(1)) {
    const canonical = canonicalSegment(segment);
    if (canonical.kind === "refused") return canonical;
    segments.push(canonical.value);
  }
  const path = segments.join("/");
  return ok({ kind: "base", url: `${url.origin}/${path}${path === "" || path.endsWith("/") ? "" : "/"}` });
}

function parseWhere(text: string): Result<Where> {
  if (text.startsWith("gh:")) return parseGithub(text.slice(3));
  if (/^https?:/.test(text)) return parseBase(text);
  return refused("a store is gh:owner/repository or a base URL that starts with https://");
}

/** One store's text, as the fragment holds it between `?`, `&` and the end. */
export function parseAddress(text: string): Result<Address> {
  if (text.length > LONGEST_STORE) return refused(`a store's text is at most ${LONGEST_STORE} characters`);
  let rest = text;
  let label: Label = { kind: "derived" };
  const equals = rest.indexOf("=");
  if (equals >= 0) {
    const name = rest.slice(0, equals);
    if (!LABEL.test(name)) return refused(`${JSON.stringify(name)} is not a label (a-z, 0-9, _ and -, at most 32)`);
    label = { kind: "named", name };
    rest = rest.slice(equals + 1);
  }
  let pin: Pin = { kind: "any" };
  const bang = rest.lastIndexOf("!");
  if (bang >= 0) {
    const genesis = rest.slice(bang + 1);
    if (!GENESIS.test(genesis)) return refused("the text after ! is a genesis: 64 lowercase hex");
    pin = { kind: "genesis", genesis };
    rest = rest.slice(0, bang);
  }
  const where = parseWhere(rest);
  if (where.kind === "refused") return where;
  return ok({ label, where: where.value, pin });
}

function printWhere(where: Where): string {
  switch (where.kind) {
    case "github": {
      const ref = where.ref.kind === "named" ? `@${escape(where.ref.name)}` : "";
      const dir = where.dir.map((segment) => `/${escape(segment)}`).join("");
      return `gh:${escape(where.owner)}/${escape(where.repo)}${ref}${dir}`;
    }
    case "base":
      return where.url;
  }
}

export function printAddress(address: Address): string {
  const label = address.label.kind === "named" ? `${address.label.name}=` : "";
  const pin = address.pin.kind === "genesis" ? `!${address.pin.genesis}` : "";
  return `${label}${printWhere(address.where)}${pin}`;
}

// --- the route -------------------------------------------------------------------

function decoded(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

/** The route part of a fragment. Text that names no page is the list, as before stores. */
export function parseRoute(text: string): Route {
  const every: Within = { kind: "every" };
  const within = /^\/in\/([^/]+)$/.exec(text);
  if (within && LABEL.test(within[1])) return { page: "list", within: { kind: "store", label: within[1] } };
  const item = /^\/todo\/(.+)$/.exec(text);
  if (item) {
    const slash = item[1].indexOf("/");
    const label = slash > 0 ? decoded(item[1].slice(0, slash)) : "";
    const todo = decoded(item[1].slice(slash + 1));
    if (slash > 0 && LABEL.test(label) && TODO.test(todo)) {
      return { page: "item", within: { kind: "store", label }, todo };
    }
    return { page: "item", within: every, todo: decoded(item[1]) };
  }
  return { page: "list", within: every };
}

export function printRoute(route: Route): string {
  const store = route.within.kind === "store" ? route.within.label : null;
  switch (route.page) {
    case "list":
      return store === null ? "/" : `/in/${store}`;
    case "item":
      return `/todo/${store === null ? "" : `${store}/`}${encodeURIComponent(route.todo)}`;
  }
}

// --- the fragment ------------------------------------------------------------------

/** The text after `#` in a page's address: `location.hash` with its `#`, or without. */
export function parseFragment(hash: string): Read {
  const text = hash.startsWith("#") ? hash.slice(1) : hash;
  const question = text.indexOf("?");
  const route = parseRoute(question < 0 ? text : text.slice(0, question));
  if (question < 0) return { route, stores: [] };
  if (text.length > LONGEST_FRAGMENT) {
    return { route, stores: [{ kind: "refused", text: "", why: `the address is longer than ${LONGEST_FRAGMENT} characters` }] };
  }
  const parts = text
    .slice(question + 1)
    .split("&")
    .filter((part) => part !== "");
  const stores: Parsed[] = parts.map((part, at): Parsed => {
    if (at >= MOST_STORES) return { kind: "refused", text: part, why: `a link names at most ${MOST_STORES} stores` };
    const address = parseAddress(part);
    return address.kind === "ok" ? { kind: "address", address: address.value } : { kind: "refused", text: part, why: address.why };
  });
  return { route, stores };
}

/** The stores' part of a fragment, `?` included, or nothing when no store is named. */
export function printStores(stores: Address[]): string {
  return stores.length === 0 ? "" : `?${stores.map(printAddress).join("&")}`;
}

export function printFragment(fragment: Fragment): string {
  return `#${printRoute(fragment.route)}${printStores(fragment.stores)}`;
}

// --- what an address means --------------------------------------------------------

/** The URL under which a store's `objects/` lies, with its `/`. */
export function baseOf(where: Where): string {
  switch (where.kind) {
    case "github": {
      // raw.githubusercontent.com reads a ref's `/` as part of the ref.
      const ref = where.ref.kind === "named" ? where.ref.name.split("/") : ["HEAD"];
      const path = [where.owner, where.repo, ...ref, ...where.dir].map(encodeURIComponent).join("/");
      return `https://raw.githubusercontent.com/${path}/`;
    }
    case "base":
      return where.url;
  }
}

/** A label for an address whose link gives none: its last directory, else its repository or its host. */
function derive(where: Where): string {
  const text = (() => {
    switch (where.kind) {
      case "github":
        return where.dir.length > 0 ? where.dir[where.dir.length - 1] : where.repo;
      case "base": {
        const url = new URL(where.url);
        const segments = url.pathname.split("/").filter((s) => s !== "");
        return segments.length > 0 ? decoded(segments[segments.length - 1]) : url.hostname;
      }
    }
  })();
  const label = text
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 32);
  return label === "" ? "store" : label;
}

/**
 * Each address's label, in order: its own where the link names one, else a
 * derived one. A derived label that another address already holds takes
 * the first free suffix `-2`, `-3`, and so on. Two named labels that
 * are the same are both kept, and the page refuses the second store.
 */
export function labels(addresses: Address[]): string[] {
  const taken = new Set(addresses.flatMap((a) => (a.label.kind === "named" ? [a.label.name] : [])));
  return addresses.map((address) => {
    if (address.label.kind === "named") return address.label.name;
    const base = derive(address.where);
    let label = base;
    for (let n = 2; taken.has(label); n++) label = `${base.slice(0, 32 - String(n).length - 1)}-${n}`;
    taken.add(label);
    return label;
  });
}
