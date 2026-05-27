# Changelog

All notable changes to Mighty are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

For the full per-release notes, see
[`dev/history/releases/`](dev/history/releases/).

## [Unreleased]

- v0.27 candidates (from v0.26 Track E's 6 follow-ups + v0.25
  carry-forward QoL):
  (1) **`@tool` source-level parser surface.** The v0.26 Track B
  macro is registered through the existing `mty_macros` registry,
  but the source-level `@tool(...)` form is not yet parser-wired.
  Track E demo had to fall back to doc-comment spec. Parser
  extension + attribute-macro typed expansion.
  (2) **Opaque-ADT ctor scope + agent ADT fields → wasm32-web.**
  The `std.llm` / `std.memory` handles can't be agent fields on
  wasm yet; must pass through ctor args. Two related typeck +
  emitter changes that together let Mighty agents own
  `LlmClient` + `MemoryStore` handles directly.
  (3) **Real OpenAI / Gemini / Bedrock provider bodies.** Track
  A's skeletons ship auth + endpoint + body shape correctly but
  stub `complete()`; v0.27 wires the response-parsing + streaming
  bodies for each, promoting to SHIPPED-FULL across the matrix.
  (4) **`Vector.is_empty()` + source-level `stream!` macro + `mty
  run` argv forwarding (QoL gaps).** Small ergonomic gaps Track E
  surfaced. Bundle with v0.25 carry-forward `const`-in-match-
  patterns + `format!("{n}", n=value)` shorthand.
  (5) **Multi-agent swarm + cost consensus.** The v0.27 forcing-
  function demo: `swarm!(claude, gpt, gemini, q)` macro fires the
  same prompt at multiple providers under a shared
  `DollarBudget`, votes the consensus answer (or hands back the
  cheapest one if the answers disagree, with the typed diff).
  Plus the carry-forward items from v0.23 / v0.24 (MT3012 / BOLT
  / multi-socket NUMA bench / `mty conform <kit.tar.gz>` / spec
  validation sweep) if they don't fit into v0.27's swarm budget.
  There is **no remaining Post-v1.0 backlog** — only RFC comment
  windows stand between current main and v1.0 GA.

## [0.26.1] - 2026-05-27

**Hotfix: SSE parser tolerates CRLF input on Windows checkouts.**
Anthropic SSE-event boundaries are `\n\n` per spec; on Windows
checkouts with `core.autocrlf=true`, the captured streaming
fixtures load as CRLF and `parse_anthropic_sse`'s
`rsplit_once("\n\n")` never matches, returning the entire body as
the tail and dropping every event on the floor. v0.26.1
normalises CRLF → LF at the head of the parser so fixtures (and
any upstream proxy that rewrites line endings) parse identically
on every platform. The Anthropic real-network path is unaffected —
Anthropic emits LF only. CI red on windows-latest at v0.26.0
(5/5 `llm_streaming` tests failed); CI green at v0.26.1. No new
tests; the existing 5 `llm_streaming` tests now pass on Windows.

## [0.26.0] - 2026-05-27

**Mighty is now an LLM-agent language: typed providers,
capability-enforced tools, MCP server/client, and memory
primitives. Demo 07 puts it all together.** v0.26 is the agent-
features turning-point release. Three new stdlib surfaces
(`std.llm` + `@tool` / `std.mcp` + `std.memory`) land in parallel
with the v0.25 carry-over cleanup and a 213-LOC research-agent
demo that consumes the new surfaces end-to-end. **Track A** ships
`mty_stdlib::llm::LlmProvider` as the single typed trait every
backend implements, with Anthropic as the SHIPPED-FULL reference
(real HTTP/1.1 over `hyper` + `tokio-rustls`, SSE streaming via
`event: content_block_delta` / `message_stop`, typed `ContentBlock::
ToolUse { id, name, input }` for tool-use blocks, typed `Budget`
with per-method short-circuit returning `LlmError::BudgetExhausted`
off the request estimate, typed `LlmError` covering
`BudgetExhausted` / `Network` / `Status` / `Decode` / `Stream`)
and OpenAI / Gemini / Bedrock as SHIPPED-SKELETON (auth +
endpoint + body shape correct against the canonical vendor URL +
typed schema; `complete()` returns a stub
`Message::assistant_text("[<vendor> stub v0.26 ...]")`; v0.27
wires the response parser + streaming bodies); 49 new tests.
**Track B** ships `@tool` as a typed attribute macro through
`mty_macros` (signature `@tool(description: Str, cap:
CapabilitySet)`; expansion emits a synthesised `__tool_<name>`
companion fn with the fn metadata + registry registration call;
the macro is registered at Rust level — the source-level
`@tool(...)` parse is v0.27 work), plus `std.mcp` server (stdio
+ http auto-exposes registered tools) + `std.mcp` client (runs
the JSON-RPC initialise + tools/list + tools/call handshake) +
5-family CapabilitySet enforcement (`Fs` / `Net` / `Clock` /
`Model` / `Custom(Str)` checked at every tool invocation; per-
invocation capability ledger accumulates for replay); new
`MT6011`–`MT6016` diagnostic band; 48 new tests. **Track C**
ships `std.memory` with three primitives — `VectorStore` (local
flat-list cosine-similarity index + qdrant skeleton), `Episodic`
(in-memory ring buffer + sqlite-backed persistence via opt-on
`memory-sqlite` feature; `(rowid, key TEXT, value JSON,
recorded_at TEXT)` schema), `Working` (token-budgeted scratchpad
with FIFO drop-oldest on budget overflow) — and replay
integration via a new `MemoryDelta { store, op, key, value }`
event variant routed through the existing `record_io_read` hook
so `mty replay` reconstructs memory state at any frame; 63 new
tests. **Track D** closes 3 of 5 v0.25 Track F gaps: wasm32-web
agent persistence emitter-side via per-agent 64KB linear-memory
regions + `__agent_<Name>__inst_ptr` global + callback exports
loading state pointer + calling handler with state as implicit
first arg (closes Track F §C); extern_js name canonicalised via
`kebab()` (pivoted from v0.25's "preserve `_` verbatim" because
`wit_parser` rejects `_`-prefixed identifiers even with
`%`-escape; closes Track F §B; side effect: existing hand-written
JS shims targeting `_foo` must migrate to `foo` in the WIT-
binding layer); canvas taint through fn parameters via type-based
detection (extends v0.25 Track A's per-fn scheme to flow taint
into callees when a param resolves to `std.web.Canvas`; closes
Track F §A); 15 new tests. Track F's remaining 2 gaps (§D
`const` in match patterns, §E `format!("{n}", n=value)`)
roll forward to v0.27 QoL. **Track E** ships demo 07 research
agent — 213-LOC `.mty` source that consumes `std.llm` +
`std.memory` (indexes a local 5-doc corpus into the VectorStore,
calls the LLM provider, dispatches tool invocations against the
`@tool`-tagged fns, persists episodic memory across turns, writes
the final answer back into the corpus), opt-in mock-LLM smoke
(`MTY_AGENT_SMOKE=1 bash demos/07_research_agent/smoke.sh`) + real
Anthropic invocation path (`ANTHROPIC_API_KEY=sk-ant-... mty
run`). **SHIPPED-PARTIAL**: 6 narrow v0.27 follow-ups documented
(`@tool` source-level parser; opaque-ADT ctor scope + agent ADT
fields → wasm32-web; `mty run` argv forwarding; `Vector.is_empty()`;
source-level `stream!` macro). **Integrator fixes (this tag
commit):** `crates/mty-cli/src/cmd/fmt.rs` normalises CRLF → LF
before the `fmt --check` compare and preserves the file's original
line-ending convention on write — `fmt --check` was failing on
Windows checkouts (`core.autocrlf=true`) because the formatter
emits LF and exact-string compare against CRLF was always reporting
"would reformat"; the v0.26 swarm Windows smoke would have shipped
red without this fix. Plus 4 demo formatter-idempotence sweeps
(`demos/0{1,2,3,4}/src/main.mty` each had an extra blank line the
formatter collapses to canonical single-blank) and one unused-
import removal in `crates/mty-stdlib/tests/memory_episodic.rs` that
v0.26 Track C left after a late-merge refactor. **KNOWN_ISSUES
net: 0.** No new entries. P2 #9 (demo 06 RAF-mid-frame phash flake,
4/5 success rate, predates v0.24, no required-gate impact) stays
open. P1 stays empty. **v1.0 freeze gate status: unchanged
structurally.** Blockers #1 + #3 stay CLOSED; #2 (8 RFC comment
windows) infrastructure + dashboard stay live + discussion threads
opened 2026-05-26 (commit `bf4261e`); earliest possible v1.0.0 tag
remains **2026-07-26**. Conformance kit stable at **159 cases**
(new surfaces are stdlib, not normative). Rust test count grows
**1790 → 1989** (+199; A +49, B +48, C +63, D +15, E +0, +24
integrator / scaffolding). Python stable at **490**. Self-host
driver still at **23**. Combined (with 159 conformance cases):
**2661** (+185 vs v0.25). See
[`dev/history/releases/RELEASE-v0.26.md`](dev/history/releases/RELEASE-v0.26.md).

## [0.25.0] - 2026-05-26

