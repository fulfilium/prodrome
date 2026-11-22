// What a store costs the page to find and fetch, two ways, over a simulated
// network: every request waits RTT milliseconds before its answer.
//
//   node viewer/test/measure.mjs PRODROME_NODE STORE [RTT]
//
// PRODROME_NODE is the `nodejs/` glue of `nix build .#prodrome-wasm`, and
// STORE a store's directory. RTT defaults to 80. The requests of one round
// go out together, as a browser's do over HTTP/2.
//
// - ALL: read `index.json` (the sorted names), then fetch every object.
// - WALK: read the heads (the objects' tips), fetch them, and fetch the
//   objects they rest on that the page lacks, round by round, until none is
//   lacking. The core finds what is lacking: `Todos.tips` reports each
//   parent that no object sent is, as left out and "not held".
//
// Each way ends when the core has checked the objects and answered the
// list (`Todos.entries`): the time to the first list, without Typst. The
// bytes count the bodies only.
import { createServer } from "node:http";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createRequire } from "node:module";

const [glue, store, rtt = "80"] = process.argv.slice(2);
if (!glue || !store) {
  console.error("usage: measure.mjs PRODROME_NODE STORE [RTT]");
  process.exit(2);
}
const core = createRequire(import.meta.url)(join(glue, "prodrome.js"));
const delay = Number(rtt);

const dir = join(store, "objects");
const files = new Map(
  readdirSync(dir)
    .filter((f) => f.endsWith(".py"))
    .map((f) => [f.slice(0, -3), readFileSync(join(dir, f))]),
);
const all = [...files].map(([hash, bytes]) => ({ hash, text: bytes.toString("utf8") }));
const tips = JSON.parse(new core.Todos(JSON.stringify(all)).tips()).tips;
const answers = {
  "/index.json": Buffer.from(JSON.stringify([...files.keys()].sort())),
  "/heads.json": Buffer.from(JSON.stringify(tips)),
};

const server = createServer((req, res) => {
  const object = /^\/objects\/([0-9a-f]{64})\.py$/.exec(req.url);
  const body = object ? files.get(object[1]) : answers[req.url];
  setTimeout(() => (body ? res.writeHead(200).end(body) : res.writeHead(404).end()), delay);
});
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const base = `http://127.0.0.1:${server.address().port}`;

let requests = 0;
let bytes = 0;
async function get(path) {
  requests++;
  const response = await fetch(`${base}${path}`);
  const text = await response.text();
  bytes += Buffer.byteLength(text);
  return text;
}
const fetchAll = (names) => Promise.all(names.map(async (hash) => ({ hash, text: await get(`/objects/${hash}.py`) })));

function list(objects) {
  const replica = new core.Todos(JSON.stringify(objects));
  try {
    return JSON.parse(replica.entries(null, "[]")).entries.length;
  } finally {
    replica.free();
  }
}

async function measure(way) {
  requests = 0;
  bytes = 0;
  const start = performance.now();
  let rounds = 1;
  let objects;
  if (way === "all") {
    objects = await fetchAll(JSON.parse(await get("/index.json")));
    rounds = 2;
  } else {
    objects = await fetchAll(JSON.parse(await get("/heads.json")));
    for (;;) {
      rounds++;
      const lacking = JSON.parse(new core.Todos(JSON.stringify(objects)).tips())
        .excluded.filter((left) => left.why === "not held")
        .map((left) => left.name);
      if (lacking.length === 0) break;
      objects = objects.concat(await fetchAll(lacking));
    }
  }
  const rows = list(objects);
  const ms = Math.round(performance.now() - start);
  return { way, objects: objects.length, rows, requests, bytes, rounds, ms };
}

console.log(JSON.stringify({ store, rtt: delay, objects: files.size }));
// Each way runs twice, and the second run is printed: the first pays for
// starting the module and the connections.
for (const way of ["all", "walk"]) {
  await measure(way);
  console.log(JSON.stringify(await measure(way)));
}
server.close();
