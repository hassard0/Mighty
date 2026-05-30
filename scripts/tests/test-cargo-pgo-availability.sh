#!/usr/bin/env bash
# scripts/tests/test-cargo-pgo-availability.sh — v0.38 T1
#
# Linux-only smoke test that asserts the CI PGO pipeline's tooling
# is present and self-consistent:
#
#   1. `cargo-pgo` is installed and discoverable as a cargo subcommand.
#   2. `cargo pgo --help` exits 0 (the binary actually runs — catches
#      broken installs where the file exists but is unlinkable).
#   3. The `llvm-profdata` reachable to cargo-pgo (via the same rustup
#      sysroot it uses) reports the same LLVM major version as `rustc`
#      itself. This is the EXACT mismatch v0.37 hit on darwin-arm64
#      (raw=8 vs expected=10 inside the same toolchain channel); the
#      cargo-pgo migration is the v0.38 fix, and this gate ensures we
#      don't silently regress if the runner image bumps tooling out
#      of sync.
#
# Wired into ci.yml as a Linux-only step so every push gets the
# assertion without paying for it on the macOS / Windows legs (which
# build cargo-pgo as part of the release pipeline anyway).
#
# Run:  bash scripts/tests/test-cargo-pgo-availability.sh
# Exit: 0 if every assertion passes, 1 on first failure.

set -euo pipefail

PASS=0
FAIL=0

note_pass() { echo "PASS: $1"; PASS=$((PASS + 1)); }
note_fail() { echo "FAIL: $1"; FAIL=$((FAIL + 1)); }

TOOLCHAIN="${TOOLCHAIN:-1.95.0}"

# ----------------------------------------------------------------
# Assertion 1: cargo-pgo is installed.
# ----------------------------------------------------------------
if command -v cargo-pgo >/dev/null 2>&1; then
  note_pass "cargo-pgo binary on PATH"
else
  note_fail "cargo-pgo binary on PATH"
  echo "  hint: cargo install cargo-pgo --version 0.2.9 --locked" >&2
fi

# ----------------------------------------------------------------
# Assertion 2: `cargo pgo --help` exits 0. Catches partial installs
# where the binary exists but is the wrong arch / has broken deps.
# ----------------------------------------------------------------
if cargo pgo --help >/dev/null 2>&1; then
  note_pass "cargo pgo --help exits 0"
else
  note_fail "cargo pgo --help exits 0"
fi

# ----------------------------------------------------------------
# Assertion 3: llvm-profdata in the rustup sysroot matches rustc's
# LLVM major version. cargo-pgo auto-discovers a matching profdata
# at runtime, but on CI we want a hard fail BEFORE the release
# pipeline burns 20 minutes of fat-LTO compile time.
# ----------------------------------------------------------------
SYSROOT="$(rustc +"$TOOLCHAIN" --print sysroot 2>/dev/null || true)"
if [[ -z "$SYSROOT" ]]; then
  note_fail "rustc +$TOOLCHAIN --print sysroot returned empty"
else
  HOST="$(rustc +"$TOOLCHAIN" -vV 2>/dev/null | awk '/^host:/ { print $2 }')"
  PROFDATA=""
  for tuple in "$HOST" "x86_64-unknown-linux-gnu" "aarch64-unknown-linux-gnu"; do
    [[ -z "$tuple" ]] && continue
    candidate="$SYSROOT/lib/rustlib/$tuple/bin/llvm-profdata"
    if [[ -x "$candidate" ]]; then
      PROFDATA="$candidate"
      break
    fi
  done
  if [[ -z "$PROFDATA" ]]; then
    note_fail "llvm-profdata discoverable under rustup sysroot"
    echo "  hint: rustup component add llvm-tools-preview --toolchain $TOOLCHAIN" >&2
  else
    note_pass "llvm-profdata found at $PROFDATA"

    # Versions. `llvm-profdata --version` prints (note: no colon):
    #   LLVM version 22.1.2-rust-1.95.0-stable
    # and `rustc -vV` prints (with colon):
    #   LLVM version: 22.1.2
    # so we parse them with separate awk pulls. Field 3 in both cases
    # is the dotted version. The trailing `-rust-...` suffix on the
    # profdata version is fine because we only use the `major` split.
    PROFDATA_LLVM="$("$PROFDATA" --version 2>/dev/null | awk '/LLVM version/ { print $3 }' | head -1)"
    RUSTC_LLVM="$(rustc +"$TOOLCHAIN" -vV 2>/dev/null | awk '/LLVM version:/ { print $3 }')"
    echo "  llvm-profdata LLVM: ${PROFDATA_LLVM:-unknown}"
    echo "  rustc          LLVM: ${RUSTC_LLVM:-unknown}"
    if [[ -z "$PROFDATA_LLVM" || -z "$RUSTC_LLVM" ]]; then
      note_fail "LLVM major versions resolved"
    else
      PROFDATA_MAJOR="${PROFDATA_LLVM%%.*}"
      RUSTC_MAJOR="${RUSTC_LLVM%%.*}"
      if [[ "$PROFDATA_MAJOR" == "$RUSTC_MAJOR" ]]; then
        note_pass "LLVM major versions match (rustc=$RUSTC_MAJOR, profdata=$PROFDATA_MAJOR)"
      else
        note_fail "LLVM major versions match (rustc=$RUSTC_MAJOR, profdata=$PROFDATA_MAJOR)"
        echo "  This is the EXACT v0.37 darwin-arm64 failure mode." >&2
        echo "  cargo-pgo handles cross-tool discovery, but a major-version" >&2
        echo "  skew between rustc + bundled llvm-profdata in the SAME" >&2
        echo "  toolchain channel will still break the PGO pipeline." >&2
        echo "  Investigate before tagging a release." >&2
      fi
    fi
  fi
fi

echo
echo "Result: $PASS passed, $FAIL failed"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
exit 0
