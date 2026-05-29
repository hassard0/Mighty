#!/usr/bin/env bash
# scripts/tests/test-build-pgo-paths.sh — v0.37 T4
#
# Unit tests for the `locate_llvm_profdata_in` helper in
# `scripts/build-pgo.sh`. We mock a rustup sysroot under a temp dir,
# populate one or more `lib/rustlib/<tuple>/bin/llvm-profdata` stubs,
# then assert the helper picks the first one in the fallback chain.
#
# Why: v0.36.1 disabled darwin-arm64 PGO because the helper picked the
# wrong llvm-profdata when the host tuple's bin/ dir didn't have it.
# This test pins the fallback order so a future refactor can't regress
# the macos-14 fix that re-enables darwin-arm64 PGO in v0.37.
#
# Run:  bash scripts/tests/test-build-pgo-paths.sh
# Exit: 0 if every assertion passes, 1 on first failure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# Source build-pgo.sh in "library mode" so we get the
# `locate_llvm_profdata_in` function without running the actual build.
BUILD_PGO_SOURCE_ONLY=1 source "$SCRIPT_DIR/build-pgo.sh"

PASS=0
FAIL=0

make_stub() {
  # $1: sysroot, $2: tuple
  local dir="$1/lib/rustlib/$2/bin"
  mkdir -p "$dir"
  # Tag the stub with the tuple so we can identify which one got
  # picked when reading the function's output.
  printf '#!/bin/sh\necho %s\n' "$2" > "$dir/llvm-profdata"
  chmod +x "$dir/llvm-profdata"
}

assert_eq() {
  local label="$1" expected="$2" actual="$3"
  if [[ "$actual" == "$expected" ]]; then
    echo "PASS: $label"
    PASS=$((PASS + 1))
  else
    echo "FAIL: $label"
    echo "  expected: $expected"
    echo "  actual:   $actual"
    FAIL=$((FAIL + 1))
  fi
}

# ----------------------------------------------------------------
# Test 1: host tuple wins when present.
# ----------------------------------------------------------------
SROOT="$(mktemp -d)"
trap 'rm -rf "$SROOT"' EXIT
make_stub "$SROOT" "x86_64-unknown-linux-gnu"
make_stub "$SROOT" "aarch64-apple-darwin"
got="$(locate_llvm_profdata_in "$SROOT" "x86_64-unknown-linux-gnu" || true)"
assert_eq "host tuple (linux) takes precedence" \
  "$SROOT/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-profdata" \
  "$got"
rm -rf "$SROOT"

# ----------------------------------------------------------------
# Test 2: macos-14 host = aarch64-apple-darwin, normal layout. The
# host tuple stub is present so the helper picks it directly.
# ----------------------------------------------------------------
SROOT="$(mktemp -d)"
make_stub "$SROOT" "aarch64-apple-darwin"
got="$(locate_llvm_profdata_in "$SROOT" "aarch64-apple-darwin" || true)"
assert_eq "macos-14 aarch64 host: arm64 stub wins" \
  "$SROOT/lib/rustlib/aarch64-apple-darwin/bin/llvm-profdata" \
  "$got"
rm -rf "$SROOT"

# ----------------------------------------------------------------
# Test 3: macos-14 host claims aarch64 but llvm-profdata is only under
# x86_64-apple-darwin (the v0.36.1 regression shape). The Darwin
# fallback chain must catch it.
# ----------------------------------------------------------------
SROOT="$(mktemp -d)"
make_stub "$SROOT" "x86_64-apple-darwin"
got="$(locate_llvm_profdata_in "$SROOT" "aarch64-apple-darwin" || true)"
assert_eq "macos-14: falls back to x86_64-apple-darwin sibling" \
  "$SROOT/lib/rustlib/x86_64-apple-darwin/bin/llvm-profdata" \
  "$got"
rm -rf "$SROOT"

# ----------------------------------------------------------------
# Test 4: inverse — host = x86_64-apple-darwin but stub lives under
# aarch64-apple-darwin. The chain order puts aarch64 second so it
# wins.
# ----------------------------------------------------------------
SROOT="$(mktemp -d)"
make_stub "$SROOT" "aarch64-apple-darwin"
got="$(locate_llvm_profdata_in "$SROOT" "x86_64-apple-darwin" || true)"
assert_eq "macos-14: x86 host falls back to aarch64 sibling" \
  "$SROOT/lib/rustlib/aarch64-apple-darwin/bin/llvm-profdata" \
  "$got"
rm -rf "$SROOT"

# ----------------------------------------------------------------
# Test 5: wildcard last-resort. Stub is under a tuple we never
# enumerated; the `find` scan picks it up.
# ----------------------------------------------------------------
SROOT="$(mktemp -d)"
make_stub "$SROOT" "riscv64gc-unknown-linux-gnu"
got="$(locate_llvm_profdata_in "$SROOT" "some-future-tuple" || true)"
assert_eq "wildcard catches future tuple" \
  "$SROOT/lib/rustlib/riscv64gc-unknown-linux-gnu/bin/llvm-profdata" \
  "$got"
rm -rf "$SROOT"

# ----------------------------------------------------------------
# Test 6: empty sysroot fails cleanly (non-zero exit, empty stdout).
# Note: command substitution masks the inner exit code; run the
# helper directly and capture $? to assert on it.
# ----------------------------------------------------------------
got="$(locate_llvm_profdata_in "" "x86_64-unknown-linux-gnu" 2>/dev/null || true)"
assert_eq "empty sysroot returns empty" "" "$got"
if locate_llvm_profdata_in "" "x86_64-unknown-linux-gnu" >/dev/null 2>&1; then
  echo "FAIL: empty sysroot should exit non-zero, but exited 0"
  FAIL=$((FAIL + 1))
else
  echo "PASS: empty sysroot exits non-zero"
  PASS=$((PASS + 1))
fi

# ----------------------------------------------------------------
# Test 7: sysroot with no llvm-profdata anywhere → non-zero exit.
# ----------------------------------------------------------------
SROOT="$(mktemp -d)"
mkdir -p "$SROOT/lib/rustlib/x86_64-unknown-linux-gnu/bin"
got="$(locate_llvm_profdata_in "$SROOT" "x86_64-unknown-linux-gnu" 2>/dev/null || true)"
assert_eq "no stubs returns empty" "" "$got"
if locate_llvm_profdata_in "$SROOT" "x86_64-unknown-linux-gnu" >/dev/null 2>&1; then
  echo "FAIL: no stubs should exit non-zero, but exited 0"
  FAIL=$((FAIL + 1))
else
  echo "PASS: no stubs exits non-zero"
  PASS=$((PASS + 1))
fi
rm -rf "$SROOT"

echo
echo "Result: $PASS passed, $FAIL failed"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
exit 0
