#!/usr/bin/env bash
# scripts/tests/test-bench-results-headers.sh — v0.38 T6
#
# Asserts every docs/benchmarks/*.md result page carries a recent
# "Last refreshed:" header. The freshness gate is set per the rolling
# benchmark refresh policy: a result page is "fresh" if its header
# parses to a Mighty version >= MIN_VERSION.
#
# Why: v0.36.2 docs sweep flagged that several result pages still
# carried "v0.6 baseline" / "v0.33" callouts after newer refreshes
# had landed. This test pins the freshness gate so a future docs
# refresh doesn't silently let some pages drift back.
#
# The set of pages this gate applies to is every Markdown file under
# docs/benchmarks/ EXCEPT:
#   - README.md                 (no header, intentional)
#   - methodology.md            (procedural doc, no per-version numbers)
#   - index.md                  (covered separately — see below)
#
# index.md is covered too because the headline summary belongs to the
# same refresh as the per-category pages.
#
# Run:  bash scripts/tests/test-bench-results-headers.sh
# Exit: 0 if every page parses to >= MIN_VERSION, 1 on first failure.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DOC_DIR="$ROOT/docs/benchmarks"

# Minimum acceptable version. Bump this when a new refresh lands so
# the test enforces forward progress. Format is "MAJOR MINOR" because
# bash arithmetic doesn't natively grok "0.36"-style strings.
MIN_MAJOR=0
MIN_MINOR=36

PASS=0
FAIL=0

# v0.38 T6: explicit page list keeps the test deterministic vs
# whatever happens to be in docs/benchmarks/. README + methodology
# are exempt; everything else gates on the freshness header.
PAGES=(
  "$DOC_DIR/index.md"
  "$DOC_DIR/parse_throughput.md"
  "$DOC_DIR/agent_send_latency.md"
  "$DOC_DIR/mailbox_throughput.md"
  "$DOC_DIR/http_server_throughput.md"
  "$DOC_DIR/compile_to_native.md"
  "$DOC_DIR/wasm_size.md"
)

# Extract the highest "Last refreshed: vX.Y" mention in the file. We
# scan all lines (not just the first) because some pages mention
# older baselines in continuity callouts; the gate is "does the page
# have AT LEAST one >= MIN_VERSION refresh stamp?".
#
# Output: "MAJOR MINOR" or empty if no match.
highest_version() {
  local file="$1"
  # Match "Last refreshed: vMAJOR.MINOR" — capture digits only. Sed
  # is portable across linux + macOS bash; grep -oE works on both.
  grep -oE 'Last refreshed:[^v]*v[0-9]+\.[0-9]+' "$file" 2>/dev/null \
    | grep -oE 'v[0-9]+\.[0-9]+' \
    | sed 's/v//' \
    | awk -F. '
      { if ($1 > maj || ($1 == maj && $2 > min)) { maj=$1; min=$2 } }
      END { if (maj != "" || min != "") print maj, min }
    '
}

assert_fresh() {
  local file="$1"
  local label
  label="$(basename "$file")"
  if [[ ! -f "$file" ]]; then
    echo "FAIL: $label — file missing"
    FAIL=$((FAIL + 1))
    return
  fi
  local got
  got="$(highest_version "$file" || true)"
  if [[ -z "$got" ]]; then
    echo "FAIL: $label — no 'Last refreshed: vX.Y' header found"
    FAIL=$((FAIL + 1))
    return
  fi
  local got_major got_minor
  got_major="$(echo "$got" | awk '{print $1}')"
  got_minor="$(echo "$got" | awk '{print $2}')"
  if (( got_major > MIN_MAJOR )) || \
     { (( got_major == MIN_MAJOR )) && (( got_minor >= MIN_MINOR )); }; then
    echo "PASS: $label — refreshed at v${got_major}.${got_minor}"
    PASS=$((PASS + 1))
  else
    echo "FAIL: $label — refreshed at v${got_major}.${got_minor}, need >= v${MIN_MAJOR}.${MIN_MINOR}"
    FAIL=$((FAIL + 1))
  fi
}

for page in "${PAGES[@]}"; do
  assert_fresh "$page"
done

echo
echo "Result: $PASS passed, $FAIL failed (min version: v${MIN_MAJOR}.${MIN_MINOR})"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
exit 0
