// Service Worker der Ablage-PWA.
// Regeln (Ablage_Format_Spezifikation.txt, 1): i/ nie aus dem Zwischenspeicher; o/ und m/ sind
// unveränderlich (Pfad = SHA-256) und werden dauerhaft gespeichert; die Hülle selbst: Netz zuerst.

const SHELL_CACHE = 'ablage-shell-v1';
const DATA_CACHE = 'ablage-immutable-v1';
const SHELL = ['./', 'index.html', 'style.css', 'manifest.webmanifest', 'icon.svg', 'pkg/pwa/pwa.js', 'pkg/pwa/pwa_bg.wasm'];

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(SHELL_CACHE).then((c) => c.addAll(SHELL)));
  self.skipWaiting();
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
        if (res.ok) cache.put(req, res.clone());
        return res;
      })
    );
    return;
  }

  // Veränderlich: i/… (Startdatei, Index, Modulliste) – immer aus dem Netz, sonst Browser-Standard.
  if (/\/i\//.test(url.pathname)) return;

  // Hülle und Shell: Netz zuerst, bei Fehler der Zwischenspeicher (offline).
  event.respondWith(
    fetch(req)
      .then((res) => {
        if (res.ok && url.origin === self.location.origin) {
          caches.open(SHELL_CACHE).then((c) => c.put(req, res.clone()));
        }
        return res;
      })
      .catch(() => caches.match(req).then((hit) => hit || caches.match('index.html')))
  );
});
