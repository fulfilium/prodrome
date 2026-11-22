// What the sanitiser keeps of typeset HTML (viewer/src/sanitise.ts), case by
// case, in a browser: it needs the DOM's parser. A build for tests
// (`VIEWER_LOOPBACK=1 viewer/build.sh`) bundles this file as
// `sanitise-test.js`, and `stores.mjs` runs it in Chromium. It leaves the
// failed cases in `window.sanitised`: none when every case holds.

import { body } from "../src/sanitise";

/** One case: the HTML, and a check of what the page keeps of it. */
interface Case {
  what: string;
  html: string;
  holds: (kept: HTMLElement) => boolean;
}

const SVG = (inner: string) =>
  `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" style="overflow: visible; width: 1em; height: 1em;">${inner}</svg>`;

const CASES: Case[] = [
  { what: "an HTML link to javascript: loses it", html: `<a href="javascript:alert(1)">x</a>`, holds: (k) => !k.querySelector("a")?.hasAttribute("href") },
  { what: "an HTML link to data: loses it", html: `<a href="data:text/html,x">x</a>`, holds: (k) => !k.querySelector("a")?.hasAttribute("href") },
  { what: "an HTML link to https keeps it", html: `<a href="https://example.org/">x</a>`, holds: (k) => k.querySelector("a")?.getAttribute("href") === "https://example.org/" },
  { what: "an SVG link to javascript: loses it", html: SVG(`<a href="javascript:alert(1)"><text>x</text></a>`), holds: (k) => !k.querySelector("a")?.hasAttribute("href") },
  { what: "an SVG link to data: loses it", html: SVG(`<a xlink:href="data:text/html,x"><text>x</text></a>`), holds: (k) => k.querySelector("a")?.attributes.length === 0 },
  { what: "an SVG link to https keeps it", html: SVG(`<a href="https://example.org/"><text>x</text></a>`), holds: (k) => k.querySelector("a")?.getAttribute("href") === "https://example.org/" },
  { what: "an SVG use keeps a link within the page", html: SVG(`<use xlink:href="#g12BE9A54F1E674DDD48"/>`), holds: (k) => k.querySelector("use")?.getAttribute("xlink:href") === "#g12BE9A54F1E674DDD48" },
  { what: "an SVG use loses a link out of the page", html: SVG(`<use href="https://example.org/x.svg#a"/>`), holds: (k) => !k.querySelector("use")?.hasAttribute("href") },
  { what: "a style element goes", html: `<style>#notices{display:none}</style><p>x</p>`, holds: (k) => k.querySelector("style") === null },
  { what: "a style attribute goes", html: `<div style="position:fixed;inset:0">x</div>`, holds: (k) => !k.querySelector("div")?.hasAttribute("style") },
  { what: "Typst's own SVG style stays", html: SVG(""), holds: (k) => k.querySelector("svg")?.getAttribute("style") === "overflow: visible; width: 1em; height: 1em;" },
  { what: "another SVG style goes", html: `<svg xmlns="http://www.w3.org/2000/svg" style="position:fixed"></svg>`, holds: (k) => !k.querySelector("svg")?.hasAttribute("style") },
  { what: "a name and an id that clobber the DOM go", html: `<img name="createElement" id="createElement">`, holds: (k) => k.querySelector("img")?.attributes.length === 0 },
  { what: "a glyph id of Typst's stays", html: SVG(`<symbol id="g12BE9A54F1E674DDD48"></symbol>`), holds: (k) => k.querySelector("symbol")?.id === "g12BE9A54F1E674DDD48" },
  { what: "an animation goes", html: SVG(`<a><animate attributeName="href" to="javascript:alert(1)"/></a>`), holds: (k) => k.querySelector("animate") === null },
  { what: "a foreign object goes", html: SVG(`<foreignObject><p>x</p></foreignObject>`), holds: (k) => k.querySelector("foreignObject, foreignobject") === null },
  { what: "a handler goes", html: `<p onclick="alert(1)">x</p>`, holds: (k) => !k.querySelector("p")?.hasAttribute("onclick") },
  { what: "a form and its button go", html: `<form><button formaction="https://example.org/">x</button></form>`, holds: (k) => k.querySelector("form, button") === null },
  { what: "ping and srcset go", html: `<a href="#a" ping="https://example.org/"><img srcset="https://example.org/a.png 1x"></a>`, holds: (k) => !k.querySelector("a")?.hasAttribute("ping") && !k.querySelector("img")?.hasAttribute("srcset") },
];

const failed: string[] = [];
for (const c of CASES) {
  const kept = document.createElement("div");
  kept.append(body(`<!doctype html><html><body>${c.html}</body></html>`));
  if (!c.holds(kept)) failed.push(`${c.what}: ${kept.innerHTML}`);
}
(window as unknown as { sanitised: { cases: number; failed: string[] } }).sanitised = { cases: CASES.length, failed };
