const CACHE_NAME = "xavier-legal-v1";
const STATIC_ASSETS = [
  "/",
  "/index.html",
  "/manifest.webmanifest",
  "/favicon.ico",
  "/icon-128.png",
  "/icon-256.png",
  "/icon-512.png",
];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => {
      return cache.addAll(STATIC_ASSETS);
    })
  );
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((keys) => {
      return Promise.all(
        keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key))
      );
    })
  );
  self.clients.claim();
});

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);

  // For same-origin static assets, serve from cache with stale-while-revalidate
  if (url.origin === self.location.origin && !url.pathname.startsWith("/panel/api") && !url.pathname.startsWith("/health") && !url.pathname.startsWith("/v1") && !url.pathname.startsWith("/auth") && !url.pathname.startsWith("/api")) {
    event.respondWith(
      caches.match(event.request).then((cached) => {
        const fetchPromise = fetch(event.request).then((networkResponse) => {
          if (networkResponse && networkResponse.status === 200) {
            const clone = networkResponse.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
          }
          return networkResponse;
        }).catch(() => cached);

        return cached || fetchPromise;
      })
    );
    return;
  }

  // API calls: Network first
  event.respondWith(
    fetch(event.request).catch(() => {
      return new Response(
        JSON.stringify({ offline: true, message: "Operación en cola local. Se sincronizará al conectar." }),
        { headers: { "Content-Type": "application/json" } }
      );
    })
  );
});

// Background sync handler
self.addEventListener("sync", (event) => {
  if (event.tag === "xavier-sync-outbox") {
    event.waitUntil(
      self.clients.matchAll().then((clients) => {
        clients.forEach((client) => {
          client.postMessage({ type: "SYNC_OUTBOX_TRIGGER" });
        });
      })
    );
  }
});
