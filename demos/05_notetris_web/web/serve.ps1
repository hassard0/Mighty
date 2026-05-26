# demos/05_notetris_web/web/serve.ps1 — Windows variant of serve.sh.
# Stage + serve the Notetris demo on http://localhost:8000.

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Resolve-Path (Join-Path $here "..\..\..")
$port = if ($env:PORT) { $env:PORT } else { 8000 }

$stage = Join-Path $here ".stage"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item (Join-Path $here "index.html")  $stage
Copy-Item (Join-Path $here "dom-shim.js") $stage

$wasm = Join-Path $root "demos\05_notetris_web\target\main.wasm"
if (-not (Test-Path $wasm)) {
    Write-Host "serve: $wasm not built yet — building now..."
    & (Join-Path $root "target\debug\mty.exe") build --target wasm32-web `
        --out-dir (Join-Path $root "demos\05_notetris_web\target") `
        (Join-Path $root "demos\05_notetris_web\src\main.mty")
}
Copy-Item $wasm $stage

Write-Host "serving $stage on http://localhost:$port  (Ctrl-C to stop)"
Set-Location $stage
& python -m http.server $port
