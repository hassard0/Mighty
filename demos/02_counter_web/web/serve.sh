#!/usr/bin/env bash
# demos/02_counter_web/web/serve.sh — minimal static server for the
# wasm demo. Uses python3's http.server so there's zero Stardust- or
# node-side dependency. Default port: 8000.
#
# After it boots, open http://localhost:8000 and click "+1".

set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
PORT="${PORT:-8000}"

# Stage the wasm + html into a serving root so `fetch('main.wasm')`
# resolves to the freshly-built artifact.
STAGE="$HERE/.stage"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$HERE/index.html" "$STAGE/index.html"
WASM="$ROOT/demos/02_counter_web/target/main.wasm"
if [[ ! -f "$WASM" ]]; then
  echo "serve: $WASM not built yet — run:" >&2
  echo "       cargo build -p sdust-cli" >&2
  echo "       ./target/debug/sdust build --target wasm32-web \\" >&2
  echo "             --out-dir demos/02_counter_web/target \\" >&2
  echo "             demos/02_counter_web/src/main.sd" >&2
  exit 2
fi
cp "$WASM" "$STAGE/main.wasm"

echo "serving $STAGE on http://localhost:$PORT"
cd "$STAGE"
# Cascade through python / python3 / py so this works on Windows
# (where bare `python3` is often the Microsoft Store launcher
# stub rather than a real interpreter). Pattern lifted from demo 06.
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
