// HOW THE PAGE GETS A STORE'S OBJECTS FROM A STATIC HOST. A plain file host
// cannot list a directory, so the page first reads a LISTING, the names of
// the objects, and then fetches each object by its name:
//
// 1. `<base>index.json`, a JSON array of the names, as `viewer/assemble.sh`
//    writes it beside a published copy of a store;
// 2. for a repository on GitHub with no such file, the host's tree listing
//    (one request to api.github.com, which limits a visitor who is not
//    signed in; see docs/design-viewer-stores.md).
//
// The page trusts neither the listing nor the host. A listing can leave an
// object out, and a host can serve any bytes: the core keeps an object only
// if its bytes hash to its name (stores.ts). Every failure here is a value
// that the page shows, never an exception that ends the page.

import type { Where } from "./address";
import type { ObjectIn } from "./view";

/** A name as SPEC §3 has it: the SHA-256 of an object's print, 64 lowercase hex. */
const NAME = /^[0-9a-f]{64}$/;

/**
 * THE BOUNDS ON ONE ADDRESS, so that a link to a hostile or huge store costs
 * a visitor a known amount. The page says which bound a store met.
 */
export const BOUNDS = {
  /** The most objects the page reads of one address. */
  objects: 20_000,
  /** The largest object, in bytes. */
  objectBytes: 1 << 20,
  /** The most bytes of objects the page reads of one address. */
  storeBytes: 64 << 20,
  /** The largest listing, in bytes: an `index.json`, or GitHub's tree listing. */
  listingBytes: 16 << 20,
  /** The most requests to one address at once. */
  atOnce: 16,
  /** The longest the page waits for one address, listing and objects, in milliseconds. */
  deadline: 60_000,
};

/**
 * How the page fetches from a store's host: with no cookies or other
 * credentials, and no `Referer`. A redirect is followed. The redirect can
 * lead anywhere, and that changes nothing about trust: the core checks
 * every object's bytes against its name, wherever the bytes came from, and
 * the page's policy allows only https.
 */
const ASK: RequestInit = { credentials: "omit", referrerPolicy: "no-referrer", redirect: "follow", mode: "cors" };

/** A body read whole, or why not: larger than `most` bytes, or not sent. */
async function bounded(response: Response, most: number): Promise<{ kind: "read"; bytes: Uint8Array } | { kind: "large" }> {
  const said = Number(response.headers.get("content-length"));
  if (Number.isFinite(said) && said > most) return { kind: "large" };
  const bytes = new Uint8Array(await response.arrayBuffer());
  return bytes.length > most ? { kind: "large" } : { kind: "read", bytes };
}

/** UTF-8 text, byte for byte: a leading byte-order mark is kept, so the core sees what the host sent. */
const UTF8 = new TextDecoder("utf-8", { ignoreBOM: true });

/** The names a listing gave, how the page got them, and what it noticed on the way. */
export type Listing =
  | { kind: "listed"; names: string[]; how: string; notes: string[] }
  | { kind: "failed"; why: string };

/** One object as the host answered for it. */
export type Fetched = { kind: "held"; object: ObjectIn } | { kind: "missing"; hash: string; why: string };

/** Where a listing comes from: the site's own store, or a store's address. */
export type Lister = { kind: "site" } | { kind: "where"; where: Where };

/** The JSON array of an `index.json`, its names kept and the rest counted. */
function names(json: unknown, from: string): Listing {
  if (!Array.isArray(json)) return { kind: "failed", why: `${from} is not a JSON array of names` };
  if (json.length > BOUNDS.objects) return { kind: "failed", why: `${from} lists ${json.length} objects, and the page reads at most ${BOUNDS.objects}` };
  const kept = json.filter((name): name is string => typeof name === "string" && NAME.test(name));
  const notes = kept.length < json.length ? [`${from} holds ${json.length - kept.length} entries that are not object names`] : [];
  return { kind: "listed", names: [...new Set(kept)], how: from, notes };
}

async function index(base: string, signal: AbortSignal): Promise<Listing | { kind: "absent" }> {
  try {
    const response = await fetch(`${base}index.json`, { ...ASK, cache: "no-cache", signal });
    if (response.status === 404) return { kind: "absent" };
    if (!response.ok) return { kind: "failed", why: `${base}index.json: HTTP ${response.status}` };
    const body = await bounded(response, BOUNDS.listingBytes);
    if (body.kind === "large") return { kind: "failed", why: `${base}index.json is larger than ${BOUNDS.listingBytes} bytes` };
    return names(JSON.parse(UTF8.decode(body.bytes)), "index.json");
  } catch (error) {
    return { kind: "failed", why: `${base}index.json could not be read (${String(error)})` };
  }
}

/** The instant a rate limit renews, as the host's epoch seconds say it. */
function renews(reset: string | null): string {
  const seconds = Number(reset);
  return Number.isFinite(seconds) && seconds > 0 ? ` They renew at ${new Date(seconds * 1000).toUTCString()}.` : "";
}

interface Tree {
  tree: { path: string; type: string }[];
  truncated: boolean;
}

