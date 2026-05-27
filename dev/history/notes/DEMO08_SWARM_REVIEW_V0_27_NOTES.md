# demo 08 — `swarm_review` v0.27 engineering notes

## Scope

The forcing-function demo for v0.27's five shipped tracks
(`@tool` decorator parser / opaque-ADT handler scope / full LLM
providers / `std.swarm` / `mty run -- argv` plumbing). One
~216-LOC Mighty program (`src/main.mty`) consumes every track
end-to-end so subsequent v0.27 regressions surface as a `mty check`
failure in this demo's smoke.sh.

The forcing function is intentional: every prior demo (06 canvas, 07
research agent) caught at least one parser / type-check / lowering gap
that the slice-time tests didn't, because the demo exercises shapes the
unit tests don't combine. This demo continues that pattern — it
surfaced four v0.28 follow-ups (catalogued below) that the v0.27 track
tests did not.

## What landed cleanly vs what required fallbacks

| Surface | Source-side use | Status |
|---|---|---|
| Track A — `@tool("desc", cap: fs.read)` decorator | `read_snippet(path)` carries the decorator on a `fn -> Str` | **Clean.** Parses + lowers + macro-expands; `mty check` exits 0. |
| Track B — `Working.new()` inside `on Review` handler | constructed inline in `CodeReviewer.on Review` | **Clean.** Track B's `handler_safe_adts` carve-out covers `Working`. |
| Track C — `Member.anthropic/openai/gemini` ctors | three calls in `main()` | **Clean** at the type-check layer. The real wire-up only fires under `mty build --target host`; SIR interp dispatches `Member::ask` as an extern. |
| Track D — `swarm(prompt, panel, spend_cap, ConsensusStrategy.Majority)` | called from a top-level `run_panel_review` helper (NOT the handler — see fallback A below) | **Partial.** Source typechecks; SIR returns Unit. v0.28 follow-up: `BuiltinId::Swarm` arm. |
| Track E — `std.env.args()` + `.get(0).unwrap_or(...)` | `main()` picks up the snippet from the `--`-tail | **Clean.** `mty run demos/08_swarm_review/src/main.mty -- "<snippet>"` reaches Mighty source via `std.env.args()`. |

## Fallbacks taken

### Fallback A — the swarm call lives in a top-level fn, not the handler

**Symptom**: `let consensus = swarm(..., ConsensusStrategy.Majority)` inside
`on Review` trips MT2021 (`cannot find value 'ConsensusStrategy' in scope
(strict handler scope; v0.3 A65)`).

**Cause**: v0.27 Track B's `handler_safe_adts` set only covers the
std.memory + std.llm opaque ADTs (`Working`, `VectorStore`, `Episodic`,
`Snapshot`, `AnthropicClient`, `OpenAIClient`, `GeminiClient`,
`BedrockClient`, `Message`, `MessageStream`, `McpServer`, `McpClient`,
`ToolRegistry`). The four std.swarm types — `ConsensusStrategy`,
`Member`, `DollarBudget`, `Consensus` — never landed in that set
because Track D shipped after Track B's `prelude.rs` edits.

**Fix taken**: Extract the swarm call into a top-level `fn run_panel_review`
(permissive scope) and have `on Review` delegate to it. The handler
still demonstrates Track B via `Working.new()`.

**Real fix**: Add the four swarm types to
`crates/mty-types/src/prelude.rs::handler_safe_opaque_names`. Tracked
as v0.28 follow-up #2 in the README.

### Fallback B — `read_snippet` body uses `_ignored` discard pattern

**Symptom**: `fn read_snippet(path: Str) -> Str { std.fs.read_to_string(path) }`
trips `MT2001: expected 'Str', found 'Unit'`.

