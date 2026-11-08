// Auf sich aufmerksam machen (Modul „Aushang“ u. a.): Systembenachrichtigung, Zähler am App-Symbol,
// Prüfung im Hintergrund. Die Hülle (Rust) ruft window.cm1Melden auf; Module nur „anfragen“ und
// „gelesen“. Ohne Erlaubnis bleibt es bei der Anzeige in der App (Kachel, Hinweis auf der Startseite).
//
// Im Hintergrund (App geschlossen): Periodic Background Sync (Chrome, installierte App) – der
// Service Worker vergleicht die Indexdateien i/<Dataset> der meldenden Module (ohne Schlüssel,
// nur die Prüfsumme) und meldet „Neues“. Was genau neu ist, zeigt die App beim Öffnen.

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

window.cm1Melden = {
  // Erlaubnis erfragen (nur nach Antippen), danach Prüfung im Hintergrund anmelden.
  async anfragen() {
    if (!('Notification' in window)) return 'unsupported';
    const r = await Notification.requestPermission();
    if (r === 'granted') await hintergrund();
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
