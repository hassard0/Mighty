# demos/02_counter_web/smoke.ps1 — PowerShell equivalent of smoke.sh.
$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot/../..").Path
$sdust = $env:SDUST
if (-not $sdust) { $sdust = Join-Path $root "target\debug\sdust.exe" }
if (-not (Test-Path $sdust)) {
    Write-Error "smoke: sdust not built. Run: cargo build -p sdust-cli"
    exit 2
}

$out = Join-Path $root "demos\02_counter_web\target"
if (-not (Test-Path $out)) { New-Item -ItemType Directory -Path $out | Out-Null }

# 1) check + build
& $sdust check (Join-Path $root "demos\02_counter_web\src\main.sd") | Out-Null
& $sdust build --target wasm32-web --out-dir $out (Join-Path $root "demos\02_counter_web\src\main.sd") | Out-Null

$wasm = Join-Path $out "main.wasm"
if (-not (Test-Path $wasm)) { Write-Error "smoke FAIL: missing $wasm"; exit 1 }

# 2) magic bytes
$bytes = [System.IO.File]::ReadAllBytes($wasm)
$expected = @(0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00)
for ($i = 0; $i -lt 8; $i++) {
    if ($bytes[$i] -ne $expected[$i]) {
        Write-Error "smoke FAIL: wasm preamble byte $i = $($bytes[$i]), expected $($expected[$i])"
        exit 1
    }
}

# 3) size sanity
if ($bytes.Length -le 200) {
    Write-Error "smoke FAIL: component too small ($($bytes.Length) bytes)"
    exit 1
}

# 4) embedded 'stardust:web/log' string
$text = [System.Text.Encoding]::ASCII.GetString($bytes)
if (-not ($text -like "*stardust:web/log*")) {
    Write-Error "smoke FAIL: 'stardust:web/log' import not found"
    exit 1
}

# 5) host run
$host_out = & $sdust run (Join-Path $root "demos\02_counter_web\src\main.sd") 2>&1 | Out-String
if (-not ($host_out -like "*counter_web: built*")) {
    Write-Host "host output:"
    Write-Host $host_out
    Write-Error "smoke FAIL: host run did not log 'counter_web: built'"
    exit 1
}

Write-Host "02_counter_web: PASS (component size = $($bytes.Length) bytes)"