**Cause**: `std.fs.read_to_string` is a module-qualified path call that
falls through to the host extern table; the type-checker sees the call's
result as `Unit` (the extern table doesn't have typed signatures yet).
For `Result[Str, FsError]` to flow through to the return slot the v0.28
typed-extern surface would need to land.

**Fix taken**: Discard the value (`let _ignored = std.fs.read_to_string(path)`)
and return `path` as the body's `Str`. This keeps the `@tool` decorator
descriptor's `cap: fs.read` text faithful to the actual capability the
runtime would need, even though the SIR body doesn't fire the read.

### Fallback C — `log(report)` wrapped in `format!("{}", report)`

**Symptom**: `let report = reviewer!Review(snippet); log(report)` trips
`MT2001: expected 'Str', found 'Unit'`.

**Cause**: The protocol declares `Review(snippet: Str) -> Str` but the
v0.27 SIR interpreter resolves the bang-send (`reviewer!Review(...)`)
as `Value::Unit` — the typed return shape doesn't reach the call site
through the SIR path.

**Fix taken**: `log(format!("{}", report))`. `format!("{}", _)` is
permissively typed as `Str` so the typecheck passes; at runtime the
empty Unit renders as `()`. The handler still returns the rendered
consensus body; only the call-site type erases.

**Real fix**: Bang-send return-type lowering for typed protocols.
Tracked as v0.28 follow-up #3 in the README.

### Fallback D — local named `spend_cap`, not `budget`

**Symptom**: `agent CodeReviewer(panel, budget): ReviewerInput { ... }`
trips a chain of parser errors at the comma between the two ctor
params.

**Cause**: `budget` is a reserved keyword in the v0.3 grammar
(used by the cap-narrow `with budget(...)` clause — see
`crates/mty-syntax/src/syntax_kind.rs::BUDGET_KW`). The parser
matches `budget` as a keyword token and can't accept it in an ident
position.

**Fix taken**: Rename the local + ctor arg to `spend_cap`. The
underlying type is still the swarm-side `SharedDollarBudget` (constructed
as `DollarBudget.from_dollars(0.50)`).

**Possible real fix**: Demote `budget` from a reserved keyword to a
soft keyword (only recognised in cap-narrow position). Out of scope
for v0.27.

## v0.28 follow-ups generated by this demo

1. **`BuiltinId::Swarm` interpreter arm.** The SIR interpreter
   currently dispatches `swarm(...)` through `BuiltinId::Extern("swarm")`
   and the host returns `Value::Unit`. A dedicated arm in
   `mty-ir::interp::run::Interp::call_builtin` would route the call
   to `mty_stdlib::swarm::swarm` and surface a real `Consensus` value
   — letting `mty run` exercise the live three-provider round-trip
   without the `mty build --target host` hop.

2. **Handler-safe carve-out for the four swarm ADTs.** Add
   `ConsensusStrategy`, `Member`, `DollarBudget`, `Consensus` to
   `handler_safe_opaque_names` in `crates/mty-types/src/prelude.rs`
   so swarm code can live directly in `on Review(...)` rather than
   in a delegated top-level fn.

3. **Bang-send return-type lowering for typed protocols.** The
   protocol's typed return reaches the runtime as Unit. The typed
   shape needs to plumb through the handler-invoke path so call sites
   see `Str` (or whatever the protocol declared).

4. **Per-provider base-URL env vars on `from_env`.** `AnthropicClient`,
   `OpenAiClient`, `GeminiClient` should consult `ANTHROPIC_BASE_URL` /
   `OPENAI_BASE_URL` / `GEMINI_BASE_URL` so the mock-LLM smoke can
   actually fire the real HTTP path. Today the smoke sets the vars
   in forward-compat anticipation but they're ignored.

5. **`budget` as a soft keyword.** The v0.3 cap-narrow `with budget(...)`
   clause owns the `budget` keyword unconditionally, blocking it as a
   local-binding name across the whole language. A soft-keyword
   relaxation (only recognise `budget` as a keyword inside `with(...)`)
   would unblock `let budget = ...` everywhere else.

## Files touched

CREATED:

- `demos/08_swarm_review/src/main.mty` (216 LOC)
- `demos/08_swarm_review/mighty.toml`
- `demos/08_swarm_review/README.md`
- `demos/08_swarm_review/smoke.sh`
- `demos/08_swarm_review/tools/mock_llm/server.py` (three-route mock)
- `demos/08_swarm_review/tools/sample_snippets/01_safe.txt`
- `demos/08_swarm_review/tools/sample_snippets/02_unsafe.txt`
- `demos/08_swarm_review/tools/sample_snippets/03_unclear.txt`
- `dev/history/notes/DEMO08_SWARM_REVIEW_V0_27_NOTES.md` (this file)

NO crate-source edits. The demo consumes the v0.27 surface as-is; every
fallback above ships as a v0.28 follow-up rather than a v0.27 patch.

## Smoke test result

```
$ bash demos/08_swarm_review/smoke.sh
smoke: mty check OK
smoke: mty fmt --check OK
smoke OK: .../main.mty (9965 bytes / 216 LOC, 3 sample snippets, 9 v0.27 surface markers)

$ MTY_AGENT_SMOKE=1 bash demos/08_swarm_review/smoke.sh
smoke: mty check OK
smoke: mty fmt --check OK
smoke OK: .../main.mty (...)
smoke: MTY_AGENT_SMOKE=1 — running mock-LLM end-to-end stage
smoke OK: mock-LLM pipeline markers present in run.log
```

Both modes pass.
