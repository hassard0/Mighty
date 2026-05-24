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
exec python3 -m http.server "$PORT"