**Closed all 7 v0.24 demo-blocking gaps + extended `format!()` +
real `std.String` / `std.Vec[T]`. Demo 06 V2 shim −48 %.** v0.25
is a six-track parallel swarm that closes every gap v0.24 Track
E flagged for v0.25 plus extends the language surface with two
foundational stdlib types. **Track A** wires `canvas.fill_rect(...)`
through HIR → IR → wasm32-web import via a per-fn canvas-handle
taint scheme on `FnBuilder::canvas_locals` (constructor taints
the result local; `bind_pat_assign` propagates through let-
rebind; `lower_expr::MethodCall` and `lower_call`'s local-method-
call arm route tainted-receiver calls to `BuiltinId::CanvasOp`)
+ fixes the latent Unit-returning user-fn stack-balance bug
(`emit_call`'s `FnRef::User` arm now pushes a placeholder
`i32.const 0` for Unit / Never callees, matching every other
arm) — closes v0.24 Track E gaps A + B + KNOWN_ISSUES P2 #8;
24 new tests. **Track B** lifts `extern js { fn _foo() }`
into real `(import "mty:web/js" "_foo" ...)` entries via a new
`Program::extern_bindings` IR side-table populated by
`record_extern_bindings` in `register_fn_shells`, an
`Emitter::predeclare_extern_js_imports` pre-declare pass that
runs before `declare_fns` (so the function-index space is
correct), and a per-program `interface js { ... }` WIT stub —
closes v0.24 Track E gap E; 13 new tests. **Track C** fixes
agent fields with `[T; N]` types — the parser already accepted
the surface, but HIR lowering's `TYPE_ARRAY` arm dropped the
length expression (`len: None` → slice degrade); 12-line fix
captures the expression as `ExprId` and passes `len = Some(...)`
through to the downstream `const_eval_len` path. Plus pins SIR-
runtime cross-callback persistence with three regression tests
(persistence already worked there; never tested). Designs the
wasm32-web single-agent-instance pattern for v0.26 — closes
v0.24 Track E gaps C + D; 12 new tests. **Track D** extends
`format!()` to the full Rust layout grammar
(`[[fill]align][sign][#][0][width][.precision][type]`): `{:5}`,
`{:05}`, `{:<5}` / `{:>5}` / `{:^5}`, `{:*<5}` fill char,
`{:.3}` precision, `{:+}` sign, `{:#x}` / `{:#X}` / `{:#b}` /
`{:#o}` alt prefixes, `{:b}` / `{:o}` no-prefix. Combined specs
respect canonical ordering (`{:#05x}` → `0x0ff`). New
diagnostics MT6011 (`UNSUPPORTED_FORMAT_TYPE`), MT6012
(`MALFORMED_FORMAT_WIDTH`), MT6013 (`MALFORMED_FORMAT_PRECISION`).
Defers positional `{0}`, dynamic `{:1$}`/`{:.*}`, explicit
`n=v` named-arg passthrough to v0.26 — closes v0.24 Track E
gap F; 64 new tests + 6 conformance fixtures. **Track E**
lands real `std.String` (UTF-8 byte string, `Vec<u8>`-backed,
`String.new` / `with_capacity` / `from_str` / `from_utf8` /
`len` (bytes) / `push_str` / `push` / `clear` cap-preserve /
`as_str` / `to_str`, no `unsafe`) and `std.Vec[T]`
(`#[repr(transparent)]` over `std::vec::Vec<T>` so the wasm
Component ABI `list<T>` layout matches; `new` / `with_capacity` /
`push` / `pop` / `get` / `len` / `is_empty` / `clear` / `iter`)
in `mty-stdlib`; new `examples/26_string_vec.mty`; 41 unit
tests. **Track F** rewrites demo 06_canvas_game canvas-direct
against Tracks A–E's outputs — `agent Notetris: NotetrisInput
{ board: [U32; 200] = [0; 200], score = 0, ... }` is the
protocol of record, `frame(dt)` opens `let canvas = ...` and
routes 30+ render ops through the local, HUD lines use
`format!("score: {:>4}", n)`. JS shim drops **213 → 110 LOC
(−48 %)**; Mighty source grows 186 → 313 LOC (now carries the
canonical agent decl + canvas-direct render + Vec[U32] board
construction). Surfaces **5 narrow v0.26 gaps**: (§A) canvas-
handle taint through fn params; (§B) extern_js kebab-vs-`_`
drift through `wit-component`; (§C) wasm32-web agent
persistence emitter-side; (§D) `const` identifier in match
patterns; (§E) `format!("{n}", n=value)` named-arg shorthand.
**Integrator fixes (this tag commit):** orchestrator commit
`4b8ae7a` ("ci: fix clippy-strict failures across v0.25
swarm") pinned 5 cross-track clippy-strict lints
(`manual_let_else` + 4 others) the unified
`-D warnings` sweep surfaced that no individual track ran;
this tag commit fixes two example-file formatter idempotence
drifts in `examples/25_agent_array.mty` (CRLF line ending) +
`examples/26_string_vec.mty` (blank lines around a `// -----`
divider). **KNOWN_ISSUES net: −1.** P2 #8 (wasm32-web Unit-fn
stack-balance) resolved by Track A's `emit_call` fix; P2 #9
(demo 06 RAF-mid-frame phash flake, 4/5 success, not a
required-gate blocker) stays open. P1 stays empty. **v1.0
freeze gate status: unchanged structurally.** Blockers #1 + #3
stay CLOSED; #2 (8 RFC comment windows) infra + dashboard
stay live; earliest possible v1.0.0 tag remains
**2026-07-26**. Conformance kit grows **156 → 159 cases**
(+6 new `format_*` fixtures, replaces 3 v0.24 stubs). Rust
test count grows **1675 → 1790** (+115). Python grows **474 →
490** (+16; format-spec parser tests). Self-host driver still
at **23**. Driver bucket grows **153 → 173** (+20). Combined:
**2476** (+148). See
[`dev/history/releases/RELEASE-v0.25.md`](dev/history/releases/RELEASE-v0.25.md).

## [0.24.0] - 2026-05-26

