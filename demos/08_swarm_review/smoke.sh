#!/usr/bin/env bash
# demos/08_swarm_review/smoke.sh — build + validate the v0.27 swarm-review
# demo.
#
# Two modes (matching demos 05/06/07's contract):
#
#   * Default mode: `mty check` + `mty fmt --check` + sanity check of
#     the sample snippets + the three @tool synth fns. No LLM call.
#
#   * MTY_AGENT_SMOKE=1 mode: spin up the multi-provider mock LLM stub
#     on localhost:8776, run the agent against the canned panel
#     responses, assert stdout carries the expected swarm markers
#     (`swarm_review: spawned`, `evt:reviewer:review`). Exercises the
#     whole pipeline (spawn + Working construction + run_panel_review
#     delegation + std.env.args() argv plumbing) without burning real
#     API tokens. The mock has no network egress so this works on
#     air-gapped CI.
#
# Mirrors the contract of demos 05/06/07 (parse → fmt → build/check →
# opt-in end-to-end smoke).

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi
if [[ ! -x "$MTY" ]]; then
  echo "smoke: mty not built. Run: cargo build -p mty-cli" >&2
  exit 2
fi

DEMO="$ROOT/demos/08_swarm_review"
SRC="$DEMO/src/main.mty"

# 1) mty check
"$MTY" check "$SRC" >/dev/null
echo "smoke: mty check OK"

# 2) mty fmt --check (catches drift after a manual edit)
"$MTY" fmt --check "$SRC" >/dev/null
echo "smoke: mty fmt --check OK"

# 3) Sample snippets present + non-empty.
SNIPS="$DEMO/tools/sample_snippets"
[[ -d "$SNIPS" ]] || { echo "smoke FAIL: missing $SNIPS" >&2; exit 1; }
N_SNIPS=$(find "$SNIPS" -maxdepth 1 -name "*.txt" | wc -l | tr -d ' ')
if [[ "$N_SNIPS" -lt 3 ]]; then
  echo "smoke FAIL: expected >=3 sample snippets, found $N_SNIPS" >&2
  exit 1
fi
for slug in 01_safe 02_unsafe 03_unclear; do
  if [[ ! -s "$SNIPS/$slug.txt" ]]; then
    echo "smoke FAIL: missing or empty snippet: $slug.txt" >&2
    exit 1
  fi
done

# 4) Sanity-check the demo source carries the v0.27 surface markers.
# This is the regression hook for "did anyone accidentally rip out the
# @tool decorator / the swarm call / the Working construction inside
# the handler / the std.env.args argv hop?"
for marker in "@tool(" "swarm(" "Working.new()" "std.env.args()" "Member.anthropic(" "Member.openai(" "Member.gemini(" "DollarBudget.from_dollars(" "ConsensusStrategy.Majority"; do
  if ! grep -q "$marker" "$SRC"; then
    echo "smoke FAIL: missing v0.27 surface marker: $marker" >&2
    exit 1
  fi
done

SIZE=$(wc -c < "$SRC" | tr -d ' ')
LOC=$(wc -l < "$SRC" | tr -d ' ')
echo "smoke OK: $SRC ($SIZE bytes / $LOC LOC, $N_SNIPS sample snippets, 9 v0.27 surface markers)"

# 5) OPTIONAL mock-LLM end-to-end smoke. Opt in via MTY_AGENT_SMOKE=1.
# Spawns the three-route mock LLM stub on localhost:8776, runs the
# agent against the canned panel, asserts the swarm spawn + handler
# fired. As of v0.27 the SIR interpreter dispatches `swarm(...)` as a
# permissive extern (returns `Value::Unit`) — the live HTTP path
# fires only on `mty build --target host`. The smoke therefore
# validates the pipeline shape, not the wire-level LLM round-trip
# (the latter is covered by the v0.27 Track D tests in
# `crates/mty-stdlib/tests/swarm_*.rs`).
if [[ "${MTY_AGENT_SMOKE:-0}" == "1" ]]; then
  echo "smoke: MTY_AGENT_SMOKE=1 — running mock-LLM end-to-end stage"
  PY=""
  for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1 \
        && "$candidate" --version >/dev/null 2>&1; then
      PY="$candidate"
      break
    fi
  done
  if [[ -z "$PY" ]]; then
    echo "smoke: (mock-LLM smoke skipped: no working python on PATH)"
    exit 0
  fi

  PORT="${MTY_AGENT_SMOKE_PORT:-8776}"
  LOG="$DEMO/target/mock_llm.log"
  RUN_LOG="$DEMO/target/run.log"
  mkdir -p "$DEMO/target"
  PORT="$PORT" "$PY" "$DEMO/tools/mock_llm/server.py" \
      >"$LOG" 2>&1 &
  STUB_PID=$!
  cleanup() {
    kill "$STUB_PID" 2>/dev/null || true
    wait "$STUB_PID" 2>/dev/null || true
  }
  trap cleanup EXIT

  # Wait for the stub to listen. The script logs the moment it binds;
  # poll the log up to ~5s.
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    if grep -q "listening on" "$LOG" 2>/dev/null; then break; fi
    sleep 0.5
  done
  if ! grep -q "listening on" "$LOG" 2>/dev/null; then
    echo "smoke FAIL: mock LLM did not come up on :$PORT" >&2
    cat "$LOG" >&2 || true
    exit 1
  fi

  # Point every provider client at the local mock. The v0.27 clients
  # don't currently read these env vars in `from_env` (they use the
  # hard-coded base URLs); the v0.28 follow-up wires them up. Until
  # then the env vars are forward-compat sugar — the smoke test
  # passes regardless because the SIR interpreter doesn't fire the
  # HTTP path. The presence of the marker keeps the smoke shape
  # ready for the v0.28 wiring.
  ANTHROPIC_API_KEY="sk-ant-mocktoken" \
  ANTHROPIC_BASE_URL="http://127.0.0.1:$PORT" \
  OPENAI_API_KEY="sk-mocktoken" \
  OPENAI_BASE_URL="http://127.0.0.1:$PORT" \
  GEMINI_API_KEY="mock-gemini-key" \
  GEMINI_BASE_URL="http://127.0.0.1:$PORT" \
      "$MTY" run "$SRC" -- "let x = eval(user_input)" >"$RUN_LOG" 2>&1 || true

  ok=true
  for marker in "evt:reviewer:review" "swarm_review: report follows"; do
    if ! grep -q "$marker" "$RUN_LOG"; then
      echo "smoke FAIL: marker missing from run.log: $marker" >&2
      ok=false
    fi
  done
  if $ok; then
    echo "smoke OK: mock-LLM pipeline markers present in run.log"
  else
    echo "--- run.log ---" >&2
    tail -40 "$RUN_LOG" >&2 || true
    echo "--- mock_llm.log ---" >&2
    tail -20 "$LOG" >&2 || true
    exit 1
  fi
fi
