#!/usr/bin/env bash
# demos/06_canvas_game/web/serve.sh — stage + serve the canvas-game
# demo on http://localhost:8000.
#
# Build the wasm first if it isn't there:
#   cargo build -p mty-cli
#   bash demos/06_canvas_game/smoke.sh

set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
PORT="${PORT:-8000}"

STAGE="$HERE/.stage"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$HERE/index.html"   "$STAGE/index.html"
cp "$HERE/dom-shim.js"  "$STAGE/dom-shim.js"

WASM="$ROOT/demos/06_canvas_game/target/main.wasm"
if [[ ! -f "$WASM" ]]; then
  echo "serve: $WASM not built yet — building now..."
  "$ROOT/target/debug/mty.exe" build --target wasm32-web \
    --out-dir "$ROOT/demos/06_canvas_game/target" \
    "$ROOT/demos/06_canvas_game/src/main.mty"
fi
cp "$WASM" "$STAGE/main.wasm"

echo "serving $STAGE on http://localhost:$PORT  (Ctrl-C to stop)"
cd "$STAGE"
if command -v python >/dev/null 2>&1; then
  exec python -m http.server "$PORT"
elif command -v python3 >/dev/null 2>&1; then
  exec python3 -m http.server "$PORT"
elif command -v py >/dev/null 2>&1; then
  exec py -3 -m http.server "$PORT"
else
  echo "serve: no python on PATH (tried python / python3 / py)" >&2
  exit 2
fi
