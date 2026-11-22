// The documents the viewer typesets, and the one door into typst-wasm.
//
// Each page is a two-line Typst document that imports `prodrome-typst` and
// hands it the data; the package path's files (`prodrome-typst` and the
// design system it imports) are bundled into the app, and the data is passed
// as a virtual file, so a document compiled here is the same document
// `typst compile` reads from `typst/examples/`.

import { PACKAGES } from "typst-packages";
import type { Page } from "./stores";

/** The part of typst-wasm this reads — the glue module satisfies it. */
export interface Typst {
  compile_html(source: string, files_json: string): string;
  highlight(source: string): string;
  complete(source: string, cursor: number, explicit?: boolean | null): string;
  hover(source: string, cursor: number): string;
  add_font(data: Uint8Array): number;
}

export interface Diagnostic {
  severity: "error" | "warning";
  message: string;
  hints: string[];
  file: string | null;
  from: number | null;
  to: number | null;
}

export interface Compiled {
  html: string | null;
  diagnostics: Diagnostic[];
}

const PACKAGE = "@local/prodrome-typst:0.1.0";

export const IMPORT = `#import "${PACKAGE}": *`;

/** The packages' files and the data, keyed the way typst-wasm finds them. */
export function files(view: Page): Record<string, string> {
  return { ...PACKAGES, "/data.json": JSON.stringify(view) };
}

/**
 * The list. Its title, and the stores' part of every link (`query`), come
 * from the data, so no text of a link is spliced into code.
 */
export function listDocument(markup: boolean): string {
  return [
    IMPORT,
    `#let data = json("/data.json")`,
    `#roadmap(data, title: data.title, href: key => "#/todo/" + key + data.query, markup: ${markup})`,
    ``,
  ].join("\n");
}

/** The item named by the data's `focus`, so an id is never spliced into code. */
export function itemDocument(markup: boolean): string {
  return [
    IMPORT,
    `#let data = json("/data.json")`,
    `#item(data, data.focus, markup: ${markup}, objects: data.objects, back: "#/" + data.query)`,
    ``,
  ].join("\n");
}

/** The editor's starting text: a small document over the same data. */
export function scratchDocument(): string {
  return [
    IMPORT,
    `#let data = json("/data.json")`,
    `#let e = data.entries.find(e => e.key == data.focus)`,
    `// A scratch pad over this item's data. Nothing here is saved.`,
    ``,
    `= Notes on #raw(e.todo)`,
    ``,
    `Worth #price(e.value) now, which is *#state-of(e.value)*.`,
    ``,
    `#trace-of(data, e.key)`,
    ``,
  ].join("\n");
}

export function compile(typst: Typst, source: string, view: Page): Compiled {
  return JSON.parse(typst.compile_html(source, JSON.stringify(files(view)))) as Compiled;
}

/**
 * How a page reads its items' text: as Typst markup, which only this
 * site's own store may have, or as written. The text of a store that a
 * link names is never run: Typst markup is code, and it can read every
 * loaded store's data and spell any element.
 */
export type Texts = { kind: "markup" } | { kind: "verbatim" };

/** How a page's text was read in the end; see `compilePage`. */
export type TextRead = "markup" | "invalid" | "foreign";

/**
 * Compile a page. With `markup`, the items' text is read as Typst, and
 * again verbatim if that fails: nothing promises a store's text is Typst,
 * and one item's stray `#` should cost that reading, not the page.
 */
export function compilePage(
  typst: Typst,
  document: (markup: boolean) => string,
  view: Page,
  texts: Texts,
): Compiled & { read: TextRead } {
  if (texts.kind === "verbatim") return { ...compile(typst, document(false), view), read: "foreign" };
  const typeset = compile(typst, document(true), view);
  if (typeset.html !== null) return { ...typeset, read: "markup" };
  return { ...compile(typst, document(false), view), read: "invalid" };
}

/**
 * Elements a typeset page never keeps: scripts, frames, plugins, forms and
 * their controls, styles, media, and SVG's animations and foreign objects,
 * which can set an attribute or hold HTML.
 */
const DROPPED = new Set(
  [
    "script", "iframe", "frame", "frameset", "object", "embed", "portal", "link", "meta", "base",
    "form", "button", "input", "select", "textarea", "dialog", "style", "template",
    "audio", "video", "source", "track",
    "foreignobject", "animate", "animatemotion", "animatetransform", "set", "discard",
  ],
);

/** Attributes a typeset page never keeps, besides the handlers `on…`. */
const STRIPPED = new Set([
  "name", "ping", "formaction", "srcset", "srcdoc", "form", "target", "attributename",
  "poster", "background", "data", "popover", "popovertarget",
]);

const SVG = "http://www.w3.org/2000/svg";
/** The one `style` Typst writes: an inline SVG's root, sized in em. */
const FRAME_STYLE = /^overflow: visible; width: [0-9.]+em; height: [0-9.]+em;( display: block)?$/;
/** The ids Typst writes: a glyph's or a gradient's, a letter and hex digits. */
const TYPST_ID = /^[a-z][0-9A-F]{16,}$/;

/** Whether an element keeps an attribute: Typst's own `style` and `id` in SVG, and safe links. */
function kept(el: Element, name: string, raw: string): boolean {
  const value = raw.trim().toLowerCase();
  if (name.startsWith("on") || STRIPPED.has(name)) return false;
  const svg = el.namespaceURI === SVG;
  if (name === "style") return svg && el.localName === "svg" && FRAME_STYLE.test(raw.trim());
  if (name === "id") return svg && TYPST_ID.test(raw);
  if (svg && (name === "href" || name === "xlink:href")) return value.startsWith("#") || el.localName === "a";
  const url = name === "href" || name === "src" || name === "xlink:href" || name === "action";
  return !url || /^(#|https?:|objects\/|[a-z0-9._-]+(\/|$))/.test(value);
}

/**
 * The body of a compiled document, as nodes safe to put in the page. Typst
 * escapes text, but `html.elem` can spell any element where an item's text
 * is typeset as markup, which the page allows only for this site's own
 * store. So every page drops scripts, frames, forms, styles and animations,
 * every handler, every `style` and `id` but Typst's own on its drawings,
 * every `name`, and every link that does not go to `#`, http(s) or this
 * site. The page's Content Security Policy forbids inline script as well.
 */
export function body(html: string): DocumentFragment {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_ELEMENT);
  const doomed: Element[] = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const el = node as Element;
    if (DROPPED.has(el.localName.toLowerCase())) {
      doomed.push(el);
      continue;
    }
    for (const attr of Array.from(el.attributes)) {
      if (!kept(el, attr.name.toLowerCase(), attr.value)) el.removeAttribute(attr.name);
    }
  }
  for (const el of doomed) el.remove();
  const out = document.createDocumentFragment();
  out.append(...Array.from(doc.body.childNodes).map((n) => document.importNode(n, true)));
  return out;
}
