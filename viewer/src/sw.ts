// The service worker: the app shell cached per build, the objects forever.
//
// An object's name is the SHA-256 of its bytes, so `objects/<name>.py` can
// never change: once fetched, from this site or from any store's host, it
// is served from the cache for good, and the core checks it again anyway.
// The shell (page, script, wasm, fonts) lives in a cache named after the
// build, so a new build is a new cache and the old one is dropped. A listing
// (`index.json`, or a git host's tree listing) is the one mutable file of a
// store, so it is asked of the network first and read from the cache only
// offline.

/// <reference lib="webworker" />
export {};
declare const self: ServiceWorkerGlobalScope;
declare const __BUILD__: string;
declare const __SHELL__: string[];

const SHELL = `prodrome-shell-${__BUILD__}`;
const OBJECTS = "prodrome-objects";

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(SHELL)
      .then((cache) => cache.addAll(__SHELL__))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key.startsWith("prodrome-shell-") && key !== SHELL)
            .map((key) => caches.delete(key)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

async function cacheFirst(name: string, request: Request): Promise<Response> {
  const cache = await caches.open(name);
  const hit = await cache.match(request);
  if (hit) return hit;
  const response = await fetch(request);
  if (response.ok) await cache.put(request, response.clone());
  return response;
}

async function networkFirst(request: Request): Promise<Response> {
  const cache = await caches.open(OBJECTS);
  try {
    const response = await fetch(request, { cache: "no-cache" });
    if (response.ok) await cache.put(request, response.clone());
    return response;
  } catch (error) {
    const hit = await cache.match(request);
    if (hit) return hit;
    throw error;
  }
}

/** A store's object, by its name, on any host. */
const OBJECT = /\/objects\/[0-9a-f]{64}\.py$/;
/** A store's listing on any host: its `index.json`, or GitHub's tree listing. */
const LISTING = /(\/index\.json|^\/repos\/[^/]+\/[^/]+\/git\/trees\/.+)$/;

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET") return;
  if (url.origin !== self.location.origin) {
    if (OBJECT.test(url.pathname)) event.respondWith(cacheFirst(OBJECTS, request));
    else if (LISTING.test(url.pathname)) event.respondWith(networkFirst(request));
    return;
  }
  const scope = new URL(self.registration.scope);
  const path = url.pathname.slice(scope.pathname.length);
  if (/^objects\/[0-9a-f]{64}\.py$/.test(path)) {
    event.respondWith(cacheFirst(OBJECTS, request));
  } else if (path === "index.json") {
    event.respondWith(networkFirst(request));
  } else if (request.mode === "navigate") {
    event.respondWith(
      fetch(request).catch(async () => (await caches.match("index.html")) ?? Response.error()),
    );
  } else {
    event.respondWith(cacheFirst(SHELL, request));
  }
});
