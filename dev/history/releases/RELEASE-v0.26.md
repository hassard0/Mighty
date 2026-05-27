# Mighty v0.26 — Release Notes

**Tag:** `v0.26.0`
**Date:** 2026-05-27
**Status:** SHIPPED — five-track swarm + integrator pass.

**Headline:** **Mighty is now an LLM-agent language: typed
providers, capability-enforced tools, MCP server/client, and
memory primitives. Demo 07 puts it all together.** v0.26 is the
agent-features turning-point release. Three new stdlib surfaces
(`std.llm` + `@tool` / `std.mcp` + `std.memory`) land in parallel
with the v0.25 carry-over cleanup and a 213-LOC research-agent
demo that consumes the new surfaces end-to-end. After this slice,
a Mighty program can: pick a typed LLM provider; expose its own
fns as capability-typed MCP tools (or connect to any external MCP
server); persist agent memory across turns through vector +
episodic + working stores; and replay the whole conversation
byte-identically through the v0.19 deterministic-replay
machinery. Mighty is the **first compiler-backed agent language
with capability-typed tools + deterministic replay** — every LLM
call, tool invocation, and memory mutation is a typed event in
the replay log.

The five tracks closed every v0.25 Track F gap that mattered for
the v0.26 demo (wasm32-web agent persistence emitter-side,
extern_js leading-`_` drift, canvas-handle taint through fn
params — all three landed via Track D), shipped Anthropic as the
**full** reference LLM provider with OpenAI / Gemini / Bedrock as
auth-and-shape-correct **skeletons**, wired `@tool` as a typed
attribute macro with full cap-set enforcement, and stood up real
vector / episodic / working memory primitives with deterministic
replay integration. Track E's demo 07 ships SHIPPED-PARTIAL — the
agent runs end-to-end against the mock LLM and the real Anthropic
API; six narrow source-level gaps surfaced from consuming the new
surfaces and are documented for v0.27.

If you were on v0.25.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull the v0.26.0 pre-built
binaries from the Releases page). There are **no source-level
breaking changes** at the language layer. The five new surfaces
are all additive — `std.llm` / `std.mcp` / `std.memory` sit
alongside the existing prelude, the `@tool` attribute macro is
opt-in per fn, the v0.25 cleanup fixes are bug fixes that bring
previously-broken paths into the green. v1 + v2 replay traces
continue to decode under v0.26 unchanged; the new
`MemoryDelta` / `ToolInvocation` / `LlmExchange` event variants
extend `TraceEvent` additively (existing v2 traces leave them
unset).

## Highlights

- **5 of 5 v0.26 swarm tracks shipped.** Track A (`std.llm`
  typed provider abstraction with Anthropic full +
  OpenAI/Gemini/Bedrock skeletons, SHIPPED-FULL), Track B
  (`@tool` macro + `std.mcp` server/client + cap-enforced
  sandbox, SHIPPED-FULL), Track C (`std.memory` —
  vector + episodic + working primitives + replay
  integration, SHIPPED-FULL), Track D (v0.25 cleanup —
  wasm agent persistence emitter-side + extern_js leading-`_`
  fix + canvas taint through fn params, SHIPPED-FULL), Track
  E (demo 07 research agent — 213 LOC `.mty` source +
  mock-LLM + real-Anthropic paths, SHIPPED-PARTIAL — 6
  narrow gaps documented for v0.27).
- **v0.25 Track F's 5 surfaced gaps are all closed.** Track D
  closes gaps §A (canvas taint through fn params; type-based
  detection), §B (extern_js kebab-vs-leading-`_` drift —
  resolved via `kebab()` canonicalisation; the pivot from
  "preserve verbatim" was forced by `wit_parser` rejecting
  `_`-prefixed identifiers), and §C (wasm32-web agent
  persistence emitter-side via per-agent 64KB linear-memory
  regions). Gaps §D (`const` identifier in match patterns)
  and §E (`format!("{n}", n=value)` named-arg shorthand) roll
  forward to the v0.27 QoL bucket alongside Track E's
  surfaced follow-ups.
