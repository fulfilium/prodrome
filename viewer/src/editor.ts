// A small Typst editor: a textarea over its own highlighting, completion
// from typst-ide, a hover line, and a live preview. A DEMO — it saves
// nothing, and nothing it does reaches the store.
//
// Every offset typst-wasm answers is in UTF-16 code units, the unit a
// textarea's selection counts in, so nothing here converts.

import { body } from "./sanitise";
import { compile, type Typst } from "./typst";
import type { Page } from "./stores";

interface Highlight {
  from: number;
  to: number;
  tag: string;
}

interface Item {
  label: string;
  kind: unknown;
  insert: string;
  cursor: number;
  detail: string | null;
}

interface Completions {
  from: number;
  items: Item[];
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> = {},
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const el = Object.assign(document.createElement(tag), props);
  el.append(...children);
  return el;
}

/** The source as highlighted nodes: plain text between the tagged spans. */
function shaded(typst: Typst, source: string): Node[] {
  const spans = JSON.parse(typst.highlight(source)) as Highlight[];
  const out: Node[] = [];
  let at = 0;
  for (const { from, to, tag } of spans) {
    if (from > at) out.push(document.createTextNode(source.slice(at, from)));
    out.push(element("span", { className: tag }, source.slice(from, to)));
    at = to;
  }
  // A trailing newline in a <pre> is swallowed; one more keeps the last line.
  out.push(document.createTextNode(source.slice(at) + "\n"));
  return out;
}

/** Line and column (1-based) of a UTF-16 offset, for a diagnostic. */
function position(source: string, offset: number): string {
  const before = source.slice(0, offset).split("\n");
  return `${before.length}:${before[before.length - 1].length + 1}`;
}

const IDENT = /[\p{L}\p{N}_.#-]$/u;

export function editor(typst: Typst, view: Page, initial: string): HTMLElement {
  const input = element("textarea", {
    spellcheck: false,
    value: initial,
    ariaLabel: "Typst source (not saved)",
  });
  input.setAttribute("autocapitalize", "off");
  input.setAttribute("autocomplete", "off");
  const shade = element("pre", { className: "shade", ariaHidden: "true" });
  const menu = element("ul", { className: "completions", hidden: true, role: "listbox" });
  const hoverLine = element("div", { className: "hover" });
  const problems = element("ul", { className: "problems" });
  const preview = element("div", { className: "preview typst" });

  let offered: Completions | null = null;
  let chosen = 0;
  let timer: number | undefined;

  const repaint = () => {
    shade.replaceChildren(...shaded(typst, input.value));
    shade.scrollTop = input.scrollTop;
    shade.scrollLeft = input.scrollLeft;
  };

  const render = () => {
    const source = input.value;
    const answer = compile(typst, source, view);
    if (answer.html !== null) preview.replaceChildren(body(answer.html));
    problems.replaceChildren(
      ...answer.diagnostics
        .filter((d) => d.severity === "error" || d.file === "/main.typ")
        .map((d) => {
          const where = d.file === "/main.typ" && d.from !== null ? `${position(source, d.from)} ` : "";
          const hints = d.hints.map((h) => `\nhint: ${h}`).join("");
          return element("li", { className: d.severity }, `${where}${d.severity}: ${d.message}${hints}`);
        }),
    );
    preview.classList.toggle("stale", answer.html === null);
  };

  const close = () => {
    offered = null;
    menu.hidden = true;
  };

  const showMenu = () => {
    if (!offered || offered.items.length === 0) return close();
    menu.replaceChildren(
      ...offered.items.slice(0, 12).map((item, i) => {
        const li = element(
          "li",
          { role: "option", ariaSelected: String(i === chosen) },
          element("code", {}, item.label),
          item.detail ? element("span", { className: "detail" }, ` ${item.detail}`) : "",
        );
        li.addEventListener("mousedown", (event) => {
          event.preventDefault();
          chosen = i;
          accept();
        });
        return li;
      }),
    );
    menu.hidden = false;
  };

  const offer = (explicit: boolean) => {
    const cursor = input.selectionStart;
    const found = JSON.parse(typst.complete(input.value, cursor, explicit)) as Completions | null;
    offered = found && found.items.length > 0 ? found : null;
    chosen = 0;
    showMenu();
  };

  function accept() {
    if (!offered) return;
    const item = offered.items[chosen];
    const cursor = input.selectionStart;
    input.setRangeText(item.insert, offered.from, cursor, "end");
    const caret = offered.from + item.cursor;
    input.setSelectionRange(caret, caret);
    close();
    changed();
  }

  const describe = () => {
    const found = JSON.parse(typst.hover(input.value, input.selectionStart)) as {
      kind: string;
      text: string;
    } | null;
    hoverLine.textContent = found ? found.text : "";
  };

  function changed() {
    repaint();
    window.clearTimeout(timer);
    timer = window.setTimeout(render, 120);
  }

  input.addEventListener("input", () => {
    changed();
    const before = input.value.slice(0, input.selectionStart);
    if (IDENT.test(before)) offer(false);
    else close();
  });
  input.addEventListener("scroll", repaint);
  input.addEventListener("keyup", (event) => {
    if (!event.key.startsWith("Arrow") || menu.hidden) describe();
  });
  input.addEventListener("click", () => {
    close();
    describe();
  });
  input.addEventListener("blur", close);
  input.addEventListener("keydown", (event) => {
    if (event.key === " " && event.ctrlKey) {
      event.preventDefault();
      offer(true);
      return;
    }
    if (!offered) return;
    const count = Math.min(offered.items.length, 12);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      chosen = (chosen + (event.key === "ArrowDown" ? 1 : count - 1)) % count;
      showMenu();
    } else if (event.key === "Enter" || event.key === "Tab") {
      event.preventDefault();
      accept();
    } else if (event.key === "Escape") {
      close();
    }
  });

  repaint();
  render();
  return element(
    "section",
    { className: "editor" },
    element("h2", {}, "Try Typst on this item"),
    element(
      "p",
      { className: "note" },
      "A live preview over this item's data, with highlighting and completion from typst-wasm (Ctrl+Space). Nothing here is saved.",
    ),
    element("div", { className: "field" }, shade, input),
    menu,
    hoverLine,
    problems,
    preview,
  );
}
