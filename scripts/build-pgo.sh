#!/usr/bin/env bash
# scripts/build-pgo.sh — v0.22 Profile-Guided Optimization (PGO)
# build pipeline for the `mty` binary.
#
# Two-stage build:
#
#   1. Instrumented build:  RUSTFLAGS="-Cprofile-generate=$PROFDIR"
#      cargo +<tc> build --profile release-pgo -p mty-cli
#   2. Profile collection:  drive the instrumented binary against the
#      bundled examples (check sweep + one wasm32-wasi build) and let
#      it emit .profraw shards into $PROFDIR.
#   3. Merge:               llvm-profdata merge $PROFDIR/*.profraw …
#   4. Optimised rebuild:   RUSTFLAGS="-Cprofile-use=$PROFDIR/merged.profdata
#                                       -Clinker-plugin-lto"
#      cargo +<tc> build --profile release-pgo -p mty-cli
#   5. Artifact copy:       target/release-pgo/mty  →  target/mty-pgo
#
# Environment:
#   PROFDIR    Where to put .profraw shards. Default: target/pgo-profiles
#   TOOLCHAIN  Rust toolchain (must have llvm-tools-preview). Default: 1.95.0
#
# Platform support: Linux + macOS. Windows is best-driven through
# `scripts/build-pgo.ps1` (which has the same shape but uses the
# llvm-profdata that ships with the rustup `llvm-tools-preview`
# component).
#
# Reference: docs/internals/pgo.md, dev/history/notes/PGO_V0_22_NOTES.md

set -euo pipefail

PROFDIR="${PROFDIR:-target/pgo-profiles}"
TOOLCHAIN="${TOOLCHAIN:-1.95.0}"

# v0.35.1 fix: rustc resolves `-Cprofile-use=<path>` at compile
# time from each build script's own CWD (package dir), not the
# workspace root. A relative `target/pgo-profiles/merged.profdata`
# works for `-Cprofile-generate` (registered into the binary and
# resolved at *runtime* CWD) but blows up at `-Cprofile-use` for
# every build script: "file `target/pgo-profiles/merged.profdata`
# ... does not exist". Promote PROFDIR to absolute before any rustc
# sees it.
mkdir -p "$PROFDIR"
PROFDIR="$(cd "$PROFDIR" && pwd)"

# ----------------------------------------------------------------
# Sanity: locate llvm-profdata. We try, in order:
#   1. The rustup-managed one inside the active toolchain's sysroot
#      under `lib/rustlib/<host>/bin/llvm-profdata` (preferred — it
#      version-matches the rustc that produced the .profraw shards).
#   2. `llvm-profdata` on PATH (system LLVM) as a last resort.
# We need *one* of them — fail loudly otherwise.
#
# v0.36.1: order flipped — system LLVM on macOS-14 GitHub runners
# expects raw profile format v10 while rust 1.95.0 emits v8, which
# produces:
#   raw profile version mismatch:
#   Profile uses raw profile format version = 8; expected version = 10
# at the Phase 3 merge step. The rustup-shipped llvm-profdata is the
# version that wrote the .profraw, so it parses them by definition.
# Falling back to system LLVM only when rustup's variant is missing.
#
# v0.37 T4: expanded the rustup search to a fallback chain that also
# tries the macOS sibling tuple (aarch64↔x86_64) before falling back
# to system LLVM. v0.36.1 disabled darwin-arm64 PGO because the
# integrator hit a profile-format mismatch — the root cause was that
# `rustc -vV | grep host` on macos-14 sometimes resolved to a tuple
# whose llvm-tools-preview sub-component path didn't exist (e.g. if
# the toolchain was installed before the host tuple changed, or if
# the rust-installer ordering on macos-14 lands `llvm-tools-preview`
# under one tuple's `bin/` rather than the host's). Trying both
# Darwin tuples plus the host tuple covers every macos-14 layout we
# saw without needing system LLVM at all.
# ----------------------------------------------------------------
# `locate_llvm_profdata_in` is the pure-function inner helper. Given a
# sysroot and a host tuple, it walks the fallback chain and emits the
# first executable llvm-profdata it finds, or empty + nonzero exit if
# none. Factored out from `locate_llvm_profdata` so the test harness in
# `scripts/tests/test-build-pgo-paths.sh` can drive it against a fake
# sysroot without needing rustup.
locate_llvm_profdata_in() {
  local sysroot="$1"
  local host="$2"
  [[ -z "$sysroot" ]] && return 1
  local tuple
  for tuple in \
    "$host" \
    "aarch64-apple-darwin" \
    "x86_64-apple-darwin" \
    ; do
    [[ -z "$tuple" ]] && continue
    local candidate="$sysroot/lib/rustlib/$tuple/bin/llvm-profdata"
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
  done
  # Last-ditch: any rustlib bin dir that has it. Covers future
  # tuples we haven't enumerated.
  local wildcard
  wildcard="$(find "$sysroot/lib/rustlib" -maxdepth 3 -type f -name llvm-profdata -perm -u+x 2>/dev/null | head -n 1)"
  if [[ -n "$wildcard" && -x "$wildcard" ]]; then
    echo "$wildcard"
    return 0
  fi
  return 1
}

