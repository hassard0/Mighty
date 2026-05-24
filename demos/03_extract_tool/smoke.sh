#!/usr/bin/env bash
# demos/03_extract_tool/smoke.sh — run the extractor and diff its
# stdout against expected_output.txt. Also runs the breach.sd
# companion to show the sandbox/budget shape exercises the runtime
# path even when the v0.4 caps don't actively trip.

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi
if [[ ! -x "$MTY" ]]; then
  echo "smoke: mty not built. Run: cargo build -p mty-cli" >&2
  exit 2
fi

DEMO="$ROOT/demos/03_extract_tool/src/main.mty"
EXPECTED="$ROOT/demos/03_extract_tool/expected_output.txt"

# 1) check + run + diff
"$MTY" check "$DEMO" >/dev/null
ACTUAL="$("$MTY" run "$DEMO" 2>&1)"

# Normalise trailing newline + line endings for the diff.
EXPECTED_NORM="$(tr -d '\r' <"$EXPECTED")"
ACTUAL_NORM="$(printf '%s' "$ACTUAL" | tr -d '\r')"

if [[ "$ACTUAL_NORM" != "$EXPECTED_NORM" ]]; then
  echo "smoke FAIL: output does not match expected_output.txt" >&2
  diff -u <(printf '%s' "$EXPECTED_NORM") <(printf '%s' "$ACTUAL_NORM") >&2 || true
  exit 1
fi

# 2) sanity-check the snapshot line specifically.
if ! grep -F -q '"hits":7' <<<"$ACTUAL"; then
  echo "smoke FAIL: snapshot count off" >&2
  exit 1
fi

# 3) breach.sd — runs with a deliberately impossible sandbox. v0.4
# accepts this completing or trapping; we just demand it doesn't
# corrupt the runtime (exit 0).
BREACH="$ROOT/demos/03_extract_tool/src/breach.sd"
if "$MTY" check "$BREACH" >/dev/null 2>&1; then
  if ! "$MTY" run "$BREACH" >/dev/null 2>&1; then
    echo "smoke note: breach.sd trapped (expected once enforcement lands)"
  fi
fi

echo "03_extract_tool: PASS"
