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
  const kept = json.filter((name): name is string => typeof name === "string" && NAME.test(name));
  const notes = kept.length < json.length ? [`${from} holds ${json.length - kept.length} entries that are not object names`] : [];
  return { kind: "listed", names: [...new Set(kept)], how: from, notes };
}

async function index(base: string): Promise<Listing | { kind: "absent"; status: string }> {
  let response: Response;
  try {
    response = await fetch(`${base}index.json`, { cache: "no-cache" });
  } catch (error) {
    return { kind: "failed", why: `${base}index.json could not be fetched (${String(error)})` };
  }
  if (response.status === 404) return { kind: "absent", status: "404" };
  if (!response.ok) return { kind: "failed", why: `${base}index.json: HTTP ${response.status}` };
  try {
    return names(await response.json(), "index.json");
  } catch {
    return { kind: "failed", why: `${base}index.json is not JSON` };
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
async function githubTree(where: Extract<Where, { kind: "github" }>): Promise<Listing> {
  const ref = where.ref.kind === "named" ? where.ref.name : "HEAD";
  const repo = `${encodeURIComponent(where.owner)}/${encodeURIComponent(where.repo)}`;
  const url = `https://api.github.com/repos/${repo}/git/trees/${encodeURIComponent(ref)}?recursive=1`;
  let response: Response;
  try {
    response = await fetch(url, { headers: { accept: "application/vnd.github+json" }, cache: "no-cache" });
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
    tree = (await response.json()) as Tree;
  } catch {
    return { kind: "failed", why: "the GitHub listing is not JSON" };
  }
  const prefix = [...where.dir, "objects", ""].join("/");
  const found = (Array.isArray(tree.tree) ? tree.tree : [])
    .filter((entry) => entry.type === "blob" && entry.path.startsWith(prefix) && entry.path.endsWith(".py"))
    .map((entry) => entry.path.slice(prefix.length, -3))
    .filter((name) => NAME.test(name));
  if (found.length === 0) return { kind: "failed", why: `the repository has no objects under ${prefix}` };
  const notes = tree.truncated
    ? ["GitHub cut its listing short, so some objects may be missing: add an index.json to the store"]
    : [];
  return { kind: "listed", names: [...new Set(found)], how: "the GitHub tree listing", notes };
}

/** The names of a store's objects: its `index.json`, else the git host's listing where there is one. */
export async function list(lister: Lister, base: string): Promise<Listing> {
  const first = await index(base);
  if (first.kind !== "absent") return first;
  switch (lister.kind) {
    case "site":
      return { kind: "failed", why: "the site has no index.json" };
    case "where":
      switch (lister.where.kind) {
        case "github":
          return githubTree(lister.where);
        case "base":
          return { kind: "failed", why: `${base} has no index.json, and a plain host cannot list objects/ (see docs/viewer.md)` };
      }
  }
}

/** Every named object, each held or missing with the reason. */
export function fetchObjects(base: string, names: string[]): Promise<Fetched[]> {
  return Promise.all(
    names.map(async (hash): Promise<Fetched> => {
      try {
        const response = await fetch(`${base}objects/${hash}.py`);
        if (!response.ok) return { kind: "missing", hash, why: `HTTP ${response.status}` };
        return { kind: "held", object: { hash, text: await response.text() } };
      } catch (error) {
        return { kind: "missing", hash, why: String(error) };
      }
    }),
  );
}