locate_llvm_profdata() {
  local sysroot
  sysroot="$(rustc +"$TOOLCHAIN" --print sysroot 2>/dev/null || true)"
  local host
  host="$(rustc +"$TOOLCHAIN" -vV 2>/dev/null | awk '/^host:/ { print $2 }')"
  local found
  if found="$(locate_llvm_profdata_in "$sysroot" "$host")"; then
    echo "$found"
    return 0
  fi
  if command -v llvm-profdata >/dev/null 2>&1; then
    echo "llvm-profdata"
    return 0
  fi
  echo ""
  return 1
}

# v0.37 T4: when sourced for unit-testing (BUILD_PGO_SOURCE_ONLY=1
# from the test harness), bail out before running the build. Lets the
# test re-use `locate_llvm_profdata_in` as a library without launching
# rustc / cargo.
if [[ "${BUILD_PGO_SOURCE_ONLY:-0}" == "1" ]]; then
  return 0 2>/dev/null || exit 0
fi

LLVM_PROFDATA="$(locate_llvm_profdata || true)"
if [[ -z "$LLVM_PROFDATA" ]]; then
  echo "ERROR: llvm-profdata not found." >&2
  echo "  Install with: rustup component add llvm-tools-preview --toolchain $TOOLCHAIN" >&2
  exit 1
fi
echo "Using llvm-profdata: $LLVM_PROFDATA"

# ----------------------------------------------------------------
# Phase 0: clean the profile directory so we don't merge stale data
# from an older instrumented build. We keep most of `target/` intact —
# the cargo cache for non-PGO deps is still useful between PGO runs.
#
# v0.36 T5: also wipe `target/release-pgo/build` and
# `target/release-pgo/deps`. The v0.35.2 hot-spot LLVM error
# (`Broken module found, module flag identifiers must be unique
# !"CG Profile"`) and the profile-format mismatch (raw=8 vs expected
# =10) on macOS+Windows BOTH traced to stale `target/release-pgo/`
# artifacts surviving across runs: the instrumented Phase 1 reuses
# Phase 4's prior `-Cprofile-use` codegen, doubling up the CG Profile
# metadata. Force fresh codegen on every release build.
# ----------------------------------------------------------------
echo "=== Phase 0: prepare profile dir + wipe stale PGO build artifacts ==="
rm -rf "$PROFDIR"
mkdir -p "$PROFDIR"
# Wipe per-PGO build state but keep the cargo dep cache (registry/
# git/etc). `target/release-pgo/{build,deps,incremental}` is
# specifically what needs to be fresh; the rest of `target/` (e.g.
# `target/debug`, `target/release`, `target/<triple>`) is
# untouched.
if [[ -d target/release-pgo ]]; then
  rm -rf target/release-pgo/build \
         target/release-pgo/deps \
         target/release-pgo/incremental \
         target/release-pgo/.fingerprint
fi

# ----------------------------------------------------------------
# Phase 1: instrumented build. `release-pgo` already pins fat LTO +
# single codegen unit; the only thing we add here is the profile-
# generate flag, which makes rustc insert counters around branches
# and function entries.
# ----------------------------------------------------------------
echo "=== Phase 1: instrumented build (profile-generate) ==="
RUSTFLAGS="-Cprofile-generate=$PROFDIR" \
  cargo +"$TOOLCHAIN" build --profile release-pgo -p mty-cli

MTY_BIN="target/release-pgo/mty"
if [[ ! -x "$MTY_BIN" ]]; then
  # Windows under git-bash drops the binary as `mty.exe`.
  if [[ -x "${MTY_BIN}.exe" ]]; then
    MTY_BIN="${MTY_BIN}.exe"
  else
    echo "ERROR: instrumented mty binary not found at $MTY_BIN" >&2
    exit 1
  fi
fi

