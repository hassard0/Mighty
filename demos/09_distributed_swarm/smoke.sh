#!/usr/bin/env bash
# demos/09_distributed_swarm/smoke.sh — v0.29 forcing-function demo
# for the distributed swarm surface. Two modes:
#
#   * Default mode: `mty check` + `mty fmt --check` on both files +
#     sanity-check that the demo carries every v0.29 surface marker.
#     No cluster wiring, no LLM calls.
#
#   * MTY_CLUSTER_SMOKE=1 mode: spin up the node-B sibling under
#     `MTY_NODE_ID=node-b` in the background, then run the node-A
#     reviewer under `MTY_NODE_ID=node-a` against the canonical
#     snippet, assert both ends fired their handlers.
#
# Mirrors the contract of demos 05/06/07/08 (parse → fmt → markers →
# opt-in end-to-end smoke).

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi
if [[ ! -x "$MTY" ]]; then
  echo "smoke: mty not built. Run: cargo build -p mty-cli" >&2
  exit 2
fi

DEMO="$ROOT/demos/09_distributed_swarm"
MAIN="$DEMO/src/main.mty"
SIB="$DEMO/src/sibling.mty"

# 1) mty check both files.
"$MTY" check "$MAIN" >/dev/null
echo "smoke: mty check main OK"
"$MTY" check "$SIB" >/dev/null
echo "smoke: mty check sibling OK"

# 2) mty fmt --check both files (catches drift after a manual edit).
"$MTY" fmt --check "$MAIN" >/dev/null
echo "smoke: mty fmt --check main OK"
"$MTY" fmt --check "$SIB" >/dev/null
echo "smoke: mty fmt --check sibling OK"

# 3) v0.29 surface markers on the node-A reviewer.
for marker in "swarm(" "Member.anthropic(" "Member.openai(" "Member.gemini(" \
              "DollarBudget.from_dollars(" "ConsensusStrategy.Majority" \
              "spawn Sibling()" "sibling ! Review(" "let budget" \
              "let sibling_verdict: Str ="; do
  if ! grep -q "$marker" "$MAIN"; then
    echo "smoke FAIL: missing v0.29 surface marker in main.mty: $marker" >&2
    exit 1
  fi
done

# 4) v0.29 surface markers on the node-B sibling.
for marker in "swarm(" "DollarBudget.from_dollars(" "ConsensusStrategy.Majority" \
              "let budget" "agent Sibling:"; do
  if ! grep -q "$marker" "$SIB"; then
    echo "smoke FAIL: missing v0.29 surface marker in sibling.mty: $marker" >&2
    exit 1
  fi
done

MAIN_SIZE=$(wc -c < "$MAIN" | tr -d ' ')
SIB_SIZE=$(wc -c < "$SIB" | tr -d ' ')
echo "smoke OK: main.mty ($MAIN_SIZE bytes) + sibling.mty ($SIB_SIZE bytes), 10 + 5 v0.29 surface markers"

# 5) OPTIONAL real two-process cluster smoke. Opt in via
# MTY_CLUSTER_SMOKE=1. Spawns the node-B sibling under
# `MTY_NODE_ID=node-b` in the background, then runs the node-A
# reviewer under `MTY_NODE_ID=node-a` against the canonical snippet,
# asserts both processes fired their handlers.
if [[ "${MTY_CLUSTER_SMOKE:-0}" == "1" ]]; then
  echo "smoke: MTY_CLUSTER_SMOKE=1 — running two-process cluster stage"
  mkdir -p "$DEMO/target"
  SIB_LOG="$DEMO/target/sibling.log"
  MAIN_LOG="$DEMO/target/main.log"

  MTY_NODE_ID=node-b "$MTY" run "$SIB" >"$SIB_LOG" 2>&1 &
  SIB_PID=$!
  cleanup() {
    kill "$SIB_PID" 2>/dev/null || true
    wait "$SIB_PID" 2>/dev/null || true
  }
  trap cleanup EXIT

  # Wait for the sibling to advertise its mesh listener.
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    if grep -q "sibling: listening on node-b" "$SIB_LOG" 2>/dev/null; then break; fi
    sleep 0.5
  done
  if ! grep -q "sibling: listening on node-b" "$SIB_LOG" 2>/dev/null; then
    echo "smoke FAIL: sibling did not come up on node-b" >&2
    cat "$SIB_LOG" >&2 || true
    exit 1
  fi

  MTY_NODE_ID=node-a "$MTY" run "$MAIN" -- "let x = eval(user_input)" \
      >"$MAIN_LOG" 2>&1 || true

  ok=true
  for marker in "evt:reviewer:review" "evt:reviewer:joined" \
                "distributed_swarm: joined consensus follows"; do
    if ! grep -q "$marker" "$MAIN_LOG"; then
      echo "smoke FAIL: marker missing from main.log: $marker" >&2
      ok=false
    fi
  done
  if ! grep -q "evt:sibling:review" "$SIB_LOG"; then
    echo "smoke FAIL: sibling never received the cluster hop (no evt:sibling:review in sibling.log)" >&2
    ok=false
  fi
  if $ok; then
    echo "smoke OK: cluster two-process pipeline markers present"
  else
    echo "--- main.log ---" >&2
    tail -40 "$MAIN_LOG" >&2 || true
    echo "--- sibling.log ---" >&2
    tail -40 "$SIB_LOG" >&2 || true
    exit 1
  fi
fi
