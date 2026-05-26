#!/usr/bin/env bash
# demos/02_counter_web/smoke.sh — build the wasm Component and check
# that the artifact is well-formed (correct magic bytes, non-trivial
# size, and the embedded core module contains the expected `log`
# import). Exits 0 on PASS.

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi
if [[ ! -x "$MTY" ]]; then
  echo "smoke: mty not built. Run: cargo build -p mty-cli" >&2
  exit 2
fi

OUT="$ROOT/demos/02_counter_web/target"
mkdir -p "$OUT"

# 1) check + build
"$MTY" check "$ROOT/demos/02_counter_web/src/main.mty" >/dev/null
"$MTY" build --target wasm32-web --out-dir "$OUT" \
    "$ROOT/demos/02_counter_web/src/main.mty" >/dev/null

WASM="$OUT/main.wasm"
[[ -s "$WASM" ]] || { echo "smoke FAIL: missing $WASM" >&2; exit 1; }

# 2) magic bytes (component preamble: 00 61 73 6d 0d 00 01 00)
read -r b0 b1 b2 b3 b4 b5 b6 b7 < <(od -An -N8 -tx1 "$WASM" | tr -s ' ')
expected="00 61 73 6d 0d 00 01 00"
got="$b0 $b1 $b2 $b3 $b4 $b5 $b6 $b7"
if [[ "$got" != "$expected" ]]; then
  echo "smoke FAIL: wasm preamble = $got, expected $expected" >&2
  exit 1
fi

# 3) size sanity
sz=$(wc -c < "$WASM" | tr -d ' ')
(( sz > 200 )) || { echo "smoke FAIL: component too small ($sz bytes)" >&2; exit 1; }

# 4) embedded core module + `log` import — search the raw bytes.
if ! grep -aFq "mty:web/log" "$WASM"; then
  echo "smoke FAIL: 'mty:web/log' import not found in component" >&2
  exit 1
fi

# 5) also run the .sd under the host interpreter to verify the source
# itself runs cleanly (we share the same agent shape between host +
# wasm targets).
host_out="$("$MTY" run "$ROOT/demos/02_counter_web/src/main.mty" 2>&1)"
grep -F -q "counter_web: built" <<<"$host_out" || {
  echo "smoke FAIL: host run did not log 'counter_web: built'" >&2
  echo "host output:" >&2
  echo "$host_out" >&2
  exit 1
}

echo "02_counter_web: PASS (component size = ${sz} bytes)"

# 6) OPTIONAL headless-browser smoke (v0.23, Track E).
# Opt in with MTY_WEB_SMOKE=1. Validates that the page actually renders +
# JS runs + the canvas-or-equivalent surface drew something — catches
# regressions like the long-standing "magic-bytes pass but browser
# instantiate-fail" trap. Requires: Node + tests/web-smoke/ npm install.
if [[ "${MTY_WEB_SMOKE:-0}" == "1" ]]; then
  echo "smoke: MTY_WEB_SMOKE=1 — running headless-browser stage"
  WEB_PORT="${MTY_WEB_SMOKE_PORT:-8764}"
  WEB_URL="http://localhost:${WEB_PORT}"
  SMOKE_SCRIPT="$ROOT/tests/web-smoke/smoke-headless.mjs"
  if ! command -v node >/dev/null 2>&1; then
    echo "smoke: (headless smoke skipped: node not on PATH)"
  elif [[ ! -f "$SMOKE_SCRIPT" ]]; then
    echo "smoke: (headless smoke skipped: $SMOKE_SCRIPT missing)" >&2
  else
    # Boot serve.sh in background, capture its PID, ensure cleanup.
    PORT="$WEB_PORT" bash "$ROOT/demos/02_counter_web/web/serve.sh" \
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

    if ! node "$SMOKE_SCRIPT" "$WEB_URL" counter-web; then
      echo "smoke FAIL: headless-browser smoke failed for counter-web" >&2
      exit 1
    fi

    echo "02_counter_web: PASS (headless-browser smoke + magic bytes)"
  fi
fi
