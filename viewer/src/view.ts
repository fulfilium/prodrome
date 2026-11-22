// FROM OBJECTS TO THE DATA A PAGE TYPESETS — one pure function, run in the
// browser against the core compiled to WebAssembly.
//
// Nothing here evaluates FPL. Every number is one the core answered:
// `entries` composes the rows (SPEC §6.7), `fold` gives each todo's function
// and the environment, `link` closes a function over the others, and
// `fulfillment`, `series_knots` and `explain` read it. What this file adds is
// WHICH instants to ask about — the thirty days of the trace, fifteen back
// and fifteen ahead — and the shape `typst/lib.typ` reads. A value is a number or `∅`
// (`null`): a term with no value at an instant, never read as a number.

import { DAY, iso, naive, plus, type Naive } from "./time";

/** The part of prodrome-wasm's `Todos` class this reads: one set of objects, read once. */
export interface Replica {
  tips(): string;
  since(tips: string): string;
  entries(at: string | null | undefined, untrusted: string): string;
  prices(at: string | null | undefined, untrusted: string): string;
  free(): void;
}

/** The part of prodrome-wasm this reads — the glue module satisfies it. */
export interface Core {
  Todos: new (objects: string) => Replica;
  entries(objects: string, at: string | null | undefined, untrusted: string): string;
  fold(objects: string, at: string | null | undefined, untrusted: string): string;
  link_in(term: string, specs: string, stores: string): string;
  fulfillment(term: string, now: string, env: string): number | undefined;
  explain(term: string, now: string, env: string): string;
  series_knots(term: string, from: string, to: string, history: string): string;
}

/** One object as the site serves it: its name and its canonical print. */
export interface ObjectIn {
  hash: string;
  text: string;
}

/** An §6.7 row, as prodrome-wasm's `entries` puts it on the wire. */
export interface Entry {
  todo: string;
  state: string;
  at: string;
  claimed: string;
  /** A number, `"absent"` where the function reads ∅, `null` where it does not link. */
  value: number | "absent" | null;
  unlinked: string | null;
  unconfirmed: boolean;
  conflicts: Record<string, string[]>;
  content: string | null;
  spec: string;
  stream: string[];
}

/**
 * What a todo's function refers to: a todo of its own store (`Ref`), or an
 * entity of the store whose genesis is `store` (`RefIn`, SPEC §7.2).
 */
export type Target = { kind: "home"; todo: string } | { kind: "other"; store: string; entity: string };

/** One store's reading: what a page typesets of it, and what each todo's function refers to. */
export interface Folded {
  view: View;
  targets: Record<string, Target[]>;
}

/** A reading at one instant: a number, or `null` for ∅. */
export type Reading = number | null;

/** One event on a todo's timeline, from `fold`'s `stream`. */
export interface Marker {
  at: string;
  kind: string;
  actor: string;
  hash: string;
}

/**
 * A todo's trace: `values` evenly spaced from `back` days before now to
 * `ahead` days after it, both ends included, one of them at now.
 */
export interface Marks {
  back: number;
  ahead: number;
  values: Reading[];
}

/** What `typst/lib.typ` reads — see typst/README.md. */
export interface View {
  at: string;
  entries: Entry[];
  /** The rows' todo ids in the core's list order (SPEC §6.7). */
  order: string[];
  records: Record<string, Record<string, unknown>>;
  /** Per todo, its first `Created`'s instant and its last one's text. */
  created: Record<string, { at: string; text: string }>;
  marks: Record<string, Marks>;
  explain: Record<string, unknown>;
  history: Record<string, Marker[]>;
  /** The todo a page is about, if it is about one. */
  focus: string | null;
}

export const BACK_DAYS = 15;
export const AHEAD_DAYS = 15;
/** Samples a day: every six hours, so a day's shape shows and now is one of them. */
export const PER_DAY = 4;

interface Fold {
  env: unknown;
  /** Every todo the objects mention, its function or `absent`: what a ref links against. */
  functions: Record<string, unknown>;
  history: unknown;
  stream: Record<string, Marker[]>;
}

interface Knots {
  knots: [string, Reading][];
  exact: boolean;
}

/**
 * A curve read off its knots, straight between them, flat past the ends. No
 * line is drawn to or from ∅: between a knot with no value and any other, the
 * reading is ∅.
 */
