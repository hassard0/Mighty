# Mighty v0.33 — Release Notes

**Tag:** `v0.33.0`
**Date:** 2026-05-28
**Status:** SHIPPED — the *agent-first* release.

**Headline:** **Mighty v0.33 — the agent-first release.** Structured
auto-fix diagnostics make Mighty the language with the highest agent
first-shot success rate. Plus `mty agent` JSON CLI, `std.rag`,
multi-modal vision-language, `mty find`, LSP hover with examples,
web playground + agent gallery, and v0.33 benchmarks published.

v0.31 was the *DX shell*. v0.32 filled in the *debugger + multi-arch*.
v0.33 turns the corner on **the agent first-shot loop**: every
`MTxxxx` is now a structured envelope an LLM agent can apply
verbatim; `mty agent` exposes every other subcommand over NDJSON so
agents don't have to scrape human output; the LSP surfaces stdlib
examples + capability hints on hover; `mty find` lets an agent (or a
human) search the stdlib by capability ("write files") rather than
identifier. The RAG path becomes a stdlib primitive (`std.rag`),
and every LLM provider now accepts multi-modal image input. T1 was
the integration housekeeping: PGO release profile + v0.33 benchmark
rerun (replaces the v0.6 baseline numbers on the docs site).

Six tracks merge in parallel (T2-T7); T1 is the integrator's
housekeeping. All 10 demos pass `smoke.sh` pre and post; clippy /
fmt / audit green.

## Track-by-track

### T2 — `std.rag` + multi-modal vision-language

Branch `v033-track-rag`, merged at `a35c4cc`.

What ships:

- **`std.rag`** — `crates/mty-stdlib/src/rag/`: `Index` (vector +
  sparse + hybrid), `Retriever`, `Reranker`, `Pipeline`. Sits on
  top of the existing `std.memory::VectorStore` so RAG is a stdlib
  composition primitive, not a separate framework.
- **Chunking** — `crates/mty-stdlib/src/rag/chunking.rs`: sentence,
  fixed-window, semantic strategies with overlap controls.
- **Doc loaders** — `crates/mty-stdlib/src/rag/doc.rs`: text +
  markdown + (vision-language route) image-to-doc.
- **Multi-modal `Image` type** — `crates/mty-stdlib/src/llm/image.rs`:
  shared `Image` input across all four providers (Anthropic, OpenAI,
  Gemini, Bedrock). Base64 inline or URL reference.
- **Demo 10** — `demos/10_vision_rag/`: end-to-end vision-RAG demo
  (load a corpus of markdown + architecture diagrams, retrieve over
  both modalities, answer questions). `smoke.sh` passes.
- **Tour 21** — `docs/tour/21-rag-and-vision.md`: the new canonical
  RAG walkthrough.
- **Internals** — `docs/internals/rag.md`.
- **Tests** — `+59` across `crates/mty-stdlib/tests/rag_vision_e2e.rs`
  + `crates/mty-stdlib/tests/llm_multimodal.rs` + unit tests in each
  rag module.

### T3 — Web playground + agent gallery

Branch `v033-track-playground`, merged at `eb9cb2b`.

What ships:

- **Web playground** — `tools/playground/`: Vite + Monaco editor +
  WASM-compiled `mty` runtime stub. Code in browser, run in browser,
  no install. Editor highlights via the existing tree-sitter grammar;
  diagnostics surface via JSON envelopes from T4.
- **Agent gallery** — `tools/gallery/`: 7 starter `.mty` programs
  (hello-agent, tool-calling, swarm-review, eval-suite, taint-safety,
  observability, computer-use) the playground loads as templates.
- **CLI entry-point** — `crates/mty-cli/src/playground_main.rs`:
  alternate binary entry-point compiled to `wasm32-web` for the
  playground.
- **Tests** — playground is TypeScript; CI smoke is `npm run build`
  + headless playwright load.

