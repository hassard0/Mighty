#!/usr/bin/env bash
# demos/13_rag_regex/smoke.sh — build + validate the v0.40 T4 RAG-with-regex demo.
#
# Default mode:
#   * `mty check` + `mty fmt --check` on src/main.mty.
#   * `mty run` the regex-augmented RAG pipeline and assert every
#     event marker (`evt:rag:ask`, `evt:rag:index-built`,
#     `evt:rag:pipeline-ready`, `evt:rag:date-filter`,
#     `evt:rag:answered`) plus the final summary line.
#   * Grep the source for the v0.40 T4 regex surface markers
#     (`std.regex.Regex.new(`, `.find_all(`, `.captures_all(`,
#     `.is_match(`, `.replace_all(`) so a future refactor that drops
#     a primitive trips the smoke.
#   * Grep the source for the v0.33 RAG surface markers
#     (`Index.new(`, `Rag.new(`, `Member.anthropic(`, ...) so the
#     pipeline cannot silently regress to a regex-only demo.
#   * Sanity-check the bundled corpus is present + non-trivial.
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

DEMO="$ROOT/demos/13_rag_regex"
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
check ask             'evt:rag:ask'
check index-built     'evt:rag:index-built'
check pipeline-ready  'evt:rag:pipeline-ready'
check date-filter     'evt:rag:date-filter'
check answered        'evt:rag:answered'
check final           'rag_regex: pipeline OK'
if [[ "$fail" -ne 0 ]]; then
  echo "---- captured output ----" >&2
  printf '%s\n' "$out" >&2
  exit 1
fi
echo "smoke: runtime markers OK"

# 4) v0.40 T4 regex surface markers.
for marker in "use std.regex" "std.regex.Regex.new(" \
              ".find_all(" ".captures_all(" \
              ".is_match(" ".replace_all("; do
  if ! grep -q "$marker" "$SRC"; then
    echo "smoke FAIL: missing v0.40 T4 regex surface marker: $marker" >&2
    exit 1
  fi
done
echo "smoke: regex surface markers OK"

# 5) v0.33 RAG surface markers — demo 13 extends, not replaces.
for marker in "use std.rag" "Index.new(" "Rag.new(" \
              ".with_index(" ".with_retriever_top_k(" \
              ".with_member(" "Member.anthropic("; do
  if ! grep -q "$marker" "$SRC"; then
    echo "smoke FAIL: missing v0.33 RAG surface marker: $marker" >&2
    exit 1
  fi
done
echo "smoke: RAG surface markers OK"

# 6) Corpus sanity-check.
CORPUS="$DEMO/corpus"
[[ -d "$CORPUS" ]] || { echo "smoke FAIL: missing $CORPUS" >&2; exit 1; }
N_CORPUS=$(find "$CORPUS" -maxdepth 1 -name "*.md" | wc -l | tr -d ' ')
if [[ "$N_CORPUS" -lt 3 ]]; then
  echo "smoke FAIL: expected >=3 corpus files, found $N_CORPUS" >&2
  exit 1
fi

# 7) At least one corpus doc should have a 2026 date so the date-
#    filter recipe has a hit; at least one should NOT so the filter
#    has something to reject. (Validates the corpus fixture matches
#    the demo body's pre-filter rationale.)
if ! grep -lq "2026-" "$CORPUS"/*.md; then
  echo "smoke FAIL: no corpus doc has a 2026- date — the date filter has nothing to accept" >&2
  exit 1
fi
if ! grep -lq "2025-" "$CORPUS"/*.md; then
  echo "smoke FAIL: no corpus doc has a 2025- date — the date filter has nothing to reject" >&2
  exit 1
fi

SIZE=$(wc -c < "$SRC" | tr -d ' ')
echo "13_rag_regex: PASS ($SRC, $SIZE bytes, $N_CORPUS corpus files, 6 regex + 7 RAG surface markers)"
