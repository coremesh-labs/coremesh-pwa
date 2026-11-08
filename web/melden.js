// Auf sich aufmerksam machen (Modul „Aushang“ u. a.): Systembenachrichtigung, Zähler am App-Symbol,
// Prüfung im Hintergrund. Die Hülle (Rust) ruft window.cm1Melden auf; Module nur „anfragen“ und
// „gelesen“. Ohne Erlaubnis bleibt es bei der Anzeige in der App (Kachel, Hinweis auf der Startseite).
//
// Im Hintergrund (App geschlossen): Periodic Background Sync (Chrome, installierte App) – der
// Service Worker vergleicht die Indexdateien i/<Dataset> der meldenden Module (ohne Schlüssel,
// nur die Prüfsumme) und meldet „Neues“. Was genau neu ist, zeigt die App beim Öffnen.
//
// Push (Web Push, wenn die Ablage es anbietet): Die Hülle meldet die Push-Adresse des Browsers
// beim Server an (abo). Der Server schickt Meldungen der Fachmodule direkt über den Push-Dienst
// des Browsers; der Service Worker zeigt sie auch bei geschlossener App (sw.js, Ereignis push).

const GEMELDET = 'cm1.gemeldet'; // schon benachrichtigt (Kennungen)

function lies(k) {
  try { return JSON.parse(localStorage.getItem(k) || '[]'); } catch { return []; }
}
function schreib(k, v) {
  try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* privat: ohne */ }
}

async function registrierung() {
  if (!('serviceWorker' in navigator)) return null;
  try { return await navigator.serviceWorker.ready; } catch { return null; }
}

async function hintergrund() {
  const reg = await registrierung();
  if (!reg || !('periodicSync' in reg)) return;
  try {
    const p = await navigator.permissions.query({ name: 'periodic-background-sync' });
    if (p.state !== 'granted') return;
    await reg.periodicSync.register('cm1-pruefen', { minInterval: 3 * 60 * 60 * 1000 });
  } catch { /* nicht unterstützt */ }
}

function b64url(s) {
  const b = atob(s.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (s.length % 4)) % 4));
  return Uint8Array.from(b, (c) => c.charCodeAt(0));
}
function gleich(buf, bytes) {
  if (!buf) return false;
  const a = new Uint8Array(buf);
  return a.length === bytes.length && a.every((x, i) => x === bytes[i]);
}

window.cm1Melden = {
  // Erlaubnis erfragen (nur nach Antippen), danach Prüfung im Hintergrund anmelden.
  async anfragen() {
    if (!('Notification' in window)) return 'unsupported';
    const r = await Notification.requestPermission();
    if (r === 'granted') {
      await hintergrund();
      window.dispatchEvent(new Event('cm1-gelesen')); // Hülle prüft und meldet Push an
    }
    return r;
  },
  // Ein Modul hat Neues als gelesen markiert: die Hülle frischt Kachel und Zähler auf.
  gelesen() {
    window.dispatchEvent(new Event('cm1-gelesen'));
    return 'ok';
  },
  // Zähler am App-Symbol (wo unterstützt).
  zaehler(n) {
    try {
      if (n > 0) navigator.setAppBadge?.(n);
      else navigator.clearAppBadge?.();
    } catch { /* ohne */ }
  },
  // liste: [{modul, name, neu: [{id, titel}]}] – benachrichtigt einmal je neuer Kennung.
  async melde(json) {
    const liste = JSON.parse(json || '[]');
    const alt = new Set(lies(GEMELDET));
    const jetzt = [];
    const frisch = [];
    for (const m of liste) {
      for (const n of m.neu || []) {
        const k = m.modul + ':' + n.id;
        jetzt.push(k);
        if (!alt.has(k)) frisch.push({ modul: m.modul, name: m.name, titel: n.titel });
      }
    }
    schreib(GEMELDET, jetzt); // nur aktuelle behalten
    if (!frisch.length || !('Notification' in window) || Notification.permission !== 'granted') return;
    if (document.visibilityState === 'visible') return; // App ist offen: die Hinweise stehen dort
    const reg = await registrierung();
    if (!reg) return;
    const erster = frisch[0];
    const titel = frisch.length === 1 ? 'Neu: ' + erster.titel : frisch.length + ' neue Mitteilungen';
    const text = frisch.length === 1 ? erster.name : frisch.map((f) => f.titel).join(' · ');
    await reg.showNotification(titel, {
      body: text, tag: 'cm1-' + erster.modul, renotify: true, icon: 'icon.svg', badge: 'icon.svg',
      vibrate: [200, 100, 200], data: { modul: erster.modul },
    });
  },
  // Push-Adresse des Browsers für den VAPID-Schlüssel der Ablage (JSON) bzw. '' ohne Erlaubnis.
  async abo(vapid) {
    if (typeof Notification === 'undefined' || Notification.permission !== 'granted') return '';
    const reg = await registrierung();
    if (!reg || !reg.pushManager) return '';
    try {
      const key = b64url(vapid);
      let sub = await reg.pushManager.getSubscription();
      if (sub && !gleich(sub.options && sub.options.applicationServerKey, key)) {
        await sub.unsubscribe(); // neuer Schlüssel der Ablage
        sub = null;
      }
      if (!sub) sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key });
      return JSON.stringify(sub.toJSON());
    } catch {
      return '';
    }
  },
  // Indexdateien der meldenden Module: der Service Worker merkt sich den jetzigen Stand
  // (die App hat ihn gesehen) und meldet im Hintergrund nur Änderungen danach.
  async beobachten(json) {
    const reg = await registrierung();
    const sw = reg && (reg.active || navigator.serviceWorker.controller);
    if (sw) sw.postMessage({ art: 'cm1-beobachten', liste: JSON.parse(json || '[]') });
    if (typeof Notification !== 'undefined' && Notification.permission === 'granted') await hintergrund();
  },
};

// Benachrichtigung angetippt, App war offen: Modul über #m=<modul> zeigen (die Hülle hört auf hashchange).
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.addEventListener('message', (e) => {
    if (e.data && e.data.art === 'cm1-modul' && e.data.modul) location.hash = '#m=' + e.data.modul;
  });
}