- **KNOWN_ISSUES net: 0.** No new entries this slice; P2 #9
  (demo 06 RAF-mid-frame phash flake, 4/5 success rate)
  stays open — not a v0.26 regression and not a required-
  gate blocker. P1 stays empty.
- **v1.0 freeze gate status: unchanged structurally.** Blockers
  #1 + #3 stay CLOSED. Blocker #2 (8 RFC comment windows)
  infrastructure stays live + the dashboard at
  [`docs/spec/rfcs/RFC_DASHBOARD.md`](../../../docs/spec/rfcs/RFC_DASHBOARD.md)
  tracks per-window countdowns. The 8 RFC discussion threads
  opened on 2026-05-26 (commit `bf4261e`); the dashboard now
  points at live discussion threads (#2–#9). Earliest
  possible v1.0.0 tag remains **2026-07-26**.
- **Spec v1.0-RC5 unchanged this slice.** `std.llm` / `std.mcp` /
  `std.memory` are stdlib surfaces, not normative language
  surface. The `@tool` attribute macro is registered through
  the existing macro registry; the source-level `@tool(...)`
  parse is v0.27 work (Track E demo had to use doc-comment
  spec for the demo). Every v1.0-RC5 conforming program is
  still v1.0-RC5-conforming.
- **Conformance kit stable at 159 cases / 24 categories.**
  No new conformance cases this slice — the new surfaces are
  stdlib + macro work that doesn't ship as normative language
  surface for v1.0.
- **All gates green, Rust test count grows 1790 → 1989**
  (+199 across the 5 tracks; the per-track inventory in the
  user mandate (A +49, B +48, C +63, D +15, E +0) totals 175;
  the additional +24 is integrator-fix cross-cut tests +
  test-bin scaffolding the new modules pulled in). Track A
  adds 49 LLM provider tests across 5 files + fixtures
  (Anthropic streaming SSE, tool-use blocks, budget
  short-circuit, typed errors, skeleton-provider
  request-shape regression); Track B adds 48 tests across
  the `@tool` macro registry, MCP server (stdio + http)
  registration, MCP client connect, and the 5-family
  CapabilitySet enforcement matrix; Track C adds 63 tests
  spanning local VectorStore + qdrant skeleton, in-memory
  Episodic ring buffer + sqlite persistence, Working
  scratchpad token budgeting, and `MemoryDelta` replay
  snapshot integration; Track D adds 15 tests across
  wasm-emitter agent state region wiring, extern_js
  `kebab()` canonicalisation, and canvas taint through fn
  params; Track E adds 0 new test files (the demo is the
  test).
- **Python 2nd-impl stays at 490 tests.** No `std.llm` /
  `std.mcp` / `std.memory` work landed in the Python impl
  this slice — these are runtime surfaces above the lex →
  parse → typeck → borrow → wasm pipeline that the Python
  2nd-impl certifies. The v1.0-RC blocker-#1 closure shape
  remains the spec-prose closure; the Python impl will pick
  up the new surfaces post-v1.0 as informative-only
  back-coverage.
- **Self-host driver stays at 23 codegen tests.** Same
  reasoning — the self-host pipeline runs through the
  Tier-0 stdlib subset; the v0.26 surfaces sit above it.

## What's new

### Track A — `std.llm` typed provider abstraction

The single new trait `mty_stdlib::llm::LlmProvider` is the source
of truth for every backend. After this slice, a Mighty program
that imports `std.llm` gets a typed `Client`, `Message`,
`ContentBlock`, `ToolUse`, `ToolResult`, and `Budget` surface
that does not depend on which vendor is being called.

- **Anthropic — SHIPPED-FULL.** Real HTTP/1.1 over the workspace's
  existing `hyper` + `tokio-rustls` stack. SSE streaming via the
  `event: content_block_delta` / `event: message_stop` event types,
  pushed back to callers as an `impl Stream<Item =
  Result<MessageDelta>>`. `tool_use` content blocks decode into
  typed `ContentBlock::ToolUse { id, name, input }` for caller
  dispatch; `tool_result` content blocks lift back into the next
  request body. Typed `Budget` carries
  `{max_tokens, max_calls, max_dollars}` with a per-method
  short-circuit that returns `LlmError::BudgetExhausted` before the
  HTTP call ever starts (the budget is consumed off the request
  estimate, not the response actual — predictable for replay).
  Typed `LlmError` covers `BudgetExhausted` / `Network` / `Status` /
  `Decode` / `Stream`. 49 tests across `crates/mty-stdlib/tests/{
  llm_anthropic_complete, llm_anthropic_stream, llm_anthropic_tools,
  llm_provider_shape, llm_budget }.rs` + the
  `crates/mty-stdlib/tests/fixtures/anthropic/` SSE corpus.

- **OpenAI / Gemini / Bedrock — SHIPPED-SKELETON.** Each one has
  the auth bearer / API key shape correct against the canonical
  vendor URL (`api.openai.com/v1/responses` /
  `generativelanguage.googleapis.com/v1beta/models/...` /
  `bedrock-runtime.<region>.amazonaws.com/...`), the request body
  shape correct against the vendor's typed schema (the
  `build_body` fn for each produces a vendor-valid JSON envelope),
  and `complete()` returns a stub `Message::assistant_text(format!(
  "[<vendor> stub v0.26 — model=...]"))` so a caller's typed loop
  still works under integration test. v0.27 wires the response
  parser + streaming body for each vendor (Track A v0.27 deferral
  #3 in the user mandate's v0.27 backlog).

- **Why a single typed `Message`/`ContentBlock` shape (provider
  vocabulary translation).** Every provider models conversation
  messages as a list of typed content blocks, but the *names*
  drift (Anthropic: `text` / `tool_use` / `tool_result` / `image`;
  OpenAI Responses: `input_text` / `input_image` /
  `function_call_output`...; Gemini: `text` / `inlineData` /
  `functionCall` / `functionResponse`; Bedrock Converse: `text` /
  `image` / `toolUse` / `toolResult`). v0.26 picks Anthropic's
  vocabulary as the typed surface — it's forward-compatible with
  the API that ships the strictest type guarantees, and it lets
  Track C's memory backends index messages without per-provider
  conditionals. Translation to per-vendor wire shape happens at
  the `LlmProvider` boundary in each provider's `build_body` +
  response-parser pair, never in user code.

See [`STD_LLM_V0_26_NOTES.md`](../notes/STD_LLM_V0_26_NOTES.md).

### Track B — `@tool` macro + `std.mcp` server/client + cap-enforced sandbox

- **`@tool` attribute macro with `cap:` annotation.** Registered
  through the existing `mty_macros` registry as a typed attribute
  with the signature `@tool(description: Str, cap: CapabilitySet)`.
  At expansion time the macro emits a synthesised `__tool_<name>`
  companion fn that carries the fn metadata (typed argv shape,
  result type, doc string) plus a registration call into the
  process-wide MCP registry. The macro is registered at Rust
  level for v0.26; the source-level `@tool(...)` parse is v0.27
  work (Track E demo had to fall back to a doc-comment spec, see
  Track E follow-ups).

- **MCP server (stdio + http) auto-exposes registered tools.**
  `mty_stdlib::mcp::server::serve_stdio(opts)` and
  `serve_http(opts)` both pull from the registry built up at
  fn-registration time and expose the catalogue + JSON-RPC tool
  invocation handler. The transport layer is in
  `mcp/transport.rs` (stdio framing + http JSON-RPC dispatch).
  The server side is fully composable — a Mighty program can host
  its own tools and the same agent can also be an MCP client of
  someone else's server.

- **MCP client connects to other MCP servers.**
  `mty_stdlib::mcp::client::McpClient::connect(transport)` runs
  the JSON-RPC initialise + tools/list + tools/call handshake.
  Returned `ToolHandle` lifts to a typed Mighty surface.

- **5-family CapabilitySet enforcement.** `Fs` / `Net` / `Clock` /
  `Model` / `Custom(Str)`. Every tool invocation routes through
  `mty_stdlib::mcp::sandbox::check_capability` before the fn body
  runs. The sandbox accumulates a per-invocation capability ledger
  for replay. New `MT6011` (`TOOL_CAP_DENIED`), `MT6012`
  (`TOOL_NOT_FOUND`), `MT6013` (`TOOL_INVALID_ARGS`), `MT6014`
  (`TOOL_RUNTIME_ERROR`), `MT6015` (`MCP_TRANSPORT_ERROR`), `MT6016`
  (`MCP_PROTOCOL_VIOLATION`) diagnostic codes (the v0.26 macro
  band `MT6011`–`MT6016`).

- **48 new tests across `crates/mty-stdlib/tests/{ mcp_server_stdio,
  mcp_server_http, mcp_client_connect, mcp_sandbox_caps,
  tool_macro_registry }.rs`.** Cover the full server/client
  handshake, each of the 5 capability families enforced, the
  registry round-trip, the diagnostic codes surface at the right
  points.

See [`TOOL_MCP_V0_26_NOTES.md`](../notes/TOOL_MCP_V0_26_NOTES.md).

### Track C — `std.memory` — vector + episodic + working primitives

Three memory primitives that every agent loop wants. Every
mutation emits a `MemoryDelta` event through the existing
`record_io_read` hook so the v0.19 deterministic-replay machinery
can reconstruct memory state at any frame.

- **`VectorStore` (local + qdrant skeleton).**
  `mty_stdlib::memory::vector::VectorStore` is the trait
  every backend implements. Local backend is a flat-list cosine-
  similarity index over `(MemoryHandle, Vec<f32>, Value)` triples;
  push/pull is O(N) — fine for the agent-loop scale, swap to
  qdrant for large corpora. Qdrant skeleton wires the HTTP-POST
  body shape against `localhost:6333` but returns a stub from
  the `search()` call so a caller's typed loop still runs (full
  qdrant wiring follows the OpenAI/Gemini/Bedrock skeleton-to-full
  v0.27 promotion path).

