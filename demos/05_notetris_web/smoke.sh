#!/usr/bin/env bash
# demos/05_notetris_web/smoke.sh — build + validate the Notetris demo.
# Mirrors demo 02_counter_web's contract: check + build produces a
# Component-shaped wasm artifact, magic bytes verified.

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi
if [[ ! -x "$MTY" ]]; then
  echo "smoke: mty not built. Run: cargo build -p mty-cli" >&2
  exit 2
fi

OUT="$ROOT/demos/05_notetris_web/target"
mkdir -p "$OUT"

# 1) check + fmt + build
"$MTY" check "$ROOT/demos/05_notetris_web/src/main.mty" >/dev/null
"$MTY" fmt --check "$ROOT/demos/05_notetris_web/src/main.mty" >/dev/null
"$MTY" build --target wasm32-web --out-dir "$OUT" \
    "$ROOT/demos/05_notetris_web/src/main.mty" >/dev/null

WASM="$OUT/main.wasm"
[[ -s "$WASM" ]] || { echo "smoke FAIL: missing $WASM" >&2; exit 1; }

# 2) magic bytes (Component preamble: 00 61 73 6d 0d 00 01 00)
read -r b0 b1 b2 b3 b4 b5 b6 b7 < <(od -An -N8 -tx1 "$WASM" | tr -s ' ')
expected="00 61 73 6d 0d 00 01 00"
got="$b0 $b1 $b2 $b3 $b4 $b5 $b6 $b7"
if [[ "$got" != "$expected" ]]; then
  echo "smoke FAIL: bad magic: $got (expected: $expected)" >&2
  exit 1
fi

SIZE=$(wc -c < "$WASM" | tr -d ' ')
echo "smoke OK: $WASM ($SIZE bytes, component magic verified)"
echo "next: bash demos/05_notetris_web/web/serve.sh   # opens http://localhost:8000"

# 3) OPTIONAL headless-browser smoke (v0.23, Track E).
# Opt in with MTY_WEB_SMOKE=1. Validates that the page actually renders +
# JS runs + the canvas drew something — catches regressions like the
# "magic-bytes pass but browser instantiate-fail" trap demo 02 hid for
# many releases. Requires: Node + tests/web-smoke/ npm install (one time).
if [[ "${MTY_WEB_SMOKE:-0}" == "1" ]]; then
  echo "smoke: MTY_WEB_SMOKE=1 — running headless-browser stage"
  WEB_PORT="${MTY_WEB_SMOKE_PORT:-8765}"
  WEB_URL="http://localhost:${WEB_PORT}"
  SMOKE_SCRIPT="$ROOT/tests/web-smoke/smoke-headless.mjs"
  if ! command -v node >/dev/null 2>&1; then
    echo "smoke: (headless smoke skipped: node not on PATH)"
  elif [[ ! -f "$SMOKE_SCRIPT" ]]; then
    echo "smoke: (headless smoke skipped: $SMOKE_SCRIPT missing)" >&2
  else
    # Boot serve.sh in background, capture its PID, ensure cleanup.
    PORT="$WEB_PORT" bash "$ROOT/demos/05_notetris_web/web/serve.sh" \
        >"$OUT/serve.log" 2>&1 &
    SERVE_PID=$!
    cleanup() { kill "$SERVE_PID" 2>/dev/null || true; wait "$SERVE_PID" 2>/dev/null || true; }
    trap cleanup EXIT

    # Wait for the server to come up (max ~10s).
    for i in 1 2 3 4 5 6 7 8 9 10; do
      if curl -fsS -o /dev/null "$WEB_URL/" 2>/dev/null; then break; fi
      sleep 1
    done

    if ! curl -fsS -o /dev/null "$WEB_URL/" 2>/dev/null; then
      echo "smoke FAIL: serve.sh did not come up on $WEB_URL" >&2
      echo "--- serve.log ---" >&2
      cat "$OUT/serve.log" >&2 || true
      exit 1
    fi

    if ! node "$SMOKE_SCRIPT" "$WEB_URL" notetris; then
      echo "smoke FAIL: headless-browser smoke failed for notetris" >&2
      exit 1
    fi

    echo "05_notetris_web: PASS (headless-browser smoke + magic bytes)"
  fi
fi
