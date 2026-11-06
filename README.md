# Ablage-PWA (Rust → WASM)

Progressive Web App für die verschlüsselte Ablage im Format **cm1** (Spezifikation in `spez/`).
Die Hülle und das erste Modul „Dateien“ (File Explorer) sind in Rust geschrieben und als WASM kompiliert.
Module werden über den Server verteilt (`m/<sha256>`, signierte Modulliste `i/modules`).

## Aufbau

| Pfad | Inhalt |
|---|---|
| `crates/cm1` | Format cm1: Umschlag, Modelle, Prüfen (Hash, Ed25519, age, gzip), Schreiben. Tests: `tests/roundtrip.rs` |
| `crates/pwa` | Hülle (WASM): Einrichten, Startdatei, Dataset-Indizes, Datendateien, Module laden, `host`-Schnittstelle |
| `crates/explorer` | Modul „Dateien“ (WASM): Ordner, Suche, Filter, Vorschau (PDF, Bild), Download einzeln und als Zip mit Inhaltsverzeichnis.csv |
| `crates/vorgang` | Modul „Vorgänge“ (WASM): Objektdaten process/1 aus Vorgang_Objektdaten_Konzept.txt, Abschnitt 7 – Liste mit Filtern, Detail mit allen Abschnitten nach Umfang, Stände, Vorschau, Offline-Vorabladen, „neu seit dem letzten Öffnen“ |
| `crates/tools` | `cm1tool fixture`: erzeugt eine Prüfablage (Testbestand) im cm1-Format, mit Dataset „Explorer“ und Dataset „Vorgang“ |
| `web/` | Statische PWA: `index.html`, `sw.js` (Service Worker), Manifest, Styles |
| `scripts/` | `build.ps1` (Build und Prüfablage), `serve.ps1` (statischer Server auf Port 8080) |

Die Module-Schnittstelle (`host.datasets / index / open / save`) und der Modulvertrag (`mount(host, root)`)
sind in `crates/explorer/src/lib.rs` und `crates/pwa/src/module.rs` beschrieben.

## Bauen und testen

Voraussetzungen: Rust (stable) mit Target `wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.130 und ein Linker.

```powershell
# Tests (native)
cargo test -p cm1 -p explorer

# Build, Glue, Modulpaket, Prüfablage
powershell -ExecutionPolicy Bypass -File scripts\build.ps1

# Server, dann den Einrichtungslink aus web\einrichtungslink.txt öffnen
powershell -ExecutionPolicy Bypass -File scripts\serve.ps1
```

Unter Linux (auch für die Pakete, die der Export mitliefert):

```bash
scripts/build.sh                 # Tests, WASM, build/app.zip und build/explorer.zip
scripts/build.sh --fixture http://localhost:8080/ablage/   # zusätzlich die Prüfablage unter web/
```

`build/app.zip` ist die App (Inhalt von `web/` samt `pkg/pwa`, auch `snippets/`), `build/explorer.zip` das
Modulpaket. Beide übernimmt coremesh-erp mit `make pwa-bundle` in den Export (Modul `export`): der Explorer
geht verschlüsselt mit eigenem Modulschlüssel unter `m/` in jede Ablage, die App wird in das App-Verzeichnis
des Ziels entpackt bzw. ist dort über „App herunterladen“ erhältlich.

`cargo test -p cm1 --test go_vectors` prüft gegen die Prüfdateien des Go-Exports
(`../coremesh-erp/pkg/cm1/testdata`, sonst `CM1_VECTORS=<Verzeichnis>`).

Auf dem Entwicklungsrechner (Windows ohne MSVC-Build-Tools) wurde die GNU-Toolchain mit WinLibs verwendet:
`RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-gnu`.

## Stand

Geprüft:
- `cargo test -p cm1` (10 Format-Tests: Rundlauf, Signatur, Schlüssel, Pfad, Typ, Länge, Hash, Fassung) und
  `cargo test -p explorer` (6 Logiktests: Suche, Filter, Ordner, Sortierung, Inhaltsverzeichnis, Größen).
- Im Browser (Prüfablage): Einrichtungslink, Startdatei geprüft und entschlüsselt, Modul aus `m/` geladen,
  Dataset-Index, PDF-Vorschau (Inhalt stimmt), Zip-Download mit zwei PDFs und Inhaltsverzeichnis.

Noch offen (bewusst nicht in dieser Fassung):
- **Schutz des privaten Schlüssels**: liegt im `localStorage` im Klartext. Vorgabe ist WebCrypto bzw. PIN (Konzept, Frage E).
- **Sandbox**: Module laufen in derselben Seite. Vorgabe ist ein Worker bzw. iframe ohne Netzzugang.
- **Speicher**: Startdatei und Indizes im `localStorage`, nicht in IndexedDB. Datendateien liegen im Cache des Service Workers.
- Nicht umgesetzt: „gesehen“-Merker und „neu seit Besuch“, Freigabe-Warnung (30 Tage), Suche über alle Datasets,
  zusammengesetzte Sichten (`teile`), Vorab-Laden und Speicherübersicht, Rückkanal.
- Gegen die Prüfdateien des Go-Exports geprüft (`tests/go_vectors.rs`, alle Fälle gleich) und im Browser gegen eine
  Ablage aus dem Export (Startdatei, Modul aus `m/` mit Modulschlüssel, Index). Offen auf der App-Seite:
  `null` in JSON wie fehlend behandeln (Spezifikation 1), Datasets anderer Module als „explorer“ an das
  jeweilige Modul geben (`ablauf::explorer_datasets` filtert fest auf explorer).
- Produktion: Die Ablage auf einem anderen Ursprung braucht CORS; die Seite braucht `script-src 'self' blob:` (Modul-Import).
- Der Offline-Betrieb (Service Worker mit abgeschalteter Verbindung) ist nicht im Browser getestet.
