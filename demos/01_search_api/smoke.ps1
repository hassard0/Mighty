# demos/01_search_api/smoke.ps1 — PowerShell equivalent of smoke.sh.
# Exits 0 on pass, non-zero on fail.

$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot/../..").Path
$mty = $env:MTY
if (-not $mty) { $mty = Join-Path $root "target\debug\mty.exe" }
if (-not (Test-Path $mty)) {
    Write-Error "smoke: mty binary not found at $mty. Build with: cargo build -p mty-cli"
    exit 2
}

$demo = Join-Path $root "demos\01_search_api\src\main.mty"
$out = & $mty run $demo 2>&1 | Out-String

$expectations = @(
    @{ Label = "health";   Needle = '{"status":"ok"}' },
    @{ Label = "search";   Needle = '{"q":"mighty","hits":[]}' },
    @{ Label = "search-2"; Needle = '{"q":"agents","hits":[]}' },
    @{ Label = "metrics";  Needle = '{"health":1,"search":2}' },
    @{ Label = "404";      Needle = '{"error":"not found"}' }
)

$fail = 0
foreach ($e in $expectations) {
    if (-not ($out -like "*$($e.Needle)*")) {
        Write-Host "smoke FAIL [$($e.Label)]: expected output to contain: $($e.Needle)" -ForegroundColor Red
        $fail = 1
    }
}

if ($fail -ne 0) {
    Write-Host "---- captured output ----"
    Write-Host $out
    exit 1
}
Write-Host "01_search_api: PASS"
