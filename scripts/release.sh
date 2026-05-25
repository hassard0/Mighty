#!/usr/bin/env bash
# scripts/release.sh — single-command release workflow for Mighty.
#
# Usage:
#   scripts/release.sh [--dry-run] <new-version>
#
# Example:
#   scripts/release.sh 0.9.0
#   scripts/release.sh --dry-run 0.9.0-rc1
#
# Steps (in order):
#   1. Verify the working tree is clean.
#   2. Run `cargo test --workspace`.
#   3. Bump `workspace.package.version` in `Cargo.toml` to <new-version>.
#   4. Append a release stub to CHANGELOG.md (creates the file on first
#      release).
#   5. Commit + tag (`vN.M.P`) + push.
#   6. Optionally bundle + sign each publishable package (skipped in
#      v0.9 — real marketplace publishing comes in v0.10).
#
# `--dry-run` performs steps 1+2, prints what steps 3-6 would do, and
# exits without touching anything.

set -euo pipefail

DRY_RUN=0
NEW_VERSION=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      sed -n '2,22p' "$0"
      exit 0
      ;;
    *)
      if [[ -n "$NEW_VERSION" ]]; then
        echo "release: unexpected extra argument: $1" >&2
        exit 2
      fi
      NEW_VERSION="$1"
      shift
      ;;
  esac
done

if [[ -z "$NEW_VERSION" ]]; then
  echo "release: missing <new-version> argument (e.g. 0.9.0)" >&2
  exit 2
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CARGO_TOML="$ROOT/Cargo.toml"
CHANGELOG="$ROOT/CHANGELOG.md"
TAG="v$NEW_VERSION"

say() { printf '\033[1;36m[release]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[release]\033[0m %s\n' "$*" >&2; exit 1; }

# ----------------------------------------------------------------
# Step 1 — clean working tree.
# ----------------------------------------------------------------
say "step 1/6: verify clean working tree"
if [[ -n "$(git status --porcelain)" ]]; then
  die "working tree is dirty; commit or stash before releasing"
fi

# Tag must not already exist.
if git rev-parse "$TAG" >/dev/null 2>&1; then
  die "tag $TAG already exists"
fi

# ----------------------------------------------------------------
# Step 2 — test.
# ----------------------------------------------------------------
say "step 2/6: cargo test --workspace"
cargo test --workspace

# ----------------------------------------------------------------
# Step 3 — bump version.
# ----------------------------------------------------------------
CURRENT_VERSION="$(grep -E '^version = "' "$CARGO_TOML" | head -1 | sed -E 's/version = "([^"]+)"/\1/')"
if [[ -z "$CURRENT_VERSION" ]]; then
  # workspace.package layout — read the version field under [workspace.package].
  CURRENT_VERSION="$(awk '
    /^\[workspace.package\]/ { in_section = 1; next }
    /^\[/                    { in_section = 0 }
    in_section && /^version *=/ {
      gsub(/[" ]/, "", $3); print $3; exit
    }
    in_section && /^version *=/ {
      # fallback for the `version = "x.y.z"` form
      n = split($0, parts, "\"")
      if (n >= 2) print parts[2]
      exit
    }
  ' "$CARGO_TOML")"
fi
[[ -n "$CURRENT_VERSION" ]] || die "could not parse current version from Cargo.toml"

say "step 3/6: bump version $CURRENT_VERSION -> $NEW_VERSION"
if [[ $DRY_RUN -eq 1 ]]; then
  say "  (dry-run) would edit $CARGO_TOML"
else
  # Replace only the first `version = "..."` after `[workspace.package]`.
  python3 - "$CARGO_TOML" "$CURRENT_VERSION" "$NEW_VERSION" <<'PY'
import sys, re
path, cur, new = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(path).read()
# Find [workspace.package] and the next `version = "..."` line.
m = re.search(r'(\[workspace\.package\][^\[]*?version\s*=\s*")[^"]+(")', text, re.S)
if not m:
    sys.exit("could not locate workspace.package version line")
text = text[:m.start()] + m.group(1) + new + m.group(2) + text[m.end():]
open(path, 'w').write(text)
PY
fi

# ----------------------------------------------------------------
# Step 4 — changelog.
# ----------------------------------------------------------------
say "step 4/6: prepend CHANGELOG entry"
if [[ $DRY_RUN -eq 1 ]]; then
  say "  (dry-run) would prepend a stub for $TAG to $CHANGELOG"
else
  DATE="$(date -u +%Y-%m-%d)"
  STUB="## [$NEW_VERSION] - $DATE\n\n- TODO: fill in release notes.\n"
  if [[ -f "$CHANGELOG" ]]; then
    {
      head -1 "$CHANGELOG"
      echo
      printf '%b\n' "$STUB"
      tail -n +2 "$CHANGELOG"
    } > "$CHANGELOG.tmp" && mv "$CHANGELOG.tmp" "$CHANGELOG"
  else
    {
      echo "# Changelog"
      echo
      printf '%b\n' "$STUB"
    } > "$CHANGELOG"
  fi
fi

# ----------------------------------------------------------------
# Step 5 — commit + tag + push.
# ----------------------------------------------------------------
say "step 5/6: commit + tag $TAG"
if [[ $DRY_RUN -eq 1 ]]; then
  say "  (dry-run) would: git add Cargo.toml CHANGELOG.md && git commit && git tag $TAG && git push --follow-tags"
else
  git add Cargo.toml CHANGELOG.md
  git commit -m "release: $TAG"
  git tag -a "$TAG" -m "Mighty $TAG"
  git push --follow-tags
fi

# ----------------------------------------------------------------
# Step 6 — bundle + sign each publishable package.
# ----------------------------------------------------------------
say "step 6/6: bundle + sign publishable packages"
if [[ $DRY_RUN -eq 1 ]]; then
  say "  (dry-run) would: cargo build -p mty-cli && mty pkg publish (each pkg root)"
else
  cargo build -q -p mty-cli
  MTY="$ROOT/target/debug/mty"
  [[ -x "$MTY" ]] || MTY="$MTY.exe"
  # v0.9: no per-crate marketplace publish yet. The bundling/signing
  # is wired through `mty pkg publish` and is exercised by `cargo
  # test -p mty-pkg signing`. Real marketplace upload comes in v0.10.
  say "  (v0.9) publishing is wired but disabled at the marketplace layer."
  say "  Run \`mty pkg publish\` in any package root to produce a signed bundle."
fi

say "done — released $TAG"
