# Mighty

[![Status](https://img.shields.io/badge/status-v0.9-green)](https://github.com/hassard0/Mighty/releases/tag/v0.9.0)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#license)

Mighty is an agent-first systems programming language. It is statically
typed, ownership-based, and treats agents, protocols, capabilities, effects,
arenas, and budgets as first-class concepts. The toolchain targets both
native code (Cranelift JIT + AOT; LLVM behind `--features llvm`) and
WebAssembly (Component Model by default; bare core modules via
`--no-component`).

**v0.9 is shipped.** The v0.9 milestone tag
[`v0.9.0`](https://github.com/hassard0/Mighty/releases/tag/v0.9.0)
is the **RC-prep + freeze-readiness** milestone. It promotes the
v1.0 spec to **v1.0-RC2** with all 10 OPEN amendments resolved
(3 FREEZE-MVP, 7 DEFER-V1.1) and 6 first-draft RFCs, brings up a
four-target **cargo-fuzz** harness (parser / typeck / fmt /
codegen-cranelift), fixes the **three P0 OOM parser bugs** the
fuzzer surfaced plus an audit-sweep over every sibling loop
(see [`PARSER_AUDIT_V0_9.md`](PARSER_AUDIT_V0_9.md)), self-hosts the
MtyIR lowering on examples 01-03 (joining lexer + parser + HIR +
typeck; **34 self-host tests passing**), fixes the long-standing
demo 02 `cabi_realloc` regression (3/3 demos passing again),
publishes the [GitHub Pages docs site](https://hassard0.github.io/Mighty/),
hardens CI, and ships reproducible release scripts.

**v1.0 freeze date proposed: 2026-09-01** (~3 months from v0.9 tag).
Blockers: second independent implementation (RFC-007, v0.10), the
six RFC 30-day comment windows, and the normative conformance suite
publication. See [`SLICE_V0_9.md`](SLICE_V0_9.md) for the full
breakdown.

955 tests passing (was 927 at v0.8.0). See
[`RELEASE-v0.9.md`](RELEASE-v0.9.md) for the headline numbers and
[`SLICE_V0_9.md`](SLICE_V0_9.md) for the shipped/deferred detail.

Prior milestones remain tagged:
[`v0.8.0`](https://github.com/hassard0/Mighty/releases/tag/v0.8.0)
(loose-end closure + self-host HIR + perf + spec v1.0-RC),
[`v0.7.0-rebrand`](https://github.com/hassard0/stardust/releases/tag/v0.7.0-rebrand)
(Stardust → Mighty naming-only release, 0 behavioural deltas),
[`v0.6.0`](https://github.com/hassard0/stardust/releases/tag/v0.6.0)
(multi-core + benchmarks + self-host parser),
[`v0.5.0`](https://github.com/hassard0/stardust/releases/tag/v0.5.0)
(self-hosting + dogfood-completion),
[`v0.4.0`](https://github.com/hassard0/stardust/releases/tag/v0.4.0)
(dogfood + ecosystem),
[`v0.3.0`](https://github.com/hassard0/stardust/releases/tag/v0.3.0)
(soundness hardening),
[`v0.2.0`](https://github.com/hassard0/stardust/releases/tag/v0.2.0)
(LSP + pkg + doc + stdlib + DWARF + Wasm CM), and
[`v0.1.0`](https://github.com/hassard0/stardust/releases/tag/v0.1.0)
(initial slice 1-8 ladder).

### v0.9 highlights

- **v1.0 spec promoted to v1.0-RC2** at `docs/spec/v1.0-rc.md`. All
  10 OPEN amendments resolved: 3 FREEZE-MVP (A15, A31, A49), 7
  DEFER-V1.1 (A11, A45, A47, A94, A97, A102, A103). Six follow-up
  RFCs ship as first-drafts under `docs/spec/rfcs/`.
- **Cargo-fuzz harness** with 4 targets (parser_fuzz, typeck_fuzz,
  fmt_idempotence, codegen_fuzz). 27-file seed corpus per target.
  60-second `parser_fuzz` smoke runs cleanly post-audit (13 859
  mutations, 0 OOM, 0 panic).
- **3 P0 parser OOM bugs fixed + audit sweep** — the non-progress-
  guard family (`enum_decl`, `protocol_decl/msg`, plus 7 audit-sweep
  siblings: `struct_decl`, `trait_decl`, `impl_block`, `sandbox_decl`,
  `attribute`, `supervisor_decl`, `match_expr`, `extern_block`).
  16-test regression suite + saved-fuzz-artifact replay. Pre-fix
  these took ~5 s + 12 GB before aborting; post-fix microseconds.
- **Self-host MtyIR** — `selfhost/ir/{lib,nodes,lower}.mty` (~680
  LOC) pass 7/9 tests on examples 01-03 (04 + 05 deferred for the
  same reason as v0.8 HIR/typeck deferrals). **34 self-host tests
  passing** (4 lexer + 13 parser + 5 HIR + 5 typeck + 7 MtyIR).
- **Demo 02 `cabi_realloc` fixed** — wasm-component emitter now
  synthesises `cabi_realloc(i32, i32, i32, i32) -> i32` as a bump
  allocator initialised at `CABI_REALLOC_HEAP_BASE = 32 768`.
  `bash demos/02_counter_web/smoke.sh` PASSes (component size 1523
  bytes). **3/3 demos passing**.
- **GitHub Pages docs site** at
  [hassard0.github.io/Mighty](https://hassard0.github.io/Mighty/),
  mkdocs-material, deploys on push to main.
- **CI hardened** — matrix (stable/beta/nightly), minimal-versions,
  strict, MSRV jobs; pinned cargo + rustup caches.
- **Release scripts** (`scripts/release.{sh,ps1}`) — reproducible
  tag + push + GH release + asset upload.
- **Package-signing stub** (sigstore-style) — shape ready, real
  Fulcio integration is post-v1.0.
- **955 tests passing** (was 927 at v0.8.0; +28 net from parser
  non-progress regressions + new self-host MtyIR cases).

### v0.8 highlights

- **Loose-end closure (4/5)** — proc-macro sandboxed execution
  (100 ms wall + 100k steps + 16 MB cap; MT6007 runtime-impure /
  MT6008 resource-exceeded), real per-agent HTTP routing keyed by
  `(method, path, agent)`, LSP cross-file workspace resolve over
  the `mighty.toml` package tree, canonical-ABI return-area for DOM
  string returns from wasm-component bindings.
- **Self-host HIR + minimal typeck** — `selfhost/hir/lower.mty`
  (~960 LOC, round-trips byte-for-byte vs Rust for examples 01-03)
  and `selfhost/typeck/infer.mty` (~153 LOC, minimal HM for the
  same subset). Total self-host tests: 4 lexer + 13 parser + 5 HIR
  + 5 typeck = 27.
- **Performance optimisations (3/4 landed)** — 64-byte token cache
  with ±1-token widen for incremental re-lex (+27% parse on the 10
  KLOC fixture), `SlabPool::acquire_empty()` fast path that
  bypasses the per-slot lock for `SmallPayload::Empty` (+7% mailbox),
  `Mailbox::try_recv_many()` free function on the raw receiver
  (~800 ns agent send latency). Parallel mono was honest-reverted
  after measurement.
- **v1.0-RC spec published** at `docs/spec/v1.0-rc.md` — single
  normative document folding 88 amendments through v0.1 → v0.7, with
  12 cross-amendment contradictions reconciled. `scripts/classify_amendments.py`
  is the reproducible status-line injector.
- **Rebrand residuals closed** — runtime ABI symbols
  (`stardust_runtime_*` → `mty_runtime_*` across LLVM + Cranelift
  codegen + the matching `pub extern "C" fn` definitions), DWARF
  producer (`"stardust-0.2"` → `"mighty-0.8"`), `mty-bench` fixture
  (`stardust_10kloc()` → `mty_10kloc()`), template comment headers,
  insta snapshot source headers, and back-compat fallbacks for
  legacy `sd`/`stardust` code-block tags in `mty-doc`.

### v0.6 highlights

- **Multi-core scheduler** — `RuntimeBuilder` defaults to
  `available_parallelism()` workers (A106); each worker owns its own
  tokio current-thread runtime + crossbeam-deque with work-stealing
  across siblings (A101); driver runtime separated from worker
  runtimes so `block_on(user_main)` doesn't deadlock (A105); agent
  affinity hints (`AffinityHint::Sticky` / `Sticky(worker_id)`,
  A102); lightweight migration via routing-table retargeting on next
  spawn (A103); per-worker telemetry via `Scheduler::stats()` (A104).
  23 new runtime tests + 2 new conformance cases under
  `mailbox_ordering/06_multicore_fifo` + `07_multicore_throughput_smoke`.
- **First honest benchmarks** — new `mty-bench` workspace crate
  covers six categories (parse_throughput, agent_send_latency,
  mailbox_throughput, http_server_throughput, compile_to_native,
  wasm_size) with criterion harness + a CLI runner + per-category
  docs under `docs/benchmarks/`. Cross-language comparators ship
  as code for Rust (`tokio mpsc`, `logos`, `hyper`, `rustc`,
  `wasm32-rust`), Go (`chan`, `net/http`, `TinyGo`), and C++
  (`asio coro`, `cpp-httplib`, `clang`, `Emscripten`).
- **Self-host parser subset** — `selfhost/parser/parser.sd` at
  ~1930 LOC parses Mighty source through the MtyIR interpreter via
  a `SelfhostParserHost` bootstrap bridge; the v0.6 production
  matrix covers everything examples 01-05 reach (fn/struct/enum
  decls, all type shapes, Pratt expressions, generics, lambdas,
  macro calls, …) with 13/13 bootstrap tests passing.
- **DOM MtyIR lowering** — `BuiltinId::DomOp(name)` MtyIR variant
  + lowerer + wasm32-web `emit_dom_call` dispatch (A108) — Mighty
  source `d.set_text(...)` on a `Dom` cap now lowers to a real
  `mighty:web/dom` import call instead of an opaque MethodCall.
- **Central SD catalog** — MT6001-MT6006 (macro band) move to
  `sdust_diagnostics::codes`; `sdust_macros::diag` re-exports the
  `u16`s for compat (A107). `mty explain SDxxxx` is single-sourced.
- **FsCap per-call isolation** — contract test pins that two
  `FsCap` values with disjoint allowlists in one process never leak
  across the divide on read/write/exists/list_dir (A109).
- **885 tests pass** (+46 over v0.5), 0 clippy warnings, 20/20
  examples on native + wasm32-web Component, 3/3 demos pass
  `smoke.sh`.

### v0.5 highlights

### v0.5 highlights

- **Loop control flow + iterator protocol** — `break <value>?` /
  `continue` are real HIR nodes (A80); `for x in arr` terminates
  on iterator exhaustion via `__sdust_iter_next` (A81); borrow
  checker uses a bounded fixed-point at loop back-edges (A82,
  16-iter cap). 5 new conformance cases under
  `tests/conformance/control_flow/`.
- **Self-host lexer full diff** — the v0.4 `#[ignore]`'d
  `selfhost_lexer_full_diff_against_rust` test now passes
  byte-for-byte (loop CF + iterators unblocked it).
- **Dogfood completion (5 gaps)** — `std.http.serve` binds a real
  TCP socket (A96), `mighty:web/dom` ships as a 4-method WIT
  interface + 4 core imports (A97), the `Str` method table has
  real impls for contains/find/slice/trim/replace/etc. (A98),
  the MtyIR interp gains `MemBudgetExceeded` with byte-level
  charging on `AdtInit`/`TupleInit`/`ArrayInit` (A99), and the
  `FsCap` allowlist is enforced process-wide with `Forbidden(path)`
  results (A100).
- **Macros completion** — `name!(args)` invocation syntax + MT6001
  unknown_macro activated (A90/A91); extended hygiene over tuple/
  struct/ref patterns (A92); cross-file `pub macro` (A93);
  proc-macro skeleton parses + stores but execution is MT6006-
  gated (A94); standard macro library shipped (assert!, assert_eq!,
  assert_ne!, debug!, unreachable!) (A95).
- **LSP advanced** — semanticTokens (full+range), rename +
  prepareRename, inlayHint, codeAction (MT2021/MT2002/MT3001/
  MT4001 quick fixes), signatureHelp, workspaceFolders, and
  receiver-aware semantic completion (A74). 45 LSP tests across
  9 files.
- **839 tests pass** (+147 over v0.4), 0 clippy warnings, 20/20
  examples on native + wasm32-web Component (unchanged from v0.4),
  3/3 demos pass `smoke.sh`.

### v0.4 highlights

### v0.4 highlights

- **Three dogfood demos** at `demos/01_search_api`,
  `demos/02_counter_web`, `demos/03_extract_tool` — each with a
  `smoke.sh` + `smoke.ps1` that gates the demo on a passing run
  through the v0.4 compiler + runtime
- **Real package registry transport** — GitHub Releases REST
  client, on-disk index cache with 1-hour TTL + `If-Modified-Since`,
  sha256 sidecar verification, deterministic `.tar.gz` bundles,
  three new CLI subcommands (`search` / `info` / `login`),
  offline-first (`pkg add` / `update` use cache; `--refresh` for
  network) (A59/A60/A61)
- **Hygienic declarative macros** — new `mty-macros` crate;
  `let IDENT` bindings mangled per expansion site so macros don't
  collide with caller-scope names; MT6001..MT6004 catch unknown /
  arity-mismatch / depth-exceeded / bad-arg conditions; capped at
  `MAX_EXPANSION_DEPTH = 32` (A57/A58)
- **Self-host lexer (subset bootstrap)** — `selfhost/lexer/lexer.sd`
  compiles, type-checks, borrow-checks, and round-trips its first
  token through the host bridge (`std.io.lex_init` /
  `lex_byte_at` / `lex_emit` etc.); seven v0.3 language gaps
  catalogued in `SELFHOST_V0_4_NOTES.md` (A63)
- **MtyIR loop terminator fix** — `crates/mty-sir/src/lower/exprs.rs`
  previously collapsed every `while` / `loop` / `for` into a single
  iteration; v0.4 routes each body's terminator to `Goto(header)`
  so loops genuinely iterate. Bounded by the step budget pending
  v0.5's `break` HIR node (A62)
- **692 tests pass** (+69 over v0.3), 0 clippy warnings, 20/20
  examples on native + bare-wasm + Wasm-Component (unchanged from
  v0.3), 3/3 demos pass `smoke.sh`

### v0.3 highlights

- **NLL last-use + field-level Places** — the borrow checker now
  deactivates borrows at their last use and tracks disjoint fields
  separately (A54/A55/A56)
- **Scope-aware strict tolerance** — agent/handler/supervisor bodies
  hard-error unresolved names with MT2021 (A65); permissive scopes
  keep the slice-3 fresh-var fallback
- **Sendable trait** — formal cross-agent message-arg contract (Copy
  ∨ owned-Sized-no-refs ∨ `derive(Sendable)`); MT3011 at every
  `!Msg(...)` / `?Msg(...)` site (A65.b)
- **Cooperative mid-turn cancellation** — per-turn deadlines now
  interrupt blocking handlers (A70); closes A41
- **OTLP wire-format telemetry** — `STARDUST_OTLP_ENDPOINT` routes
  spans/metrics to any collector via tonic-gRPC (A71); closes A38
- **Slab-pool mailbox frames** — per-mailbox `SlabPool` reuses
  pre-allocated `MessageFrame` slots (A72); closes A40
- **Stdlib really runs under `mty run`** — driver wired to
  `sdust_stdlib::host::dispatch` via CLI bridge
- **20/20 wasm Components** (was 14/20 in v0.2)
- **623 tests pass** (+73 over v0.2), 0 clippy warnings

### v0.2 highlights

- **`mty lsp`** — LSP 3.17 server (diagnostics, hover, completion,
  go-to-def) plus a VS Code extension scaffold
- **`mty pkg`** — package manager (resolver + lockfile + path/git
  fetchers + publisher); CLI `add` / `remove` / `update` / `fetch` /
  `list` / `publish`
- **`mty doc`** — doc generator producing markdown or HTML with an
  item index, per-item pages, back-links, and a search index
- **20/20 native + 20/20 wasm core-module compilation** across the
  example corpus (Cranelift + wasm backend now cover ADT,
  `?`-propagation, agent handlers, monomorphization)
- **Real stdlib** (`std.json`, `std.tls`, `std.http`, `std.fs`,
  `std.time`, `std.test`) backed by `rustls`, `hyper`, `serde_json`,
  `tokio`
- **DWARF v4 debug info** (Cranelift) + wasm `name` section +
  source-map v3 sidecar
- **Wasm Component Model output by default** (`wit-component`); use
  `--no-component` for a bare core module

```bash
# Compile and JIT-run
mty run examples/01_hello.sd
# → hello, Mighty

# Build a native executable (linker-permitting)
mty build examples/01_hello.sd
# wrote target/01_hello

# Build a WebAssembly module
mty build --target wasm32-wasi examples/01_hello.sd
# wrote target/01_hello.wasm
```

The CLI ships `mty new`, `mty check`, `mty fmt`, `mty dump`,
`mty run`, `mty build`, and `mty explain`. Runtime diagnostics
range from `MT0001` (parse errors) through `MT8010` (codegen traps);
`mty explain SDxxxx` prints a paragraph describing each.

## Install

A versioned release is not yet published. Build from source:

```bash
git clone https://github.com/hassard0/stardust
cd mighty
cargo install --path crates/mty-cli
```

This installs the `mty` binary. The minimum supported Rust version is
1.85 (slice 8 bumped from 1.82 because the cranelift dependency chain
pulls in `indexmap 2.14`, which requires edition2024).

## Hello, Mighty

```bash
mty new hello
cd hello
mty check src/main.sd
```

`mty new` produces:

```sd
fn main() {
  log("hello, Mighty")
}
```

`mty check` lexes, parses, lowers, type-checks, and borrow-checks
the source, reporting any diagnostics. `mty run src/main.sd`
executes the program under the slice-6 interpreter. `mty explain
SDxxxx` prints a paragraph describing any diagnostic code emitted.

## Documentation

- [Getting started](docs/getting-started.md)
- [Tour](docs/tour/README.md) — walk through the twenty canonical examples
- [Language specification v0.1](docs/spec/v0.1.md)
- [Reference](docs/reference/README.md) — CLI, manifest, diagnostics
- [Internals](docs/internals/README.md) — compiler architecture
- [FAQ](docs/faq.md)
- [Contributing](docs/contributing.md)

## Project layout

The compiler is a Rust workspace of twenty crates:

| Crate | Responsibility |
|---|---|
| `mty-syntax` | lexer (logos), CST (rowan), parser |
| `mty-ast` | typed AST view over the CST |
| `mty-diagnostics` | diagnostic types, SD-coded labels, ariadne rendering |
| `mty-hir` | name-resolved HIR with arena storage; v0.4 macro preprocessor hook |
| `mty-types` | resolved Ty, HM inference, bidirectional type checker, effects + capabilities; v0.3 scope-strict + Sendable |
| `mty-borrow` | ownership/move/borrow/affine/arena analysis; v0.3 field-level Places + NLL last-use |
| `mty-sir` | mid-level IR + tree-walking interpreter (slice 6); v0.4 loop terminator fix |
| `mty-runtime` | concurrent tokio runtime: agents, mailboxes, supervisors, budgets (slice 7); v0.3 mid-turn cancel + OTLP + slab pool |
| `mty-codegen-cranelift` | native backend — JIT + AOT object (slice 8 + v0.2 completion) |
| `mty-codegen-wasm` | wasm32-wasi / wasm32-web core module + Component Model emitter |
| `mty-codegen-llvm` | LLVM backend (real lowering behind `--features llvm`; v0.2) |
| `mty-debuginfo` | DWARF v4 builder + wasm source-map + `name` section (v0.2) |
| `mty-fmt` | canonical formatter (Wadler/Lindig pretty-printer) |
| `mty-driver` | compilation pipeline and `mighty.toml` manifest loader |
| `mty-pkg` | package manager: resolver, lockfile, fetchers, publish (v0.2); v0.4 GH Releases registry transport |
| `mty-lsp` | LSP 3.17 server over stdio (v0.2) |
| `mty-doc` | doc generator (extract + render markdown/HTML) (v0.2) |
| `mty-stdlib` | real `std.json` / `tls` / `http` / `fs` / `time` / `test` (v0.2) |
| `mty-macros` | declarative-macro registry + expander + hygiene (v0.4) |
| `mty-cli` | the `mty` binary |

## Roadmap

The full plan is in `stardust_language_spec_v0_1.md` §31. The slices
implemented or planned:

| Slice | Scope | Status |
|---|---|---|
| 1 | parser, formatter, HIR, CLI, examples | shipped (`v0.1.0-phase1`) |
| 2 | per-node formatter, lambdas, if-let, turbofish, polish | shipped (`v0.2.0-phase1-polish`) |
| 3 | type checker, generics MVP, `?` propagation | shipped (`v0.3.0-typeck`) |
| 4 | ownership / borrow / affine / arena + slice-3 hardening | shipped (`v0.4.0-borrowck`) |
| 5 | effects, capabilities, traits, `dyn`, derives, strict protocols | shipped (`v0.5.0-effects`) |
| 6 | MtyIR and interpreter | shipped (`v0.6.0-sir`) |
| 7 | runtime MVP (scheduler, mailboxes, supervisors) | shipped (`v0.7.0-runtime`) |
| 8 | native (Cranelift) and Wasm backends | shipped (`v0.8.0-codegen` / `v0.1.0`) |
| **v0.2** | LSP + pkg + doc + full codegen + stdlib + DWARF + Wasm CM | **shipped (`v0.2.0`)** |
| **v0.3** | Soundness hardening: NLL last-use + field Places, scope-strict + Sendable, mid-turn cancel + OTLP + slab mailboxes, v0.2 cleanup (stdlib install, 20/20 wasm-CM, 5→2 ignored) | **shipped (`v0.3.0`)** |
| **v0.4** | Dogfood demos (3), real GH-Releases registry transport, hygienic declarative macros (MT6001..MT6004), self-host lexer (subset bootstrap via `std.io`), MtyIR loop terminator fix | **shipped (`v0.4.0`)** |
| **v0.5** | `break` / `continue` HIR + iterator protocol + bounded-fixed-point loop borrows, self-host lexer full diff, dogfood completion (5 gaps: real http.serve, Wasm DOM imports, full Str methods, mem-budget auto-charge, FsCap allowlist), macros completion (`name!(args)`, extended hygiene, cross-file `pub macro`, proc-macro skeleton, stdlib macros), LSP advanced (semantic tokens, rename, inlay hints, code actions, signature help, workspace folders, semantic completion) | **shipped (`v0.5.0`)** |
| **v0.6** | Multi-core scheduler (per-worker tokio runtimes + crossbeam-deque work-stealing + affinity hints + lightweight migration + per-worker stats), first honest benchmarks (mty-bench crate + 6 categories with Rust/Go/C++ comparators), self-host parser subset (~1930 LOC, 13/13 bootstrap tests, examples 01-05 covered), DOM MtyIR lowering (`BuiltinId::DomOp` end-to-end), central MT6001-MT6006 catalog merge, per-call FsCap isolation contract | **shipped (`v0.6.0`)** |
| **v0.7** | Stardust → Mighty rebrand: 20 `sdust-*` crates renamed to `mty-*`, `.sd` → `.mty` source-file extension, `star.toml`/`star.lock` → `mighty.toml`/`mighty.lock`, `SD####` → `MT####` diagnostic codes (with `SD` alias preserved for `mty explain`), WIT `stardust:*` → `mty:*`, VS Code extension repackaged. 0 behavioural deltas — 885 tests still pass byte-for-byte. | **shipped (`v0.7.0-rebrand`)** |
| **v0.8** | Loose-end closure (proc-macro sandboxed execution + MT6007/MT6008, real per-agent HTTP routing, LSP cross-file workspace resolve, WIT canonical-ABI return-area for DOM strings), self-host HIR + minimal typeck (~1.1 KLOC of Mighty self-host code; 5+5 new tests on examples 01-03), perf wins (parse +27%, mailbox +7%, agent send ~800 ns), spec consolidation **v1.0-RC** at `docs/spec/v1.0-rc.md` (88 amendments classified, 12 contradictions reconciled), rebrand residuals closed (`stardust_runtime_*` ABI + DWARF + `mty-bench` fixture + `mty-doc` templates + `mty-hir` insta headers). 927 tests passing. | **shipped (`v0.8.0`)** |
| **v0.9** | RC-prep + freeze-readiness: v1.0-RC2 spec (10 OPEN amendments resolved — 3 FREEZE-MVP + 7 DEFER-V1.1 + 6 follow-up RFCs), cargo-fuzz harness (4 targets + 27-file seed corpus), **parser non-progress-guard family fix** (3 P0 OOM bugs + 7 audit-sweep extras + 16 regression tests + 60-second clean fuzz smoke), self-host MtyIR (7/9 tests on examples 01-03; **34 self-host tests total**), `demos/02_counter_web` `cabi_realloc` fix (3/3 demos passing), GitHub Pages docs site, CI hardening (matrix + minimal-versions + strict + MSRV), release scripts, package-signing stub. 955 tests passing. | **shipped (`v0.9.0`)** |

### Post-v0.9 roadmap

The v1.0 spec is feature-complete at v1.0-RC2. **Proposed v1.0
freeze date: 2026-09-01** (~3 months from the v0.9 tag). Blockers:
two independent implementations (RFC-007), 30-day RFC comment
windows (RFC-001 through RFC-006), and a published normative
conformance suite.

Targeting **v0.10**:

| Slice | Scope | Status |
|---|---|---|
| v0.10 | Second independent compiler implementation effort (RFC-007), real `cabi_realloc` allocator (free-list replaces v0.9 bump), CI fuzz wiring (nightly 5-min + release-gate 30-min sweeps), Cranelift egraph upstream report + workaround (Bug 4 in FUZZ_V0_9_NOTES.md), self-host HIR + typeck examples 04 + 05 (still open from v0.8/v0.9), full `TokenStream` marshalling for proc-macros, `mty-pkg` cross-file resolution, parametric newtypes for self-host arena ids, WASM size + HTTP server throughput optimisation targets, set-of-scopes hygiene cleanup (A111), real sigstore integration, open RFC-001..006 30-day comment periods, publish normative conformance suite, mkdocs `--strict` cleanup | planned |
| v1.0-RC2 → v1.0 GA | Spec freeze at v1.0-final (after second-impl validation + RFC comment-period close), deprecation removal sweep (`SD####` aliases, `--legacy-interp`, legacy `sd`/`stardust` code-block tags per A45's DEFER-V1.1 resolution), stability commitment, release-candidate cycle | planned |
| - | Lossless live migration, per-message work-stealing, OTLP exporter wiring for `Scheduler::stats()` gauges, `agent X with affinity = sticky` front-end syntax | planned |
| - | Polonius-style borrows, real cap-name resolution wiring, MtyIR-side cancellation polling, WASI Preview 2 + user WIT, DWARF v5 + per-instr line program | planned |
| - | `dyn` dispatch + closure capture in compiled code, `escalate` supervisor action | planned |
| - | PGO/ThinLTO, distributed agents, effect-row polymorphism | future |

## License

Mighty is dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this work shall be dual-licensed as above,
without any additional terms or conditions.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and
[docs/contributing.md](docs/contributing.md).
