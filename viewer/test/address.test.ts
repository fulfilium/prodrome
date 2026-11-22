// The address grammar's laws, over generated values (docs/design-viewer-stores.md):
//
// 1. Round trip: for every fragment value `f`, `parseFragment(printFragment(f))`
//    reads `f` back, every store an address.
// 2. One form: for every text `t` that parses, the print of what it reads
//    parses to the same value, so a page that rewrites its address never
//    moves it.
// 3. Refusal is a value: the parser never throws, whatever the text.
// 4. The address printed is the address fetched: for every `gh:` text that
//    parses, `.` or `..` in it attacks included, the URL the page fetches is
//    one that a browser does not rewrite, under the owner and repository
//    that the address names.
//
// `viewer/build.sh` bundles this file with esbuild and runs it with node, so
// `nix build .#prodrome-viewer` fails when a law fails. The generator is a
// fixed-seed PRNG, so a failure names its case and repeats.

import {
  baseOf,
  parseAddress,
  parseFragment,
  printAddress,
  printFragment,
  type Address,
  type Fragment,
  type Label,
  type Pin,
  type Route,
  type Where,
  type Within,
} from "../src/address";

/** The number of generated cases for each law. */
const CASES = 2000;

/** mulberry32: a small PRNG with a fixed seed. */
function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const random = prng(0x5eed);
const int = (n: number) => Math.floor(random() * n);
const pick = <T>(items: readonly T[]): T => items[int(items.length)];
const many = <T>(most: number, one: () => T): T[] => Array.from({ length: int(most + 1) }, one);

const LOWER = "abcdefghijklmnopqrstuvwxyz0123456789_-";
const ALNUM = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
/** Characters that the grammar reserves or escapes, beside plain ones and some outside ASCII. */
const ANY = [..."abcXYZ019-._~:$'()*+,;", ..."%&=!?#@/ ", "é", "€", "日", "\u{1F600}"];

const word = (alphabet: string | string[], least: number, most: number) =>
  Array.from({ length: least + int(most - least + 1) }, () => pick([...alphabet])).join("");

const aLabel = (): string => word(LOWER, 1, 32);
const aTodo = (): string => word(LOWER, 1, 24);
const aGenesis = (): string => word("0123456789abcdef", 64, 64);

/** A directory segment: any text but empty, `.`, `..`, or one with `/`. */
function aSegment(): string {
  for (;;) {
    const segment = word(ANY.filter((ch) => ch !== "/"), 1, 12);
    if (segment !== "." && segment !== "..") return segment;
  }
}

function aGithub(): Where {
  const owner = pick([...ALNUM]) + word(ALNUM + "-", 0, 20);
  let repo = word(ALNUM + "._-", 1, 30);
  if (repo === "." || repo === "..") repo = "r";
  return {
    kind: "github",
    owner,
    repo,
    ref: random() < 0.5 ? { kind: "default" } : { kind: "named", name: [aSegment(), ...many(2, aSegment)].join("/") },
    dir: many(3, aSegment),
  };
}

/** A base URL's text, as a person may write it; it parses to a canonical value or is refused. */
function aBaseText(): string {
  const origin = pick([
    "https://example.org",
    "https://EXAMPLE.org:443",
    "https://stores.example.net:8443",
    "http://localhost:8000",
    "http://127.0.0.1:9",
    "http://localhost",
  ]);
  const path = many(4, aSegment)
    .map((segment) => (random() < 0.5 ? encodeURIComponent(segment) : segment.replace(/[%?#]/g, "")))
    .join("/");
  return `${origin}/${path}${random() < 0.5 ? "/" : ""}`;
}

function aWhere(): Where {
  if (random() < 0.5) return aGithub();
  for (;;) {
    const read = parseAddress(aBaseText());
    if (read.kind === "ok") return read.value.where;
  }
}

const aLabelOf = (): Label => (random() < 0.5 ? { kind: "derived" } : { kind: "named", name: aLabel() });
const aPin = (): Pin => (random() < 0.7 ? { kind: "any" } : { kind: "genesis", genesis: aGenesis() });
const anAddress = (): Address => ({ label: aLabelOf(), where: aWhere(), pin: aPin() });
const aWithin = (): Within => (random() < 0.5 ? { kind: "every" } : { kind: "store", label: aLabel() });
const aRoute = (): Route =>
  random() < 0.5 ? { page: "list", within: aWithin() } : { page: "item", within: aWithin(), todo: aTodo() };
const aFragment = (): Fragment => ({ route: aRoute(), stores: many(8, anAddress) });

let failures = 0;
function check(holds: boolean, law: string, input: unknown) {
  if (holds) return;
  failures++;
  if (failures <= 5) console.error(`FAIL ${law}: ${JSON.stringify(input)}`);
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

for (let n = 0; n < CASES; n++) {
  const fragment = aFragment();
  const printed = printFragment(fragment);
  const read = parseFragment(printed);
  const stores = read.stores.flatMap((s) => (s.kind === "address" ? [s.address] : []));
  check(
    read.stores.length === fragment.stores.length && same({ route: read.route, stores }, fragment),
    "a fragment round-trips",
    { fragment, printed, read },
  );
}

for (let n = 0; n < CASES; n++) {
  const text = random() < 0.5 ? aBaseText() : printAddress(anAddress());
  const first = parseAddress(text);
  if (first.kind !== "ok") continue;
  const again = parseAddress(printAddress(first.value));
  check(again.kind === "ok" && same(again.value, first.value), "a parsed address has one form", { text, first, again });
}

/** A `gh:` text whose ref and directory mix plain parts with `.`, `..` and escapes of them. */
function aWalk(): string {
  const part = () => pick(["..", ".", "%2E%2E", "%2e", "x", "main", "%2F", "..%2F..", "a%2F..%2F..%2Fb"]);
  const ref = random() < 0.8 ? `@${Array.from({ length: 1 + int(4) }, part).join(pick(["%2F", "/"]))}` : "";
  return `gh:ada/stores${ref}${many(3, part).map((p) => `/${p}`).join("")}`;
}

for (let n = 0; n < CASES; n++) {
  const text = random() < 0.5 ? aWalk() : printAddress({ label: { kind: "derived" }, where: aGithub(), pin: aPin() });
  const read = parseAddress(text);
  if (read.kind !== "ok" || read.value.where.kind !== "github") continue;
  const where = read.value.where;
  const base = baseOf(where);
  const url = new URL(base);
  const prefix = `/${encodeURIComponent(where.owner)}/${encodeURIComponent(where.repo)}/`;
  check(url.href === base && url.pathname.startsWith(prefix), "the address printed is the address fetched", { text, base });
}

for (let n = 0; n < CASES; n++) {
  const text = `#${word([...ANY, "/", "?", "&", "="], 0, 80)}`;
  let threw = false;
  try {
    parseFragment(text);
  } catch {
    threw = true;
  }
  check(!threw, "the parser never throws", text);
}

if (failures > 0) throw new Error(`address laws: ${failures} cases failed`);
console.log(`address laws: ${4 * CASES} cases hold`);
