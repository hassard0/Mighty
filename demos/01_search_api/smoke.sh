#!/usr/bin/env bash
# demos/01_search_api/smoke.sh — drive `mty run` on the demo and check
# that every endpoint produces the expected response line.
#
# Exit code 0 = pass, non-zero = fail.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ ! -x "$MTY" && ! -x "$MTY.exe" ]]; then
  echo "smoke: mty binary not found at $MTY" >&2
  echo "        build it with: cargo build -p mty-cli" >&2
  exit 2
fi
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi

DEMO="$ROOT/demos/01_search_api/src/main.mty"

out="$("$MTY" run "$DEMO" 2>&1)"
fail=0
check() {
  local label="$1"; shift
  local needle="$1"; shift
  if ! grep -F -q "$needle" <<<"$out"; then
    echo "smoke FAIL [$label]: expected output to contain: $needle" >&2
    fail=1
  fi
}

check health   '{"status":"ok"}'
check search   '{"q":"stardust","hits":[]}'
check search-2 '{"q":"agents","hits":[]}'
check metrics  '{"health":1,"search":2}'
check 404      '{"error":"not found"}'

if [[ "$fail" -ne 0 ]]; then
  echo "---- captured output ----" >&2
  printf '%s\n' "$out" >&2
  exit 1
fi
echo "01_search_api: PASS"
