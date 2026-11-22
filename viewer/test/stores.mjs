// A headless test of a page of several stores, in Chromium.
//
//   node viewer/test/stores.mjs APP STORES [SHOTS]
//
// APP is `nix build .#prodrome-viewer`'s output, and STORES holds the stores
// `alpha` and `beta` that `make-stores.mjs` makes (`viewer/test/stores/`).
// The test serves the app on one local origin and the stores on another,
// which sends `Access-Control-Allow-Origin: *` as GitHub's raw host does. The
// store host writes each store's `index.json` from its `objects/`, and serves
// four altered copies of `alpha`: a copy with the objects of `a2` left out, a
// copy whose `Created` of `a2` has one byte changed, a copy whose listing names
// an object it does not serve, and the same store under a second address.
// SHOTS, if given, is a directory for pictures of the list and of an item.
// Needs Playwright, as smoke.mjs does; not part of `nix flake check`.
import { createServer } from "node:http";
import { readdir, readFile, stat } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");

const [app, stores, shots] = process.argv.slice(2);
if (!app || !stores) {
  console.error("usage: stores.mjs APP STORES");
  process.exit(2);
}

const TYPES = {
  ".html": "text/html", ".js": "text/javascript", ".css": "text/css",
  ".json": "application/json", ".wasm": "application/wasm", ".otf": "font/otf",
  ".py": "text/plain; charset=utf-8", ".md": "text/plain; charset=utf-8",
};

async function objectsOf(store) {
  const dir = join(stores, store, "objects");
  const names = (await readdir(dir)).filter((f) => f.endsWith(".py")).map((f) => f.slice(0, -3)).sort();
  return Promise.all(names.map(async (hash) => ({ hash, text: await readFile(join(dir, `${hash}.py`), "utf8") })));
}

const alpha = await objectsOf("alpha");
const beta = await objectsOf("beta");
const ofA2 = alpha.filter((o) => o.text.includes("todo='a2'"));
const createdA2 = ofA2.find((o) => o.text.includes("Created("));
const absent = "0".repeat(64);

/** Each path the store host serves: its objects, and what its listing names. */
const served = {
  alpha: { objects: alpha, listed: alpha.map((o) => o.hash) },
  beta: { objects: beta, listed: beta.map((o) => o.hash) },
  "alpha-copy": { objects: alpha, listed: alpha.map((o) => o.hash) },
  "alpha-part": { objects: alpha, listed: alpha.filter((o) => !ofA2.includes(o)).map((o) => o.hash) },
  "alpha-bad": {
    objects: alpha.map((o) => (o === createdA2 ? { ...o, text: o.text.replace("a2", "a3") } : o)),
    listed: alpha.map((o) => o.hash),
  },
  "alpha-gap": { objects: alpha, listed: [...alpha.map((o) => o.hash), absent] },
};

const requests = { app: [], stores: [] };
const storeServer = createServer((req, res) => {
  requests.stores.push(req.url);
  const headers = { "access-control-allow-origin": "*" };
  const [, store, ...rest] = decodeURIComponent(new URL(req.url, "http://x").pathname).split("/");
  const at = served[store];
  if (at === undefined) return res.writeHead(404, headers).end();
  const path = rest.join("/");
  if (path === "index.json") {
    return res.writeHead(200, { ...headers, "content-type": TYPES[".json"] }).end(JSON.stringify(at.listed));
  }
  const object = /^objects\/([0-9a-f]{64})\.py$/.exec(path);
  const found = object && at.objects.find((o) => o.hash === object[1]);
  if (!found) return res.writeHead(404, headers).end();
  res.writeHead(200, { ...headers, "content-type": TYPES[".py"] }).end(found.text);
});

const appServer = createServer(async (req, res) => {
  requests.app.push(req.url);
  const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
  let file = normalize(join(app, path));
  try {
    if ((await stat(file)).isDirectory()) file = join(file, "index.html");
    res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
    res.end(await readFile(file));
  } catch {
    res.writeHead(404).end();
  }
});

