# demo 08 — multi-agent code-review swarm

A ~200-LOC Mighty program that asks a panel of three LLMs (Claude /
GPT-5 / Gemini) whether a code snippet is safe, returns the majority
verdict with dissents, and respects a 50-cent shared spending cap.

This is the v0.27 forcing-function demo — every one of v0.27's five
shipped tracks gets exercised end-to-end in the same file:

| Track | What it ships | How this demo consumes it |
|---|---|---|
| **A** — `@tool(...)` decorator parser | source-level `@tool("desc", cap: fs.read)` on user fns; macro-expands into `__tool_descriptor_*` / `__tool_invoke_*` / `__tool_register_*` companion fns | `read_snippet(path)` carries the decorator; `mty check` exits 0 |
| **B** — opaque-ADT handler scope | std.* opaque ADTs (`Working` / `VectorStore` / `Episodic` / `AnthropicClient` / …) constructible inside strict handler scope without tripping MT2021 | `CodeReviewer.on Review` constructs `Working.new()` inline |
| **C** — full LLM providers | `OpenAiClient` / `GeminiClient` / `BedrockClient` promoted from v0.26 stubs to full impls (auth + streaming + tool-use + structured outputs + budget short-circuit) | `Member.anthropic` + `Member.openai` + `Member.gemini` build a mixed-provider panel |
| **D** — `std.swarm` consensus primitive | `swarm(prompt, panel, budget, strategy)` returns a `Consensus` with `majority` + `dissents` + `budget_exhausted`; four strategies (Majority / Unanimous / WeightedVote / FirstAgreed) | `run_panel_review` calls `swarm(..., ConsensusStrategy.Majority)` and renders the result |
| **E** — `mty run -- <argv>` plumbing | trailing positionals after `--` reach Mighty source as `std.env.args()` (`List[Str]`) | `argv.get(0).unwrap_or(...)` picks up the snippet text on the command line |

## Run

Default (no LLM calls — exercises the parse/check/run pipeline):

```bash
$ mty check demos/08_swarm_review/src/main.mty
ok: demos/08_swarm_review/src/main.mty

$ mty run demos/08_swarm_review/src/main.mty -- "fn double(x: i32) -> i32 { x * 2 }"
evt:reviewer:review
swarm_review: report follows
```

The SIR interpreter dispatches `swarm(...)` as a permissive extern call
(it returns `Value::Unit` from the host shim) — no real LLM calls fire
on `mty run` in v0.27. The forcing-function purpose is the source-side
shape; the live wire-up moves to `mty build --target host` once the
v0.28 `BuiltinId::Swarm` arm lands.

Live three-provider run (needs all three API keys):

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

`tools/sample_snippets/` ships three short snippets with known verdicts:

| File | Expected verdict | Why |
|---|---|---|
| `01_safe.txt` | SAFE | Pure arithmetic helper; no I/O, no user input |
| `02_unsafe.txt` | UNSAFE | Forwards untrusted stdin into `os.system` (command injection) |
| `03_unclear.txt` | UNCLEAR | File read with a user-controlled path — depends on the caller's validation step |

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

The mock LLM (`tools/mock_llm/server.py`) is stdlib-only Python that
distinguishes between provider request shapes by URL path:

- `POST /v1/messages` → Anthropic Messages API
- `POST /v1/responses` → OpenAI Responses API
- `POST /v1beta/models/<m>:generateContent` → Google Gemini

Each route returns a canned reply tuned so the swarm consensus on the
same prompt lands on `SAFE` (Anthropic + OpenAI agree, Gemini dissents
with UNCLEAR) — which is what `smoke.sh` asserts.

## Files

```
demos/08_swarm_review/
├── README.md                              ← this file
├── mighty.toml
├── smoke.sh
├── src/
│   └── main.mty                           ← 216 LOC
└── tools/
    ├── mock_llm/
    │   └── server.py                      ← three-route mock LLM
    └── sample_snippets/
        ├── 01_safe.txt
        ├── 02_unsafe.txt
        └── 03_unclear.txt
```

## v0.28 follow-ups surfaced by this demo

1. **`BuiltinId::Swarm` interpreter arm.** Today `swarm(...)` falls
   through `BuiltinId::Extern("swarm")` and the host shim returns
   `Value::Unit`. The SIR interpreter never fires the real HTTP path;
   the demo demonstrates source shape but live wire-up requires
   `mty build --target host`. A dedicated arm in
   `mty-ir::interp::run::Interp::call_builtin` would let `mty run`
   exercise the live three-provider round-trip.

2. **Handler-safe carve-out for swarm ADTs.** `ConsensusStrategy`,
   `Member`, `DollarBudget`, and `Consensus` aren't in v0.27's
   `handler_safe_adts` set. As a result the swarm call site has to
   live in a top-level `fn` (permissive scope) rather than directly
   in `on Review(...)`. Adding the four types to
   `crates/mty-types/src/prelude.rs::handler_safe_opaque_names`
   closes the gap.

3. **Bang-send return-type lowering for typed protocols.** The
   protocol `Review(snippet: Str) -> Str` types as `Str` but the
   SIR interpreter resolves `reviewer!Review(snippet)` as
   `Value::Unit`. The demo wraps the result in `format!("{}", report)`
   so `log(...)` sees a concrete `Str` — but the typed return shape
   should reach the call site directly.

4. **Per-provider base-URL env vars on `from_env`.** The v0.27
   `AnthropicClient::from_env()` / `OpenAiClient::from_env()` /
   `GeminiClient::from_env()` ctors don't currently consult
   `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL` / `GEMINI_BASE_URL`. The
   demo's mock-LLM smoke sets all three env vars in forward-compat
   anticipation; the actual point-at-mock plumbing is a v0.28 chore.

See `dev/history/notes/DEMO08_SWARM_REVIEW_V0_27_NOTES.md` for the
fuller engineering log + the exact span of each fallback the demo
took to fit the v0.27 surface.
