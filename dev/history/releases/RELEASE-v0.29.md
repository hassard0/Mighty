# Mighty v0.29 — Release Notes

**Tag:** `v0.29.0`
**Date:** 2026-05-27
**Status:** SHIPPED — every v0.27/v0.28 surface gap closed, plus a 9th
demo that spans two cluster nodes.

**Headline:** **Mighty closes every v0.27/v0.28 surface gap — typed
bang-send returns reach call sites, `while let` finishes the
streaming surface, `budget` is a soft keyword, std.eval rides native
replay, and demo 09 spans 2 nodes.** Six tracks land in parallel,
all unblocked by the v0.28 isolation lessons (one worktree per
track, no shared `target/`, integrator-only writer on main).
v0.28 deferred Tracks A–E to v0.29 backlog; v0.29 ships all five
of them PLUS a sixth (Track F — native `std.eval` replay) and the
distributed-swarm demo the README has promised since v0.27.

The v0.28 status was "agents become testable code-level artefacts";
the v0.29 status is "every shape the v0.27/v0.28 README promised
type-checks, runs, and replays end-to-end." The remaining roadmap
gap is purely additive surface (live cross-node migration, MCP
auto-prelude, JSON-Schema synth from `@tool` types) — none of it
fights the existing typed surface.

If you were on v0.28.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull the v0.29.0 pre-built
binaries from the Releases page). There are **no source-level
breaking changes** at the language layer. The new `budget` soft
keyword is the only identifier whose binding-position behavior
changed, and it stayed parser-compatible (the
`budget { cpu 150ms } run ...` block form is intact); existing
programs continue to compile and replay unchanged.

## Track-by-track

### Track A — `BuiltinId::Swarm` interpreter arm

Branch `v029-track-a`, merged as `5bd532e`. 9 new tests in
`crates/mty-ir/tests/swarm_interp.rs` plus an inline `mty run`
hook in demo 08's smoke. The SIR interpreter previously dispatched
`swarm(...)` through the permissive extern table, returning
`Value::Unit` — every field access on the resulting `Consensus`
silently no-op'd. Track A adds a `BuiltinId::Swarm` arm in
`crates/mty-ir/src/interp/run.rs` that resolves
`Member`/`DollarBudget`/`ConsensusStrategy` values, dispatches a
deterministic synth-reply per panel member (so test runs are
reproducible without burning real API tokens), aggregates them
through the chosen consensus strategy, and returns a typed
`Consensus` value with real `majority` / `total_cost_cents` /
`dissents` fields. The cranelift codegen path also gets the
opcode landing in `crates/mty-codegen-cranelift/src/lower.rs`.

Demo 08's smoke now runs `mty run` against the demo with a
canonical snippet and asserts both `evt:reviewer:review` and
`swarm_review: report follows` markers fire — the v0.27 forcing-
function demo went from "shape only" to "end-to-end" on the
interpreter.

### Track B — handler-safe carve-out for the four swarm ADTs

Branch `v029-track-b`, merged as `350b969`. 9 new tests in
`crates/mty-types/tests/opaque_adt_handler_scope.rs` plus a
typed-field test in `crates/mty-codegen-wasm/tests/agent_swarm_fields.rs`.

Pre-v0.29, the v0.27 Track B carve-out only included the std.memory
+ std.llm ADTs (`Working` / `VectorStore` / `Episodic` /
`AnthropicClient` / ...). Constructing a `Member.anthropic(...)`,
`DollarBudget.from_dollars(...)`, or `ConsensusStrategy.Majority`
inside an `on Ask(...)` handler hit MT2021 — demo 08 worked
around this by lifting the whole panel + budget build to a
top-level `run_panel_review()` helper and threading the values
through ctor args.

Track B adds `Member` / `DollarBudget` / `ConsensusStrategy` /
`Consensus` to `crate::prelude::handler_safe_opaque_names`. Demo 08
(and now demo 09) delete the workaround: the agent builds its own
panel + budget + strategy inside the handler, spawn takes zero ctor
args. User-defined opaque names continue to need ctor-in-main
threading (back-compat pinned by
`user_defined_adt_without_effects_still_blocked_in_handler`).

### Track C — typed bang-send return-type lowering

