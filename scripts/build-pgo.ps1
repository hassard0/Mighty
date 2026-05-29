# scripts/build-pgo.ps1 — v0.22 Profile-Guided Optimization (PGO)
# build pipeline for the `mty` binary on Windows.
#
# Mirrors `scripts/build-pgo.sh` but uses PowerShell idioms and the
# rustup-managed llvm-profdata under the toolchain sysroot (Windows
# rarely has a system LLVM on PATH).
#
# Environment / parameters:
#   -ProfDir   Where to put .profraw shards. Default: target/pgo-profiles
#   -Toolchain Rust toolchain (must have llvm-tools-preview). Default: 1.95.0
#
# Reference: docs/internals/pgo.md, dev/history/notes/PGO_V0_22_NOTES.md

[CmdletBinding()]
param(
    [string]$ProfDir = "target/pgo-profiles",
    [string]$Toolchain = "1.95.0"
)

$ErrorActionPreference = "Stop"

function Find-LlvmProfdata {
    param([string]$Toolchain)

    $cmd = Get-Command llvm-profdata -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    # rustup-managed: <sysroot>/lib/rustlib/<host>/bin/llvm-profdata.exe
    $sysroot = & rustc "+$Toolchain" --print sysroot 2>$null
    if (-not $sysroot) { return $null }
    $sysroot = $sysroot.Trim()

    $vv = & rustc "+$Toolchain" -vV 2>$null
    # NB: `$host` is a built-in automatic variable in PowerShell — use a
    # different name for the local.
    $hostTriple = ($vv | Where-Object { $_ -match '^host:' } | ForEach-Object { ($_ -split '\s+')[1] })
    if (-not $hostTriple) { return $null }

    $candidate = Join-Path $sysroot "lib\rustlib\$hostTriple\bin\llvm-profdata.exe"
    if (Test-Path $candidate) { return $candidate }
    return $null
}

$LlvmProfdata = Find-LlvmProfdata -Toolchain $Toolchain
if (-not $LlvmProfdata) {
    Write-Error @"
llvm-profdata not found.
  Install with: rustup component add llvm-tools-preview --toolchain $Toolchain
"@
    exit 1
}
Write-Host "Using llvm-profdata: $LlvmProfdata"

# v0.35.1 fix: rustc resolves `-Cprofile-use=<path>` at compile
# time from each build script's own CWD (package dir), not the
# workspace root. A relative path works for `-Cprofile-generate`
# (resolved at runtime CWD) but blows up at `-Cprofile-use`.
# Promote $ProfDir to absolute before any rustc sees it.
New-Item -ItemType Directory -Force -Path $ProfDir | Out-Null
$ProfDir = (Resolve-Path $ProfDir).Path

# ----------------------------------------------------------------
# Phase 0: prepare profile dir + wipe stale PGO build artifacts.
#
# v0.36 T5: also wipe `target/release-pgo/{build,deps,incremental,
# .fingerprint}`. The v0.35.2 profile-format mismatch (raw=8 vs
# expected=10) on Windows traced to stale `target/release-pgo/`
# artifacts surviving across runs: the instrumented Phase 1 reused
# Phase 4's prior `-Cprofile-use` codegen, putting the wrong
# profile-format header in the new binary. Force fresh codegen on
# every release build. The rest of `target/` (debug, release,
# per-triple) stays cached.
# ----------------------------------------------------------------
Write-Host "=== Phase 0: prepare profile dir + wipe stale PGO build artifacts ==="
if (Test-Path $ProfDir) { Remove-Item -Recurse -Force $ProfDir }
New-Item -ItemType Directory -Force -Path $ProfDir | Out-Null
if (Test-Path "target/release-pgo") {
    foreach ($sub in @("build", "deps", "incremental", ".fingerprint")) {
        $p = "target/release-pgo/$sub"
        if (Test-Path $p) { Remove-Item -Recurse -Force $p }
    }
}

# ----------------------------------------------------------------
# Phase 1: instrumented build. We thread the flag through the env
# var that cargo expects rather than -C rustflags so the value
# survives PowerShell quoting.
# ----------------------------------------------------------------
Write-Host "=== Phase 1: instrumented build (profile-generate) ==="
$env:RUSTFLAGS = "-Cprofile-generate=$ProfDir"
try {
    & cargo "+$Toolchain" build --profile release-pgo -p mty-cli
    if ($LASTEXITCODE -ne 0) { throw "instrumented build failed" }
} finally {
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
}

