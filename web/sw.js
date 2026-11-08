// Service Worker der Ablage-PWA.
// Regeln (Ablage_Format_Spezifikation.txt, 1): i/ nie aus dem Zwischenspeicher; o/ und m/ sind
// unveränderlich (Pfad = SHA-256) und werden dauerhaft gespeichert; die Hülle selbst: Netz zuerst.

const SHELL_CACHE = 'ablage-shell-v16';
const SIGNED_CACHE = 'ablage-shell-signiert'; // geprüfte Fassung (shell.sig)
const TRUST_CACHE = 'ablage-vertrauen-v1'; // Schlüssel des Servers, geprüfte shell.sig, offene Meldungen
// Ausgang: große Nachrichten (Bilder, PDFs) aus dem Modul „Melden“, bis sie gesendet sind – nie löschen
const OUTBOX_CACHE = 'ablage-ausgang';
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
  if (event.data && event.data.art === 'cm1-vertrauen') {
    event.waitUntil(vertrauen(event.data.keys || []).then(() => appPruefen(true)));
    return;
  }
  if (event.data && event.data.art === 'cm1-alarme') {
    event.waitUntil(alarmeAbgeben());
    return;
  }
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
      Promise.all(keys.filter((k) => ![SHELL_CACHE, DATA_CACHE, WATCH_CACHE, SIGNED_CACHE, TRUST_CACHE, OUTBOX_CACHE].includes(k)).map((k) => caches.delete(k)))
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

  // Signierte Hülle (seit export 0.18.0): die geprüfte Fassung zuerst, Neues nur nach Prüfung.
  if (url.origin === self.location.origin) {
    event.respondWith(
      gepinnt().then(async (pin) => {
        if (!pin) return netzZuerst(event, req, url);
        event.waitUntil(appPruefen(false));
        const c = await caches.open(SIGNED_CACHE);
        const hit = await c.match(req, { ignoreSearch: true }) || (req.mode === 'navigate' ? await c.match(new URL('index.html', self.registration.scope).href) : undefined);
        return hit || fetch(req);
      })
    );
    return;
  }
  event.respondWith(netzZuerst(event, req, url));
});

