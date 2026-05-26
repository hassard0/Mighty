#!/usr/bin/env bash
# Build a downloadable conformance kit tarball.
#
# Usage:
#   scripts/build-conformance-kit.sh [VERSION]
#
# If VERSION is omitted, `git describe --tags --always` is used.
#
# The tarball includes:
#   * tests/conformance/        — the full normative test corpus
#   * docs/spec/v1.0-rc.md      — the normative spec the kit pins to
#   * docs/spec/conformance.md  — the kit's normative description
#   * tests/conformance/CONFORMANCE_KIT.md
#                               — the kit's manifest
#
# This script closes v1.0-freeze blocker #3 (publishable conformance
# suite). The actual GitHub-release upload step is user-driven; this
# script produces the artifact.
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="${1:-$(git describe --tags --always 2>/dev/null || echo unknown)}"
KIT="mty-conformance-kit-${VERSION}.tar.gz"

if [ ! -d tests/conformance ]; then
  echo "error: tests/conformance/ not found (run from repo root)" >&2
  exit 1
fi
if [ ! -f docs/spec/conformance.md ]; then
  echo "error: docs/spec/conformance.md missing — kit is incomplete" >&2
  exit 1
fi
if [ ! -f tests/conformance/CONFORMANCE_KIT.md ]; then
  echo "error: tests/conformance/CONFORMANCE_KIT.md missing — kit is incomplete" >&2
  exit 1
fi

# Build the tarball. Exclude .git, target/, __pycache__/ for cleanliness.
tar --exclude='.git' \
    --exclude='target' \
    --exclude='__pycache__' \
    --exclude='*.pyc' \
    -czf "$KIT" \
    tests/conformance/ \
    docs/spec/v1.0-rc.md \
    docs/spec/conformance.md

SIZE=$(du -h "$KIT" | cut -f1)
CASES=$(find tests/conformance -name 'input.mty' | wc -l | tr -d ' ')
CATEGORIES=$(find tests/conformance -maxdepth 1 -mindepth 1 -type d | wc -l | tr -d ' ')

cat <<EOF
Built ${KIT} (${SIZE})
  * version:     ${VERSION}
  * categories:  ${CATEGORIES}
  * cases:       ${CASES}
  * spec doc:    docs/spec/v1.0-rc.md
  * kit doc:     tests/conformance/CONFORMANCE_KIT.md

Verify with:
  tar -tzf ${KIT} | head -20
  tar -xzf ${KIT} -C /tmp/mty-kit-check && ls /tmp/mty-kit-check/tests/conformance
EOF
