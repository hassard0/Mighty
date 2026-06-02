#!/usr/bin/env bash
# scripts/test-like-gha.sh — v0.45 T4
#
# Run `cargo test --workspace` with the same disk-pressure profile and
# Windows-serial test path that GitHub Actions Ubuntu/macOS/Windows
# runners use. This lets a swarm agent dogfood a realistic preview of
# what main CI will do BEFORE pushing.
#
# Why this exists (the v0.42 T1 incident, recapped in v0.44 release
# notes): a track shipped what looked like a green fix — Vulcan Linux
# passed, local Windows passed — but GHA Ubuntu SIGSEGV'd the
# regression tests. Root cause was GitHub runner disk exhaustion:
# debuginfo-heavy test binaries exceeded the small ephemeral disk
# ceiling and produced truncated artifacts that the test harness then
# crashed on. v0.44 had to rerun two infra-failed jobs. The fix on
# the CI side lives in `.github/workflows/ci.yml` (also coming via
# `codex/ci-disk-headroom`): drop debuginfo on the dev/test profiles
# and force Windows to `--test-threads=1`. This script mirrors that
# exact configuration locally so the next swarm-agent track doesn't
# get a "passed locally, exploded on GHA" surprise.
#
# Envs set (must match `.github/workflows/ci.yml`):
#
#   CARGO_TERM_COLOR=always
#   RUSTFLAGS=-D warnings
#   CARGO_PROFILE_DEV_DEBUG=0    # drop debuginfo on dev profile
#   CARGO_PROFILE_TEST_DEBUG=0   # drop debuginfo on test profile
#
# OS routing:
#
#   Linux / macOS  → cargo test --workspace
#   Windows (msys / cygwin / mingw bash) → cargo test --workspace
#                                          -- --test-threads=1 --nocapture
#
# `$CARGO_TARGET_DIR` is honored so parallel-worktree agents using
# per-worktree target dirs (mandated by feedback_mighty_target_dir_isolation)
# do not collide.
#
# Exit code is the cargo-test exit code.
#
# Usage:
#
#     scripts/test-like-gha.sh
#
# Bypass nothing — this script does NOT take `--no-verify` style
# escapes. It is meant to be run before push, on top of the existing
# pre-push hook (which gates fmt + clippy + mty-fmt). Optional
# pre-push integration: set MTY_PRE_PUSH_GHA=1 to have the hook also
# invoke this script on the next push.

set -euo pipefail

# Optional override: callers may set MTY_TEST_LIKE_GHA_QUIET=1 to
# suppress the banner (useful when the pre-push hook invokes us).
QUIET="${MTY_TEST_LIKE_GHA_QUIET:-0}"

# OS detection — mirror the same matrix-leg routing CI uses. Treat any
# MSYS/MINGW/CYGWIN bash on Windows as the Windows leg.
case "${OSTYPE:-}" in
  msys*|cygwin*|win32*) IS_WINDOWS=1 ;;
  *)
    case "$(uname -s 2>/dev/null || echo unknown)" in
      MINGW*|MSYS*|CYGWIN*|Windows_NT) IS_WINDOWS=1 ;;
      *) IS_WINDOWS=0 ;;
    esac
    ;;
esac

if [ "$QUIET" != "1" ]; then
  echo "================================================================"
  echo "  Running tests with GHA disk profile (scripts/test-like-gha.sh)"
  echo "  CARGO_PROFILE_DEV_DEBUG=0   CARGO_PROFILE_TEST_DEBUG=0"
  echo "  RUSTFLAGS=-D warnings        CARGO_TERM_COLOR=always"
  if [ "$IS_WINDOWS" = "1" ]; then
    echo "  Windows detected → cargo test --workspace -- --test-threads=1 --nocapture"
  else
    echo "  Linux/macOS      → cargo test --workspace"
  fi
  if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    echo "  CARGO_TARGET_DIR=${CARGO_TARGET_DIR}"
  fi
  echo "================================================================"
fi

export CARGO_TERM_COLOR=always
export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0

if [ "$IS_WINDOWS" = "1" ]; then
  cargo test --workspace -- --test-threads=1 --nocapture
else
  cargo test --workspace
fi
