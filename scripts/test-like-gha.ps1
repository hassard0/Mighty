# scripts/test-like-gha.ps1 — v0.45 T4
#
# Run `cargo test --workspace` with the same disk-pressure profile and
# Windows-serial test path that the GitHub Actions Windows runner uses,
# so a Windows swarm-agent can dogfood the real CI configuration BEFORE
# pushing.
#
# Why this exists (the v0.42 T1 incident, recapped in v0.44 release
# notes): a track shipped what looked like a green fix — Vulcan Linux
# passed, local Windows passed — but GHA Ubuntu SIGSEGV'd the
# regression tests. Root cause was GitHub runner disk exhaustion:
# debuginfo-heavy test binaries exceeded the small ephemeral disk
# ceiling and produced truncated artifacts the test harness crashed on.
# v0.44 had to rerun two infra-failed jobs. The fix on the CI side
# lives in `.github/workflows/ci.yml` (also coming via
# `codex/ci-disk-headroom`): drop debuginfo on the dev/test profiles
# and force Windows to `--test-threads=1`. This script is the local
# mirror for Windows dev boxes.
#
# Envs set (must match `.github/workflows/ci.yml`):
#
#   CARGO_TERM_COLOR=always
#   RUSTFLAGS=-D warnings
#   CARGO_PROFILE_DEV_DEBUG=0
#   CARGO_PROFILE_TEST_DEBUG=0
#
# Always runs the Windows serial test path:
#
#   cargo test --workspace -- --test-threads=1 --nocapture
#
# `$env:CARGO_TARGET_DIR` is honored so parallel-worktree agents using
# per-worktree target dirs (mandated by
# feedback_mighty_target_dir_isolation) do not collide.
#
# Usage:
#
#     scripts/test-like-gha.ps1
#
# Pre-push integration: set $env:MTY_PRE_PUSH_GHA = "1" to have the
# pre-push hook (a bash script) also invoke scripts/test-like-gha.sh on
# the next push.

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$quiet = $env:MTY_TEST_LIKE_GHA_QUIET
if (-not $quiet) { $quiet = "0" }

if ($quiet -ne "1") {
    Write-Host "================================================================"
    Write-Host "  Running tests with GHA disk profile (scripts/test-like-gha.ps1)"
    Write-Host "  CARGO_PROFILE_DEV_DEBUG=0   CARGO_PROFILE_TEST_DEBUG=0"
    Write-Host "  RUSTFLAGS=-D warnings        CARGO_TERM_COLOR=always"
    Write-Host "  Windows serial → cargo test --workspace -- --test-threads=1 --nocapture"
    if ($env:CARGO_TARGET_DIR) {
        Write-Host "  CARGO_TARGET_DIR=$($env:CARGO_TARGET_DIR)"
    }
    Write-Host "================================================================"
}

$env:CARGO_TERM_COLOR = "always"
if (-not $env:RUSTFLAGS) {
    $env:RUSTFLAGS = "-D warnings"
}
$env:CARGO_PROFILE_DEV_DEBUG = "0"
$env:CARGO_PROFILE_TEST_DEBUG = "0"

# `--%` stops PowerShell argument parsing so `--test-threads=1` reaches
# the test harness intact rather than being eaten as a PS switch.
cargo test --workspace --% -- --test-threads=1 --nocapture
exit $LASTEXITCODE
