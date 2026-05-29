# Compiler internals

The Mighty compiler is a Rust workspace of **twenty crates** spanning
the frontend, codegen, runtime, formatter, package manager, doc
generator, LSP, and stdlib. This section describes the pipeline and
the responsibilities of each subsystem.

If you are looking to use the compiler, see the
[reference](../reference/README.md). If you are looking to learn the
language, see the [tour](../tour/README.md).

## Pages

**Frontend pipeline**

- [Architecture](architecture.md) — pipeline diagram and crate map.
- [Lexer](lexer.md) — the logos-based tokenizer.
- [Parser](parser.md) — recursive descent + Pratt over a rowan builder.
- [CST and AST](cst-ast.md) — the lossless tree and its typed view.
- [HIR](hir.md) — name-resolved IR with arena storage.
- [Typeck](typeck.md) — Hindley-Milner + bidirectional checker.
- [Borrowck](borrowck.md) — ownership / move / borrow / NLL.
- [Effects](effects.md) — capability + effect rows (RFC-008).
- [IR](ir.md) — mid-level MtyIR for codegen.
- [Diagnostics](diagnostics.md) — diagnostic types, MT-codes, rendering.

**Codegen + runtime**

- [Codegen (Cranelift)](codegen-cranelift.md) — native JIT/AOT
  (U8 widening + dynamic log lowering landed v0.36 T1).
- [Codegen (LLVM)](codegen-llvm.md) — opt-in LLVM backend.
- [Codegen (Wasm)](codegen-wasm.md) — wasm32-wasi + Component Model
  + real free-list `cabi_realloc` (v0.18).
- [Extern C matrix](extern-c-matrix.md) — `extern c { ... }`
  signature shapes known-good on v0.36 + `[[extern_lib]]` linker
  contract (v0.36 T2).
- [Debug Info](debug-info.md) — DWARF v4 + Wasm source maps.
- [PGO](pgo.md) — profile-guided optimisation pipeline (re-enabled
  on Linux/macOS-x86_64/Windows in v0.36 T5).
- [Runtime](runtime.md) — concurrent agent runtime architecture.
- [Agents](agents.md) — cross-cutting agent overview
  (surface → HIR → typeck → spawn → turn loop).
- [Scheduler](scheduler.md) — per-worker tokio + work-stealing.
- [Mailboxes](mailboxes.md) — bounded MPSC details.
- [Supervisors](supervisors.md) — restart strategies + rate limits.
- [Cluster](cluster.md) — distributed agents (Tier 4.1, v0.18).
- [Budgets](budgets.md) — per-handler CPU / mem / tick / wall limits.

**Observability**

- [Telemetry](telemetry.md) — slice-7 JSON-line emitter.
- [Telemetry Spans](telemetry-spans.md) — OpenTelemetry agent spans
  (Tier 1.2 + 1.3, v0.16).
- [Telemetry OTLP](telemetry-otlp.md) — OTLP bridge (span names
  renamed `stardust.*` → `mty.*` in v0.36 T4 with `legacy_span_name`
  back-compat).
- [Observability](observability.md) — `std.observe` cost-tracking
  SQLite store + `mty inspect --cost` reader.
- [Introspect](introspect.md) — `mty inspect` + control socket
  (Tier 1.1, v0.16).
- [Replay](replay.md) — deterministic replay (Tier 1.4, v0.17/18).

**Tooling + ecosystem**

- [Formatter](formatter.md) — Wadler/Lindig pretty-printer.
- [Macros](macros.md) — declarative-macro registry + expander
  + set-of-scopes hygiene (RFC-009).
- [LSP](lsp.md) — LSP 3.17 server.
- [LSP hover](lsp-hover.md) — hover catalog source-of-truth +
  drift gate (v0.35 T5).
- [Doc Generator](doc-generator.md) — markdown / HTML output.
- [Stdlib docs pipeline](stdlib-docs-pipeline.md) — per-module
  `.docstub` extraction + Strategy B drift gate (`mty doc --check`).
- [Package Manager](package-manager.md) — `mty pkg`. Legacy
  `stardust-pkg/registry` slug still resolves via
  `is_official_registry` (v0.36 T4 compat).
- [Package Signing](package-signing.md) — real Sigstore keyless
  (v0.18 with `sigstore-real` feature).
- [Fuzzing](fuzzing.md) — cargo-fuzz across parser / typeck / fmt
  / codegen.
