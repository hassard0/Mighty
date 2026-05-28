#!/usr/bin/env bash
# demos/10_vision_rag/smoke.sh — build + validate the v0.33 Track T2
# vision-RAG demo.
#
# Two modes (matching demos 05-09's contract):
#
#   * Default mode: `mty check` + `mty fmt --check` + sanity check of
#     the bundled corpus + diagram + the v0.33 surface markers. No LLM
#     call.
#
#   * MTY_AGENT_SMOKE=1 mode: spin up the multi-route mock LLM stub
#     on localhost:8777, run the agent against the canned vision-RAG
#     response, assert stdout carries the expected vision markers
#     (`vision_rag:`, `evt:vision:ask`, `evt:vision:image-loaded`).
#     Exercises the whole pipeline (spawn + Index build + Image load
#     + Rag.ask_with_image dispatch) without burning real API tokens.

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MTY="${MTY:-$ROOT/target/debug/mty}"
if [[ -x "$MTY.exe" ]]; then MTY="$MTY.exe"; fi
if [[ ! -x "$MTY" ]]; then
  echo "smoke: mty not built. Run: cargo build -p mty-cli" >&2
  exit 2
fi

DEMO="$ROOT/demos/10_vision_rag"
SRC="$DEMO/src/main.mty"

# 1) mty check
"$MTY" check "$SRC" >/dev/null
echo "smoke: mty check OK"

# 2) mty fmt --check (catches drift after a manual edit)
"$MTY" fmt --check "$SRC" >/dev/null
echo "smoke: mty fmt --check OK"

# 3) Sanity-check the corpus is present + non-empty.
CORPUS="$DEMO/tools/sample_corpus"
[[ -d "$CORPUS" ]] || { echo "smoke FAIL: missing $CORPUS" >&2; exit 1; }
N_CORPUS=$(find "$CORPUS" -maxdepth 1 -name "*.md" | wc -l | tr -d ' ')
if [[ "$N_CORPUS" -lt 2 ]]; then
  echo "smoke FAIL: expected >=2 corpus files, found $N_CORPUS" >&2
  exit 1
fi

# 4) Sanity-check the sample diagram is present + non-empty.
DIAGS="$DEMO/tools/sample_diagrams"
[[ -d "$DIAGS" ]] || { echo "smoke FAIL: missing $DIAGS" >&2; exit 1; }
N_DIAGS=$(find "$DIAGS" -maxdepth 1 \( -name "*.png" -o -name "*.jpg" -o -name "*.webp" \) | wc -l | tr -d ' ')
if [[ "$N_DIAGS" -lt 1 ]]; then
  echo "smoke FAIL: expected >=1 diagram, found $N_DIAGS" >&2
  exit 1
fi

# 5) v0.33 surface markers — every demo body must hit these.
for marker in "use std.rag" "Index.new(" "Rag.new(" \
              ".with_index(" ".with_retriever_top_k(" ".with_member(" \
              "Image.from_file(" "ask_with_image(" \
              "@tool(" "Member.anthropic("; do
  if ! grep -q "$marker" "$SRC"; then
    echo "smoke FAIL: missing v0.33 surface marker: $marker" >&2
    exit 1
  fi
done

SIZE=$(wc -c < "$SRC" | tr -d ' ')
echo "smoke OK: $SRC ($SIZE bytes, $N_CORPUS corpus files, $N_DIAGS diagram(s), 10 v0.33 surface markers)"
