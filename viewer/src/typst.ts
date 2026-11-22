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
 *
 * AN ITEM'S TEXT IS SHOWN AS WRITTEN, never read as Typst (`markup:
 * false`). Typst markup is code: it can spell any element and loop for
 * ever. A store's text comes from whoever can write to the store, and the
 * issue mirror writes an issue's title, which anyone may write. No store
 * can say yet which writers may write markup, so the page runs no item's
 * text. The package's `markup` option stays for a host that can say.
 */
export function listDocument(): string {
  return [
    IMPORT,
    `#let data = json("/data.json")`,
    `#roadmap(data, title: data.title, href: key => "#/todo/" + key + data.query, markup: false)`,
    ``,
  ].join("\n");
}

/** The item named by the data's `focus`, so an id is never spliced into code; its text as written. */
export function itemDocument(): string {
  return [
    IMPORT,
    `#let data = json("/data.json")`,
    `#item(data, data.focus, markup: false, objects: data.objects, back: "#/" + data.query)`,
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
