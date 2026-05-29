#!/usr/bin/env bash
# demos/11_ffi_winit_stub/smoke.sh — opt-in FFI smoke test.
#
# Gated on MTY_FFI_SMOKE=1 so CI doesn't try to compile + link C
# without a C compiler on PATH. The smoke:
#   1. Compiles vendor/winit_shim.c -> vendor/libwinit_shim.a
#   2. Runs `mty build src/main.mty --release`
#   3. Executes the produced binary
#   4. Asserts the shim's stderr marker lines appear
#
# Exit 0 = pass, non-zero = fail (or skip when gate is off).

set -euo pipefail

if [[ "${MTY_FFI_SMOKE:-0}" != "1" ]]; then
  echo "demo 11 FFI smoke: skipped (set MTY_FFI_SMOKE=1 to run)"
  exit 0
fi

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DEMO="$ROOT/demos/11_ffi_winit_stub"
MTY="${MTY:-$ROOT/target/release/mty}"
if [[ ! -x "$MTY" && ! -x "$MTY.exe" ]]; then
  echo "smoke: mty binary not found at $MTY" >&2
  echo "        build it with: cargo build -p mty-cli --release" >&2
  exit 2
fi
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi

CC="${CC:-cc}"
AR="${AR:-ar}"
if ! command -v "$CC" >/dev/null; then
  echo "smoke: $CC not on PATH" >&2
  exit 2
fi
if ! command -v "$AR" >/dev/null; then
  echo "smoke: $AR not on PATH" >&2
  exit 2
fi

# 1. Compile the shim
"$CC" -c -O0 -fPIC "$DEMO/vendor/winit_shim.c" -o "$DEMO/vendor/winit_shim.o"
"$AR" rcs "$DEMO/vendor/libwinit_shim.a" "$DEMO/vendor/winit_shim.o"

# 2. Build the demo. The manifest's [[extern_lib]] entry threads
# vendor/libwinit_shim.a onto the linker command.
out="$("$MTY" build "$DEMO/src/main.mty" --release --out-dir "$DEMO/target" 2>&1)"
echo "$out"

# 3. Run it
BIN="$DEMO/target/main"
if [[ -x "$BIN.exe" ]]; then BIN="$BIN.exe"; fi
runout="$("$BIN" 2>&1 || true)"
echo "--- binary output ---"
echo "$runout"

# 4. Assert markers
fail=0
for marker in "winit_shim_init: stub ok" "winit_shim_open_window: 640x480" "winit_shim_shutdown: stub ok"; do
  if ! grep -F -q "$marker" <<<"$runout"; then
    echo "smoke FAIL: expected marker missing: $marker" >&2
    fail=1
  fi
done

if [[ $fail -eq 0 ]]; then
  echo "demo 11 FFI smoke: PASS"
fi
exit $fail