- [Benchmarking](benchmarking.md) — criterion + the `mty-bench` runner.
- [Self-hosting](self-hosting.md) — Mighty-in-Mighty bootstrap.
- [Rename compat](rename-compat.md) — `STARDUST_*` → `MTY_*` env-var
  precedence + WIT / OTLP / registry / segment legacy-name acceptance
  (v0.36 T4).

**Spec details**

- [Capabilities](capabilities.md) — capability table + narrowing.
- [Sandbox Enforcement](sandbox-enforcement.md) — surface → cap-table.
- [Sendable](sendable.md) — cross-agent message-arg soundness.
- [Traits](traits.md) — generic dispatch + monomorphization.
- [Iterators](iterators.md) — iter-protocol.
- [Loops](loops.md) — `loop` / `while` / `for` lowering.
- [Multi-core](multi-core.md) — per-worker runtimes.
- [Stdlib](stdlib.md) — `std.*` shape.
- [Interpreter](interpreter.md) — tree-walking fallback.

**Roadmap**

- [Agent-features Roadmap](agent-features-roadmap.md) — Tier 1–5 plan.

## Where to find things

| Concern | Crate | Entry point |
|---|---|---|
| Token kinds, lexer regex | `mty-syntax` | `src/syntax_kind.rs`, `src/lexer.rs` |
| Parser productions | `mty-syntax` | `src/parser/` |
| AST accessors | `mty-ast` | `src/generated.rs` |
| Diagnostic types | `mty-diagnostics` | `src/diagnostic.rs`, `src/codes.rs` |
| HIR nodes and ids | `mty-hir` | `src/nodes.rs`, `src/ids.rs` |
| HIR lowering | `mty-hir` | `src/lower/` |
| Type checker | `mty-types` | `src/check/` |
| Borrow checker | `mty-borrow` | `src/lib.rs` |
| Mid-level IR | `mty-ir` | `src/lib.rs`, `src/lower/` |
| Wasm `cabi_realloc` allocator | `mty-codegen-wasm` | `src/cabi_realloc.rs` (v0.18 extract) |
| Wasm Preview 2 dispatch | `mty-codegen-wasm` | `src/emit.rs` (P2DirectImport tree) |
| Cranelift JIT entry | `mty-codegen-cranelift` | `src/lib.rs`, `src/jit.rs` |
| Runtime entry | `mty-runtime` | `src/runtime.rs::RuntimeBuilder` |
| Replay hot-path hooks | `mty-runtime` | `src/replay/recorder.rs::with_recorder` |
| Cluster mesh | `mty-runtime` | `src/cluster/mesh.rs` |
| Introspect snapshot | `mty-runtime` | `src/introspect.rs::AgentSnapshot` |
| OTel agent spans | `mty-runtime` | `src/telemetry/spans.rs` |
| Sigstore keyless sign | `mty-pkg` | `src/signing.rs::sign_keyless` (feature `sigstore-real`) |
| Formatter combinators | `mty-fmt` | `src/doc.rs`, `src/printer.rs` |
| Per-node format rules | `mty-fmt` | `src/fmt/` |
| Compilation pipeline | `mty-driver` | `src/pipeline.rs` |
| Manifest loader | `mty-driver` | `src/manifest.rs` |
| LSP request dispatch | `mty-lsp` | `src/lib.rs::Server::handle_request` |
| Package resolver | `mty-pkg` | `src/resolver.rs` |
| Doc generator | `mty-doc` | `src/lib.rs::render_*` |
| Stdlib bridge | `mty-stdlib` | `src/lib.rs` |
| CLI dispatch | `mty-cli` | `src/main.rs`, `src/cmd/` |

## Test corpus

The internals are exercised by a layered test corpus:

| Layer | Where | Approx count |
|---|---|---|
| Unit + per-crate integration | `crates/*/tests/`, `src/**/tests` | 1300+ Rust |
| 2nd-impl Python typeck | `impl-py/tests/` | 274 Python |
| Normative conformance | `tests/conformance/` | 159 cases / 24 categories |
| Self-host driver codegen | `crates/mty-driver/tests/selfhost_*.rs` | 23 |
| Demo end-to-end | `demos/*/smoke.sh` | 11 |
| Fuzzing | `fuzz/fuzz_targets/` | 4 targets |

CI workflow jobs (see `.github/workflows/ci.yml`): `test (ubuntu)`,
`test (macos)`, `test (windows)`, `test (minimal features)`,
`clippy (strict)`, `multi-arch dry-run`. The same checks run
locally via the `mty hooks install` pre-push hook (v0.34 T4).