$MtyBin = "target/release-pgo/mty.exe"
if (-not (Test-Path $MtyBin)) {
    $MtyBin = "target/release-pgo/mty"
    if (-not (Test-Path $MtyBin)) {
        Write-Error "instrumented mty binary not found"
        exit 1
    }
}

# ----------------------------------------------------------------
# Phase 2: profile collection. Sweep `mty check` over the bundled
# examples (skipping any @typeck-pending markers) and run one
# wasm32-wasi build to exercise codegen.
# ----------------------------------------------------------------
Write-Host "=== Phase 2: profile collection ==="
$examples = Get-ChildItem "examples" -Filter "*.mty" -ErrorAction SilentlyContinue
if (-not $examples) {
    Write-Warning "no .mty examples found under examples/ — profile will be thin"
}
foreach ($f in $examples) {
    $marker = Select-String -Path $f.FullName -Pattern "@typeck-pending" -Quiet -ErrorAction SilentlyContinue
    if ($marker) {
        Write-Host "  skip (typeck-pending): $($f.Name)"
        continue
    }
    Write-Host "  check: $($f.Name)"
    & $MtyBin check $f.FullName 2>&1 | Out-Null
    # tolerate non-zero exit; the instrumented binary may legitimately reject a file
}

$hello = "examples/01_hello.mty"
if (Test-Path $hello) {
    Write-Host "  build wasm32-wasi: $hello"
    & $MtyBin build $hello --target wasm32-wasi 2>&1 | Out-Null
}

$benchBin = "target/release-pgo/mty-bench-pgo.exe"
if (-not (Test-Path $benchBin)) { $benchBin = "target/release-pgo/mty-bench-pgo" }
if (Test-Path $benchBin) {
    Write-Host "  mty-bench-pgo workloads"
    & $benchBin --quick 2>&1 | Out-Null
}

# ----------------------------------------------------------------
# Phase 3: merge .profraw shards.
# ----------------------------------------------------------------
Write-Host "=== Phase 3: merge profiles ==="
$raws = Get-ChildItem $ProfDir -Filter "*.profraw" -ErrorAction SilentlyContinue
if (-not $raws -or $raws.Count -eq 0) {
    Write-Error "no .profraw files were produced — check Phase 1+2"
    exit 1
}
Write-Host "  merging $($raws.Count) .profraw shards"
$merged = Join-Path $ProfDir "merged.profdata"
& $LlvmProfdata merge -o $merged $raws.FullName
if ($LASTEXITCODE -ne 0) { throw "llvm-profdata merge failed" }

# ----------------------------------------------------------------
# Phase 4: optimised rebuild.
#
# v0.36 T5: dropped `-Clinker-plugin-lto`. The `release-pgo` profile
# already pins `lto = "fat"` which is the heaviest layout rustc
# supports. `-Clinker-plugin-lto` is a separate flag for cross-LTO
# between rustc bitcode and LLVM-built static libs; the mty dep
# graph doesn't have those, so the flag was zero-value on Windows
# but contributed to PGO module-flag collisions on linux-x86_64.
# Same fix on both platforms keeps the script consistent.
# ----------------------------------------------------------------
Write-Host "=== Phase 4: optimised rebuild (profile-use) ==="
$env:RUSTFLAGS = "-Cprofile-use=$merged"
try {
    & cargo "+$Toolchain" build --profile release-pgo -p mty-cli
    if ($LASTEXITCODE -ne 0) { throw "optimised rebuild failed" }
} finally {
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
}

# ----------------------------------------------------------------
# Phase 5: stable artifact path.
# ----------------------------------------------------------------
Write-Host "=== Phase 5: copy artifact ==="
$src = "target/release-pgo/mty.exe"
$dst = "target/mty-pgo.exe"
if (-not (Test-Path $src)) {
    $src = "target/release-pgo/mty"
    $dst = "target/mty-pgo"
}
Copy-Item $src $dst -Force
Write-Host "Built $dst"
Write-Host "Done."
