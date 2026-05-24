# demos/02_counter_web/web/serve.ps1 — PowerShell equivalent of serve.sh.
$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$root = (Resolve-Path "$here/../../..").Path
$port = if ($env:PORT) { $env:PORT } else { "8000" }

$stage = Join-Path $here ".stage"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item (Join-Path $here "index.html") (Join-Path $stage "index.html")

$wasm = Join-Path $root "demos\02_counter_web\target\main.wasm"
if (-not (Test-Path $wasm)) {
    Write-Error @"
serve: $wasm not built yet. Run:
  cargo build -p sdust-cli
  .\target\debug\sdust.exe build --target wasm32-web ``
        --out-dir demos\02_counter_web\target ``
        demos\02_counter_web\src\main.sd
"@
    exit 2
}
Copy-Item $wasm (Join-Path $stage "main.wasm")

Write-Host "serving $stage on http://localhost:$port"
Set-Location $stage
python -m http.server $port