# ----------------------------------------------------------------
# Phase 2: profile collection. We sweep `mty check` over every
# example that isn't gated on incomplete typeck (the `@typeck-pending`
# marker pattern is preserved for forward-compat — v0.21's examples
# are all clean) and then drive one `mty build` against the canonical
# hello-world example for wasm32-wasi to exercise the codegen path
# the instrumented binary needs to learn.
# ----------------------------------------------------------------
echo "=== Phase 2: profile collection ==="
shopt -s nullglob
EXAMPLES=(examples/*.mty)
shopt -u nullglob
if [[ ${#EXAMPLES[@]} -eq 0 ]]; then
  echo "WARN: no .mty examples found under examples/ — profile will be thin" >&2
fi
for f in "${EXAMPLES[@]}"; do
  if grep -q '@typeck-pending' "$f" 2>/dev/null; then
    echo "  skip (typeck-pending): $f"
    continue
  fi
  echo "  check: $f"
  # `mty check` is the fast path; failures are tolerated (the
  # instrumented binary may legitimately reject a syntax-only file).
  "$MTY_BIN" check "$f" >/dev/null 2>&1 || true
done

if [[ -f examples/01_hello.mty ]]; then
  echo "  build wasm32-wasi: examples/01_hello.mty"
  "$MTY_BIN" build examples/01_hello.mty --target wasm32-wasi >/dev/null 2>&1 || true
fi

# Optional: route through mty-bench-pgo if it's been built and
# available alongside the runner. This widens the profile to include
# the in-process bench paths.
BENCH_PGO_BIN="target/release-pgo/mty-bench-pgo"
if [[ -x "$BENCH_PGO_BIN" ]] || [[ -x "${BENCH_PGO_BIN}.exe" ]]; then
  [[ -x "${BENCH_PGO_BIN}.exe" ]] && BENCH_PGO_BIN="${BENCH_PGO_BIN}.exe"
  echo "  mty-bench-pgo workloads"
  "$BENCH_PGO_BIN" --quick >/dev/null 2>&1 || true
fi

# ----------------------------------------------------------------
# Phase 3: merge .profraw shards into a single .profdata. The merge
# step is what `cargo +nightly rustc -Cprofile-use=` actually consumes.
# ----------------------------------------------------------------
echo "=== Phase 3: merge profiles ==="
RAW_COUNT="$(find "$PROFDIR" -maxdepth 1 -name '*.profraw' | wc -l | tr -d ' ')"
if [[ "$RAW_COUNT" -eq 0 ]]; then
  echo "ERROR: no .profraw files were produced — check Phase 1+2" >&2
  exit 1
fi
echo "  merging $RAW_COUNT .profraw shards"
"$LLVM_PROFDATA" merge -o "$PROFDIR/merged.profdata" "$PROFDIR"/*.profraw

# ----------------------------------------------------------------
# Phase 4: optimised rebuild.
#
# v0.36 T5: dropped `-Clinker-plugin-lto`. The `release-pgo` profile
# in workspace Cargo.toml already pins `lto = "fat"`, which gives
# rustc full link-time optimisation. `-Clinker-plugin-lto` is a
# SEPARATE flag that asks the *linker* (lld/ld64) to additionally
# cross-LTO between rustc-generated bitcode and LLVM-built static
# libs in the dep graph. On linux-x86_64 this collides with PGO's
# `CG Profile` module flag and trips:
#
#   LLVM ERROR: Broken module found, module flag identifiers must
#   be unique !"CG Profile"
#
# Fat LTO (already enabled) is the heaviest layout rustc supports;
# linker-plugin-LTO only adds value when the dep graph has llvm-bc
# static libs (rare in mty). Drop it.
# ----------------------------------------------------------------
echo "=== Phase 4: optimised rebuild (profile-use) ==="
RUSTFLAGS="-Cprofile-use=$PROFDIR/merged.profdata" \
  cargo +"$TOOLCHAIN" build --profile release-pgo -p mty-cli

# ----------------------------------------------------------------
# Phase 5: stable artifact path. CI + measurement scripts look for
# `target/mty-pgo`; we copy rather than move so the next PGO run
# doesn't have to rebuild from scratch.
# ----------------------------------------------------------------
echo "=== Phase 5: copy artifact ==="
SRC="target/release-pgo/mty"
DST="target/mty-pgo"
if [[ ! -x "$SRC" ]] && [[ -x "$SRC.exe" ]]; then
  SRC="$SRC.exe"
  DST="$DST.exe"
fi
cp "$SRC" "$DST"
echo "Built $DST"
echo "Done."
