# Baut die Ablage-PWA: Tests, WASM (Hülle + Explorer-Modul), JS-Glue, Modulpaket und Prüfablage.
# Voraussetzungen: Rust (stable) mit Target wasm32-unknown-unknown, wasm-bindgen-cli 0.2.130 und ein Linker.
# Aufruf aus dem Projektverzeichnis:  powershell -ExecutionPolicy Bypass -File scripts\build.ps1
param(
    [string]$Base = "http://localhost:8080/ablage/",
    [switch]$SkipTests
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not $SkipTests) {
    cargo test -p cm1 -p explorer -p vorgang
    if ($LASTEXITCODE -ne 0) { throw "Tests fehlgeschlagen" }
}

cargo build --release --target wasm32-unknown-unknown -p pwa -p explorer -p vorgang
if ($LASTEXITCODE -ne 0) { throw "WASM-Build fehlgeschlagen" }

$wasmDir = Join-Path $root 'target\wasm32-unknown-unknown\release'

# Hülle: Glue und WASM nach web\pkg\pwa
wasm-bindgen --target web --out-dir (Join-Path $root 'web\pkg\pwa') (Join-Path $wasmDir 'pwa.wasm')
if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen (pwa) fehlgeschlagen" }

# Explorer-Modul: nach build\explorer, mit Manifest
$modDir = Join-Path $root 'build\explorer'
wasm-bindgen --target web --out-dir $modDir (Join-Path $wasmDir 'explorer.wasm')
if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen (explorer) fehlgeschlagen" }
Copy-Item (Join-Path $root 'crates\explorer\manifest.json') $modDir -Force

# Modul „Vorgänge“: eigenes Paket
$vgDir = Join-Path $root 'build\vorgang'
wasm-bindgen --target web --out-dir $vgDir (Join-Path $wasmDir 'vorgang.wasm')
if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen (vorgang) fehlgeschlagen" }
Copy-Item (Join-Path $root 'crates\vorgang\manifest.json') $vgDir -Force

# Prüfablage (Ablage unter web\ablage, Einrichtungslink in web\einrichtungslink.txt)
cargo run --release -p tools -- fixture --module $modDir --module $vgDir --out (Join-Path $root 'web') --base $Base
if ($LASTEXITCODE -ne 0) { throw "Prüfablage fehlgeschlagen" }

Write-Host ""
Write-Host "Fertig. Starten mit:  python -m http.server 8080 --directory web"
Write-Host "Dann den Einrichtungslink aus web\einrichtungslink.txt öffnen."
