#!/usr/bin/env bash
# Baut die Ablage-PWA unter Linux: Tests, WASM (Hülle + Explorer-Modul), JS-Glue und die
# beiden Pakete für den Server:
#   build/app.zip       die App (Inhalt von web/ mit pkg/pwa) – auf dem Webserver unter pwa_url
#   build/explorer.zip  Modulpaket „Dateien“ (manifest.json, explorer.js, explorer_bg.wasm)
#   build/vorgang.zip   Modulpaket „Vorgänge“ (manifest.json, vorgang.js, vorgang_bg.wasm)
# Optional --fixture <Adresse der Ablage>: Prüfablage unter web/ablage (wie build.ps1).
# Voraussetzungen: Rust (stable) mit Target wasm32-unknown-unknown, wasm-bindgen-cli 0.2.130, zip.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
export PATH="$HOME/.cargo/bin:$PATH"
skip_tests=0; fixture=""
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-tests) skip_tests=1 ;;
    --fixture) fixture="$2"; shift ;;
    *) echo "Aufruf: scripts/build.sh [--skip-tests] [--fixture <Adresse der Ablage>]" >&2; exit 2 ;;
  esac
  shift
done
target="${CARGO_TARGET_DIR:-$root/target}"

[ $skip_tests = 1 ] || cargo test -p cm1 -p explorer -p vorgang
cargo build --release --target wasm32-unknown-unknown -p pwa -p explorer -p vorgang
wasm="$target/wasm32-unknown-unknown/release"

rm -rf web/pkg/pwa build/explorer build/vorgang build/app
wasm-bindgen --target web --out-dir web/pkg/pwa "$wasm/pwa.wasm"
wasm-bindgen --target web --out-dir build/explorer "$wasm/explorer.wasm"
cp crates/explorer/manifest.json build/explorer/
wasm-bindgen --target web --out-dir build/vorgang "$wasm/vorgang.wasm"
cp crates/vorgang/manifest.json build/vorgang/

# Modulpakete: nur Manifest, Einstieg und WASM, ohne Verzeichnisse, feste Reihenfolge
for m in explorer vorgang; do
  rm -f "build/$m.zip"
  (cd "build/$m" && zip -q -X -D "../$m.zip" manifest.json "$m.js" "${m}_bg.wasm")
done

# App: web/ ohne Prüfablage und Einrichtungslink
mkdir -p build/app
cp -r web/index.html web/sw.js web/manifest.webmanifest web/style.css web/icon.svg build/app/
mkdir -p build/app/pkg && cp -r web/pkg/pwa build/app/pkg/ && rm -f build/app/pkg/pwa/*.d.ts
rm -f build/app.zip
(cd build/app && zip -q -X -r ../app.zip .)

if [ -n "$fixture" ]; then
  cargo run --release -p tools -- fixture --module build/explorer --module build/vorgang --out web --base "$fixture"
fi
ls -l build/app.zip build/explorer.zip build/vorgang.zip