**wasm32-web emitter completed + `format!()` + v1.0-RC5 spec
polish + deterministic `mty serve --watch`.** v0.24 closes the v0.23
Track D #1 / #2 / #3 language gaps at the emitter + macro layer,
drops a long-standing `#[ignore]` on the watcher integration test,
walks the spec from RC4 to RC5 (+414 lines normative prose; §12.6
`Resumable` / §12.7 `MT506x` reload band / §12.8 Tier 4.3
migration + `PlacementPolicy` / §20.6 cap-name resolver active
emit / §22.5 per-message work-stealing / §25.8.1-8
`mty:web/canvas@0.1` + `mty:web/input@0.1`), ships a live RFC
dashboard with per-window countdowns + per-RFC implementation
status, declares the v1.0 GA normative/informative conformance
split (104 normative / 49 informative), and rewrites demo
06_canvas_game against the new exports + `format!()` (Mighty
source 195 → 186 LOC; JS shim 235 → 213 LOC). **Track A** ships
`BuiltinId::CanvasOp(CanvasOpKind)` SIR variant + wasm32-web
dispatch arm + `is_web_callback_export` wiring (`frame` /
`keydown` / `keyup` now reach the embedded core module's export
section; 10 codegen tests). **Track B** ships `format!()` as a
first-class Mighty macro (`{}` / `{:x}` / `{:X}` / `{:?}` /
named-arg passthrough / brace escapes + MT6009 + MT6010
diagnostics; 22 integration + 19 unit tests + 3 conformance
fixtures). **Track C** drops the v0.23 `#[ignore]` on
`serve_watch_rebuilds_on_change` via an env-gated test hook
(`MTY_SERVE_TEST_WATCH_HOOK=1`) that bypasses OS-watcher event-
timing jitter; 5/5 deterministic; +2 net tests. **Track D** ships
[`docs/spec/rfcs/RFC_DASHBOARD.md`](docs/spec/rfcs/RFC_DASHBOARD.md),
annotates all 8 RFC files with `## Implementation Status`, walks
`docs/spec/v1.0-rc.md` from RC4 to RC5, and declares
[`tests/conformance/v1.0-NORMATIVE.md`](tests/conformance/v1.0-NORMATIVE.md).
**Track E** rewrites demo 06_canvas_game and surfaces **6 v0.25
gaps**: (A) HIR → IR routing for `canvas.fill_rect(...)`, (B)
Unit-returning user-fn call stack-balance failure at wasm-component
validate (KNOWN_ISSUES #8; reproduces against v0.23.0, NOT a v0.24
regression), (C) agent fields don't survive across exported-
callback invocations, (D) arrays in agent fields don't parse, (E)
`extern js { fn _foo() }` declarations don't emit wasm imports,
(F) `format!()` extended specs (width / precision / alignment)
deferred from Track B. KNOWN_ISSUES picks up entries #8 (gap B
latent emitter bug) + #9 (demo 06 headless-smoke phash flake on
RAF-mid-frame capture moments, 4/5 success rate, predates v0.24).
v1.0 freeze gate: blockers #1 + #3 stay CLOSED; #2 (8 RFC comment
windows) infra stays live + dashboard added; earliest possible
v1.0.0 tag remains **2026-07-26**. Rust test count **1604 →
1675** (+71). Python stays at **474**. Conformance kit grows
**153 → 156 cases** (+3 from Track B's format!() fixtures).
Self-host driver still at **23**. Combined: **2328** (+74). See
[`dev/history/releases/RELEASE-v0.24.md`](dev/history/releases/RELEASE-v0.24.md).

## [0.23.0] - 2026-05-26

**Mighty can run a web game on localhost.** v0.23 lands the
`mty:web/canvas@0.1` + `mty:web/input@0.1` WIT interfaces, the
`std.web` host bindings, a `wasm32-web` regression harness that
locks in the embedded core-module invariant, a `mty serve` dev
server with hot-reload + a `mty new --template web-game` scaffold,
headless-browser visual smoke for every web demo, and a 6th demo
where the Mighty agent drives the canvas via the new WIT surface.
The Tetris demo at the end of v0.22 was the right stress-test: it
surfaced exactly how thin the canvas + keyboard story was. v0.23
closes that gap end-to-end. **Track A** (canvas + keyboard WIT)
ships `crates/mty-stdlib/src/web/{canvas,input}.rs` (~430 LOC) with
`WIT_IMPORT_*` / `WIT_EXPORT_*` drift-guard constants + 8 codegen
tests + 13 stdlib unit tests covering `Canvas::clear/fill_rect/
request_animation_frame` and `Input::poll_keydown/keyup`. **Track
B** (wasm32-web embedded core module) is a no-code-change recon
outcome — the long-standing suspicion that wit-component shipped a
"header-only" component was wrong; the core module IS embedded at
byte offset 189, and a 5-test regression harness
(`crates/mty-codegen-wasm/tests/embedded_core_module.rs`) now locks
the invariant in via `wasmparser` walks against the 2055-byte
framing floor. **Track C** (`mty serve` + `mty new --template
web-game`) lands `crates/mty-cli/src/cmd/serve.rs` (+~340 LOC) with
a hand-rolled HTTP/1.1 server + RFC 6455 hand-rolled websocket
hot-reload over `notify` file watches, plus a template registry
(`crates/mty-cli/src/cmd/new.rs`) with two templates (`blank` +
`web-game`) embedded via `include_str!`; 22 tests. **Track D**
(demo 06_canvas_game) ships a 6th demo where the Mighty agent owns
score/level/piece/board and drives the canvas via Track A's WIT;
JS shim down **32% (345 → 235 LOC)** vs demo 05; headless smoke
locks in a `canvas_game.phash` golden. Three language gaps
surfaced — `BuiltinId::CanvasOp(...)` lowering, `format!()` /
interpolation, `export fn` reaching the core export table — and are
**flagged for v0.24** (the canvas-game runs; not every piece of
logic lives in Mighty source yet). **Track E** (headless-browser
visual smoke) lands `tests/web-smoke/smoke-headless.mjs` (+~380
LOC) — Playwright-driven, 8x8 average-hash perceptual-hash golden
under `tests/web-smoke/golden/<name>.phash`, hamming-distance
tolerance 12, opt-in via `MTY_WEB_SMOKE=1`, skips cleanly when
Playwright isn't installed; manual `web-smoke.yml` workflow_dispatch
job; wired into demos 02 + 05 + 06. **Integration fixes (this
tag commit):** (a) `crates/mty-cli/tests/cmd_serve.rs` port flake —
`pick_port` was nanosecond-hashed mod 10000 and collided
deterministically under workspace-wide parallel testing; replaced
with OS-assigned via `TcpListener::bind("127.0.0.1:0")` then
drop-and-reuse. (b) `crates/mty-runtime/tests/telemetry.rs`
cross-test env pollution — tests 2 + 7 (`#[tokio::test]`) set
`MTY_OTLP_ENDPOINT` while tests 8 + 9 (plain `#[test]`) raced their
remove; defensive `remove_var` at start of plain tests. (c)
`crates/mty-cli/src/cmd/new.rs` path-as-package-name bug — `mty
new --template web-game /tmp/asteroids` was substituting the full
path into `{{NAME}}` → generated `package /tmp/asteroids` → parse
error; new `package_name_from_path` helper sanitises basename to a
valid identifier + 4 new tests. (d) `tests/web-smoke/
smoke-headless.mjs` canvas-or-DOM mode — Track E's
counter-web wiring required a `<canvas>` that the counter demo
doesn't have; new `--mode {canvas,dom}` flag validates `#count` or
`[data-mty-output]` for DOM-mode demos. (e) `demos/02_counter_web/
web/serve.sh` python3 portability — Windows aliases bare `python3`
to the MS Store launcher stub; backported the cascading `python` →
`python3` → `py` lookup from demo 06's serve.sh. (f) `demos/
05_notetris_web/{mighty.toml, README.md, src/, web/}` untracked-file
recovery — the v0.22 notetris demo source had been written to disk
but never `git add`-ed (only Track E's smoke.sh was committed);
files were complete + consistent, pulled into the tag. **All gates
green, Rust test count grows 1554 → 1604** (+50: +8 Track A
codegen + +13 stdlib + +5 Track B regression harness + +22 Track C
serve/new + +5 cross-cut integrator). Python stays at **474** (no
impl-py changes in this slice). Conformance grows to **153 cases /
24 categories** (+6: Track A wasm_component additions + Track B
codegen regression cases). Self-host driver still at **23**.
Combined: **2254** (+56 vs v0.22's 2198). **KNOWN_ISSUES P1 + P2
lists stay empty.** v1.0 freeze gates: blockers #1 + #3 unchanged
(CLOSED); blocker #2 (RFC comment windows) still infrastructure-
ready, user-action pending. **Earliest v1.0.0 tag: 2026-07-26**
(unchanged).

## [0.22.0] - 2026-05-26

**All post-v1.0 roadmap items now landed pre-v1.0 — work-stealing
(Tier 5) + PGO/ThinLTO + Python full pipeline. Only RFC comment
windows remain for v1.0 GA.** v0.22 closes the v0.21 "Post-v1.0"
block end-to-end. **Per-message work-stealing (Tier 5)** lands —
the v0.10 affinity-hint scheduler is promoted to true crossbeam-
deque per-worker queues with NUMA-locality steal ordering (own
NUMA → same socket → anywhere) and a new process-wide
`worker.steals_total{src,dst}` OTel counter; the `local → siblings
→ injector` phase reversal alone produces a 61% speed-up on
pinned-task bursts vs v0.21 (1000 pinned tasks: 12.1 ms → 4.7 ms;
1000 injector tasks: 5.4 ms → 4.9 ms). New
`crates/mty-runtime/src/scheduler/work_stealing.rs` (+395 LOC) +
`scheduler/locality.rs` (+333 LOC) + `telemetry/sink.rs` (+118 LOC)
+ 7 work_stealing integration tests. **PGO + ThinLTO build
profile** lands — new `[profile.release-pgo]` cargo profile +
two-stage `scripts/build-pgo.{sh,ps1}` pipeline (instrumented
build → `mty-bench-pgo` sweep over `examples/*.mty` →
`llvm-profdata merge` → final build with `-Cprofile-use` +
`-Clinker-plugin-lto`); new `mty-bench-pgo` binary
(`crates/mty-bench/src/bin/mty-bench-pgo.rs`, +160 LOC); new
manual `.github/workflows/pgo-bench.yml` runs the pipeline on
`workflow_dispatch` and writes baseline-vs-PGO `mty check`
wall-clock delta to the workflow summary; PGO **not** wired into
`release.yml` (v0.22 ships measurement, not gating; v0.23's BOLT
follow-up turns it into the default release artifact pipeline).
**Python 2nd-impl full pipeline** lands — the impl-py 2nd-impl
now covers lex → parse → lower → typeck → borrow → wasm end-to-
end. Borrow checker (`impl-py/mty/borrow.py`, +865 LOC) is an
NLL-flavoured subset (scope-based loan lifetimes; MT3001 move-
while-borrowed, MT3002 move-out-of-borrow, MT3003 mut+shared
conflict, MT3004 use-after-move, MT3005 double `&mut`) with
branch joining via AND-of-moved-flags. Wasm codegen
(`impl-py/mty/codegen_wasm.py`, +954 LOC) emits Core 1.0 wasm
bytes — magic + 5 sections (type, function, memory, export,
code); i32 arithmetic, comparisons, bitwise, control flow,
calls, locals; if/else block-type i32; while as block+loop+br_if;
deduplicated function-type table; structural validation via
`parse_sections`. Full-pipeline sweep
(`tests/test_examples_full_pipeline.py`) parametrised over 24
examples × 4 phases = 96 cases; coverage gate `≥ 15/24 examples
emit wasm fn body`, **21/24 actual**. Python test count
**311 → 474** (+163: +28 borrow + +37 codegen + +98 sweep).
**Diagnostic-code coverage closure** activates 7 of the 8
v0.21-uncovered codes — MT0004 UNKNOWN_DURATION_UNIT + MT0030
DEPTH_LIMIT_EXCEEDED via a new `Parser::pre_lex_scan` (INT_LITERAL
+ IDENT zero-gap with duration-unit-like text and DURATION_LITERAL
+ IDENT unconditional emit MT0004; paren/brace/bracket nesting >
256 emits MT0030) + driver `parse_source` preserving
`ParseError::code` instead of funneling to UNEXPECTED_TOKEN;
MT2015 NON_EXHAUSTIVE_MATCH + MT2016 UNREACHABLE_MATCH_ARM via
`synth_match`; MT2018 IF_BRANCH_MISMATCH via `synth_expr_inner`
If branch; MT2019 RETURN_TYPE_MISMATCH via custom function-body
path in `items` (synthesises tail without expected-propagation,
unifies against ret); MT3015 USE_OF_UNINITIALIZED via
`mty-borrow::flow::walk_stmt` binding `let x: T;` as
`Ownership::Uninit`. **MT3012 DROP_IN_CONST_CONTEXT explicitly
deferred to v0.23** — HIR's `lower_item` punts on `CONST_DECL`
(`mty-hir/src/lower/items.rs:33`), so emit-site activation
requires (1) full `CONST_DECL → HirConst` lowering, (2) a
const-context flag propagated through the HIR walker,
(3) a borrow-check pass over const initialisers — each a slice's
worth of work; bundling them into the closure slice would burst
its scope. +7 conformance fixtures (`parser/02`, `parser/03`,
`type_checking/28..31`, `borrow_checking/15`). Coverage delta:
covered 62 → 69 (+7), uncovered 8 → 1 (-7, MT3012), direct % 56
→ 63, any-harness % 93 → 99. **MtyIR `Stmt` source-span carrier**
lands — every MtyIR `Stmt` + `Terminator` now carries a real
`SourceSpan` field (default `SourceSpan::ZERO` for manually-
constructed programs); HIR spans propagate through
`lower → MtyIR → cranelift SourceLoc → DWARF v5 line row`, so
v0.21's synthetic-uniform per-statement byte-offset spread is
gone and `gdb step-line` is byte-accurate. `mty-ir/src/ir.rs`
(+74 LOC), `lower/{ctx, exprs, items, stmts, mod}.rs` (+308 LOC
across), `mty-codegen-cranelift/src/lower.rs` (+29 LOC reads
`stmt.span.start_byte`), +5 spans tests in `mty-ir/tests/spans.rs`
+ extended `debug_mach_src_loc.rs` (new
`dwarf5_row_byte_offsets_match_source`). All gates green:
**1554 Rust tests** (+25 vs v0.21), **474 Python tests**
(+163 vs v0.21), **147 conformance cases** (+7), **23 self-host
driver** tests (unchanged), **2198 combined** (+195 vs v0.21's
2003). KNOWN_ISSUES P1 + P2 stay empty.

## [0.21.0] - 2026-05-26

**The post-v1.0 roadmap continues to land pre-v1.0 — Polonius
borrows + cap-name resolver + Tier 4.3 lossless live migration +
DWARF v5 dense rows.** v0.21 finishes everything v0.20 deferred
and lands the last three items from the v0.19 "Post-v1.0" block.
**Hot reload (Tier 1.5) completes**: `MT5064` placeholder is gone —
new `crates/mty-runtime/src/reload/wasm_loader.rs` parses
`__mty_agent_type` + `__mty_schema_hash` custom sections via
`wasmparser`; `Program::with_swapped_agent` clones the per-agent
slot map; `MigrateFrom<Old>` + a `SchemaRegistry` BFS over
`(old_hash, new_hash)` edges supports schema-evolution chains
(V1 → V2 → V3 supported); the control-socket `op=reload` handler
is end-to-end via `Request::Reload { agent_type, module_b64,
deadline_ms }` + `ReloadHook` trait + process-global
`reload_hooks()` registry; the 1 ms busy-poll is gone, replaced
with a `condvar_drain::DrainSignal` (parking_lot `Condvar` over
`Mutex<DrainState>`). +27 reload tests across `reload_wasm.rs`
(6), `reload_migration.rs` (8), updated `reload.rs` baseline,
and inline control-socket / condvar / resumable / wasm_loader
tests (65 reload-related tests across the crate).
**Tier 4.3 lossless live agent migration (RFC-006)** lands: new
`crates/mty-runtime/src/cluster/migration.rs` (~680 LOC) carries
`MigrationOrchestrator::migrate_agent(agent, target, deadline)`
running the canonical drain → snapshot → ship
`WireFrame::MigrateSnapshot` → `MigrateAck` → forward queued
mailbox → mark agent `REMOTE(target, new_id)` sequence; abstracted
over the runtime via three hooks (`SnapshotSource` / `SnapshotSink`
/ mesh wire surface) so `agent.rs` / `runtime.rs` stay untouched;
6 MB hard cap on snapshot payload
(`MAX_MIGRATION_SNAPSHOT_BYTES`); new `MT507x` diagnostic band
reserved for migration (MT5071 AgentNotFound / MT5072
TargetUnreachable / MT5073 SameNode / MT5074 Deadline / MT5075
Rejected / MT5076 SnapshotTooLarge / MT5077 Mesh / MT5079
Internal — plus MT5060 IncompatibleSchema shared with reload);
new `crates/mty-runtime/src/cluster/placement.rs` (~250 LOC)
lands `PlacementPolicy` trait + 3 bundled policies (`StickyPolicy`,
`LeastLoadedPolicy`, `StaticPolicy`); supervisor's
`RestartRequested` event now carries
`placement_hint: Option<NodeId>`; new `[cluster.placement]`
manifest block with `policy = "sticky"|"least_loaded"|"static"` +
`default_node`; OTel cluster metrics (migrations_started_total /
migrations_completed_total / migrations_failed_total /
migrations_rolled_back_total / migration_state_bytes_sum /
placements_chosen_total{policy}); +8 migration tests in
`tests/cluster_migration.rs`. **DWARF v5 MachSrcLoc plumbing**:
cranelift's per-instruction `MachSrcLoc` map flows through
`Module::define_function` so the v0.20 conservative 2-entry line
table is replaced with a dense per-statement line program;
`LowerCtx` grows `fn_debug: HashMap<IrFnId, FnSrcLocMap>` + a
`capture_debug_info` flag; `FnLower::note_stmt_loc(byte_offset)`
pushes synthetic byte offsets into `stmt_byte_offsets[idx]` and
calls `b.set_srcloc(SourceLoc::new(idx))`; `lower_one_block`
invokes `note_stmt_loc` at every MtyIR statement boundary +
terminator; `.debug_loclists` per-local emitted from cranelift
slot offsets (same gap as v4 today, now closed for v5); v5
binary-size delta flips from +3.2% to -2.3% vs v4 on the
synthetic benchmark (dense `DW_LNS_advance_pc` + small-delta
`DW_LNS_copy` opcodes compress better than the equivalent v4
stream once you cross ~8 rows per fn); +5 integration tests in
`crates/mty-codegen-cranelift/tests/debug_mach_src_loc.rs`
(uses `MTY_CRANELIFT_NO_OPT=1` to keep cranelift's egraph from
coalescing arithmetic chains and breaking per-statement row
determinism). **Polonius-style borrows** ship behind the
`polonius` cargo feature: datalog fact model
(`Borrow(origin, place, mut)`, `Loan(origin, scope)`,
`Subset(o1, o2, point)`, `Invalidates(origin, point)`) + 4
inference rules (transitive subset closure, loan-region
intersection, mutual-borrow conflict, end-of-scope loan death)
+ fixpoint solver layered on the v0.3-vintage NLL walker;
default build uses NLL unchanged so v0.21 default semantics are
byte-identical to v0.20; +20 tests (10 integration + 10 inline)
in `crates/mty-borrow`. **Cap-name resolver**: new
`crates/mty-types/src/cap_resolver.rs` + `cap_check.rs` lands a
3-layer scope frame (current fn signature, enclosing impl/trait,
module-level prelude) pinning `Fs` / `Net` / `Clock` / `Dom` /
`Model` names against their cap family + narrowing surface; the
6 v0.20-uncovered MT4xxx codes (MT4060 Unbound / MT4061
FamilyMismatch / MT4062 NarrowingParamMismatch / MT4063
NarrowingInBodyButNotSignature / MT4064 FamilySurfaceInconsistency
/ MT4065 NarrowingConstructorArgShape) now actively emit; +18
unit tests in `tests/cap_resolution.rs`; +6 conformance fixtures
in `tests/conformance/type_checking/22..27/`. **Conformance
expansion**: per-backend test crates
`crates/mty-codegen-cranelift/tests/conformance_native.rs` (5
tests: 4 per-case object-shape MUSTs + best-effort `cc` link-and-
run smoke + 1 inventory) and `crates/mty-codegen-wasm/tests/
conformance_wasm_component.rs` (5 tests: 4 per-case import/export-
subset MUSTs against `expected_component.txt` + 1 inventory);
`tests/conformance/coverage.json` audit reconciles the v0.20
report against the actual fixture corpus — 9 codes promote from
`uncovered` → `covered` without writing new fixtures (MT2003 /
MT2009 / MT2014 / MT2022 / MT2023 / MT2024 / MT2025 / MT3002 /
MT3007 — existing v0.11/v0.12 emit-site work + fixture coverage
was already there); true gap drops 17 → 8; coverage 53 → 62
direct (56%) and 93% any-harness. The 8 remaining gaps (MT0004
/ MT0030 / MT2015 / MT2016 / MT2018 / MT2019 / MT3012 / MT3015)
need crate-source emit-site work + HIR shape gap closure, all
documented in the new `v0_21_audit_note` field of `coverage.json`
for v0.22 follow-up. **`docs/internals/cluster.md`** gains a new
`## Live migration (v0.21 Tier 4.3)` section with the sequence
diagram, the three-hook abstraction, the wire-frame shape, and
the placement-policy surface; `docs/internals/borrowck.md` gains
§21 Polonius; `docs/internals/capabilities.md` gains a v0.21
§Cap name resolution section; `docs/internals/hot-reload.md`
gains wasm-byte loading + schema-migration + condvar-drain +
control-socket protocol sections; RFC-006 now cross-references
the implementation at `docs/internals/cluster.md#live-migration`.
**KNOWN_ISSUES P1+P2 lists stay empty.** **v1.0 freeze blockers
unchanged from v0.19/v0.20**: #1 + #3 CLOSED, #2 infrastructure
live, awaits user-side Discussion-thread openings.
**1529 Rust + 311 Python + 140 conformance + 23 selfhost-driver =
2003 tests passing** (+96 vs v0.20), 0 failing, 7 ignored
(unchanged), 0 clippy warnings under the strict `pedantic` gate,
all 6 CI workflows green (CI / Pages / Python second-impl / bench
/ security / release), conformance kit ~108 K (unchanged)
auto-attached to v0.21.0 alongside Linux x86_64 + macOS arm64 +
Windows x86_64 binaries.

## [0.20.0] - 2026-05-26

**The full post-v1.0 roadmap is now live pre-v1.0 — hot reload,
cluster mTLS+supervisor, DWARF v5, byte-identical replay all
landed.** v0.20 collapses the entire `### Post-v1.0` block from
the v0.19 README roadmap into shipping code. **Hot reload (Tier
1.5)** ships: new `Resumable` trait (FNV-1a `SCHEMA_HASH` const +
default ciborium-backed `to_snapshot`/`from_snapshot`), the swap
pipeline (`reload::swap` — pause → drain → snapshot → schema
check → restore → resume via `ReloadGate`), `ModuleSource::SameProgram`
wired end-to-end (`ModuleSource::WasmBytes` rejected with `MT5064`
until v0.21), the `mty reload <agent-type> --from new.wasm` CLI
with `--dry-run`/`--deadline-ms`/`--sock`/`--json` flags, the new
diagnostic band `MT5060–MT5069` (IncompatibleSchema / AgentNotFound
/ DrainDeadline / Snapshot / WasmReloadNotImplemented / Internal),
and +24 tests across `crates/mty-runtime/tests/reload.rs` (9) +
inline `resumable.rs` (7) + `swap.rs` (5) + `cmd/reload.rs` (3).
**Cluster mTLS + Tier 4.2 supervisor** ships: new `cluster/tls.rs`
builds rustls accept/connect configs and pins
`verify_peer_identity(node_id, cert_der)` as a custom
`ServerCertVerifier`-driven post-handshake check; a hand-rolled
~50-LOC `extract_cn_from_der` TLV walker pulls the cert CN
(no extra dep — `x509-cert` was already transitively present via
sigstore but a single function isn't worth dep promotion); mTLS is
opt-in via the new `ClusterMesh::from_config_mtls(cfg)` constructor
(`ClusterConfig` shape unchanged so v0.18/v0.19 struct-literal
callers compile clean); new `cluster/supervisor.rs` lands
`ClusterSupervisor` with per-child state machine + 3 restart
strategies (`OneForOne`/`RestForOne`/`OneForAll`) + per-child
circuit breaker (sliding-window failure count, half-open/closed
recovery); restart decisions emit on a bounded
`SUPERVISOR_EVENT_CAPACITY = 256` channel rather than invoking
synchronously (caller picks placement; v0.21 lands `PlacementPolicy`);
mesh `notify_node_disconnect` hook marks affected children
`:noproc`; +13 tests across `cluster_mtls` (5) + `cluster_supervisor`
(6) + inline cert-walker tests (4). **DWARF v5** ships as opt-in
via `MTY_DWARF5=1` (env var, not Cargo feature — feature
unification would invalidate caches for v4 path on every test):
new `crates/mty-debuginfo/src/dwarf5.rs` (~330 LOC) emits the v5
`.debug_info` + `.debug_line` + `.debug_str` + `.debug_line_str` +
`.debug_abbrev` quintuple via `gimli::write::Dwarf::new_5()`;
`mty-codegen-cranelift/src/debug.rs` gains `build_dwarf_dispatch`;
v5 *capacity* for per-instruction line rows + cross-CU
`.debug_line_str` sharing is wired (defensive monotonic-address
skip on `gimli::write::LineProgram::generate_row`; `FileId(0)`
re-add trick because the v5 `LineProgram::new` auto-inserts
comp_file at index 0 but doesn't return its id); the *enablement*
of those wins waits on cranelift `MachSrcLoc` plumbing
(v0.21 follow-up); +5 integration tests in
`crates/mty-debuginfo/tests/dwarf5.rs` (header magic, indirect
string table, round-trip, monotonic drop, file-id-zero re-add).
**Strict-equality replay payloads** finishes the v0.18 hot-path
migration the v0.19 capability work parked: the two in-process
send callsites (`Runtime::send`, `Runtime::ask`) now call a new
`encode_payload_for_trace_structural(&[Value]) -> ReplayPayload`
helper instead of `encode_payload_for_trace`, so fresh recordings
carry `ReplayPayload::Values` payloads by default and the
`ReplayDriver`'s strict structural equality arm is the live replay
semantic (the `Opaque ≈ Opaque` loose-equality arm stays as a
backwards-compat fallback that never fires for fresh recordings;
cluster routing paths still use the byte envelope by transport
contract — the receiver structurally decodes on the other side of
the mesh); +5 strict-equality tests in
`crates/mty-runtime/tests/replay_strict_equality.rs`. **Spec
cross-reference polish** lands: 7 broken internal anchor refs in
`docs/spec/v1.0-rc.md` fixed (python-markdown `toc.slugify` collapses
non-word runs to single hyphens, so em-dash and inline-code
headings never produced double-hyphen slugs; audited via a Python
script that round-trips every heading through `slugify` and diffs
against every `](#...)` reference); one stale RFC-009 cross-ref
in `docs/spec/rfcs/RFC-008-effect-rows.md` replaced with "deferred
to a future RFC." **Conformance corpus expansion**: the four
placeholder categories from v0.19 are populated (`deterministic_replay/`
+5, `formatter_idempotence/` +5, `native_abi/` +4, `wasm_component/`
+4 = +18 cases / 122 → 140); new machine-readable
`tests/conformance/coverage.json` (53 covered / 42 auxiliary / 17
uncovered, the uncovered set unchanged from v0.11);
`.github/workflows/release.yml` gains a `conformance-kit` job that
runs in parallel with `build`, shell-execs
`scripts/build-conformance-kit.sh <tag>`, and includes the
resulting `mty-conformance-kit-<version>.tar.gz` (~108 K) in the
release's `files:` list. **KNOWN_ISSUES P1+P2 lists stay empty.**
**v1.0 freeze blockers unchanged from v0.19**: #1 + #3 CLOSED, #2
infrastructure live, awaits user-side Discussion-thread openings.
**1433 Rust + 311 Python + 140 conformance + 23 selfhost-driver =
1907 tests passing** (+73 vs v0.19), 0 failing, 2 ignored
(`capability_checking/03_narrow_to_ro`, `supervisor_restart/02_escalate`
— both pending the cap-name resolver wiring + escalation-chain
serialisation rework, both post-v1.0 backlog). Two new docs pages
land (`docs/internals/hot-reload.md`,
`docs/reference/cli/mty-reload.md`); `docs/internals/cluster.md`
and `docs/internals/debug-info.md` extended with mTLS / supervisor
and DWARF v5 sections; `mkdocs.yml` nav extended with both new
pages; `mkdocs build --strict` passes locally. **Earliest
possible v1.0.0 tag: 2026-07-26** (unchanged from v0.19; gated on
RFC-002 / RFC-006 comment windows closing).
[Release notes](dev/history/releases/RELEASE-v0.20.md).

## [0.19.0] - 2026-05-26

**The last minor before v1.0-RC — Blockers #1 + #3 closed, every
KNOWN_ISSUES P1/P2 cleared, full cluster routing + byte-identical
replay land.** v0.19 closes two of the three v1.0-freeze blockers
(#1 Python 2nd-impl through HM + closures + generic-constraints with
+37 new tests; #3 normative conformance kit + spec doc +
`scripts/build-conformance-kit.sh`) and ships the tracking
infrastructure for the third (#2 RFC comment-window tracking via
`docs/spec/rfcs/COMMENT_WINDOWS.md`; the actual window-opening is a
user-driven admin action). The replay subsystem grows a **byte-identical
re-execution** mode on wire-format v2: `ReplayPayload::Values` carries
a structural mirror of the IR `Value` type (13 variants), `ReplayDriver`
re-runs the original program against the trace and diffs each event
byte-for-byte, `mty replay --byte-identical --program <path>` is the
CLI seam, v0.18 (`version=1`) traces decode transparently via the
`V1TraceFile` back-compat shim, +24 tests in
`crates/mty-runtime/tests/replay_byte_identical.rs` + unit-test files.
**Cluster routing wires into the Runtime hot path** (Tier 4.1
follow-up): `Runtime::with_cluster(SharedRouter)` +
`send_addr(AgentAddr, …)` + `ask_addr(AgentAddr, …)` consult the
router; a new `CorrelationTable` (`cluster/correlation.rs`) demuxes
inbound `Reply` / `Error` frames into oneshot receivers; a reply-demux
task peels reply frames off the mesh inbox before the runtime sees
them; peer-disconnect fan-out cleanly fails every in-flight ask to
that node (`MT5032`); a `[cluster]` / `[[cluster.peers]]` /
`[cluster.tls]` manifest parser lands in `mty-driver/src/manifest.rs`;
+8 integration tests in `tests/cluster_routing.rs`. **HIR lowerer
reads every row var**: `EffectClause::row_var_names()` (new AST
iterator) chains the three source positions in order;
`lower_effect_clause` collects every var into a fully-populated
`Vec<HirRowVar>`; the v0.15 first-only `row_var_name()` accessor is
`#[deprecated(since = "0.19.0", …)]`; +14 tests; `examples/24_multi_row_full.mty`
typechecks. **Paper-cuts cleared**: KNOWN_ISSUES #4 (`clippy-strict`
required) re-verified, KNOWN_ISSUES #5 (`mkdocs --strict`) re-verified,
KNOWN_ISSUES #7 (`--no-default-features` example sweep) added to the
`test-minimal` job; the vendored `wasi_snapshot_preview1.*.wasm`
bytes are deleted (~125 KB removed) in favour of caller-supplied
bytes via `AdapterEmbed::new(AdapterKind, Vec<u8>)`. **All
KNOWN_ISSUES P1/P2 entries are now closed.** The release workflow
that first fired on v0.15.0 continues to ship `mty` binaries for
Linux / macOS arm64 / Windows on every `v*` tag push (Intel macOS
dropped in v0.18). **1378 Rust + 311 Python + 122 conformance + 23
selfhost-driver = 1834 tests passing** (+121 vs v0.18), 0 failing,
2 ignored (`capability_checking/03_narrow_to_ro`,
`supervisor_restart/02_escalate` — both pending the cap-name
resolver wiring + escalation-chain serialisation rework, both post-v1.0
backlog). One new internals doc page lands
(`docs/internals/conformance.md`); `docs/reference/README.md` rewrites
from stub to full landing page; `mkdocs.yml` nav extended with the
new pages + a top-level **RFCs** section; `mkdocs build --strict`
passes locally. **Earliest possible v1.0.0 tag: 2026-07-26.**
[Release notes](dev/history/releases/RELEASE-v0.19.md).

## [0.18.0] - 2026-05-26

**v1.0 freeze gates closing fast — KNOWN_ISSUES P1 list cleared
(#1, #2, #3), replay end-to-end, distributed agents land.** v0.18
clears every P1 entry on `KNOWN_ISSUES.md`, wires deterministic
replay into the Runtime hot path across 13 instrumentation sites,
and grows the agent runtime a distributed transport layer (Tier 4.1
of `docs/internals/agent-features-roadmap.md`). The spec promotes
to **v1.0-RC4** with the RFC-008 multi-row-variable parser grammar
amendment at §9.2. The `cabi_realloc` real free-list allocator
(KNOWN_ISSUES #1) extracts from inline-in-emit to its own
`cabi_realloc.rs` module (8 size classes, ~190 wasm instructions, 17
dedicated coverage tests); the `mty-pkg/sigstore-real` cargo feature
(KNOWN_ISSUES #2) now compiles and drives the real keyless flow
end-to-end (Fulcio short-lived ECDSA-P256 cert + Rekor
`hashedrekord` upload with full standard Sigstore Bundle JSON
embedded under `verificationMaterial.sigstoreBundle`; `cosign
verify-blob` consumes it directly); the v0.17 replay recorder wires
into `Runtime::{spawn_agent, send, ask, shutdown}`, `agent.rs`'s
inner `run_one_turn_with_shared_reply`, the agent loop's
budget-exhaust / cancellation / terminal-exit arms, and every
`StdHost::effect_call` route for fs / http / time / random (13
sites total, zero overhead when `MTY_RECORD_TRACE` is unset);
`AgentAddr = node:type:pid` + `ClusterMesh` with framed CBOR over
TLS lands the Tier 4.1 transport layer (`Runtime::send` consults
the router in v0.19); the parser tail accepts `(',' RowVar)*` so
the multi-row source forms (`!{| E1, E2}` / `effect a, b | E1, E2`)
parse cleanly and flip MT4059 to active emit; the MSRV gate
(KNOWN_ISSUES #3) hardens to `cargo build --workspace --tests`
which pulls in the full `[dev-dependencies]` graph. The release
workflow that first fired on v0.15.0 continues to ship `mty`
binaries for Linux / macOS×2 / Windows on every `v*` tag push.
**1324 Rust + 274 Python + 92 conformance + 23 selfhost-driver =
1713 tests passing** (+50 vs v0.17), 0 failing, 5 ignored. Three
new internals doc pages land (`agents.md`, `introspect.md`,
`replay.md`); `mkdocs build --strict` passes locally.
[Release notes](dev/history/releases/RELEASE-v0.18.md).

## [0.17.0] - 2026-05-26

**WASI Preview 2 adapter goes away (`log()` direct), deterministic
replay + recorder land, Python 2nd-impl through typeck, RFC-008
multi-row, security bundle cleared.** v0.17 removes the last
preview1-adapter dependency in the WASI P2 hot path: `log()` /
`print()` now lower to a three-call canonical-ABI sequence on
`wasi:cli/stdout@0.2.3#get-stdout` +
`wasi:io/streams@0.2.3#[method]output-stream.blocking-write-and-flush`
+ `[resource-drop]output-stream`, and the embedded adapter flips
from always-on to opt-in (`Preview2Options::new(_).embed_adapter ==
None`; `.with_adapter(Some(WASI_P1_ADAPTER_COMMAND))` reattaches it
for back-compat builds). Tier 1.4 of
`docs/internals/agent-features-roadmap.md` lands as
`crates/mty-runtime/src/replay/{wire, recorder, mod}` (8 typed
`TraceEvent` variants, `MTYTRACE`-magic + serde-additive wire format
v1, `StepHandler` trait + `CountingStepHandler`) and a `mty replay
<trace>` CLI with `--dump-json` + `--step` + `--json` modes; the
full Runtime re-execution and hot-path wire-up are deferred to v0.18.
The Python 2nd-impl (`impl-py/`) reaches typeck for the first time
via `mty/hir.py` + `mty/lower.py` + `mty/typeck.py` (Hindley-Milner
unifier with `TyAny` absorption for shapes the v0.17 surface doesn't
yet model); all 23 `examples/*.mty` typecheck clean and the test
count grows **139 → 274** (+135), substantially closing v1.0
freeze blocker #2. RFC-008's HIR widens to
`HirEffectRow::Open(concrete, Vec<HirRowVar>)`; the
`UserRowPolyMeta` side table feeds the call-site walker so MT4055
(declaration ambiguity), MT4056 (concrete + row var with no fn-typed
param), and MT4058 (call-site arity mismatch) all reach active
emission, with MT4059 reserved for the v0.18 parser ship of
`!{| E1, E2}`. The `wasmtime` dev-dep bumps 25 → 36, clearing 15
RUSTSEC advisories (`audit.toml` ignore list shrinks 16 → 3); no
production code is affected. The release workflow that first fired
on v0.15.0 continues to ship `mty` binaries for Linux / macOS×2 /
Windows on every `v*` tag push. The spec stays at v1.0-RC3.
**1274 Rust + 274 Python + 92 conformance + 23 selfhost-driver =
1663 tests passing** (+192 vs v0.16), 0 failing, 4 ignored.
[Release notes](dev/history/releases/RELEASE-v0.17.md).

## [0.16.0] - 2026-05-26

**Observability + RFC-008 typeck-finishing tier — live agent
introspection (`mty inspect` + control socket), OpenTelemetry agent
spans, user-authored effect rows typecheck end-to-end, WASI Preview 2
fs + http direct, self-host MethodCall + custom iterators.** Tier 1.1
of `docs/internals/agent-features-roadmap.md` lands as
`crates/mty-runtime/src/introspect.rs` + `control_socket.rs` and a
new `mty inspect` CLI (pretty / JSON / `--watch` modes) wired to an
opt-in `MTY_RUNTIME_CONTROL_SOCK` Unix-domain socket; `AgentSnapshot`
exposes agent type, mailbox depth + high-water, in-flight handler +
elapsed, CPU / mem / tick budgets, and the last-N messages (opt-in
body capture) at wire `version: 1` (additive evolution). Tier 1.2 +
1.3 land as a new `telemetry/` submodule under `mty-runtime`:
`span_spawn` / `span_send` / `span_ask` / `span_handler` plus
`record_restart` + `record_budget_exhausted`; the
`agent.event(name, &[(k, v)])` helper attaches user attributes to the
active handler span; lazy init from `MTY_OTLP_ENDPOINT` keeps the
runtime cost-zero when telemetry is disabled. The v0.15 RFC-008
surface syntax is wired through typed AST accessors
(`mty-ast::effects`) → `HirEffectRow` (`Closed | Open`) on
`HirFn::effect_row` → `UserRowPolyIndex` in `mty-types::effects`;
five new diagnostic codes (**MT4055 / MT4056 / MT4057 / MT4058 /
MT4059**) are wired, MT4057 actively emits, and
`examples/22_effect_row.mty` flips from `@typeck-pending` to live in
the example sweep. The WASI P2 emitter takes nine more stdlib
lowerings direct: five `std.fs` fns (`open` / `read_file` /
`write_file` / `stat` / `close`) hit
`wasi:filesystem/types@0.2.3#descriptor.*` and four `std.http`
variants (`get` / `post` / `send` / `incoming_request_consume`) hit
`wasi:http/types@0.2.3` + `wasi:http/outgoing-handler@0.2.3`; a
latent emitter import-index bug is fixed via a new `prescan_p2_direct`
predeclare pass. The self-host Wasm codegen lowers `Rvalue::MethodCall`
through the host `ir_method_resolve(name)` bridge (v0.15 emitted
`unreachable`) and desugars `for x in custom_iter` at the selfhost-IR
layer into the iter-protocol loop-match-`Some`/`None` shape; driver
tests go **17 → 23 live / 0 ignored**. The release workflow that
first fired on v0.15.0 continues to ship `mty` binaries for Linux /
macOS×2 / Windows on every `v*` tag push. The spec stays at v1.0-RC3.
**1217 Rust + 139 Python + 92 conformance + 23 selfhost-driver = 1471
tests passing** (+43 vs v0.15), 0 failing, 4 ignored.
[Release notes](dev/history/releases/RELEASE-v0.16.md).

## [0.15.0] - 2026-05-25

**Dispatch-finishing tier — HOF dispatch end-to-end, RFC-008
surface syntax, WASI P2 default, self-host 17 codegen tests,
cross-platform release binaries.** The 19 row-polymorphic stdlib
signatures that v0.14 landed as a SHIPPED-SUBSET are now wired
through call-site dispatch: a new `BuiltinMethod.row_sig` field
threads 21 sigs across 12 method names into
`walk_expr_effects`, which instantiates fresh row variables per
call and propagates closure effects into the caller (MT4050 fires
on closed-row rejection; +10 dispatch tests). RFC-008 surface
syntax `!E` / `!{a | E}` / `!{fs, net | E}` / `effect a | E`
parses through `mty-syntax` with 4 new SyntaxKind variants
(EFFECT_SET, EFFECT_NAME, EFFECT_ROW_TAIL, EFFECT_ROW_VAR), spec
§9.2.1, +16 parser tests, and `examples/22_effect_row.mty`
(parser-only; HIR/typeck wiring is v0.16). WASI Preview 2 is now
the default for `wasm32-wasi` (explicit `--wasi=p1` retains
back-compat) and four stdlib fns (`std.random.bytes`,
`std.time.now` / `monotonic_now` / `resolution`) emit direct P2
imports through `emit.rs`; the log shim + `std.fs` / `std.http`
still route through the embedded adapter (canonical-ABI rewrite
deferred to v0.16). The self-host Wasm codegen reaches **17 live /
0 ignored** (was 13) with variant-call lowering in
`mty-ir::lower::exprs::resolve_callee` (Some/Ok/MyEnum.Variant →
`Rvalue::AdtInit`), a SwitchInt cascade for dense integer matches,
and `for i in 0..n` desugar. The deprecated
`mty_macros::expand` / `expand_to_source` API is removed (9
integration test files migrated; `mty-macros` 111 → 101 tests, 10
redundant pruned + coverage preserved). The v0.13 red-shirt
`conformance/borrow_checking/14_borrow_outlives_owner` is closed
by the one-line `SyntaxKind::BLOCK` arm in
`mty-hir::lower::exprs::is_expr_node`; conformance corpus moves
**91 → 92 cases / 16 categories / 3 → 2 ignored**. A new
`.github/workflows/release.yml` produces `mty` binaries for Linux /
macOS×2 / Windows on `v*` tag push — first run on this tag. The
spec stays at v1.0-RC3 (RFC-008 + RFC-009 remain roadmap RFCs).
**1140 Rust + 139 Python + 92 conformance + 57 self-host = 1428
tests passing** (+38 vs v0.14), 0 failing, 3 ignored.
[Release notes](dev/history/releases/RELEASE-v0.15.md).

## [0.14.0] - 2026-05-25

**Integration-and-finishing tier — WASI Preview 2 with vendored
wasmtime adapter, self-host codegen reaches example 03, set-of-scopes
hygiene now powers HIR macro resolution, KNOWN_ISSUES #11 closed.**
The WASI Preview 2 backend now embeds the upstream wasmtime v32
preview1→preview2 adapter (command / reactor / proxy under
[`crates/mty-codegen-wasm/wit/adapter/`](crates/mty-codegen-wasm/wit/adapter/))
and ships the full upstream WASI 0.2.3 WIT surface; `std.random` /
`std.time` route through new `P2DirectImport` constants direct to
preview2 origins (`std.fs` / `std.http` direct lowering is v0.15).
The v0.13 internal `mighty:cli-adapter` shim is gone — components
now run unmodified on any preview2 host. The self-host codegen
([`selfhost/codegen/wasm.mty`](selfhost/codegen/wasm.mty)) grew
~400 → ~660 LOC with three new modules
(`string_pool.mty`, `adt_layout.mty`, `pattern.mty`) and the
driver test reports **13 live / 0 ignored** (example 03 passes,
was the v0.13 single ignored). `mty-hir::lower::macros` now drives
`expand_scoped_to_source` (set-of-scopes) rather than the legacy
mangler; the legacy `expand` / `expand_to_source` API stays
callable behind a `#[deprecated(since = "0.14.0")]` shim with
removal scheduled for v0.15. Two FROZEN typeck codes land their
emit-sites (MT2003 at `check_stmt(HirStmt::Let)`, MT2023 at
`resolve_generic_args`); the other four in KNOWN_ISSUES #11
(MT2009 / MT2022 / MT2024 / MT2025) were rediscovered to already
have emit-sites from v0.12 work — issue #11 closed with a per-code
closure-history table. The conformance corpus moves **89 → 91
cases** / 16 categories / 3 ignored (red-shirt
`14_borrow_outlives_owner` traced to a one-line bug in
`mty-hir::lower::exprs::is_expr_node` missing the `BLOCK` arm —
out of v0.14 swarm scope, carried over). Stdlib HOF row-polymorphism
lands 19 more row-polymorphic signatures in a new `pub mod
stdlib_sigs` (+207 LOC) as a SHIPPED-SUBSET — the signatures + 24
tests ship; the call-site dispatch through
`prelude::BuiltinMethod` is v0.15. Integrator carve-out: MT2003
exempts `let mut xs = []` (legitimate idiom — downstream assignments
unify the element type), with a regression test pinning the
behaviour. The spec stays at v1.0-RC3. **1109 Rust + 137 Python +
91 conformance + 53 self-host = 1390 tests passing** (+67 vs
v0.13), 0 failing, 4 ignored.
[Release notes](dev/history/releases/RELEASE-v0.14.md).

## [0.13.0] - 2026-05-25

**Capability tier — end-to-end self-host complete + WASI Preview 2 +
2 new RFCs (effect rows + set-of-scopes hygiene).** The Mighty
compiler front-end + Wasm core-module back-end is now implemented in
Mighty source for the slice-1 subset:
[`selfhost/codegen/wasm.mty`](selfhost/codegen/wasm.mty) (~400 LOC)
closes the bootstrap chain lexer → parser → HIR → typeck → MtyIR →
wasm codegen, with 6/6 live driver tests passing (1 ignored — example
03's generic `Option[T]`). **The self-host milestone called for since
the v0.5 lexer port is reached.** A WASI Preview 2 backend lands
behind `--wasi=p2` (default stays `p1`): new `--world <name>` flag, a
new `[wit]` section in `mighty.toml` for user-supplied WIT, a vendored
`wasi:*@0.2.3` slice covering `cli`/`io`/`clocks`/`filesystem`/`http`/
`random`, example at [`examples/21_wasi_preview2.mty`](examples/21_wasi_preview2.mty),
user-facing matrix at [`docs/reference/wasi.md`](docs/reference/wasi.md).
Two new RFCs land with usable infrastructure: **RFC-008 effect-row
polymorphism** (`!E`, `!{a | E}`, four-case unification, subsumption)
with a 450-LOC row module in `crates/mty-types/src/effects.rs::row`
and a relaxed `stdlib_list_map_sig()`; and **RFC-009 set-of-scopes
macro hygiene** (Flatt-style scope sets) with `scopes.rs` + `hygiene.rs`
+ a new `expand_scoped()` entry point alongside the legacy mangler.
Both ship as **SHIPPED-SUBSET**: infrastructure + tests + first wired
consumer, with v0.14 follow-ups for surface-syntax parsing
(RFC-008) and mty-hir rewire (RFC-009). The spec stays at v1.0-RC3;
the conformance corpus stays at 89 cases / 16 categories / 3 ignored.
**1051 Rust + 137 Python + 89 conformance + 46 self-host = 1323 tests
passing** (+82 vs v0.12), 0 failing, 5 ignored.
[Release notes](dev/history/releases/RELEASE-v0.13.md).

## [0.12.0] - 2026-05-25

**Spec-and-evidence tier — v1.0-RC3 spec released + 4th showcase
demo + conformance Gap B/C/E partial closure + Go 3rd-impl source
landed.** The normative spec advances **v1.0-RC2 → v1.0-RC3**:
operator precedence is promoted to normative §11.1.1 (was deferred
to non-normative `docs/internals/parser.md`); the full reserved
keyword set is enumerated (63 reserved + 4 contextual + 7
reserved-for-future); the 16 Python-impl spec findings from v0.11
are codified in prose (+396 spec lines, no behaviour change). A
fourth runnable showcase lands at [`demos/04_kvstore/`](demos/04_kvstore/)
— a sharded supervised in-memory key-value store (~400 LOC)
exercising agents + protocols + supervisors + restart-on-crash +
`std.http` end-to-end (the first demo whose pitch is the
supervisor restart story). The conformance corpus gains six new
fixtures (typeck 17..20, borrow 13..14) and a real MT3007
`BORROW_OUTLIVES_OWNER` emit-site in `mty-borrow/src/flow.rs`;
the harness now reports **89 cases / 16 categories / 3 ignored**
(one new red-shirt: `borrow_checking/14_borrow_outlives_owner`
needs `pending_borrower` wired through plain assignments —
deferred to v0.13). A Go 3rd-impl lands at
[`impl-go/`](impl-go/): 4848 LOC of lexer + parser + CLI + tests,
built from `docs/spec/v1.0-rc.md` (v1.0-RC3) prose alone, with
zero peeking at `crates/mty-*`, `selfhost/`, or `impl-py/`. The
Go toolchain is not installed on the v0.12 build host so
`go test ./...` has not been run; cross-validation pending v0.13.
**Closes KNOWN_ISSUES #10 (operator precedence not normative) and
#12 (`package`/`export`/`requires` keywords not in §3.3).** **977
Rust + 135 Python + 89 conformance + 40 self-host = 1241 tests
passing**, 0 failing, 3 ignored. [Release notes](dev/history/releases/RELEASE-v0.12.md).

## [0.11.0] - 2026-05-25

**Quality tier — strict-clippy gate green + Python 2nd-impl partial
+ conformance gap closure + UX polish.** The `clippy (strict)` CI
job is now **required** (no more `continue-on-error: true`) and
clean across the whole 20-crate workspace: 2341 pedantic warnings
on baseline → 0 via a workspace-level `[lints.clippy]` allowlist
plus ~30 real fixes. **All six CI jobs now run as required gates.**
An independent Python implementation of the Mighty front-end lands
at [`impl-py/`](impl-py/): pure-Python lexer + parser (~2.5 KLOC)
built from the v1.0-RC2 spec prose alone (no peeking at
`crates/mty-syntax`, `crates/mty-ast`, or `selfhost/`); **135 tests
passing, 20/20 examples lex+parse**. **Real partial credit on v1.0
freeze blocker #1** (two independent implementations). The slice
also surfaced 16 spec findings — biggest: operator precedence is
not in the normative §11 (deferred to `docs/internals/parser.md`)
and needs to be promoted before v1.0 freeze. Normative conformance
corpus grows **88% → 91% FROZEN coverage** (62% → 70% direct), 4 of
8 documented gaps closed with two harness extensions
(warning-severity assertions; per-case `mighty.toml` via `CwdGuard`)
plus 3 new positive-fire cases (MT2012, MT6003, MT6008); the 4
deferred gaps each have a precise crate-source-edit reason recorded.
UX polish: 15 high-traffic MTxxxx codes rewritten to a consistent
Cause/Example/Fix/Spec format, all 16 tour chapters refreshed
(`.sd` → `.mty`, spec links bumped to `v1.0-rc.md`), FAQ extended
12 → 26 entries, getting-started rewritten 187 → 290 lines.
Inherited from post-v0.10.0 `main`: three macOS codegen fixes
(`LC_BUILD_VERSION` on Mach-O objects + cosmetic + CI tolerance for
missing `cc`). **977 Rust tests + 135 Python tests = 1112 total.**
[Release notes](dev/history/releases/RELEASE-v0.11.md).

## [0.10.0] - 2026-05-25

**Production cleanup + conformance audit.** Lifts the v0.9 RC-prep
stubs to real implementations: `cabi_realloc` becomes a segregated
free-list allocator (8 size classes + bump tail), sigstore signing
gets a real keyless path behind the `sigstore-real` feature (default
keeps the v0.9 SHA-256 envelope shape), the Cranelift egraph fuzz
bug is filed upstream as
[wasmtime #13476](https://github.com/bytecodealliance/wasmtime/issues/13476)
with an in-tree `MTY_CRANELIFT_NO_OPT` workaround and a new
`MTY_DUMP_CLIF` debug knob. Conformance corpus grows 16 → 81 cases
(88% FROZEN coverage). Self-host examples 04 + 05 deferrals closed —
**40/40 selfhost tests now pass**. CI hardened: MSRV gate now runs
`cargo test --no-run` + bedrock subset; `mkdocs --strict` enabled
with all 55 stale links fixed; cargo-audit job added; parallel
monomorphisation honestly reverted to sequential default after
re-benching. Major repo cleanup: 62 dev artefacts archived under
`dev/history/`, README rewritten 421 → 210 lines, root
`CHANGELOG.md` introduced, license switched from Apache-2.0/MIT dual
to **MIT-only**, repo URL bumped `hassard0/stardust` →
`hassard0/Mighty`. **977 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.10.md).

## [0.9.0] - 2026-05-24

**RC-prep + freeze-readiness.** Spec promoted to **v1.0-RC2** with all
10 OPEN amendments resolved (3 FREEZE-MVP, 7 DEFER-V1.1) and six
follow-up RFCs drafted (RFC-001..RFC-006). Brought up a four-target
cargo-fuzz harness (parser / typeck / fmt / codegen) with 27-file seed
corpus, fixed three P0 OOM parser bugs the fuzzer surfaced, and did an
audit sweep over every sibling `loop` for the same anti-pattern.
Self-hosted the MtyIR lowering on examples 01-03 (joining the v0.5
lexer, v0.6 parser, v0.8 HIR + minimal typeck — **34 self-host tests
passing**). Fixed `demos/02_counter_web`'s long-standing
`cabi_realloc` regression (3/3 demos passing again). Published the
[GitHub Pages docs site](https://hassard0.github.io/Mighty/), hardened
CI (stable/beta/nightly matrix, minimal-versions, strict, MSRV), shipped
reproducible release scripts, and landed a sigstore-style package
signing stub. **955 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.9.md).

## [0.8.0] - 2026-05-24

**Loose-end closure + self-host HIR + perf + spec v1.0-RC.** Closed 4 of
5 remaining v0.5 loose ends (proc-macro sandboxed execution with
MT6007/MT6008, real per-agent HTTP routing, LSP cross-file workspace
resolve, WIT canonical-ABI return-area for DOM strings). Self-hosted
the HIR + minimal typeck phases (~1.1 KLOC of Mighty source; 5+5 new
self-host tests). Three of four perf optimisations landed (parse +27%,
mailbox +7%, ~800 ns agent-send). Consolidated 88 spec amendments into
**v1.0-RC** at `docs/spec/v1.0-rc.md`. Closed all rebrand residuals
(runtime ABI symbols, DWARF producer, bench fixture). **927 tests
passing.** [Release notes](dev/history/releases/RELEASE-v0.8.md).

## [0.7.0-rebrand] - 2026-05-24

**Stardust → Mighty rename.** Naming-only release: 20 `sdust-*` crates
renamed to `mty-*`, `.sd` → `.mty` source extension, `star.toml`/`star.lock`
→ `mighty.toml`/`mighty.lock`, `SD####` → `MT####` diagnostic codes
(with `SD` aliases preserved for `mty explain`), WIT `stardust:*` →
`mty:*`, VS Code extension repackaged. **0 behavioural deltas — 885
tests pass byte-for-byte against v0.6.0.**
[Release notes](dev/history/releases/RELEASE-v0.7.md).

## [0.6.0] - 2026-05-24

**Multi-core + benchmarks + self-host parser.** Runtime now distributes
work across N OS threads via per-worker tokio runtimes + crossbeam-deque
work-stealing + affinity hints + lightweight migration + per-worker
stats. First honest benchmarks shipped — new `mty-bench` crate covers
six categories with Rust/Go/C++ comparators. Self-host parser subset
(~1930 LOC, 13/13 bootstrap tests, examples 01-05 covered). DOM MtyIR
lowering reaches `emit_dom_call` end-to-end. MT6001-MT6006 macro codes
merged into the central `mty-diagnostics` catalog. Per-call `FsCap`
isolation contract test. **885 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.6.md).

## [0.5.0] - 2026-05-24

**Self-hosting + dogfood completion.** Loops actually terminate via
`break`/`continue`/iterator exhaustion (bounded-fixed-point loop
borrows). Self-host lexer now round-trips byte-for-byte against the
Rust lexer. Five v0.4 dogfood stopgaps replaced with real
implementations (real `std.http.serve` over TCP, Wasm DOM imports as
a 4-method WIT interface, full `Str` method table, MtyIR
mem-budget auto-charge, `FsCap` allowlist process-wide). Macros
completion: `name!(args)` invocation, extended hygiene, cross-file
`pub macro`, proc-macro skeleton, stdlib macros. LSP advanced —
semantic tokens, rename, inlay hints, code actions, signature help,
workspace folders, semantic completion. **839 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.5.md).

## [0.4.0] - 2026-05-24

**Dogfood + ecosystem.** Three end-to-end dogfood demos
(`01_search_api`, `02_counter_web`, `03_extract_tool`) with passing
smoke scripts. Real package registry transport over GitHub Releases
REST with on-disk index cache + sha256 sidecar + deterministic
`.tar.gz` bundles + three new CLI subcommands. Hygienic declarative
macros (MT6001..MT6004 catch unknown/arity/depth/bad-arg). Self-host
lexer subset bootstrap. MtyIR loop terminator fix — loops genuinely
iterate. **692 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.4.md).

## [0.3.0] - 2026-05-25

**Soundness hardening.** Borrow checker grew NLL last-use deactivation
and field-level Places. Type checker grew scope-aware tolerance and
the formal `Sendable` trait (MT3011 at every send/ask site). Runtime
grew cooperative mid-turn cancellation, OTLP wire-format telemetry,
and slab-pool mailbox frames. Closed v0.2 cleanup backlog: stdlib
install, 6/20 wasm-CM gaps, 3 of 5 INTENTIONALLY_IGNORED conformance
cases. **623 tests passing, 20/20 wasm Components.**
[Release notes](dev/history/releases/RELEASE-v0.3.md).

## [0.2.0] - 2026-05-24

**LSP + pkg + doc + DWARF + Wasm CM + stdlib.** Closed every bullet on
the v0.1 deferral list: LSP 3.17 server with VS Code scaffold, package
manager (resolver + lockfile + path/git fetchers + publisher), doc
generator (markdown + HTML + search index), real stdlib (`std.json`,
`std.tls`, `std.http`, `std.fs`, `std.time`, `std.test`) backed by
rustls/hyper/serde_json/tokio, DWARF v4 debug info + wasm source maps,
Wasm Component Model output by default (`wit-component`). 20/20 native
+ 20/20 wasm core-module compilation. **550 tests passing.**
[Release notes](dev/history/releases/RELEASE-v0.2.md).

## [0.1.0] - 2026-05-24

**First feature-complete release.** Walked the full spec §31 roadmap
across eight slices: parser → formatter → HIR → type checker → borrow
checker → effects/capabilities/traits → MtyIR + interpreter → runtime
MVP → native (Cranelift JIT + AOT) + Wasm core module codegen. `mty
new` / `check` / `fmt` / `dump` / `run` / `build` / `explain`. 65+
diagnostic codes across MT0xxx..MT8xxx. MSRV Rust 1.85. **376 tests
passing.** [Release notes](dev/history/releases/RELEASE-v0.1.md).

[Unreleased]: https://github.com/hassard0/Mighty/compare/v0.11.0...HEAD
[0.11.0]: https://github.com/hassard0/Mighty/releases/tag/v0.11.0
[0.10.0]: https://github.com/hassard0/Mighty/releases/tag/v0.10.0
[0.9.0]: https://github.com/hassard0/Mighty/releases/tag/v0.9.0
[0.8.0]: https://github.com/hassard0/Mighty/releases/tag/v0.8.0
[0.7.0-rebrand]: https://github.com/hassard0/Mighty/releases/tag/v0.7.0-rebrand
[0.6.0]: https://github.com/hassard0/Mighty/releases/tag/v0.6.0
[0.5.0]: https://github.com/hassard0/Mighty/releases/tag/v0.5.0
[0.4.0]: https://github.com/hassard0/Mighty/releases/tag/v0.4.0
[0.3.0]: https://github.com/hassard0/Mighty/releases/tag/v0.3.0
[0.2.0]: https://github.com/hassard0/Mighty/releases/tag/v0.2.0
[0.1.0]: https://github.com/hassard0/Mighty/releases/tag/v0.1.0
