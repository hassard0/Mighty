# demos/03_extract_tool/smoke.ps1 — PowerShell equivalent of smoke.sh.
$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot/../..").Path
$sdust = $env:SDUST
if (-not $sdust) { $sdust = Join-Path $root "target\debug\sdust.exe" }
if (-not (Test-Path $sdust)) {
    Write-Error "smoke: sdust not built. Run: cargo build -p sdust-cli"
    exit 2
}

$demo = Join-Path $root "demos\03_extract_tool\src\main.sd"
$expected = Join-Path $root "demos\03_extract_tool\expected_output.txt"

& $sdust check $demo | Out-Null
$actual = & $sdust run $demo 2>&1 | Out-String

$expectedText = (Get-Content -Raw $expected) -replace "`r", ""
$actualText = $actual -replace "`r", ""

# Trim trailing newlines for comparison
$expectedTrim = $expectedText.TrimEnd("`n")
$actualTrim = $actualText.TrimEnd("`n")

if ($expectedTrim -ne $actualTrim) {
    Write-Host "smoke FAIL: output does not match expected_output.txt"
    Write-Host "-- expected --"
    Write-Host $expectedTrim
    Write-Host "-- actual --"
    Write-Host $actualTrim
    exit 1
}

if (-not ($actualText -like "*`"hits`":7*")) {
    Write-Error "smoke FAIL: snapshot count off"
    exit 1
}

$breach = Join-Path $root "demos\03_extract_tool\src\breach.sd"
if (Test-Path $breach) {
    & $sdust check $breach | Out-Null
    & $sdust run $breach 2>&1 | Out-Null
}

Write-Host "03_extract_tool: PASS"