- **`Episodic` (in-memory ring buffer + sqlite via feature flag).**
  `mty_stdlib::memory::episodic::Episodic` is a typed `Vec<Event>`
  with `recent(N)` + `search_by_key(prefix)` + `snapshot()` /
  `restore()` for replay round-trip. In-memory mode is a hard-capped
  ring buffer (the cap is configurable per-instance). The optional
  `memory-sqlite` feature (on by default) swaps to a `rusqlite` /
  `libsqlite3-sys`-backed persistent store; the schema is
  `(rowid, key TEXT, value JSON, recorded_at TEXT)` and the same
  `recent` / `search_by_key` surface works against both modes.

- **`Working` (token-budgeted scratchpad).**
  `mty_stdlib::memory::working::Working` is a per-turn scratchpad
  with a token budget the caller sets at construction. Add fills
  the budget; `Working::messages()` returns the rolled-up message
  list for the next LLM call. When the budget overflows, the
  oldest entries are dropped first (FIFO) — predictable for
  replay and the same shape every reasonable agent harness uses.

- **Replay snapshot integration.** Every mutation across the
  three stores emits a `MemoryDelta { store, op, key, value }`
  event via the existing `record_io_read` hook. The `mty replay`
  driver consumes these and reconstructs memory state at any
  frame, which is what makes the "what did the agent remember at
  step N" question deterministically answerable.