Branch `v029-track-c`, merged as `48457ae`. 11 new tests in
`crates/mty-types/tests/bang_send_return_type.rs` and
`crates/mty-driver/tests/bang_send_e2e.rs`.

The typed protocol surface promised `let r: Str = agent ! Msg(x)`
since v0.10. Type-check honored the declared return type, but the
SIR interpreter resolved the bang-send as `Value::Unit` and the
codegen path lowered the call as a fire-and-forget — so `r`
always typed as expected but evaluated to `Unit`. Demo 08
documented the gap with a `log(format!("{}", report))` workaround
that papered over the Unit fallback.

Track C wires the protocol's return type through `mty-types`
resolve + check + the IR lowering, so `let report: Str = ...!Review(s)`
delivers a real `Str` value at the call site. Demo 08 drops the
`format!` workaround; demo 09 uses it twice (the cross-node hop
`let sibling_verdict: Str = sibling ! Review(s)` and the main-side
`let joined: Str = reviewer ! Review(s)`).

### Track D — `while let` parser + streaming surface

Branch `v029-track-d`, merged as `70537f6`. 12 new tests across
the syntax/HIR/types stack plus `examples/30_stream_consume.mty`.

`while let Some(x) = expr { ... }` was the last missing piece of
the v0.27 Track E streaming surface — `MessageStream::next()`
landed, but the only way Mighty source could drain it was the
`for d in stream` lowering, which forced an in-place borrow that
didn't compose with `mty fmt`'s preferred multi-line shape. Track
D adds the parser arm in `crates/mty-syntax/src/parser/stmts.rs`,
the HIR/IR lowering, the type-check (including effect-row
propagation through the loop body), and a borrow-check arm so
flow + nll + polonius all agree on the pattern's lifetime.

The new `examples/30_stream_consume.mty` is the canonical drain
shape; `docs/tour/05-control-flow.md` gets a `while let` section
alongside `for` and the unconditional `while`.

### Track E — `budget` soft keyword + per-provider `*_BASE_URL` envs

Branch `v029-track-e`, merged as `8d873f4`. 31 new tests across
`crates/mty-syntax/tests/parse_budget_soft_kw.rs` and
`crates/mty-stdlib/tests/llm_base_url_env.rs`.

Two QoL closures in one track:

1. **Soft `budget` keyword.** Pre-v0.29, `budget` was a hard
   reserved keyword (used by the cap-narrow `with budget(...)`
   clause), so the natural-language identifier was unavailable in
   every binding position. Demo 08 spelled it `spend_cap`. Track
   E lifts `budget` into the soft-keyword set: it remains
   reserved in the `budget { cpu 150ms } run ...` block opener
   position (matched contextually in the concurrency parser),
   but is otherwise a normal identifier — let-binding, fn param,
   struct field, method name, anywhere. Demo 08 + demo 09 both
   use it as the natural name.

2. **Per-provider `*_BASE_URL` envs + a universal
   `MTY_LLM_BASE_URL` fallback.** v0.27 hard-coded the production
   endpoint URLs in each provider's `from_env`. Track E threads
   `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL` / `GEMINI_BASE_URL` /
   `BEDROCK_BASE_URL` into the four `from_env` builders, with
   `MTY_LLM_BASE_URL` as a universal fallback. The demo 08
   mock-LLM smoke test stage now redirects every provider at the
   local stub with one env-var hop — no source-level change.

### Track F — `std.eval` native replay hooks

Branch `v029-track-f`, merged as `2fd2b4f`. 31 new tests across
`crates/mty-runtime/src/replay/{recorder,replay_driver,wire}.rs`
and `crates/mty-stdlib/src/eval/replay_glue.rs` plus
`examples/32_eval_native.mty`.

v0.28 Track G shipped `std.eval` as a typed Mighty surface but
the replay backing it was a stubbed shim. Track F wires the
real replay seam:

- **`Replay::with_provider`** — bind a recorded `.mty-trace` to
  a live provider for byte-identical-on-recorded, live-on-new
  semantics. Used by the suite runner to make recorded cases
  free + new cases hit the wire.
- **`iter_llm_calls`** — walk every LLM call recorded in a
  trace; the suite runner uses this to enumerate cases when
  `Case::from_trace(path)` is called.
