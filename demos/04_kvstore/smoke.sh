#!/usr/bin/env bash
# demos/04_kvstore/smoke.sh — drive `mty run` on the demo and check
# every key milestone in the deterministic workload.
#
# The kvstore demo is fully self-contained: spawn → PUT → GET →
# DELETE → CRASH → post-crash GET → stats. We assert the JSON
# round-trip per phase so a future regression is caught even when
# the supervisor wiring lands and the post-crash shape changes
# (the wiring drop should preserve the GET hit behaviour).
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

DEMO="$ROOT/demos/04_kvstore/src/main.mty"

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

# --- boot ---
check boot                   'spawned: counter, 3 shards, coordinator, frontend'

# --- PUT round-trip across all 3 shards ---
check put_alpha_shard1       '{"shard":1,"k":"alpha","v":"1","ok":1}'
check put_bravo_shard0       '{"shard":0,"k":"bravo","v":"2","ok":1}'
check put_charlie_shard2     '{"shard":2,"k":"charlie","v":"3","ok":1}'
check put_delta_shard1       '{"shard":1,"k":"delta","v":"4","ok":1}'
check put_echo_shard0        '{"shard":0,"k":"echo","v":"5","ok":1}'
check put_foxtrot_shard2     '{"shard":2,"k":"foxtrot","v":"6","ok":1}'

# --- GET round-trip ---
check get_alpha_hit          '{"shard":1,"k":"alpha","hit":true,"v":"1"}'
check get_foxtrot_hit        '{"shard":2,"k":"foxtrot","hit":true,"v":"6"}'

# --- GET miss for unknown key ---
check miss_ghost             '{"shard":2,"k":"ghost","hit":false}'

# --- DELETE + post-delete miss ---
check del_bravo              '{"shard":0,"k":"bravo","removed":1}'
check del_then_miss          '{"shard":0,"k":"bravo","hit":false}'

# --- Crash shard 1: handler panics, agent loop traps it, mailbox
#     stays alive. The Coordinator sees the trapped reply.
check crash_panic            'panic: shard 1 crashed on purpose'
check crash_trapped          '{"crashed_shard":1,"status":"trapped"}'

# --- After the crash: other shards keep serving. Slice-7 keeps
#     shard 1's in-process state too, so alpha/delta on it are
#     still observable. With v0.12 supervisor wiring the same
#     shape will return an empty result for the post-restart
#     window; tighten the assertion when that lands.
check post_crash_alpha       '{"shard":1,"k":"alpha","hit":true,"v":"1"}'
check post_crash_charlie     '{"shard":2,"k":"charlie","hit":true,"v":"3"}'
check post_crash_delta       '{"shard":1,"k":"delta","hit":true,"v":"4"}'

# --- Telemetry counts: 6 PUTs + 11 GETs (6 initial + 1 miss +
#     3 post-crash + 1 post-delete-miss) + 1 DEL + 2 MISSes (ghost
#     + post-delete) + 1 CRASH + 1 HTTP PUT/GET/DEL each.
check stats                  '"shards":[1,2,2]'
check metrics_shape          '"metrics":{"puts":'

# --- HTTP-shaped frontend round-trip ---
check http_put               '"PUT":{"shard":1,"k":"http_key","v":"http_val","ok":1}'
check http_get               '"GET":{"shard":1,"k":"http_key","hit":true,"v":"http_val"}'
check http_del               '"DELETE":{"shard":1,"k":"http_key","removed":1}'

if [[ "$fail" -ne 0 ]]; then
  echo "---- captured output ----" >&2
  printf '%s\n' "$out" >&2
  exit 1
fi
echo "04_kvstore: PASS"