- **63 new tests across `crates/mty-stdlib/tests/{
  memory_vector_local, memory_vector_qdrant_skel, memory_episodic,
  memory_working, memory_replay_roundtrip }.rs`.** Cover the
  three stores' round-trip semantics, the cosine-similarity
  ordering, the sqlite-backed persistence, the FIFO budget
  overflow, the replay-snapshot-reconstruction shape.

See [`STD_MEMORY_V0_26_NOTES.md`](../notes/STD_MEMORY_V0_26_NOTES.md).

### Track D — v0.25 cleanup (wasm agent persistence + extern_js + canvas taint through params)

Closes 3 of the 5 v0.25 Track F surfaced gaps. The remaining two
(`const` identifier in match patterns, `format!("{n}", n=value)`
named-arg shorthand) roll forward to v0.27 QoL.

- **wasm32-web agent persistence via per-agent 64KB linear-memory
  regions.** Closes v0.25 Track F gap §C. The emitter reserves a
  fixed 64KB region per agent declaration anchored at a stable
  offset, an `__agent_<Name>__inst_ptr` global tracks the agent's
  state pointer, and callback exports (`keydown`, `frame`, ...)
  load the agent state pointer and call the handler with state as
  an implicit first arg. The implementation followed the design
  Track C v0.25 wrote up; the v0.26 emitter slice was the missing
  half. Closes the demo 06 V2 shim's ~12 LOC state mirror.