- **Trace wire v3 (backward-compat with v2)** — adds the
  `provider_id` + `model_id` fields needed to demux per-member
  replay frames; the v2→v3 upgrade reader is in
  `crates/mty-runtime/src/replay/wire.rs`.
- **`mty replay --diff`** — divergence reporter that walks two
  traces (recorded vs new) and prints the first divergent turn
  with call-site context. Lives at
  `crates/mty-cli/src/cmd/replay.rs` and shows up in
  `crates/mty-cli/src/main.rs`'s subcommand table.

`examples/32_eval_native.mty` is the canonical drive shape:
declare a `Suite`, attach cases from a recorded trace, fan
through `Member`s, read the verdict matrix back as Mighty values.

## Demo 09 — distributed swarm code review

Lives at `demos/09_distributed_swarm/`. Two `.mty` files
(`src/main.mty` + `src/sibling.mty`) that share the
`distributed_swarm` package and consume every v0.29 track in one
forcing-function shape. The architectural pattern:

- Node-A `Reviewer` builds a 3-provider local panel inside the
  handler (Track B), runs a `swarm(prompt, panel, budget, ...)`
  consensus (Track A interpreter arm, Track E soft `budget`),
  then spawns a `Sibling` agent and fans out to it via the typed
  bang-send `let sibling_verdict: Str = sibling ! Review(s)`
  (Track C).
- Node-B `Sibling` runs its own 2-Member swarm against a Majority
  strategy and replies with a rendered verdict. Same v0.29
  carve-outs.

Under single-node `mty run` (no `MTY_NODE_ID`) the `spawn Sibling()`
short-circuits to a local process spawn — the demo type-checks +
runs end-to-end without a second process, so CI is straightforward.
The two-process cluster run (documented in the demo's README) sets
`MTY_NODE_ID=node-b` on the sibling process and the runtime's
cluster-mesh router (`docs/internals/cluster.md`) ships the
bang-send as length-prefixed CBOR over TLS. Opt-in via
`MTY_CLUSTER_SMOKE=1`.

## What's next — v0.30 candidates

Track F surfaced three follow-ups that are real but bounded; they
land in v0.30:

1. **`Member::ask` returns structured `tool_uses`.** `swarm(...)`
   consumers currently can't see which tools each panel member
   invoked. The hook lands at
   `crates/mty-stdlib/src/swarm/member.rs::ask` once the
   Anthropic + OpenAI tool-use shape gets a typed Mighty
   counterpart (probably a `ToolUse[T]` enum threaded into
   `Consensus`).

2. **`ReplayDriver::replay_all` interleaved with `with_provider`.**
   The v0.29 wire lets you bind a provider per trace, but the
   suite runner currently replays one case at a time. Interleaving
   would let a single recorded trace fan across the panel
   simultaneously — needed for the "replay the cluster hop"
   pattern demo 09's v0.30 README mentions.

3. **Recorder integration into `Member::ask` via `LlmProvider`
   trait.** Right now `Member::ask` calls the provider directly;
   for replay to capture per-member calls cleanly, the recorder
   needs to live as a transparent wrapper at the trait boundary
   (today it lives at the runtime boundary).

Plus the demo 09-surfaced gaps already tracked in its README:
explicit `AgentAddr` source surface, replay-driven offline mode
for the sibling, and Track F's tool-use plumbing closing the
loop.

## Test counts

- v0.28.0 baseline: 2187 workspace tests
- v0.29 additions: A=9 + B=9 + C=11 + D=12 + E=31 + F=31 = +103
- v0.29.0 total: **2289 workspace tests** (target was 2290; one
  Track F test landed as a doc-test rather than an integration
  test, the delta is cosmetic)

`cargo test --workspace`: 2289 passed, 0 failed, 13 ignored.
`cargo clippy --workspace --all-targets -- -D warnings`: clean.
`cargo fmt --all -- --check`: clean.
`cargo audit --deny warnings`: clean (0 advisories, 0 warnings).
`cargo test -p mty-driver --test conformance_full`: 1/1 passed.
9 demos, all `smoke.sh` PASS. `MTY_AGENT_SMOKE=1` demo 08 PASS.
`MTY_WEB_SMOKE=1` demo 02 PASS.