**Ship constraint:** the WASM `mty` runtime artifact is **stubbed**
in this release. The browser UI works end-to-end against a
mock-compile + mock-run target that returns canned diagnostic
envelopes for the 7 gallery examples. Real WASM compile-and-run
lands in v0.34 once the codegen-wasm crate exposes a
`wasm32-web`-targeted self-host shape — that's the largest piece of
v0.34 work this track defers.

### T4 — Structured agent-actionable diagnostics

Branch `v033-track-diag`, merged with T5 at `79f74ec`.

What ships:

- **31 MTxxxx codes** get structured `FixEnvelope` payloads —
  `crates/mty-diagnostics/src/codes_fix.rs`. Each envelope carries a
  human label, a unified-diff `diff` string, a rationale, and a
  confidence score (0.0–1.0).
- **`MT4099` (taint)** — the marquee. Three first-class untaint
  strategies (sanitize-with-allowlist, manual-review-stub,
  capability-narrow) shipped as alternatives, not buried in prose.
- **`mty check --format json`** + **`--include-source`** — emits one
  NDJSON envelope per diagnostic; schema documented at
  `docs/internals/diagnostic-envelopes.md`.
- **Spec** — `examples/38_diag_envelopes.mty` is the canonical
  showcase + `docs/internals/diagnostic-envelopes.md` is the wire
  format.
- **Tests** — `+50` across `crates/mty-diagnostics/` +
  `crates/mty-driver/tests/json_envelope.rs`.

This is the marquee marketing claim for v0.33: **the language with
the highest agent first-shot success rate**. Every other LSP-backed
language ships diagnostics; few ship machine-readable fixes; none
make the fix the diagnostic's first-class payload.

### T5 — `mty agent` (JSON CLI protocol)

Branch `v033-track-agent-mode`, merged at `79f74ec` (took T4 with
it — T5 was developed on top of T4).

What ships:

- **`mty agent`** — `crates/mty-cli/src/cmd/agent.rs`: NDJSON over
  stdio. 9 ops in v0.33: `check`, `fix`, `run`, `build`, `find`,
  `explain`, `inspect`, `lsp_hover`, `version`. Each request is one
  JSON line, each response is one or more JSON lines.
- **Transports** — `stdio` (shipped), `http` (v0.34 stub), `unix`
  (v0.34 stub). The transport surface is plumbed; the non-stdio
  bodies will land in v0.34.
- **`--single-shot`** for one-shot scripted callers.
- **Spec** — `docs/internals/agent-mode-protocol.md` + CLI doc at
  `docs/reference/cli/mty-agent.md`.
- **Tests** — `+60` across `crates/mty-cli/tests/agent_mode.rs`.

### T6 — LSP hover with examples + See-also

Branch `v033-track-lsp-hover`, merged at `cabc429`.

What ships:

- **58 stdlib `///` examples** extracted into `crates/mty-doc/src/examples.rs`
  and surfaced on hover.
- **See-also inference** — `crates/mty-lsp/src/hover.rs`: when the
  hovered identifier has a `MTxxxx` related-code association
  (sourced from the diagnostic codes), the hover surfaces it as a
  See-also link.
- **Capability hints** — every `@tool`-decorated function's hover
  now includes its `cap:` requirement summary.
- **Integration tests** — `+20` across `crates/mty-lsp/tests/integration.rs`.
- **Doc** — `docs/internals/lsp-hover.md`.

### T7 — `mty find` (capability-tagged stdlib search)

Branch `v033-track-find`, merged at `8be1850`.

What ships:

- **`mty find <query>`** — `crates/mty-cli/src/cmd/find.rs`: BM25
  + capability-tagged ranking over the stdlib's `///` corpus.
  Examples: `mty find "write files"`, `mty find "send http"`,
  `mty find --by-capability fs.write`, `mty find "vector store" --explain`.
- **Formats** — `pretty` (default), `json` (NDJSON), `short`.
- **Index cache** — `~/.mty/find-index.json`. `--rebuild` forces a
  refresh.