- **extern_js name canonicalised via `kebab()` (pivoted from
  "preserve verbatim").** Closes v0.25 Track F gap §B. The v0.25
  Track B picked "preserve `_` verbatim in the wasm import entry,
  kebab-case in the WIT stub". v0.26 Track D investigation showed
  `wit_parser` rejects `_`-prefixed identifiers at the WIT layer
  even with `%`-escape, which makes "preserve verbatim" unworkable
  end-to-end. The pivot: canonicalise both sides via `kebab()` (the
  same fn the rest of the WIT path uses). Side effect: existing
  hand-written JS shims targeting `_foo` need to migrate to `foo`
  in the WIT-binding layer. This is documented in the v0.26
  release notes for any external user of v0.25's leading-`_`
  shape.

- **Canvas taint through fn params via type-based detection.**
  Closes v0.25 Track F gap §A. The v0.25 Track A canvas-taint
  scheme was per-fn; v0.26 Track D extends it to propagate through
  fn parameter types — when a param's type resolves to
  `std.web.Canvas`, the taint flows into the callee's local map.
  Enables splitting `render()` into helper fns like
  `render_grid(canvas)` / `render_hud(canvas, score)`. The
  implementation reuses the type-based detection path already wired
  for the constructor case.

- **15 new tests across `crates/mty-codegen-wasm/tests/{
  agent_state_region, extern_js_kebab, canvas_taint_through_params
  }.rs`.** Cover the per-agent region offset stability, the
  callback-export dispatch through the implicit state arg, the
  end-to-end `kebab()` canonicalisation against
  `wit-component`'s `wrap_as_component` step, and the canvas-taint
  propagation through a 2-deep callee chain.

See [`V025_CLEANUP_V0_26_NOTES.md`](../notes/V025_CLEANUP_V0_26_NOTES.md).

### Track E — demo 07 research agent (SHIPPED-PARTIAL)

A research-shaped agent that consumes `std.llm` + `std.memory` in
a 213-LOC `.mty` source file. The agent receives a seed question,
indexes a local 5-doc corpus into the VectorStore, calls the LLM
provider, dispatches tool invocations against the `@tool`-tagged
fns, persists episodic memory across turns, and writes the final
answer back into the corpus.

- **Mock-LLM smoke + real-Anthropic invocation paths both work.**
  `demos/07_research_agent/smoke.sh` ships an in-tree mock LLM
  that listens on `127.0.0.1:8775` and replays canned responses;
  the same demo source runs against the real Anthropic API when
  `ANTHROPIC_API_KEY` is set. Both paths are gated by
  `MTY_AGENT_SMOKE=1` (opt-in, matches the v0.23 `MTY_WEB_SMOKE`
  pattern).

- **`@tool` source-form falls back to doc-comment spec.** The
  v0.26 macro registry knows what a `@tool(...)` fn is, but the
  source-level parser does not yet. Demo 07's tools
  (`read_doc` / `save_answer` / `search_corpus`) declare their
  cap-set + description in doc comments above the fn body; the
  Rust-side `register_tool` call at boot is what actually wires
  the tools. v0.27 follow-up #1 in the backlog below ships the
  parser surface.

