# scripts/build-pgo.ps1 — v0.22 Profile-Guided Optimization (PGO)
# build pipeline for the `mty` binary on Windows.
#
# v0.38 NOTE: For CI we now use the `cargo-pgo` crate
# (https://github.com/Kobzol/cargo-pgo) — see
# `.github/workflows/release.yml` and `docs/internals/pgo.md`. This
# script is preserved for LOCAL DEV where users may not want to
# install cargo-pgo.
#
# If you're touching the CI PGO pipeline, edit release.yml — NOT this
# script. The two paths can diverge for local-dev ergonomics without
# breaking the release build.
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

    # v0.36.1: prefer the rustup-shipped llvm-profdata over $PATH —
    # the version that wrote the .profraw shards is the only one
    # guaranteed to parse them. See scripts/build-pgo.sh for the
    # macOS-14 profile-version mismatch this fixes.
    #
    # v0.37 T4: expanded to a fallback chain (host tuple → known
    # Darwin tuples → rustlib wildcard) to mirror build-pgo.sh. The
    # Windows runner only ever hits the first branch in practice, but
    # keeping the scripts symmetrical means the same fix lands
    # everywhere if `rustc -vV | grep host` ever resolves to a tuple
    # that doesn't have llvm-tools-preview under it.

    $sysroot = & rustc "+$Toolchain" --print sysroot 2>$null
    if ($sysroot) {
        $sysroot = $sysroot.Trim()
        $vv = & rustc "+$Toolchain" -vV 2>$null
        # NB: `$host` is a built-in automatic variable in PowerShell — use a
        # different name for the local.
        $hostTriple = ($vv | Where-Object { $_ -match '^host:' } | ForEach-Object { ($_ -split '\s+')[1] })

        # Try the host tuple first, then known fallbacks. On Windows
        # the host tuple is virtually always x86_64-pc-windows-msvc, but
        # we keep aarch64-pc-windows-msvc and the Darwin tuples in the
        # chain so the script is portable across runners.
        $candidates = @()
        if ($hostTriple) { $candidates += $hostTriple }
        $candidates += @(
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin"
        )
        foreach ($tuple in $candidates) {
            if (-not $tuple) { continue }
            $candidate = Join-Path $sysroot "lib\rustlib\$tuple\bin\llvm-profdata.exe"
            if (Test-Path $candidate) { return $candidate }
            # Some platforms (macOS) don't add .exe, even when this
            # script runs under cross-platform PowerShell. Be lenient.
            $candidateNoExt = Join-Path $sysroot "lib\rustlib\$tuple\bin\llvm-profdata"
            if (Test-Path $candidateNoExt) { return $candidateNoExt }
        }

        # Last-ditch: any rustlib bin dir that has it.
        $wildcardRoot = Join-Path $sysroot "lib\rustlib"
        if (Test-Path $wildcardRoot) {
            $found = Get-ChildItem -Path $wildcardRoot -Recurse -Filter "llvm-profdata*" -ErrorAction SilentlyContinue |
                Where-Object { -not $_.PSIsContainer } |
                Select-Object -First 1
            if ($found) { return $found.FullName }
        }
    }

    # v0.37.1: REMOVED the system-PATH last-resort fallback.
    # XCode (or homebrew-installed LLVM, or VS Studio's bundled
    # LLVM) provides a newer llvm-profdata than rustc's statically
    # linked LLVM, producing the raw=8 vs expected=10 mismatch.
    # The rustup-bundled tool is the only one guaranteed to match
    # rustc's profraw output.
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
# v0.37.3: LLVM major-version assert removed for now. The
# Bash version (build-pgo.sh) keeps it but tolerates "unknown"
# from llvm-profdata --version. The PowerShell -match piping
# was syntactically wrong in v0.37.2 and broke the working
# Windows path. Until the assert is rewritten cleanly for ps1,
# keep the simpler "trust that rustup-bundled is right" stance.

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
