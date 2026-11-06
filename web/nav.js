// Zurück-Taste bzw. -Geste (Android) innerhalb der App. Die Module kennen keinen Verlauf;
// ohne diese Hilfe verließe „Zurück“ sofort die App.
//
// Jedes Öffnen einer Ebene (Dataset, Vorgang, Dokument) legt beim Antippen einen Eintrag im
// Verlauf an (Chrome überspringt Einträge ohne Antippen). „Zurück“ betätigt dann den passenden
// Knopf der Oberfläche: zuerst das Dokumentfenster schließen, sonst den innersten sichtbaren
// Knopf „‹ Übersicht“, „‹ Vorgänge“ bzw. „‹ Start“. Schließt man selbst über diese Knöpfe, wird der
// Eintrag wieder entfernt – so bleibt der Verlauf so tief wie die Ansicht.
// Escape schließt am PC das Dokumentfenster.

const OPEN = new Set(['modul', 'open', 'show', 'open-doc']); // öffnen eine Ebene
const CLOSE = new Set(['close', 'close-preview', 'back']); // schließen eine Ebene
const OVERLAY = ['.detail [data-act="close"]', '.vorschau [data-act="close-preview"]'];

let depth = 0; // eigene Einträge im Verlauf
let ours = false; // Klick kommt von „Zurück“, nicht vom Benutzer

function visible(el) {
  return el && el.offsetParent !== null;
}

function overlayClose() {
  for (const s of OVERLAY) {
    const b = document.querySelector(s);
    if (visible(b)) return b;
  }
  return null;
}

function backButton() {
  const all = [...document.querySelectorAll('[data-act="back"]')].filter(visible);
  return all.length ? all[all.length - 1] : null;
}

document.addEventListener(
  'click',
  (e) => {
    const el = e.target.closest?.('[data-act]');
    if (!el || ours) return;
    const act = el.dataset.act;
    if (OPEN.has(act)) {
      // im offenen Dokumentfenster (z. B. Nachfolger öffnen) keine weitere Ebene
      if (act === 'show' && overlayClose()) return;
      history.pushState({ cm: depth + 1 }, '');
      depth++;
    } else if (CLOSE.has(act) && depth > 0) {
      depth--;
      ours = true; // das folgende popstate nur verbuchen
      history.back();
    }
  },
  true
);

addEventListener('popstate', () => {
  if (ours) {
    ours = false;
    return;
  }
  if (depth === 0) return; // Startseite: Browser macht weiter wie gewohnt
  depth--;
  const b = overlayClose() || backButton();
  if (b) {
    ours = true;
    b.click();
    ours = false;
  }
});

addEventListener('keydown', (e) => {
  if (e.key === 'Escape') overlayClose()?.click();
});

// Ohne Verbindung gestartet (data-offline von der Hülle): sobald das Netz zurück ist, neu laden –
// dann holt die App Startdatei, Indizes und Module wieder vom Server.
addEventListener('online', () => {
  if (document.documentElement.dataset.offline) setTimeout(() => location.reload(), 1500);
});