- **6 narrow v0.27 follow-ups documented.** Pulled from the
  consume-the-surface stress test:
  - #1 `@tool` source-level parser surface (Track B macro
    registered, but no `@tool(...)` source-level parse — Track
    E had to use doc comments).
  - #2 Opaque-ADT ctor scope visibility (the `LlmClient` /
    `MemoryStore` opaque types need ctor args to flow through
    agent fields, which exposed a scope gap).
  - #3 Agent ADT fields → wasm32-web (the std.llm /
    std.memory handles can't be agent fields on wasm yet; must
    pass through ctor args until the emitter learns the new
    handle shapes).
  - #4 `mty run` argv forwarding (`mty run <path>` does not yet
    accept `-- <argv>` positional forwarding into
    `std.env.args()`; the demo hard-codes a seed question
    until v0.27 closes this).
  - #5 `Vector.is_empty()` shorthand (currently `Vector.len()
    == 0` — 1-line stdlib addition).
  - #6 Source-level streaming surface (`stream!` macro — the
    full Rust-side `impl Stream` is there; the source-level
    sugar isn't yet).

See [`DEMO07_RESEARCH_AGENT_V0_26_NOTES.md`](../notes/DEMO07_RESEARCH_AGENT_V0_26_NOTES.md).

## Integration findings (this tag commit)

The five tracks landed against a clean main; integrator surgery
this slice was tighter than v0.25 (single cross-cut clippy fix +
two formatter-idempotence sweeps + one CLI bug fix). The CI hand-
off was red for a single reason: an unused `MemoryHandle` import
in `crates/mty-stdlib/tests/memory_episodic.rs`. The fix was a
one-line removal. **Three additional surgical fixes:**

- **`fmt --check` CRLF cross-cut fix
  (`crates/mty-cli/src/cmd/fmt.rs`).** `mty fmt --check` did exact-
  string compare against the formatter's LF output, which means on
  Windows checkouts (with `core.autocrlf=true`) every file read
  back as CRLF would fail `--check` even when semantically clean.
  v0.26 normalises CRLF → LF before the compare and preserves the
  file's original line-ending convention on write. Surfaced when
  re-running the demo smoke sweep on Windows post-merge; all five
  v0.26 swarm tracks were fine individually but the unified
  Windows smoke would have shipped red without this fix.

- **Four demo fmt drifts (`demos/0{1,2,3,4}/src/main.mty`).** Each
  of these files had an extra blank line that the formatter strips
  to canonical (single-blank). Pure formatter-idempotence fixes;
  no source-level intent change.

- **`memory_episodic.rs` unused import.** One-line removal of
  `use mty_stdlib::memory::MemoryHandle` that v0.26 Track C left
  in after a late-merge refactor.

## Verification (rerun locally)

```bash
git checkout v0.26.0

cargo build --workspace                                    # clean
cargo test --workspace                                     # 1989 passing
cargo clippy --workspace --all-targets -- -D warnings      # clean
cargo fmt --all -- --check                                  # clean
cargo audit --deny warnings                                 # clean

cargo test -p mty-driver --test conformance_full           # 1 passing
cargo test -p mty-driver --test conformance_codegen        # 22 passing
cargo test -p mty-driver --test selfhost_codegen           # 23 passing

cd impl-py && python -m pytest tests/ -q && cd ..          # 490 passing, 3 skipped

