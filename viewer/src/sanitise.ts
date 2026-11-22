// The one door from typeset HTML into the page: what of Typst's HTML the
// page keeps.

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

/** Where a link may go: within the page (`#`), to http(s), or to this site's files. */
const SAFE_LINK = /^(#|https?:|objects\/|[a-z0-9._-]+(\/|$))/;

/**
 * Whether an element keeps an attribute: Typst's own `style` and `id` in
 * SVG, and safe links. A link of an HTML or SVG `a` takes `SAFE_LINK`; any
 * other SVG element may only point within the page, as Typst's `use` does.
 */
function kept(el: Element, name: string, raw: string): boolean {
  const value = raw.trim().toLowerCase();
  if (name.startsWith("on") || STRIPPED.has(name)) return false;
  const svg = el.namespaceURI === SVG;
  if (name === "style") return svg && el.localName === "svg" && FRAME_STYLE.test(raw.trim());
  if (name === "id") return svg && TYPST_ID.test(raw);
  const url = name === "href" || name === "src" || name === "xlink:href" || name === "action";
  if (!url) return true;
  if (svg && el.localName !== "a") return value.startsWith("#");
  return SAFE_LINK.test(value);
}

/**
 * The body of a compiled document, as nodes safe to put in the page. Typst
 * escapes text, and the page typesets no item's text as markup
 * (typst.ts); the editor's preview typesets the visitor's own. This is the
 * second lock: it drops scripts, frames, forms, styles and animations, every
 * handler, every `style` and `id` but Typst's own on its drawings, every
 * `name`, and every link that does not go to `#`, http(s) or this site. The
 * page's Content Security Policy forbids inline script as well.
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