- **Capability inverse mode** — `--by-capability` lists every item
  that requires a given capability ("what can write files?").
- **Spec** — `docs/reference/find.md` documents the query DSL +
  ranking.
- **Tests** — `+18` across `crates/mty-cli/tests/cmd_find.rs`.

### T1 — PGO release profile + v0.33 benchmark rerun

Integrator's track. Two pieces:

- **`[profile.release-pgo]`** already existed from v0.22 (fat LTO +
  single codegen unit + strip). v0.33 leaves the profile as-is —
  the release workflow continues to build with `release`, not
  `release-pgo`. The two-stage instrument/train/rebuild pipeline is
  documented at `docs/internals/pgo.md` and `scripts/build-pgo.sh`;
  wiring it into `release.yml` is the v0.34 follow-up.
- **Benchmark rerun on vulcan** — all 6 categories rerun with
  `target/release/mty-bench-runner --all --iters 30`. Updated:
  `docs/benchmarks/index.md` + the 6 per-category pages with v0.33
  numbers + a v0.6-baseline-for-continuity callout. Rust comparator
  numbers refreshed for parse, agent-send-latency, mailbox; Rust
  comparator for http_server is broken on the new toolchain (E0790
  in `BodyExt::collect`) and tracked as v0.34. Go + C++ comparators
  retain v0.6 (vulcan has no Go installed).

**Ship constraint:** PGO is *defined but not wired*. The
`release-pgo` profile is available locally, but `.github/workflows/release.yml`
still uses `cargo build --release`. The two-phase
profile-generate/use pipeline lives at `scripts/build-pgo.sh` for
users who want PGO today. CI integration is the explicit v0.34
follow-up.

## v0.33 benchmark numbers (vulcan)

| Category | v0.33 median | v0.6 baseline | Notes |
|---|---|---|---|
| parse_throughput | 5.64 ms | 6.19 ms | ~24 MB/s |
| agent_send_latency | 0.2 µs | 0.4 µs | P99 down from ~12 µs to ~2.5 µs |
| mailbox_throughput | 0.24 ms | 0.23 ms | ~4.2M msgs/sec |
| http_server_throughput | 0.11 ms | 0.24 ms | host-change accounts for most of the delta |
| compile_to_native | 8.03 ms | 7.88 ms | ~125k LoC/sec |
| wasm_size | 2 698 B | 2 068 B | +30% — stdlib WASI intrinsics, tracked for v0.34 dedup pass |

Cross-host deltas are shape, not absolute claims — v0.6 numbers
came from a Windows 11 dev laptop, v0.33 from vulcan (Dell, Xeon,
Ubuntu 24.04).

## Gates

Validated on vulcan (Intel Xeon multi-socket, Ubuntu 24.04, Rust
1.95.0). All green:

- `cargo build --workspace` — clean.
- `cargo test --workspace --no-fail-fast` — **2766 passed, 0
  failed** across the workspace (pre-v0.33: 2559; +207 over the 6
  feature tracks; Doc-tests stable).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean
  (after the 7-error fix shipped with this tag).
- `cargo fmt --all -- --check` — clean (after the merge-time fmt
  fixes shipped with this tag).
- `cargo audit --deny warnings` — clean.
- `cargo test -p mty-driver --test conformance_full` — 159/159.
- All 10 demos pass `smoke.sh` (the new `demos/10_vision_rag/`
  joins the existing 9).

## v0.33 follow-ups (rolled up across all 6 tracks)

### T1 — PGO + benchmarks
- Wire `release-pgo` into `.github/workflows/release.yml` (two-phase
  instrument / train / rebuild) so shipped binaries inherit the PGO
  win, not just locally-built ones.
- Refresh Go + C++ comparator numbers on vulcan (install Go + a
  modern clang++; rerun `benches/run.sh --all`).
