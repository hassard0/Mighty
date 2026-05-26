#!/usr/bin/env bash
# demos/06_canvas_game/smoke.sh — build + validate the v0.23 canvas-
# game demo. Mirrors demo 05's contract (mty check + fmt --check +
# build + Component magic-bytes + opt-in headless-browser smoke).

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi
if [[ ! -x "$MTY" ]]; then
  echo "smoke: mty not built. Run: cargo build -p mty-cli" >&2
  exit 2
fi

OUT="$ROOT/demos/06_canvas_game/target"
mkdir -p "$OUT"

# 1) check + fmt + build
"$MTY" check "$ROOT/demos/06_canvas_game/src/main.mty" >/dev/null
"$MTY" fmt --check "$ROOT/demos/06_canvas_game/src/main.mty" >/dev/null
"$MTY" build --target wasm32-web --out-dir "$OUT" \
    "$ROOT/demos/06_canvas_game/src/main.mty" >/dev/null

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
echo "next: bash demos/06_canvas_game/web/serve.sh   # opens http://localhost:8000"

# 3) OPTIONAL headless-browser smoke (v0.23, Track E).
# Opt in with MTY_WEB_SMOKE=1. Validates that the page actually renders
# + the canvas drew something. Catches regressions that the magic-byte
# check would miss (e.g. shim throws at import-bind time).
if [[ "${MTY_WEB_SMOKE:-0}" == "1" ]]; then
  echo "smoke: MTY_WEB_SMOKE=1 — running headless-browser stage"
  WEB_PORT="${MTY_WEB_SMOKE_PORT:-8766}"  # +1 vs demo 05 to avoid clash
  WEB_URL="http://localhost:${WEB_PORT}"
  SMOKE_SCRIPT="$ROOT/tests/web-smoke/smoke-headless.mjs"
  if ! command -v node >/dev/null 2>&1; then
    echo "smoke: (headless smoke skipped: node not on PATH)"
  elif [[ ! -f "$SMOKE_SCRIPT" ]]; then
    echo "smoke: (headless smoke skipped: $SMOKE_SCRIPT missing)" >&2
  else
    PORT="$WEB_PORT" bash "$ROOT/demos/06_canvas_game/web/serve.sh" \
        >"$OUT/serve.log" 2>&1 &
    SERVE_PID=$!
    cleanup() { kill "$SERVE_PID" 2>/dev/null || true; wait "$SERVE_PID" 2>/dev/null || true; }
    trap cleanup EXIT

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

    if ! node "$SMOKE_SCRIPT" "$WEB_URL" canvas_game; then
      echo "smoke FAIL: headless-browser smoke failed for canvas_game" >&2
      exit 1
    fi

    echo "06_canvas_game: PASS (headless-browser smoke + magic bytes)"
  fi
fi
