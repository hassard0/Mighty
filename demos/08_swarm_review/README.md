# Demo 08 — Multi-Agent Code-Review Swarm

A ~200-LOC Mighty program that asks a panel of three LLMs (Claude
/ GPT-5 / Gemini) whether a code snippet is safe, returns the
majority verdict with dissents, and respects a 50-cent shared
spending cap. The forcing-function demo for Mighty's v0.27 LLM
stack.

## What this demonstrates

| Surface | What this demo does |
|---|---|
| `@tool` source-level decorator | `@tool("desc", cap: fs.read) fn read_snippet(path)` — full parser hook in v0.27 Track A; macro-expands into `__tool_descriptor_*` / `__tool_invoke_*` / `__tool_register_*` companion fns. |
| Opaque-ADT handler scope | `Working.new()` and other std opaque ADTs constructible directly inside `CodeReviewer.on Review` without tripping MT2021 (v0.27 Track B). |
| Full LLM providers | `OpenAiClient` / `GeminiClient` / `BedrockClient` promoted from v0.26 stubs to full impls — auth + streaming + tool-use + structured outputs + budget short-circuit (v0.27 Track C). |
| `std.swarm` consensus primitive | `swarm(prompt, panel, budget, strategy)` returns a `Consensus` with `majority` + `dissents` + `budget_exhausted`. Four strategies: `Majority` / `Unanimous` / `WeightedVote` / `FirstAgreed` (v0.27 Track D). |
| `mty run -- <argv>` plumbing | Trailing positionals after `--` reach Mighty source as `std.env.args()` (`List[Str]`) (v0.27 Track E). |
| Mixed-provider panel | `Member.anthropic` + `Member.openai` + `Member.gemini` constructed in one panel; each pays into the shared `DollarBudget`. |

Brought to its current shape by **v0.27 (Tracks A–E)**. The
four v0.28 follow-ups (`BuiltinId::Swarm` interpreter arm,
handler-safe carve-out for swarm ADTs, bang-send return-type
lowering for typed protocols, per-provider base-URL env vars on
`from_env`) all closed in v0.29 — see the `v0.28 follow-ups`
section below for the live status.

## Run

### Default (no LLM calls — exercises parse / check / run)

```bash
$ mty check demos/08_swarm_review/src/main.mty
ok: demos/08_swarm_review/src/main.mty

$ mty run demos/08_swarm_review/src/main.mty -- "fn double(x: i32) -> i32 { x * 2 }"
evt:reviewer:review
swarm_review: report follows
```

Under `mty run` the v0.29 `BuiltinId::Swarm` interpreter arm fires
the typed builtin path — the source compiles cleanly and the swarm
call returns a permissive `Consensus` from the host shim. No real
LLM calls fire until you swap to a real key on `mty build --target
host`.

### Live three-provider run (needs all three API keys)

```bash
$ ANTHROPIC_API_KEY=sk-ant-... \
  OPENAI_API_KEY=sk-... \
  GEMINI_API_KEY=... \
    mty build --target host demos/08_swarm_review/src/main.mty
$ ./out/main "let x = std.fs.read(\"/etc/passwd\")"
verdict: UNSAFE
cost cents: 12
dissents: 1
  - dissent: gemini:gemini-2.5-pro UNCLEAR ...
```

## Sample snippets

`tools/sample_snippets/` ships three short snippets with known
verdicts:

| File | Expected verdict | Why |
|---|---|---|
| `01_safe.txt` | SAFE | Pure arithmetic helper; no I/O, no user input. |
| `02_unsafe.txt` | UNSAFE | Forwards untrusted stdin into `os.system` (command injection). |
| `03_unclear.txt` | UNCLEAR | File read with a user-controlled path — depends on the caller's validation step. |

## Smoke test

`smoke.sh` mirrors demos 05/06/07's contract:

```bash
# Default mode: parse + check + fmt + sanity-check sample snippets + v0.27 markers
$ bash demos/08_swarm_review/smoke.sh
smoke OK: ... main.mty (... bytes / ... LOC, 3 sample snippets, 9 v0.27 surface markers)

# Mock-LLM mode: spin up a single Python process serving all three
# provider routes on one port, run the agent against it, assert the
# pipeline markers fired.
$ MTY_AGENT_SMOKE=1 bash demos/08_swarm_review/smoke.sh
smoke: MTY_AGENT_SMOKE=1 — running mock-LLM end-to-end stage
smoke OK: mock-LLM pipeline markers present in run.log
```

The mock LLM (`tools/mock_llm/server.py`) is stdlib-only Python
that distinguishes between provider request shapes by URL path:

- `POST /v1/messages` → Anthropic Messages API
- `POST /v1/responses` → OpenAI Responses API
- `POST /v1beta/models/<m>:generateContent` → Google Gemini

Each route returns a canned reply tuned so the swarm consensus on
the same prompt lands on `SAFE` (Anthropic + OpenAI agree, Gemini
dissents with UNCLEAR) — which is what `smoke.sh` asserts.

## Files

```
demos/08_swarm_review/
├── README.md                              ← this file
├── mighty.toml
├── smoke.sh
├── src/main.mty                           ← 216 LOC
└── tools/
    ├── mock_llm/server.py                 ← three-route mock LLM
    └── sample_snippets/
        ├── 01_safe.txt
        ├── 02_unsafe.txt
        └── 03_unclear.txt
```

## v0.28 / v0.29 follow-ups (status)

The four follow-ups this demo surfaced in v0.27 all closed:

1. **`BuiltinId::Swarm` interpreter arm.** Closed in v0.29
   Track A. `mty run` now fires the typed builtin path.
2. **Handler-safe carve-out for swarm ADTs.** Closed in v0.29
   Track B. `ConsensusStrategy`, `Member`, `DollarBudget`,
   `Consensus` are all on the handler-safe list.
3. **Bang-send return-type lowering.** Closed in v0.29 Track C.
   `let r: Str = reviewer ! Review(snippet)` types `r` as `Str`.
4. **Per-provider base-URL env vars on `from_env`.** Closed in
   v0.29 — `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL` /
   `GEMINI_BASE_URL` consulted in every `from_env` ctor.

Demo 09 builds on every one of these to span the swarm across two
cluster nodes.