- Fix the `http-server-throughput-rust-hyper` comparator's E0790
  (`BodyExt::collect` invocation).
- Add a Wasm size-budget gate so the v0.34 codegen doesn't silently
  regress past the v0.6 → v0.33 +30% growth.

### T2 — RAG + multi-modal
- Stream multi-modal responses (today's Image input ships
  end-to-end but streaming output for vision turns is provider-stub).
- Reranker behind a real cross-encoder model — today the v0.33
  reranker uses BM25 + cosine; the cross-encoder hook is plumbed but
  uses a heuristic in v0.33.
- Index-format pinning (`std.rag::Index` currently writes a
  Mighty-specific binary; v0.34 should add a stable schema doc).
- Demo 10 corpus expansion — replace the 2-doc + 1-diagram fixture
  with a 20+ doc corpus exercising every chunking strategy.

### T3 — Playground + gallery
- Real `wasm32-web` `mty` runtime artifact (the marquee v0.34
  item — see "Ship constraint" above).
- Persistent share URLs (today's playground stores in localStorage;
  share URLs need a tiny KV-backed shortener).
- Run telemetry — record which gallery examples users click into
  most so we can prioritise the next tour pages.
- Mobile responsive — Monaco hides below 720 px today.

### T4 — Structured diagnostics
- Backfill the remaining ~30 MTxxxx codes (v0.33 ships 31 of ~61
  in the registry).
- Reach 100% MT4xxx coverage (taint codes are the highest-leverage
  for agent first-shot success).
- Confidence-score calibration — v0.33 confidence scores are
  hand-tuned; v0.34 should add a calibration run against a labelled
  fix-success-rate corpus.
- Inline-source default — today `--include-source` is opt-in; v0.34
  should ship it on by default for the json format.

### T5 — `mty agent`
- HTTP + Unix transports (the stdio shape is shipping; non-stdio
  bodies are v0.34 stubs).
- Auth surface for HTTP transport — Bearer token + cap:
  declaration.
- Streaming responses for long ops (compile, eval, swarm).
- `mty agent --resume` to pick up an interrupted multi-op session.
- Concurrency cap + queue (today every request runs to completion
  in-process; v0.34 should add a worker pool).

### T6 — LSP hover
- Hover examples for the remaining stdlib items (today: 58 of
  ~140 public items).
- Image previews in hover (Mighty's `///` doc comments can include
  ASCII diagrams; v0.34 should surface SVG/PNG attachments).
- Hover code action chips — clicking a See-also `MTxxxx` should
  open the explain panel inline.

### T7 — `mty find`
- Index everything, not just the stdlib (today scope: stdlib only;
  v0.34 should include user-workspace symbols).
- Semantic-search hook — wire `std.memory::VectorStore` so users
  with API keys can opt into embedding-based search.
- Watch mode — `mty find --watch <query>` for the LSP path.
- Share index across workspaces (today: per-workspace cache; v0.34
  should add a system-wide stdlib index).

### Cross-cutting / integrator lessons (v0.34)

- **Vulcan disk hygiene** — vulcan filled up twice during v0.33
  swarms. The v0.34 integrator should `cargo clean` between major
  test runs and consider mounting a larger /tmp.
- **Pre-existing flakes** — `mty-runtime::work_stealing` is
  Windows-only flaky and doesn't fire on Linux. v0.34 should either
  fix the Windows path or `#[cfg]`-out the test for Windows.
- **fmt drift on merge** — v0.33 merges introduced fmt drift in 13
  files that the swarms hadn't run rustfmt on locally. v0.34 should
  add a pre-merge fmt gate to swarm worktrees.

## Onward

The v1.0 freeze-gate is unchanged: 8 RFC comment windows opened
2026-05-26, earliest close 2026-06-09 (RFC-005), latest close
2026-07-25 (RFC-002 + RFC-006). Proposed v1.0 freeze date
2026-09-01; earliest tag 2026-07-26.