await new Promise((ok) => storeServer.listen(0, "127.0.0.1", ok));
await new Promise((ok) => appServer.listen(0, "127.0.0.1", ok));
const host = `http://127.0.0.1:${storeServer.address().port}`;
const base = `http://127.0.0.1:${appServer.address().port}/`;
const at = (store) => `${host}/${store}/`;

const failures = [];
const check = (ok, what) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) failures.push(what);
};

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM ?? undefined });
const WIDTH = 420;
const page = await browser.newPage({ viewport: { width: WIDTH, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
// The browser logs the one 404 that case 5 asks for; every other error fails.
const asked = `/alpha-gap/objects/${"0".repeat(64)}.py`;
page.on("console", (m) => m.type() === "error" && !m.location().url.endsWith(asked) && errors.push(m.text()));

/** Open a fragment in a fresh load, and wait for Typst. */
async function open(fragment) {
  await page.goto("about:blank");
  await page.goto(`${base}${fragment}`);
  await page.waitForSelector('body[data-state="typeset"]', { timeout: 120_000 });
}

/** Each row of the list: its own link's text and href, its price, and its waits. */
const rows = () =>
  page.locator(".roadmap tbody tr").evaluateAll((trs) =>
    trs.map((tr) => {
      const own = [...tr.querySelectorAll("a")].find((a) => a.closest(".waits") === null);
      return {
        key: own?.textContent ?? "",
        href: own?.getAttribute("href") ?? "",
        price: tr.querySelector(".price")?.textContent?.trim() ?? "",
        waits: tr.querySelector(".waits")?.textContent?.replace(/\s+/g, " ").trim() ?? "",
        waitHref: tr.querySelector(".waits a")?.getAttribute("href") ?? null,
      };
    }),
  );
const notices = () => page.locator("#notices").evaluate((el) => (el.hidden ? "" : el.textContent.replace(/\s+/g, " ")));

// 1. Two stores, a wait from one to the other: one list, linked.
const two = `?alpha=${at("alpha")}&beta=${at("beta")}`;
let start = Date.now();
await open(`#/${two}`);
let list = await rows();
check(
  JSON.stringify(list.map((r) => r.key)) === JSON.stringify(["beta/b1", "alpha/a1", "beta/b2", "alpha/a2"]),
  `one list of both stores in the core's order (${list.map((r) => `${r.key} ${r.price}`).join(", ")}; ${Date.now() - start} ms)`,
);
const b1 = list.find((r) => r.key === "beta/b1");
check(b1?.price.includes("40%"), `beta/b1 is priced by alpha/a1, which it waits on (${b1?.price})`);
check(b1?.waits === "waits on alpha/a1", `beta/b1 says it waits on alpha/a1 (${b1?.waits})`);
check(b1?.waitHref === `#/todo/alpha/a1${two}`, "the wait links to alpha/a1, and the link keeps the stores");
check(list.every((r) => r.href.endsWith(two)), "every row's link keeps the stores");
check((await notices()) === "", "nothing is left out of either store");
check((await page.locator("#stores a").allTextContents()).join(",") === "all,alpha,beta", "the page links each store's own list");
check((await page.evaluate(() => document.documentElement.scrollWidth)) <= WIDTH, `the list does not scroll sideways at ${WIDTH}px`);
if (shots) await page.screenshot({ path: `${shots}/stores-list.png`, fullPage: true });

await page.goto(`${base}#/in/beta${two}`);
await page.waitForFunction(() => document.querySelectorAll(".roadmap tbody tr").length === 2, null, { timeout: 60_000 });
check(
  JSON.stringify((await rows()).map((r) => r.key)) === JSON.stringify(["beta/b1", "beta/b2"]),
  "the list of one store keeps that store's rows",
);

await page.goto(`${base}#/todo/alpha/a1${two}`);
await page.waitForFunction(() => document.title.startsWith("alpha/a1"), null, { timeout: 60_000 });
await page.waitForSelector('body[data-state="typeset"]');
const waitedBy = await page.locator(".typst .waits").allTextContents();
check(waitedBy.some((t) => t.replace(/\s+/g, " ").includes("Waited on by beta/b1")), `alpha/a1's page names beta/b1 (${waitedBy.join(" | ")})`);
check((await page.locator(".marks svg").count()) === 1, "alpha/a1's page draws its thirty days");
if (shots) await page.screenshot({ path: `${shots}/stores-item.png`, fullPage: true });
const objectLinks = await page.locator(".history a").evaluateAll((as) => as.map((a) => a.getAttribute("href")));
check(objectLinks.length > 0 && objectLinks.every((h) => h.startsWith(`${at("alpha")}objects/`)), "the history links objects on alpha's own host");

// 2. The same wait with the other store not loaded.
await open(`#/?beta=${at("beta")}`);
list = await rows();
const alone = list.find((r) => r.key === "b1");
check(alone?.price.includes("unpriced") || alone?.price === "", `b1 alone reads ∅ (${alone?.price})`);
check(/^waits on [0-9a-f]{12}…\/a1, in a store not loaded$/.test(alone?.waits ?? ""), `b1 alone says its store is not loaded (${alone?.waits})`);

// 3. Two addresses of one store, one a partial copy: one store, the union of both.
await open(`#/?${at("alpha-part")}&${at("alpha-copy")}`);
list = await rows();
check(JSON.stringify(list.map((r) => r.key)) === JSON.stringify(["a1", "a2"]), `two addresses of one store are one store, holding both copies' objects (${list.map((r) => r.key)})`);
check(await page.locator("#stores").isHidden(), "one store needs no list of stores");

// 4. One object's bytes changed: refused, the rest shown, the page says which.
await open(`#/?${at("alpha-bad")}`);
list = await rows();
check(JSON.stringify(list.map((r) => r.key)) === JSON.stringify(["a1"]), `a1 is shown and a2, which rests on the changed object, is not (${list.map((r) => r.key)})`);
let said = await notices();
check(said.includes(createdA2.hash.slice(0, 12)) && said.includes("does not hash to its own name"), `the page names the changed object: ${said}`);

// 5. A listing that names an object the host does not serve.
await open(`#/?${at("alpha-gap")}`);
said = await notices();
check(said.includes(`${absent.slice(0, 12)}, which the host does not serve`), `the page names the object the host does not serve: ${said}`);
check((await rows()).length === 2, "the store's items are shown");
if (shots) await page.screenshot({ path: `${shots}/stores-gap.png`, fullPage: true });

// 6. A link that pins a genesis the store does not hold.
await open(`#/?alpha=${at("alpha")}!${"e".repeat(64)}&beta=${at("beta")}`);
said = await notices();
check(said.includes("the link expects the genesis eeeeeeeeeeee"), `a pinned genesis that is not there refuses the store: ${said}`);
check(JSON.stringify((await rows()).map((r) => r.key)) === JSON.stringify(["b2", "b1"]), "the other store is shown");

// 7. An address that is not one.
await open(`#/?ftp://example.org/s/&beta=${at("beta")}`);
check((await notices()).includes("not a store's address"), "an address that is not one is refused, by its text");

// THE ADDRESSES REACH NO HOST BUT THE STORES': the app's host is asked
// only for the app's files, and no request carries a store's address.
const leaked = requests.app.filter((url) => /alpha|beta|127\.0\.0\.1/.test(url));
check(leaked.length === 0, `the viewer's host never sees a store's address (${requests.app.length} requests)`);
check(requests.stores.every((url) => /^\/[a-z-]+\/(index\.json|objects\/[0-9a-f]{64}\.py)$/.test(url)), "the store host is asked only for listings and objects");

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.join(" | ") : ""}`);
await browser.close();
appServer.close();
storeServer.close();
if (failures.length) {
  console.error(`${failures.length} failed`);
  process.exit(1);
}