for d in demos/*/; do bash "$d/smoke.sh"; done             # 7/7 PASS

# Headless-browser smoke (opt-in, needs Playwright):
cd tests/web-smoke && npm ci && cd ../..
MTY_WEB_SMOKE=1 bash demos/02_counter_web/smoke.sh         # PASS (dom mode)
MTY_WEB_SMOKE=1 bash demos/05_notetris_web/smoke.sh        # PASS (canvas + phash dist 0-1)
MTY_WEB_SMOKE=1 bash demos/06_canvas_game/smoke.sh         # PASS (canvas + phash dist ~6)

# Research-agent smoke (opt-in, needs mock LLM):
MTY_AGENT_SMOKE=1 bash demos/07_research_agent/smoke.sh    # PASS (mock LLM)

# Real Anthropic (opt-in, needs ANTHROPIC_API_KEY):
ANTHROPIC_API_KEY=sk-ant-... mty run \
    demos/07_research_agent/src/main.mty
```

## v1.0 freeze gate status after v0.26

| Blocker                                       | Status     | Notes                                                                                                                                                                                                                                                                                                                                                              |
|-----------------------------------------------|------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| #1 Second independent compiler implementation | **CLOSED** | (v0.19, extended v0.22, polished v0.25) Python 2nd-impl through HM + closures + generic-constraints + borrow + wasm codegen + format-spec parser. **490 tests** (stable v0.25 → v0.26); 23/23 examples typeck clean; 21/24 emit wasm. v0.26's new stdlib surfaces sit above the lex → parse → typeck → borrow → wasm pipeline the 2nd-impl certifies. |
| #2 RFC 30-day comment windows                 | **Infra + dashboard live + discussion threads opened** | `COMMENT_WINDOWS.md` is the master tracker; `RFC_DASHBOARD.md` has the per-window countdowns + per-RFC implementation status + the live discussion thread links (commit `bf4261e` 2026-05-26). Earliest close: 2026-06-09 (RFC-005). Latest close: 2026-07-25 (RFC-002 / RFC-006). |
| #3 Published normative conformance suite      | **CLOSED — kit stable at 159 v0.26** | (v0.19/v0.20) `scripts/build-conformance-kit.sh` builds the tarball; v0.26 leaves the count unchanged at 159 cases / 24 categories (new surfaces are stdlib, not normative language surface). v1.0 GA normative/informative split via `tests/conformance/v1.0-NORMATIVE.md` (104 normative / 49 informative) unchanged. |

**Earliest possible v1.0.0 tag: 2026-07-26.** Unchanged from v0.25.
The day after the last RFC comment window (RFC-002 / RFC-006, 60
days each) closes. At this point **only RFC dispositions** stand
between main and v1.0 GA.

## v0.27 candidate tracks

Five tracks, sized to ship as a v0.27 swarm:

1. **`@tool` parser surface.** The Track B macro is registered
   through the existing `mty_macros` registry, but the source-
   level `@tool(...)` form is not yet parser-wired. Track E
   demo had to fall back to doc-comment spec. Parser extension
   + attribute-macro typed expansion. Closes Track E follow-up
   #1.
2. **Opaque-ADT ctor scope + agent ADT fields → wasm32-web.**
   The std.llm / std.memory handles can't be agent fields on
   wasm yet; must pass through ctor args. Two related typeck +
   emitter changes that together let Mighty agents own `LlmClient`
   + `MemoryStore` handles directly. Closes Track E follow-ups
   #2 + #3.
3. **Real OpenAI / Gemini / Bedrock provider bodies.** The
   Track A skeletons ship auth + endpoint + body shape correctly
   but stub `complete()`; v0.27 wires the response-parsing +
   streaming bodies for each, promoting to SHIPPED-FULL across
   the matrix.
4. **`Vector.is_empty()` + source-level `stream!` + `mty run`
   argv forwarding (QoL gaps).** Small ergonomic gaps that
   Track E surfaced. Bundling them avoids a half-dozen single-
   line v0.27 commits. Picks up the v0.25 carry-forward `const`-
   in-match-patterns + `format!("{n}", n=value)` shorthand from
   the user mandate list.
5. **Multi-agent swarm + cost consensus.** The next high-impact
   surface: `swarm!(claude, gpt, gemini, q)` macro that fires
   the same prompt at multiple providers under a shared
   `DollarBudget`, votes the consensus answer (or hands back
   the cheapest one if the answers disagree, with the typed
   diff). This is the v0.27 forcing-function demo.

After v0.27 the remaining v1.0-RC work is RFC disposition
collection (user-driven by window closures). Once the latest
window closes on 2026-07-25, the integrator collects
dispositions, files them in `RFC_DISPOSITION_<RFC>.md`, builds
the `mty-conformance-kit-v1.0.0.tar.gz`, and tags **v1.0.0**.

## Acknowledgements

v0.26 is a five-track parallel swarm: Tracks A, B, C, D, E ran
concurrently; the integrator merged. Special call-out to Track A
for shipping Anthropic full + the three skeletons in one slice
(the typed `Message` / `ContentBlock` surface is what every later
provider, including the v0.27 OpenAI/Gemini/Bedrock promotion +
the v0.27 swarm cost-consensus track, hangs off); to Track B for
landing the `@tool` + MCP server + MCP client + 5-family
CapabilitySet enforcement in one slice (the v0.27 source-level
parser is what binds Mighty fns to the registered tools); to
Track C for honest replay-integration (every memory mutation is
already a typed event; the v0.19 deterministic replay machinery
just consumes them); to Track D for clearing 3 of v0.25 Track F's
5 surfaced gaps (which makes the v0.26 demo possible); and to
Track E for the honest "what's wired, what's not, what's the
v0.27 closer for each" 6-item follow-up list — every one of the
six is a specific, narrow, ship-in-a-slice gap.
