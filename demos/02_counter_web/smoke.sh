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
