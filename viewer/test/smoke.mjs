// A headless smoke test of a built site: the list and one item, in Chromium.
//
//   node viewer/test/smoke.mjs SITE TODO [SHOTS] [LIST]
//
// SITE is `viewer/assemble.sh`'s output. Serves it on a local port under
// /prodrome/ (the path GitHub Pages uses), opens `#/` at a phone's width,
// waits for Typst to have typeset it, checks the list, opens `#/todo/TODO`,
// checks the item page and the editor (highlighting, completion, live
// preview), and fails on any page error or on a page that scrolls sideways.
// LIST, if given, is `prodrome list`'s output on the same store: the list
// must name the same todos in the same order. PRODROME_NODE names the
// `nodejs/` glue of `nix build .#prodrome-wasm`: the test asks that core for
// the site's open items, and the list must count as many (law 87). Needs
// Playwright (`npm i -g playwright`, or NODE_PATH at one); not part of
// `nix flake check`, which has no browser.
import { createServer } from "node:http";
import { readdir, readFile, stat } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { chromium } = require("playwright");

const [site, todo, shots, listed] = process.argv.slice(2);
if (!site || !todo || !process.env.PRODROME_NODE) {
  console.error("usage: PRODROME_NODE=GLUE smoke.mjs SITE TODO [SHOTS] [LIST]");
  process.exit(2);
}
const core = require(join(process.env.PRODROME_NODE, "prodrome.js"));

const TYPES = {
  ".html": "text/html", ".js": "text/javascript", ".css": "text/css",
  ".json": "application/json", ".wasm": "application/wasm", ".otf": "font/otf",
  ".py": "text/plain; charset=utf-8", ".md": "text/plain; charset=utf-8",
};
const server = createServer(async (req, res) => {
  const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
  if (!path.startsWith("/prodrome/")) return res.writeHead(404).end();
  let file = normalize(join(site, path.slice("/prodrome/".length)));
  try {
    if ((await stat(file)).isDirectory()) file = join(file, "index.html");
    res.writeHead(200, { "content-type": TYPES[extname(file)] ?? "application/octet-stream" });
    res.end(await readFile(file));
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const base = `http://127.0.0.1:${server.address().port}/prodrome/`;

const failures = [];
const check = (ok, what) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) failures.push(what);
};

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? undefined,
});
const WIDTH = 420;
const page = await browser.newPage({ viewport: { width: WIDTH, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push(e.message));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

const typeset = () => page.waitForSelector('body[data-state="typeset"]', { timeout: 120_000 });
const sideways = () => page.evaluate(() => document.documentElement.scrollWidth);

let start = Date.now();
await page.goto(`${base}#/`);
await typeset();
const listMs = Date.now() - start;
const rows = await page.locator(".roadmap tbody tr").count();
check(rows > 0, `the list has ${rows} rows (first render ${listMs} ms)`);
check((await page.locator(".roadmap svg").count()) > 0, "the list draws its marks as SVG");
// Each row's own link, and apart from it the links of what a row waits on.
const links = await page.locator(".roadmap a").evaluateAll((as) =>
  as.map((a) => ({ href: a.getAttribute("href"), wait: a.closest(".waits") !== null })),
);
const hrefs = links.filter((l) => !l.wait).map((l) => l.href);
const waits = links.filter((l) => l.wait).map((l) => l.href);
check(hrefs.every((h) => /^#\/todo\/.+/.test(h)), "every item links to #/todo/<id>");
check(waits.every((h) => /^#\/todo\/.+/.test(h)), `every wait links to #/todo/<id> (${waits.length} waits)`);
if (listed) {
  const cli = (await readFile(listed, "utf8")).split("\n").slice(1).filter(Boolean).map((line) => line.split(/\s+/)[1]);
  const shown = hrefs.map((h) => decodeURIComponent(h.slice("#/todo/".length))).slice(0, rows);
  check(JSON.stringify(shown) === JSON.stringify(cli), `the list is in prodrome list's order (${cli.length} rows)`);
}
// THE LIST IS THE CORE'S (law 87): it counts as open exactly the core's
// rows over the site's objects that are open and exist at the instant read.
const objects = await Promise.all(
  (await readdir(join(site, "objects")))
    .filter((file) => file.endsWith(".py"))
    .map(async (file) => ({ hash: file.slice(0, -3), text: await readFile(join(site, "objects", file), "utf8") })),
);
const now = new Date(Date.now() - new Date().getTimezoneOffset() * 60_000).toISOString().slice(0, 19);
const answer = JSON.parse(core.entries(JSON.stringify(objects), now, "[]"));
const open = answer.entries.filter((e) => e.state === "open" && e.begun <= answer.at).length;
const summary = (await page.locator(".typst").first().textContent()).match(/(\d+) open on/)?.[1];
check(rows === open && summary === String(open), `the list counts ${summary} open in ${rows} rows, as the core answers ${open}`);
check((await sideways()) <= WIDTH, `the list does not scroll sideways at ${WIDTH}px`);
const status = await page.locator("#status").textContent();
check(!/ ms|\dT\d/.test(status), `the status line reads as a date, with no timings: ${status}`);
if (shots) await page.screenshot({ path: `${shots}/list.png`, fullPage: true });

start = Date.now();
await page.evaluate(() => (document.body.dataset.state = "loading"));
await page.goto(`${base}#/todo/${encodeURIComponent(todo)}`);
await typeset();
const itemMs = Date.now() - start;
check((await page.locator(".typst h1, .typst h2").first().textContent()).length > 0, `the item page has a heading (${itemMs} ms)`);
check((await page.locator(".marks svg").count()) === 1, "the item page draws its thirty days");
check((await page.locator(".facts .price svg").count()) === 1, "the item page draws its price's pie");
check(!/\d{2}:\d{2}:\d{2}/.test(await page.locator(".typst").first().textContent()), "no instant is shown as raw ISO");
check((await sideways()) <= WIDTH, `the item page does not scroll sideways at ${WIDTH}px`);
check(await page.locator(".editor textarea").isVisible(), "the editor is on the item page");
check((await page.locator(".editor .shade span").count()) > 0, "the editor's source is highlighted");
check((await page.locator(".editor .preview svg").count()) > 0, "the editor's preview typeset the marks");

const area = page.locator(".editor textarea");
await area.click();
await page.keyboard.press("Control+End");
await page.keyboard.type("\n#hea");
await page.waitForSelector(".editor .completions:not([hidden]) li", { timeout: 10_000 });
const offered = await page.locator(".editor .completions li code").allTextContents();
check(offered.includes("heading"), `completion offers heading (${offered.slice(0, 4).join(", ")}, …)`);
await page.keyboard.press("Escape");
await page.keyboard.type("ding[Typed in the smoke test]");
await page.waitForFunction(() => document.querySelector(".editor .preview")?.textContent?.includes("Typed in the smoke test"), null, { timeout: 10_000 });
check(true, "the preview follows the typing");
await page.keyboard.type(" #nope");
await page.waitForSelector(".editor .problems li.error", { timeout: 10_000 });
check(true, `an error is shown: ${await page.locator(".editor .problems li.error").first().textContent()}`);
if (shots) await page.screenshot({ path: `${shots}/item.png`, fullPage: true });

await page.goto(`${base}#/todo/no-such-item`);
await page.evaluate(() => (document.body.dataset.state = "loading"));
await page.reload();
await typeset();
check((await page.locator(".typst").textContent()).includes("No item"), "an unknown id says so");

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.join(" | ") : ""}`);
await browser.close();
server.close();
if (failures.length) {
  console.error(`${failures.length} failed`);
  process.exit(1);
}
