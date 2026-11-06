// PDF-Vorschau ohne eingebauten Betrachter (z. B. Chrome auf Android): dort bleibt ein
// <iframe> mit einem PDF leer. Die Module zeigen PDFs als <iframe class="preview" src="blob:…">;
// ist navigator.pdfViewerEnabled false, ersetzt dieses Skript jeden solchen Rahmen durch die
// mit pdf.js gezeichneten Seiten. pdf.js (vendor/pdfjs, Apache-2.0) wird erst beim ersten PDF geladen.
// Zum Ausprobieren am PC: Adresse einmal mit ?pdfjs öffnen (gilt bis zum Schließen des Tabs).

let forced = false;
try {
  if (new URLSearchParams(location.search).has('pdfjs')) sessionStorage.setItem('cm-pdfjs', '1');
  forced = sessionStorage.getItem('cm-pdfjs') === '1';
} catch (_) { /* ohne Speicher: nur der Betrachter des Browsers zählt */ }
const needed = forced || navigator.pdfViewerEnabled === false;

let lib = null;
async function pdfjs() {
  if (!lib) {
    lib = import('./vendor/pdfjs/pdf.min.mjs').then((m) => {
      m.GlobalWorkerOptions.workerSrc = new URL('./vendor/pdfjs/pdf.worker.min.mjs', import.meta.url).href;
      return m;
    });
  }
  return lib;
}

const docs = new Map(); // blob-URL → Promise<PDFDocumentProxy> (Module zeichnen oft neu)

function load(url) {
  if (!docs.has(url)) {
    docs.set(url, (async () => {
      const m = await pdfjs();
      const data = new Uint8Array(await (await fetch(url)).arrayBuffer());
      return m.getDocument({ data, isEvalSupported: false, enableXfa: false }).promise;
    })());
  }
  return docs.get(url);
}

async function render(box, url) {
  try {
    const doc = await load(url);
    box.textContent = '';
    const width = Math.min(900, Math.max(200, box.clientWidth - 10));
    const ratio = Math.min(window.devicePixelRatio || 1, 3);
    for (let n = 1; n <= doc.numPages; n++) {
      if (!box.isConnected) return; // Ansicht inzwischen gewechselt
      const page = await doc.getPage(n);
      const base = page.getViewport({ scale: 1 });
      const vp = page.getViewport({ scale: (width / base.width) * ratio });
      const canvas = document.createElement('canvas');
      canvas.width = Math.floor(vp.width);
      canvas.height = Math.floor(vp.height);
      canvas.setAttribute('aria-label', `Seite ${n} von ${doc.numPages}`);
      box.appendChild(canvas);
      await page.render({ canvasContext: canvas.getContext('2d'), viewport: vp }).promise;
    }
  } catch (e) {
    console.error(e);
    box.innerHTML = '<p class="leer">Vorschau nicht möglich – bitte herunterladen.</p>';
  }
}

function replace(frame) {
  const url = frame.getAttribute('src') || '';
  if (!url.startsWith('blob:')) return;
  const box = document.createElement('div');
  box.className = 'preview pdfseiten';
  box.setAttribute('role', 'document');
  box.setAttribute('aria-label', frame.getAttribute('title') || 'Vorschau');
  box.innerHTML = '<p class="leer">Vorschau wird geladen …</p>';
  frame.replaceWith(box);
  render(box, url);
}

function scan(root) {
  if (root.matches?.('iframe.preview')) replace(root);
  root.querySelectorAll?.('iframe.preview').forEach(replace);
}

if (needed) {
  new MutationObserver((list) => {
    for (const r of list) r.addedNodes.forEach((n) => n.nodeType === 1 && scan(n));
  }).observe(document.documentElement, { childList: true, subtree: true });
  scan(document.documentElement);
}