function between(knots: [Naive, Reading][], at: Naive): Reading {
  if (knots.length === 0) return null;
  if (at <= knots[0][0]) return knots[0][1];
  for (let i = 1; i < knots.length; i++) {
    const [t1, v1] = knots[i];
    if (at <= t1) {
      const [t0, v0] = knots[i - 1];
      if (t1 === t0) return v1;
      if (v0 === null || v1 === null) return null;
      return v0 + ((v1 - v0) * (at - t0)) / (t1 - t0);
    }
  }
  return knots[knots.length - 1][1];
}

/**
 * The marks for one closed term. The future is asked of `fulfillment`
 * directly, under the environment as of now (nothing later is known); the
 * past is read off `series_knots`, which evaluates each knot against the
 * environment AS OF that knot — a dependency done yesterday was not done a
 * week ago.
 */
function marksOf(core: Core, closed: string, now: Naive, env: string, history: string): Marks {
  const at = (when: Naive): Reading => core.fulfillment(closed, iso(when), env) ?? null;
  const from = plus(now, -BACK_DAYS * DAY);
  const past = JSON.parse(core.series_knots(closed, iso(from), iso(now), history)) as Knots;
  const knots = past.knots.map(([when, v]) => [naive(when), v] as [Naive, Reading]);
  const values: Reading[] = [];
  for (let i = -BACK_DAYS * PER_DAY; i <= AHEAD_DAYS * PER_DAY; i++) {
    const when = plus(now, (i * DAY) / PER_DAY);
    values.push(i < 0 ? between(knots, when) : at(when));
  }
  return { back: BACK_DAYS, ahead: AHEAD_DAYS, values };
}

/**
 * Every reference in a term in the JSON shape of prodrome-wasm, in the
 * order they occur, each once. The walk reads the shape's two reference
 * kinds and nothing else of it.
 */
export function targets(term: unknown): Target[] {
  const found = new Map<string, Target>();
  const walk = (node: unknown) => {
    if (Array.isArray(node)) {
      node.forEach(walk);
      return;
    }
    if (node === null || typeof node !== "object") return;
    const record = node as Record<string, unknown>;
    if (record.kind === "ref" && typeof record.todo === "string") {
      found.set(`home/${record.todo}`, { kind: "home", todo: record.todo });
    } else if (record.kind === "refIn" && typeof record.store === "string" && typeof record.entity === "string") {
      found.set(`other/${record.store}/${record.entity}`, { kind: "other", store: record.store, entity: record.entity });
    }
    Object.values(record).forEach(walk);
  };
  walk(term);
  return [...found.values()];
}

/**
 * Everything a page needs, read at `now`. `focus` names the one todo whose
 * explanation is wanted (the item page); the list asks for none.
 *
 * `stores` is the set of prodromes a qualified reference reads (SPEC §7.2):
 * the `prices` answer of every loaded store's objects, `[{genesis, env,
 * functions}]`. A todo that waits on an entity of another loaded store
 * reads that entity's price, as the core's `entries` over all the stores
 * prices it (law 41); `"[]"` reads every such reference as `∅`.
 *
 * Throws what the core throws: objects that do not hash to their names, or do
 * not form a DAG, have no honest reading (see prodrome-wasm's `Read`). The
 * page hands it only the objects the core kept (stores.ts).
 */
export function read(core: Core, objects: ObjectIn[], now: Naive, focus: string | null, stores: string): Folded {
  const sent = JSON.stringify(objects);
  const at = iso(now);
  const rows = JSON.parse(core.entries(sent, at, "[]")) as {
    at: string;
    entries: Entry[];
    order: string[];
    records: View["records"];
    created: View["created"];
  };
  const fold = JSON.parse(core.fold(sent, at, "[]")) as Fold;
  const specs = JSON.stringify(fold.functions);
  const env = JSON.stringify(fold.env);
  const history = JSON.stringify(fold.history);

  const marks: View["marks"] = {};
  const explain: View["explain"] = {};
  const refers: Folded["targets"] = {};
  for (const [todo, term] of Object.entries(fold.functions)) {
    refers[todo] = targets(term);
    // `link_in` refuses an unknown todo or a loop; that todo then has no
    // marks, and its row already says why it has no value (`unlinked`).
    let closed: string;
    try {
      closed = core.link_in(JSON.stringify(term), specs, stores);
    } catch {
      continue;
    }
    marks[todo] = marksOf(core, closed, now, env, history);
    if (todo === focus) explain[todo] = JSON.parse(core.explain(closed, at, env));
  }

  const view = {
    at: rows.at,
    entries: rows.entries,
    order: rows.order,
    records: rows.records,
    created: rows.created,
    marks,
    explain,
    history: fold.stream,
    focus,
  };
  return { view, targets: refers };
}