/** GitHub's tree listing of a ref, filtered to the files of `<dir>/objects/`. */
async function githubTree(where: Extract<Where, { kind: "github" }>, signal: AbortSignal): Promise<Listing> {
  const ref = where.ref.kind === "named" ? where.ref.name : "HEAD";
  const repo = `${encodeURIComponent(where.owner)}/${encodeURIComponent(where.repo)}`;
  const url = `https://api.github.com/repos/${repo}/git/trees/${encodeURIComponent(ref)}?recursive=1`;
  let response: Response;
  try {
    response = await fetch(url, { ...ASK, headers: { accept: "application/vnd.github+json" }, cache: "no-cache", signal });
  } catch (error) {
    return { kind: "failed", why: `the GitHub listing could not be fetched (${String(error)})` };
  }
  if ((response.status === 403 || response.status === 429) && response.headers.get("x-ratelimit-remaining") === "0") {
    return {
      kind: "failed",
      why:
        "GitHub allows a visitor who is not signed in a small number of listings an hour, and this browser has used them." +
        renews(response.headers.get("x-ratelimit-reset")) +
        " A store with an index.json needs no listing (see docs/viewer.md).",
    };
  }
  if (!response.ok) return { kind: "failed", why: `the GitHub listing of ${where.owner}/${where.repo}@${ref}: HTTP ${response.status}` };
  let tree: Tree;
  try {
    const body = await bounded(response, BOUNDS.listingBytes);
    if (body.kind === "large") return { kind: "failed", why: `the GitHub listing is larger than ${BOUNDS.listingBytes} bytes` };
    tree = JSON.parse(UTF8.decode(body.bytes)) as Tree;
  } catch {
    return { kind: "failed", why: "the GitHub listing could not be read as JSON" };
  }
  const prefix = [...where.dir, "objects", ""].join("/");
  const found = (Array.isArray(tree.tree) ? tree.tree : [])
    .filter((entry) => entry.type === "blob" && entry.path.startsWith(prefix) && entry.path.endsWith(".py"))
    .map((entry) => entry.path.slice(prefix.length, -3))
    .filter((name) => NAME.test(name));
  if (found.length === 0) {
    return {
      kind: "failed",
      why: tree.truncated
        ? `GitHub cut its listing short before it reached ${prefix}: add an index.json to the store`
        : `the repository has no objects under ${prefix}`,
    };
  }
  if (found.length > BOUNDS.objects) {
    return { kind: "failed", why: `the GitHub listing names ${found.length} objects, and the page reads at most ${BOUNDS.objects}` };
  }
  const notes = tree.truncated
    ? ["GitHub cut its listing short, so some objects may be missing: add an index.json to the store"]
    : [];
  return { kind: "listed", names: [...new Set(found)], how: "the GitHub tree listing", notes };
}

/** The names of a store's objects: its `index.json`, else the git host's listing where there is one. */
export async function list(lister: Lister, base: string, signal: AbortSignal): Promise<Listing> {
  const first = await index(base, signal);
  if (first.kind !== "absent") return first;
  switch (lister.kind) {
    case "site":
      return { kind: "failed", why: "the site has no index.json" };
    case "where":
      switch (lister.where.kind) {
        case "github":
          return githubTree(lister.where, signal);
        case "base":
          return { kind: "failed", why: `${base} has no index.json, and a plain host cannot list objects/ (see docs/viewer.md)` };
      }
  }
}

/**
 * Every named object, each held or missing with the reason, at most
 * `BOUNDS.atOnce` requests at a time. An object larger than its bound is
 * missing; so is every object after the store's bytes met theirs, and every
 * object not read when `signal` aborts.
 */
export async function fetchObjects(base: string, names: string[], signal: AbortSignal): Promise<Fetched[]> {
  const out: Fetched[] = new Array(names.length);
  let next = 0;
  let total = 0;
  const one = async (hash: string): Promise<Fetched> => {
    if (total > BOUNDS.storeBytes) return { kind: "missing", hash, why: `the store holds more than ${BOUNDS.storeBytes} bytes, the page's bound` };
    try {
      const response = await fetch(`${base}objects/${hash}.py`, { ...ASK, signal });
      if (!response.ok) return { kind: "missing", hash, why: `HTTP ${response.status}` };
      const body = await bounded(response, BOUNDS.objectBytes);
      if (body.kind === "large") return { kind: "missing", hash, why: `larger than ${BOUNDS.objectBytes} bytes, the page's bound` };
      total += body.bytes.length;
      return { kind: "held", object: { hash, text: UTF8.decode(body.bytes) } };
    } catch (error) {
      return { kind: "missing", hash, why: signal.aborted ? `not read within ${BOUNDS.deadline / 1000} s, the page's bound` : String(error) };
    }
  };
  const worker = async () => {
    for (let at = next++; at < names.length; at = next++) out[at] = await one(names[at]);
  };
  await Promise.all(Array.from({ length: Math.min(BOUNDS.atOnce, names.length) }, worker));
  return out;
}
