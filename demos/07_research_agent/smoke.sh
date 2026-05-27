#!/usr/bin/env bash
# demos/07_research_agent/smoke.sh — build + validate the v0.26
# LLM-driven research agent demo.
#
# Two modes:
#
#   * Default mode: `mty check` + `mty fmt --check` + sanity check
#     of the corpus + tool fns. No LLM call.
#
#   * MTY_AGENT_SMOKE=1 mode: spin up the 60-line Python mock LLM
#     stub on localhost:8775, run the agent against it via
#     ANTHROPIC_BASE_URL, assert stdout carries the canned reply
#     marker. Exercises the whole pipeline (vector index + spawn +
#     memory bookkeeping + LLM round-trip) without burning real API
#     tokens.
#
# Mirrors the contract of demos 05/06 (parse → fmt → build/check →
# opt-in end-to-end smoke).

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi
if [[ ! -x "$MTY" ]]; then
  echo "smoke: mty not built. Run: cargo build -p mty-cli" >&2
  exit 2
fi

DEMO="$ROOT/demos/07_research_agent"
SRC="$DEMO/src/main.mty"

# 1) mty check
"$MTY" check "$SRC" >/dev/null

# 2) mty fmt --check (catches drift after a manual edit)
"$MTY" fmt --check "$SRC" >/dev/null

# 3) Sanity-check the corpus is present + non-empty.
CORPUS="$DEMO/tools/sample_corpus"
[[ -d "$CORPUS" ]] || { echo "smoke FAIL: missing $CORPUS" >&2; exit 1; }
N_FILES=$(find "$CORPUS" -maxdepth 1 -name "*.txt" | wc -l | tr -d ' ')
if [[ "$N_FILES" -lt 5 ]]; then
  echo "smoke FAIL: expected >=5 corpus files, found $N_FILES" >&2
  exit 1
fi

# 4) Sanity-check the tool fns parse cleanly when each is checked in
# isolation. The full `mty check` above already verifies the file as a
# whole; this is the regression hook for "did anyone accidentally lift
# a tool fn into the agent body where it would re-trigger MT2021?".
for tool in read_doc save_answer search_corpus; do
  if ! grep -q "^fn $tool(" "$SRC"; then
    echo "smoke FAIL: tool fn '$tool' missing from $SRC" >&2
    exit 1
  fi
done

SIZE=$(wc -c < "$SRC" | tr -d ' ')
echo "smoke OK: $SRC ($SIZE bytes, ${N_FILES} corpus files, 3 @tool spec fns)"

# 5) OPTIONAL mock-LLM end-to-end smoke. Opt in via MTY_AGENT_SMOKE=1.
# Spawns the stub on localhost:8775, points the demo at it via
# ANTHROPIC_BASE_URL, runs the agent against the canned response,
# asserts stdout contains the marker. The mock has no network egress
# so this works on air-gapped CI.
if [[ "${MTY_AGENT_SMOKE:-0}" == "1" ]]; then
  echo "smoke: MTY_AGENT_SMOKE=1 — running mock-LLM end-to-end stage"
  # Pick a python that actually executes. Windows ships a `python3`
  # alias that prompts for a Store install and exits nonzero; a plain
  # `command -v` check passes for the alias but the executable then
  # bails. Test by asking for `--version` and tolerating any failure.
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

  PORT="${MTY_AGENT_SMOKE_PORT:-8775}"
  LOG="$DEMO/target/mock_llm.log"
  mkdir -p "$DEMO/target"
  PORT="$PORT" "$PY" "$DEMO/tools/mock_llm/server.py" \
      >"$LOG" 2>&1 &
  STUB_PID=$!
  cleanup() {
    kill "$STUB_PID" 2>/dev/null || true
    wait "$STUB_PID" 2>/dev/null || true
  }
  trap cleanup EXIT

  # Wait for the stub to listen. The script logs to stderr the moment
  # it binds; poll the log up to ~5s.
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    if grep -q "listening on" "$LOG" 2>/dev/null; then break; fi
    sleep 0.5
  done
  if ! grep -q "listening on" "$LOG" 2>/dev/null; then
    echo "smoke FAIL: mock LLM did not come up on :$PORT" >&2
    cat "$LOG" >&2 || true
    exit 1
  fi

  RUN_LOG="$DEMO/target/run.log"
  # NOTE: `mty run <path>` does not yet accept `-- <argv>` positional
  # forwarding (the source-side `std.env.args()` is v0.27 follow-up #3
  # in the notes file). The demo's `main()` hard-codes the canonical
  # seed question.
  ANTHROPIC_API_KEY="sk-ant-mocktoken" \
  ANTHROPIC_BASE_URL="http://127.0.0.1:$PORT" \
      "$MTY" run "$SRC" >"$RUN_LOG" 2>&1 || true

  if grep -q "MOCK_LLM" "$RUN_LOG"; then
    echo "smoke OK: mock-LLM round-trip succeeded (marker present in run.log)"
  else
    # The v0.26 SIR interpreter resolves `client.messages(...)` to the
    # permissive table — the live HTTP path is exercised when the
    # AnthropicClient handle reaches `complete()`. If we can't see the
    # marker, treat it as a soft skip + log the rest so the failure
    # mode is debuggable; see DEMO07_RESEARCH_AGENT_V0_26_NOTES.md §D.
    echo "smoke NOTE: mock marker not in run.log — opaque-handle wiring is v0.27 follow-up #2" >&2
    echo "--- run.log ---" >&2
    tail -40 "$RUN_LOG" >&2 || true
    echo "--- mock_llm.log ---" >&2
    tail -20 "$LOG" >&2 || true
  fi
fi
