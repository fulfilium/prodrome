// Make the four small stores that `stores.mjs` serves, into OUT.
//
//   node viewer/test/make-stores.mjs PRODROME PRODROME_NODE OUT
//
// PRODROME is the command line's binary (`nix build .#prodrome-cli`), and
// PRODROME_NODE is the `nodejs/` glue of `nix build .#prodrome-wasm`. The
// command line writes both stores. The item `b1` of the store `beta` waits on
// the item `a1` of the store `alpha`: its spec is `RefIn(store=<alpha's
// genesis>, entity='a1')`, which the command line cannot spell, so the core
// in the glue seals that one event into `beta`. The store `gamma` holds an
// item `a1`, as `alpha` does, items whose text is crafted to attack the page
// if it ran that text as Typst, and an issue's title in Typst markup, as the
// issue mirror writes it. The store `delta` has the shape of a store that an
// application writes as content records alone: each item is an `Authored`
// record with its spec, and no `Created`, and the core in the glue seals
// each one, because the command line writes a `Created` for each item. One
// item is revised twice, one waits on another by its `waiting_on` field, one
// is completed, one is cancelled, and one is cancelled and reopened. A
// genesis holds a random nonce, so each run makes stores with new names; each
// store in `viewer/test/stores/` is the output of one run.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createRequire } from "node:module";

const [cli, glue, out] = process.argv.slice(2);
if (!cli || !glue || !out) {
  console.error("usage: make-stores.mjs PRODROME PRODROME_NODE OUT");
  process.exit(2);
}
const core = createRequire(import.meta.url)(join(glue, "prodrome.js"));
const runAs = (actor, ...args) =>
  execFileSync(cli, args, { env: { ...process.env, PRODROME_ACTOR: actor }, stdio: ["ignore", "pipe", "inherit"] });
const run = (...args) => runAs("ada", ...args);

const objects = (store) =>
  readdirSync(join(store, "objects"))
    .filter((file) => file.endsWith(".py"))
    .map((file) => ({ hash: file.slice(0, -3), text: readFileSync(join(store, "objects", file), "utf8") }));

const genesis = (store) => {
  const heads = JSON.parse(new core.Todos(JSON.stringify(objects(store))).tips()).heads;
  const names = Object.keys(heads);
  if (names.length !== 1) throw new Error(`${store} holds ${names.length} geneses`);
  return names[0];
};

const alpha = join(out, "alpha");
const beta = join(out, "beta");
run("init", alpha);
run("add", "--store", alpha, "--at", "2026-11-01T09:00:00", "--priority", "40", "--body", "Ship the parser", "a1");
run("add", "--store", alpha, "--at", "2026-11-01T09:05:00", "--priority", "80", "--body", "Tidy the parser's errors", "a2");
run("init", beta);
run("add", "--store", beta, "--at", "2026-11-02T10:00:00", "--priority", "90", "--body", "Write the guide to the parser", "b1");
run("add", "--store", beta, "--at", "2026-11-02T10:05:00", "--priority", "60", "--body", "Draw the guide's figures", "b2");

const wait =
  `SpecRevised(todo='b1', at=datetime(2026, 11, 2, 10, 10, 0), actor='ada', ` +
  `spec=RefIn(store='${genesis(alpha)}', entity='a1'), note='waits on the parser')`;
const replica = new core.Todos(JSON.stringify(objects(beta)));
const sealed = JSON.parse(replica.append(wait, genesis(beta)));
for (const { hash, text } of sealed.objects) writeFileSync(join(beta, "objects", `${hash}.py`), text);
// The heads record is derived from the objects, and the write above went
// past it, so it goes; the next program that opens the store derives it.

const gamma = join(out, "gamma");
run("init", gamma);
const crafted = {
  a1: "An item with the id of an item of alpha",
  "g-style": `#html.elem("style", "#notices, #stores { display: none }")`,
  "g-layer": `#html.elem("div", attrs: (style: "position: fixed; inset: 0; background: white"))[#link("https://example.org/")[Continue]]`,
  "g-read": `#link("https://example.org/?" + repr(json("/data.json").stores))[read]`,
  "g-loop": "#while true {}",
  "g-name": `#html.elem("img", attrs: (name: "createElement", id: "createElement"))`,
};
let minute = 0;
for (const [todo, body] of Object.entries(crafted)) {
  const at = `2026-11-03T10:${String(minute++).padStart(2, "0")}:00`;
  run("add", "--store", gamma, "--at", at, "--priority", "50", "--body", body, todo);
}
// An issue's title, as the issue mirror writes it: anyone may write one.
runAs("github-mirror", "add", "--store", gamma, "--at", "2026-11-03T11:00:00", "--priority", "50",
  "--body", `*Mirrored* #html.elem("style", "#stores { display: none }")`, "gh-1");
const delta = join(out, "delta");
run("init", delta);
const stamp = (day, hour) => `datetime(2026, 11, ${day}, ${hour}, 0, 0)`;
const authored = (todo, day, hour, body, value, waitingOn) =>
  `Authored(todo='${todo}', at=${stamp(day, hour)}, actor='ada', kind='todo', created=${stamp(4, 9)}, ` +
  `body="${body}", spec=Flat(value=${value}), rationale=('the first reason', 'the second reason'), ` +
  `category='', waiting_on='${waitingOn}', detail='The detail of ${todo}.', source=None, subtodos=(), notes=(), note='')`;
const lifecycle = (kind, todo, day, hour) => `${kind}(todo='${todo}', at=${stamp(day, hour)}, actor='ada', note='')`;
const records = [
  authored("d-parser", 4, 9, "Write the parser", 0.55, ""),
  authored("d-errors", 4, 10, "Tidy the errors", 0.3, ""),
  authored("d-figures", 4, 11, "Draw the figures", 0.7, "d-parser"),
  authored("d-release", 4, 12, "Ship the release", 0.4, ""),
  authored("d-port", 4, 13, "Port the old tests", 0.6, ""),
  authored("d-format", 4, 14, "Keep the old format", 0.8, ""),
  authored("d-errors", 5, 9, "Tidy the errors and the warnings", 0.3, ""),
  authored("d-errors", 6, 9, "Tidy every message", 0.25, ""),
  lifecycle("Completed", "d-release", 6, 10),
  lifecycle("Cancelled", "d-format", 6, 11),
  lifecycle("Cancelled", "d-port", 6, 12),
  lifecycle("Reopened", "d-port", 6, 13),
];
const writer = new core.Todos(JSON.stringify(objects(delta)));
for (const record of records) {
  for (const { hash, text } of JSON.parse(writer.append(record, genesis(delta))).objects) {
    writeFileSync(join(delta, "objects", `${hash}.py`), text);
  }
}
writer.free();
for (const store of [alpha, beta, gamma, delta]) {
  rmSync(join(store, ".heads"), { force: true });
  run("verify", "--store", store);
}
// The test reads the geneses to know how the core breaks a tie.
const geneses = { alpha: genesis(alpha), beta: genesis(beta), gamma: genesis(gamma), delta: genesis(delta) };
writeFileSync(join(out, "geneses.json"), `${JSON.stringify(geneses, null, 2)}\n`);
console.log(JSON.stringify(geneses));
