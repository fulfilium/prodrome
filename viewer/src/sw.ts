// The service worker: the app shell cached per build, the objects forever.
//
// An object's name is the SHA-256 of its bytes, so `objects/<name>.py` can
// never change: once fetched from this site, it is served from the cache
// for good, and the core checks it again anyway. An object from another
// host is served from a cache that only the page fills, with the objects
// the core has checked, up to a bound (main.ts); this worker keeps nothing
// from another host itself. The shell (page, script, wasm, fonts) lives in
// a cache named after the build, so a new build is a new cache and the old
// one is dropped. This site's `index.json` is the one mutable file of its
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

/** A store's object, by its name, on another host. */
const OBJECT = /\/objects\/[0-9a-f]{64}\.py$/;
/** The objects from other hosts that the page checked and kept (main.ts). */
const CHECKED = "prodrome-checked";

async function checked(request: Request): Promise<Response> {
  const hit = await (await caches.open(CHECKED)).match(request);
  return hit ?? fetch(request);
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET") return;
  // From another host, an object is served from the cache only once the
  // page put it there, after the core checked it; anything else goes to the
  // network and is not kept.
  if (url.origin !== self.location.origin) {
    if (OBJECT.test(url.pathname)) event.respondWith(checked(request));
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
