// Service Worker der Ablage-PWA.
// Regeln (Ablage_Format_Spezifikation.txt, 1): i/ nie aus dem Zwischenspeicher; o/ und m/ sind
// unveränderlich (Pfad = SHA-256) und werden dauerhaft gespeichert; die Hülle selbst: Netz zuerst.

const SHELL_CACHE = 'ablage-shell-v8';
const DATA_CACHE = 'ablage-immutable-v1';
// Dateien der Hülle: shell.json (schreibt scripts/build.sh), sonst das Nötigste.
const FALLBACK = ['./', 'index.html', 'style.css', 'pkg/pwa/pwa.js', 'pkg/pwa/pwa_bg.wasm'];
const shellFiles = () =>
  fetch('shell.json', { cache: 'no-store' }).then((r) => (r.ok ? r.json() : FALLBACK)).catch(() => FALLBACK);

self.addEventListener('install', (event) => {
  event.waitUntil(Promise.all([caches.open(SHELL_CACHE), shellFiles()]).then(([c, files]) => c.addAll(files)));
  self.skipWaiting();
});

// Hülle nachfüllen, falls der Zwischenspeicher leer blieb (z. B. Brave): die Seite meldet sich nach dem Start.
self.addEventListener('message', (event) => {
  if (event.data !== 'fuellen') return;
  event.waitUntil(
    caches.open(SHELL_CACHE).then(async (c) => {
      const have = new Set((await c.keys()).map((r) => r.url));
      const missing = (await shellFiles()).filter((p) => !have.has(new URL(p, self.registration.scope).href));
      await Promise.all(missing.map((p) => c.add(p).catch(() => {})));
    })
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== SHELL_CACHE && k !== DATA_CACHE).map((k) => caches.delete(k)))
    ).then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  if (req.method !== 'GET') return;
  const url = new URL(req.url);

  // Unveränderlich: o/<sha256> und m/<sha256>.
  if (/\/(o|m)\/[0-9a-f]{64}$/.test(url.pathname)) {
    event.respondWith(
      caches.open(DATA_CACHE).then(async (cache) => {
        const hit = await cache.match(req);
        if (hit) return hit;
        const res = await fetch(req);
        if (res.ok) await cache.put(req, res.clone());
        return res;
      })
    );
    return;
  }

  // Veränderlich: i/… (Startdatei, Index, Modulliste) – immer aus dem Netz, sonst Browser-Standard.
  if (/\/i\//.test(url.pathname)) return;

  // Hülle und Shell: Netz zuerst, bei Fehler der Zwischenspeicher (offline).
  // Kopie vor der Rückgabe ziehen (danach liest die Seite den Inhalt, ein späteres clone() schlägt fehl).
  event.respondWith(
    fetch(req)
      .then((res) => {
        if (res.ok && url.origin === self.location.origin) {
          const copy = res.clone();
          event.waitUntil(caches.open(SHELL_CACHE).then((c) => c.put(req, copy)));
        }
        return res;
      })
      .catch(() =>
        caches.match(req, { ignoreSearch: true }).then((hit) => hit || (req.mode === 'navigate' ? caches.match('index.html') : undefined))
          .then((hit) => hit || Response.error())
      )
  );
});
