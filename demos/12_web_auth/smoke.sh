#!/usr/bin/env bash
# demos/12_web_auth/smoke.sh — build + validate the v0.40 web-auth demo.
#
# Default mode:
#   * `mty check` + `mty fmt --check` on src/main.mty.
#   * `mty run` the login pipeline and assert every event marker
#     (`evt:auth:start`, `evt:auth:verified`, `evt:auth:cookie`,
#      `evt:auth:roundtrip-ok`, `web_auth: login pipeline OK ...`).
#   * Grep the source for the v0.39 + v0.40 surface markers
#     (`std.crypto.hmac_sha256`, `std.crypto.aes_gcm.encrypt`,
#      `std.uuid.Uuid.v7`, `std.url.percent_encode`, ...) so a future
#     refactor that drops a primitive trips the smoke.
#   * Sanity-check the `web/index.html` fixture is present.
#
# Exit code 0 = pass, non-zero = fail.

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi
if [[ ! -x "$MTY" ]]; then
  echo "smoke: mty not built. Run: cargo build -p mty-cli" >&2
  exit 2
fi

DEMO="$ROOT/demos/12_web_auth"
SRC="$DEMO/src/main.mty"

# 1) mty check
"$MTY" check "$SRC" >/dev/null
echo "smoke: mty check OK"

# 2) mty fmt --check (catches drift after a manual edit)
"$MTY" fmt --check "$SRC" >/dev/null
echo "smoke: mty fmt --check OK"

# 3) Run the demo and check every event marker fired.
out="$("$MTY" run "$SRC" 2>&1)"
fail=0
check() {
  local label="$1"; shift
  local needle="$1"; shift
  if ! grep -F -q "$needle" <<<"$out"; then
    echo "smoke FAIL [$label]: expected output to contain: $needle" >&2
    fail=1
  fi
}
check start        'evt:auth:start'
check hashed       'evt:auth:hashed'
check verified     'evt:auth:verified'
check session-id   'evt:auth:session-id'
check keyed        'evt:auth:keyed'
check sealed       'evt:auth:sealed'
check cookie       'evt:auth:cookie'
check roundtrip    'evt:auth:roundtrip-ok'
check final        'web_auth: login pipeline OK'
if [[ "$fail" -ne 0 ]]; then
  echo "---- captured output ----" >&2
  printf '%s\n' "$out" >&2
  exit 1
fi
echo "smoke: runtime markers OK"

# 4) v0.39 + v0.40 surface markers — every demo body must hit these.
for marker in "use std.crypto" "use std.encoding" "use std.url" "use std.uuid" \
              "std.crypto.hmac_sha256(" "std.crypto.aes_gcm.encrypt(" \
              "std.crypto.aes_gcm.decrypt(" "std.crypto.random_bytes(" \
              "std.encoding.hex.encode(" "std.encoding.base64.encode_url_no_pad(" \
              "std.encoding.base64.decode_url_no_pad(" \
              "std.uuid.Uuid.v7(" "std.url.percent_encode("; do
  if ! grep -q "$marker" "$SRC"; then
    echo "smoke FAIL: missing v0.39/v0.40 surface marker: $marker" >&2
    exit 1
  fi
done
echo "smoke: surface markers OK"

# 5) Sanity-check the web fixture exists.
HTML="$DEMO/web/index.html"
[[ -f "$HTML" ]] || { echo "smoke FAIL: missing $HTML" >&2; exit 1; }
if ! grep -q "POST" "$HTML"; then
  echo "smoke FAIL: $HTML doesn't reference POST" >&2
  exit 1
fi
if ! grep -q "/login" "$HTML"; then
  echo "smoke FAIL: $HTML doesn't reference /login" >&2
  exit 1
fi

SIZE=$(wc -c < "$SRC" | tr -d ' ')
echo "12_web_auth: PASS ($SRC, $SIZE bytes, 13 v0.39/v0.40 surface markers, web fixture present)"