// Hülle und Shell: Netz zuerst, bei Fehler der Zwischenspeicher (offline).
function netzZuerst(event, req, url) {
  return (
  // Kopie vor der Rückgabe ziehen (danach liest die Seite den Inhalt, ein späteres clone() schlägt fehl).
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
}

// --- Signierte App (export 0.18.0) -----------------------------------------------------------
// Der Server legt app/shell.sig ab: SHA-256 jeder Datei, signiert (Ed25519) mit seinem
// Signaturschlüssel. Die Seite gibt dem Service Worker nach dem Start die Schlüssel, denen sie
// vertraut (aus der Einrichtung). Ab dann gilt nur noch eine Fassung, deren Signatur stimmt und
// deren Dateien zu den Prüfsummen passen; sonst bleibt die alte, und die App meldet es (ALARM).
// Grenze: das allererste Laden der App und sw.js selbst (lädt der Browser) prüft niemand.

async function liesVertrauen() {
  const c = await caches.open(TRUST_CACHE);
  const r = await c.match('vertrauen');
  return r ? r.json() : { keys: [], sig: '', alarme: [] };
}

async function schreibVertrauen(t) {
  const c = await caches.open(TRUST_CACHE);
  await c.put('vertrauen', new Response(JSON.stringify(t), { headers: { 'Content-Type': 'application/json' } }));
}

async function gepinnt() {
  return !!(await liesVertrauen()).sig;
}

async function vertrauen(keys) {
  const t = await liesVertrauen();
  for (const k of keys) {
    if (k && k.key && k.public && !t.keys.some((x) => x.key === k.key)) t.keys.push({ key: k.key, public: k.public });
  }
  await schreibVertrauen(t);
}

function b64url(s) {
  const b = atob(s.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((s.length + 3) % 4));
  return Uint8Array.from(b, (c) => c.charCodeAt(0));
}

async function sha256(buf) {
  const h = await crypto.subtle.digest('SHA-256', buf);
  return [...new Uint8Array(h)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

function sigText(dateien) {
  return 'cm1-app\n' + Object.keys(dateien).sort().map((n) => n + ' ' + dateien[n] + '\n').join('');
}

async function alarm(t, art, grund, pfad) {
  t.alarme = (t.alarme || []).concat([{ art, gewicht: 'WARN', grund, pfad: pfad || '', zeit: new Date().toISOString() }]).slice(-20);
  await schreibVertrauen(t);
  await alarmeAbgeben();
}

// Offene Meldungen an die Seite geben (sie schickt sie über den Rückkanal und zeigt einen Hinweis).
async function alarmeAbgeben() {
  const t = await liesVertrauen();
  if (!t.alarme || !t.alarme.length) return;
  const clients = await self.clients.matchAll({ type: 'window' });
  if (!clients.length) return;
  for (const c of clients) c.postMessage({ art: 'cm1-alarm', alarme: t.alarme });
  t.alarme = [];
  await schreibVertrauen(t);
}

let letztePruefung = 0;

// appPruefen: shell.sig holen; neue, gültig signierte Fassung mit passenden Dateien übernehmen.
async function appPruefen(sofort) {
  const jetzt = Date.now();
  if (!sofort && jetzt - letztePruefung < 10 * 60 * 1000) return;
  letztePruefung = jetzt;
  const t = await liesVertrauen();
  if (!t.keys.length) return;
  let text;
  try {
    const res = await fetch('shell.sig', { cache: 'no-store' });
    if (res.status === 404) {
      if (t.sig) await alarm(t, 'APP_SIGNATURE', 'Die Signatur der App (shell.sig) fehlt', 'app/shell.sig');
      return;
    }
    if (!res.ok) return;
    text = await res.text();
  } catch {
    return; // offline
  }
  if (text === t.sig) return;
  let sig;
  try {
    sig = JSON.parse(text);
    const k = t.keys.find((x) => x.key === sig.key);
    if (!k) throw new Error('unbekannter Schlüssel ' + sig.key);
    const key = await crypto.subtle.importKey('raw', b64url(k.public), { name: 'Ed25519' }, false, ['verify']);
    const ok = await crypto.subtle.verify({ name: 'Ed25519' }, key, b64url(sig.signatur), new TextEncoder().encode(sigText(sig.dateien)));
    if (!ok) throw new Error('Signatur falsch');
  } catch (e) {
    if (e && e.name === 'NotSupportedError') return; // Browser ohne Ed25519: wie bisher
    await alarm(t, 'APP_SIGNATURE', 'App-Dateien ohne gültige Signatur: ' + String(e && e.message || e), 'app/shell.sig');
    return;
  }
  // Dateien holen und prüfen; erst wenn alle passen, ersetzt die neue Fassung die alte.
  const neu = await caches.open(SIGNED_CACHE + '-neu');
  for (const name of Object.keys(sig.dateien)) {
    const u = new URL(name, self.registration.scope).href;
    let res;
    try {
      res = await fetch(u, { cache: 'no-store' });
    } catch {
      await caches.delete(SIGNED_CACHE + '-neu');
      return; // Netz weg: später erneut
    }
    const buf = await res.clone().arrayBuffer();
    if (!res.ok || (await sha256(buf)) !== sig.dateien[name]) {
      await caches.delete(SIGNED_CACHE + '-neu');
      await alarm(t, 'APP_FILE', 'App-Datei passt nicht zur Signatur: ' + name, 'app/' + name);
      return;
    }
    await neu.put(u, res);
  }
  // umkopieren: geprüfte Fassung ersetzen
  await caches.delete(SIGNED_CACHE);
  const ziel = await caches.open(SIGNED_CACHE);
  for (const r of await neu.keys()) await ziel.put(r, await neu.match(r));
  await caches.delete(SIGNED_CACHE + '-neu');
  const erstes = !t.sig;
  t.sig = text;
  await schreibVertrauen(t);
  if (!erstes) {
    for (const c of await self.clients.matchAll({ type: 'window' })) c.postMessage({ art: 'cm1-app-neu' });
  }
}

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
