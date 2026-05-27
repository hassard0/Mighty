#!/usr/bin/env bash
# Open the 8 v1.0 RFC public comment-window discussion threads on
# hassard0/Mighty. Uses the existing "Ideas" category (the closest
# semantic fit for RFC proposals — Discussion-category creation is
# UI-only via the API).
#
# Idempotent? No. Running this twice creates duplicate threads. Run
# once; if a thread fails mid-way, edit this script to skip the
# already-opened ones.

set -euo pipefail

REPO_ID="R_kgDOSmOkKQ"
CAT_ID="DIC_kwDOSmOkKc4C97AE"   # Ideas

create_discussion() {
  local title="$1"
  local body="$2"
  # Python is on every Windows + Linux runner; jq isn't always.
  local payload
  payload=$(python -c "
import json, sys
print(json.dumps({
  'query': 'mutation(\$r:ID!,\$c:ID!,\$t:String!,\$b:String!){createDiscussion(input:{repositoryId:\$r,categoryId:\$c,title:\$t,body:\$b}){discussion{number url}}}',
  'variables': {'r': sys.argv[1], 'c': sys.argv[2], 't': sys.argv[3], 'b': sys.argv[4]},
}))
" "$REPO_ID" "$CAT_ID" "$title" "$body")
  echo "$payload" | gh api graphql --input - | python -c "
import json, sys
d = json.load(sys.stdin)
if 'errors' in d:
  print('ERROR:', d['errors'][0]['message'])
elif d.get('data', {}).get('createDiscussion'):
  print(d['data']['createDiscussion']['discussion']['url'])
else:
  print('UNKNOWN:', d)
"
}

open_rfc() {
  local id="$1"
  local title="$2"
  local closes="$3"
  local window="$4"
  local body
  body=$(cat <<EOF
**Public comment window** for RFC-${id} is open through **${closes}** (${window}-day window, opened 2026-05-26).

This thread is the canonical channel for feedback on RFC-${id} before it can be normative-accepted into \`docs/spec/v1.0-rc.md\` for the v1.0 freeze.

## How to participate

Reply with one of:

- ✅ **Accept** — RFC ships as written
- ✏️  **Modify** — suggest a specific change (quote the section + propose the alternative)
- ❌ **Reject** — explain why this RFC shouldn't ship in v1.0
- ❓ **Question / concern** — flag an ambiguity or gap; doesn't block acceptance but seeds polish

Detailed technical disagreements are best filed as inline comments on the RFC file via a PR; this thread is for the higher-level accept/modify/reject signal.

## Reading the RFC

- **File:** [\`docs/spec/rfcs/RFC-${id}.md\`](https://github.com/hassard0/Mighty/blob/main/docs/spec/rfcs/RFC-${id}.md)
- **Window tracker:** [\`docs/spec/rfcs/COMMENT_WINDOWS.md\`](https://github.com/hassard0/Mighty/blob/main/docs/spec/rfcs/COMMENT_WINDOWS.md)
- **Live dashboard:** [\`docs/spec/rfcs/RFC_DASHBOARD.md\`](https://github.com/hassard0/Mighty/blob/main/docs/spec/rfcs/RFC_DASHBOARD.md)
- **Implementation status** is recorded in the RFC file's header — several of these RFCs cover work that has already shipped pre-v1.0 (RFC-006 live migration in v0.21, RFC-008 effect rows in v0.13–v0.19, RFC-009 set-of-scopes in v0.13–v0.15). The comment window is procedural ratification for those + design feedback for the still-forward-looking RFCs (RFC-001..005).

## Closing the window

On ${closes} this window closes. After that, an integrator collects every reply, dispositions accept / modify / reject in the RFC file's footer, and updates \`COMMENT_WINDOWS.md\` + \`RFC_DASHBOARD.md\`. Threads stay open after that — they just stop being load-bearing for the v1.0 freeze gate.
EOF
)
  local thread_title="[RFC-${id}] ${title} — public comment window (closes ${closes})"
  echo "Opening RFC-${id}…"
  create_discussion "$thread_title" "$body"
}

open_rfc "001" "First-class union ADTs"                "2026-06-25" "30"
open_rfc "002" "Wasm Component Model wrapper"          "2026-07-25" "60"
open_rfc "003" "Sandboxed proc-macro execution"        "2026-06-25" "30"
open_rfc "004" "Per-call FsCap manifest materialisation" "2026-06-25" "30"
open_rfc "005" "Agent affinity front-end syntax"       "2026-06-09" "14"
open_rfc "006" "Lossless live agent migration"         "2026-07-25" "60"
open_rfc "008" "Effect row polymorphism"               "2026-06-25" "30"
open_rfc "009" "Set-of-scopes macro hygiene"           "2026-06-25" "30"
