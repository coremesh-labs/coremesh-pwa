// Service Worker der Ablage-PWA.
// Regeln (Ablage_Format_Spezifikation.txt, 1): i/ nie aus dem Zwischenspeicher; o/ und m/ sind
// unveränderlich (Pfad = SHA-256) und werden dauerhaft gespeichert; die Hülle selbst: Netz zuerst.

const SHELL_CACHE = 'ablage-shell-v11';
const WATCH_CACHE = 'ablage-melden-v1'; // beobachtete Indexdateien (melden.js)
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
  if (event.data && event.data.art === 'cm1-beobachten') {
    event.waitUntil(beobachten(event.data.liste || []));
    return;
  }
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
      Promise.all(keys.filter((k) => k !== SHELL_CACHE && k !== DATA_CACHE && k !== WATCH_CACHE).map((k) => caches.delete(k)))
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

// --- Melden (melden.js): Neues im Hintergrund erkennen, Benachrichtigung antippen -----------------

const WATCH_KEY = 'beobachtet.json'; // [{url, name, modul, sum}]

async function pruefsumme(url) {
  const res = await fetch(url, { cache: 'no-store' });
  if (!res.ok) return '';
  const h = await crypto.subtle.digest('SHA-256', await res.arrayBuffer());
  return [...new Uint8Array(h)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function liesListe() {
  const c = await caches.open(WATCH_CACHE);
  const r = await c.match(WATCH_KEY);
  return r ? r.json() : [];
}

async function schreibListe(l) {
  const c = await caches.open(WATCH_CACHE);
  await c.put(WATCH_KEY, new Response(JSON.stringify(l), { headers: { 'Content-Type': 'application/json' } }));
}

// Die App hat den jetzigen Stand gesehen: Prüfsummen als Ausgangspunkt merken.
async function beobachten(liste) {
  const neu = [];
  for (const e of liste) {
    try { neu.push({ ...e, sum: await pruefsumme(e.url) }); } catch { neu.push({ ...e, sum: '' }); }
  }
  await schreibListe(neu);
}

async function pruefen() {
  const l = await liesListe();
  const geaendert = [];
  for (const e of l) {
    let sum = '';
    try { sum = await pruefsumme(e.url); } catch { continue; }
    if (sum && e.sum && sum !== e.sum) geaendert.push(e);
    if (sum) e.sum = sum;
  }
  await schreibListe(l);
  if (!geaendert.length) return;
  const e = geaendert[0];
  await self.registration.showNotification('Neues: ' + e.name, {
    body: 'Tippen, um es in der App anzusehen.', tag: 'cm1-' + e.modul, renotify: true, icon: 'icon.svg', badge: 'icon.svg',
    vibrate: [200, 100, 200], data: { modul: e.modul },
  });
}

self.addEventListener('periodicsync', (event) => {
  if (event.tag === 'cm1-pruefen') event.waitUntil(pruefen());
});

// Web Push vom Server (Meldung eines Fachmoduls, z. B. neuer Aushang): {titel, text, modul, id}.
// Der Inhalt ist für dieses Gerät verschlüsselt; der Browser entschlüsselt ihn vor dem Ereignis.
self.addEventListener('push', (event) => {
  let m = {};
  try { m = event.data ? event.data.json() : {}; } catch { m = { titel: event.data ? event.data.text() : '' }; }
  const modul = m.modul || '';
  event.waitUntil(
    self.registration.showNotification(m.titel || 'Neues in der App', {
      body: m.text || 'Tippen, um es in der App anzusehen.', tag: 'cm1-' + modul + (m.id ? '-' + m.id : ''), renotify: true,
      icon: 'icon.svg', badge: 'icon.svg', vibrate: [200, 100, 200], data: { modul },
    })
  );
});

// Antippen: App nach vorn holen bzw. öffnen und das Modul zeigen (#m=<modul>).
self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const modul = (event.notification.data && event.notification.data.modul) || '';
  const ziel = new URL('./' + (modul ? '#m=' + modul : ''), self.registration.scope).href;
  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((cs) => {
      for (const c of cs) {
        if (c.url.startsWith(self.registration.scope)) {
          return c.focus().then(() => c.postMessage({ art: 'cm1-modul', modul }));
        }
      }
      return self.clients.openWindow(ziel);
    })
  );
});
