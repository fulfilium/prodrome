// Make the three small stores that `stores.mjs` serves, into OUT.
//
//   node viewer/test/make-stores.mjs PRODROME PRODROME_NODE OUT
//
// PRODROME is the command line's binary (`nix build .#prodrome-cli`), and
// PRODROME_NODE is the `nodejs/` glue of `nix build .#prodrome-wasm`. The
// command line writes both stores. The item `b1` of the store `beta` waits on
// the item `a1` of the store `alpha`: its spec is `RefIn(store=<alpha's
// genesis>, entity='a1')`, which the command line cannot spell, so the core
// in the glue seals that one event into `beta`. The store `gamma` holds an
// item `a1`, as `alpha` does, and items whose text is crafted to attack the
// page if it ran that text as Typst. A genesis holds a random
// nonce, so each run makes stores with new names; the committed copy in
// `viewer/test/stores/` is one run.
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
const env = { ...process.env, PRODROME_ACTOR: "ada" };
const run = (...args) => execFileSync(cli, args, { env, stdio: ["ignore", "pipe", "inherit"] });

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
for (const store of [alpha, beta, gamma]) {
  rmSync(join(store, ".heads"), { force: true });
  run("verify", "--store", store);
}
// The test reads the geneses to know how the core breaks a tie.
const geneses = { alpha: genesis(alpha), beta: genesis(beta), gamma: genesis(gamma) };
writeFileSync(join(out, "geneses.json"), `${JSON.stringify(geneses, null, 2)}\n`);
console.log(JSON.stringify(geneses));
