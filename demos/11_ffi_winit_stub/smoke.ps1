# demos/11_ffi_winit_stub/smoke.ps1 — PowerShell equivalent of smoke.sh.
# Gated on $env:MTY_FFI_SMOKE = "1" so default runs skip the test.

$ErrorActionPreference = "Stop"
if ($env:MTY_FFI_SMOKE -ne "1") {
    Write-Host "demo 11 FFI smoke: skipped (set MTY_FFI_SMOKE=1 to run)"
    exit 0
}

$root = (Resolve-Path "$PSScriptRoot/../..").Path
$mty  = $env:MTY
if (-not $mty) { $mty = Join-Path $root "target\release\mty.exe" }
if (-not (Test-Path $mty)) {
    Write-Error "smoke: mty binary not found at $mty. Build with: cargo build -p mty-cli --release"
    exit 2
}
$demo = Join-Path $root "demos\11_ffi_winit_stub"

# Step 1 — compile shim
$cc = $env:CC; if (-not $cc) { $cc = "clang" }
$ar = $env:AR; if (-not $ar) { $ar = "llvm-ar" }
& $cc -c -O0 (Join-Path $demo "vendor\winit_shim.c") -o (Join-Path $demo "vendor\winit_shim.o")
if ($LASTEXITCODE -ne 0) { Write-Error "cc failed"; exit 2 }
& $ar rcs (Join-Path $demo "vendor\libwinit_shim.a") (Join-Path $demo "vendor\winit_shim.o")
if ($LASTEXITCODE -ne 0) { Write-Error "ar failed"; exit 2 }

# Step 2 — build the demo
$build = & $mty build (Join-Path $demo "src\main.mty") --release --out-dir (Join-Path $demo "target") 2>&1 | Out-String
Write-Host $build

# Step 3 — run
$bin = Join-Path $demo "target\main.exe"
$out = & $bin 2>&1 | Out-String
Write-Host "--- binary output ---"
Write-Host $out

# Step 4 — assert markers (covers v0.36 + v0.37 T3 surfaces).
$fail = 0
$markers = @(
    "winit_shim_init: stub ok",
    "winit_shim_open_window: 640x480",
    "winit_shim_set_clip: rect(0,0,640x480)",
    "winit_shim_poll_event: wrote 1 to out slot",
    "winit_shim_shutdown: stub ok"
)
foreach ($marker in $markers) {
    if ($out -notlike "*$marker*") {
        Write-Error "smoke FAIL: expected marker missing: $marker"
        $fail = 1
    }
}
if ($fail -eq 0) { Write-Host "demo 11 FFI smoke: PASS" }
exit $fail
