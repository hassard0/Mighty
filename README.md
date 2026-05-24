# Stardust

[![Status](https://img.shields.io/badge/status-v0.5-green)](https://github.com/hassard0/stardust/releases/tag/v0.5.0)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#license)

Stardust is an agent-first systems programming language. It is statically
typed, ownership-based, and treats agents, protocols, capabilities, effects,
arenas, and budgets as first-class concepts. The toolchain targets both
native code (Cranelift JIT + AOT; LLVM behind `--features llvm`) and
WebAssembly (Component Model by default; bare core modules via
`--no-component`).

**v0.5 is shipped.** The v0.5 milestone tag
[`v0.5.0`](https://github.com/hassard0/stardust/releases/tag/v0.5.0)
is the self-hosting + dogfood-completion release: the Stardust
source lexer now round-trips byte-for-byte against the Rust lexer
(loop control flow + iterator protocol unblock it), every v0.4
demo stopgap has its real implementation (`std.http.serve` binds
a real socket, `stardust:web/dom` ships as a Wasm Component
import, the `Str` method table has real impls, mem-budget violations
trap deterministically, `FsCap` rejects out-of-allowlist paths),
declarative macros gain `name!(args)` invocation + extended hygiene
+ cross-file `pub macro` + a proc-macro skeleton (SD6005/SD6006),
and the LSP grows seven advanced features (semantic tokens,
rename, inlay hints, code actions, signature help, workspace
folders, semantic completion). See [`RELEASE-v0.5.md`](RELEASE-v0.5.md)
for the headline numbers and [`SLICE_V0_5.md`](SLICE_V0_5.md) for
the shipped/deferred detail.

Prior milestones remain tagged:
[`v0.4.0`](https://github.com/hassard0/stardust/releases/tag/v0.4.0)
(dogfood + ecosystem),
[`v0.3.0`](https://github.com/hassard0/stardust/releases/tag/v0.3.0)
(soundness hardening),
[`v0.2.0`](https://github.com/hassard0/stardust/releases/tag/v0.2.0)
(LSP + pkg + doc + stdlib + DWARF + Wasm CM), and
[`v0.1.0`](https://github.com/hassard0/stardust/releases/tag/v0.1.0)
(initial slice 1-8 ladder).

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
  TCP socket (A96), `stardust:web/dom` ships as a 4-method WIT
  interface + 4 core imports (A97), the `Str` method table has
  real impls for contains/find/slice/trim/replace/etc. (A98),
  the SIR interp gains `MemBudgetExceeded` with byte-level
  charging on `AdtInit`/`TupleInit`/`ArrayInit` (A99), and the
  `FsCap` allowlist is enforced process-wide with `Forbidden(path)`
  results (A100).
- **Macros completion** — `name!(args)` invocation syntax + SD6001
  unknown_macro activated (A90/A91); extended hygiene over tuple/
  struct/ref patterns (A92); cross-file `pub macro` (A93);
  proc-macro skeleton parses + stores but execution is SD6006-
  gated (A94); standard macro library shipped (assert!, assert_eq!,
  assert_ne!, debug!, unreachable!) (A95).
- **LSP advanced** — semanticTokens (full+range), rename +
  prepareRename, inlayHint, codeAction (SD2021/SD2002/SD3001/
  SD4001 quick fixes), signatureHelp, workspaceFolders, and
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
- **Hygienic declarative macros** — new `sdust-macros` crate;
  `let IDENT` bindings mangled per expansion site so macros don't
  collide with caller-scope names; SD6001..SD6004 catch unknown /
  arity-mismatch / depth-exceeded / bad-arg conditions; capped at
  `MAX_EXPANSION_DEPTH = 32` (A57/A58)
- **Self-host lexer (subset bootstrap)** — `selfhost/lexer/lexer.sd`
  compiles, type-checks, borrow-checks, and round-trips its first
  token through the host bridge (`std.io.lex_init` /
  `lex_byte_at` / `lex_emit` etc.); seven v0.3 language gaps
  catalogued in `SELFHOST_V0_4_NOTES.md` (A63)
- **SIR loop terminator fix** — `crates/sdust-sir/src/lower/exprs.rs`
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
  hard-error unresolved names with SD2021 (A65); permissive scopes
  keep the slice-3 fresh-var fallback
- **Sendable trait** — formal cross-agent message-arg contract (Copy
  ∨ owned-Sized-no-refs ∨ `derive(Sendable)`); SD3011 at every
  `!Msg(...)` / `?Msg(...)` site (A65.b)
- **Cooperative mid-turn cancellation** — per-turn deadlines now
  interrupt blocking handlers (A70); closes A41
- **OTLP wire-format telemetry** — `STARDUST_OTLP_ENDPOINT` routes
  spans/metrics to any collector via tonic-gRPC (A71); closes A38
- **Slab-pool mailbox frames** — per-mailbox `SlabPool` reuses
  pre-allocated `MessageFrame` slots (A72); closes A40
- **Stdlib really runs under `sdust run`** — driver wired to
  `sdust_stdlib::host::dispatch` via CLI bridge
- **20/20 wasm Components** (was 14/20 in v0.2)
- **623 tests pass** (+73 over v0.2), 0 clippy warnings

### v0.2 highlights

- **`sdust lsp`** — LSP 3.17 server (diagnostics, hover, completion,
  go-to-def) plus a VS Code extension scaffold
- **`sdust pkg`** — package manager (resolver + lockfile + path/git
  fetchers + publisher); CLI `add` / `remove` / `update` / `fetch` /
  `list` / `publish`
- **`sdust doc`** — doc generator producing markdown or HTML with an
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
sdust run examples/01_hello.sd
# → hello, Stardust

# Build a native executable (linker-permitting)
sdust build examples/01_hello.sd
# wrote target/01_hello

# Build a WebAssembly module
sdust build --target wasm32-wasi examples/01_hello.sd
# wrote target/01_hello.wasm
```

The CLI ships `sdust new`, `sdust check`, `sdust fmt`, `sdust dump`,
`sdust run`, `sdust build`, and `sdust explain`. Runtime diagnostics
range from `SD0001` (parse errors) through `SD8010` (codegen traps);
`sdust explain SDxxxx` prints a paragraph describing each.

## Install

A versioned release is not yet published. Build from source:

```bash
git clone https://github.com/hassard0/stardust
cd stardust
cargo install --path crates/sdust-cli
```

This installs the `sdust` binary. The minimum supported Rust version is
1.85 (slice 8 bumped from 1.82 because the cranelift dependency chain
pulls in `indexmap 2.14`, which requires edition2024).

## Hello, Stardust

```bash
sdust new hello
cd hello
sdust check src/main.sd
```

`sdust new` produces:

```sd
fn main() {
  log("hello, Stardust")
}
```

`sdust check` lexes, parses, lowers, type-checks, and borrow-checks
the source, reporting any diagnostics. `sdust run src/main.sd`
executes the program under the slice-6 interpreter. `sdust explain
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
| `sdust-syntax` | lexer (logos), CST (rowan), parser |
| `sdust-ast` | typed AST view over the CST |
| `sdust-diagnostics` | diagnostic types, SD-coded labels, ariadne rendering |
| `sdust-hir` | name-resolved HIR with arena storage; v0.4 macro preprocessor hook |
| `sdust-types` | resolved Ty, HM inference, bidirectional type checker, effects + capabilities; v0.3 scope-strict + Sendable |
| `sdust-borrow` | ownership/move/borrow/affine/arena analysis; v0.3 field-level Places + NLL last-use |
| `sdust-sir` | mid-level IR + tree-walking interpreter (slice 6); v0.4 loop terminator fix |
| `sdust-runtime` | concurrent tokio runtime: agents, mailboxes, supervisors, budgets (slice 7); v0.3 mid-turn cancel + OTLP + slab pool |
| `sdust-codegen-cranelift` | native backend — JIT + AOT object (slice 8 + v0.2 completion) |
| `sdust-codegen-wasm` | wasm32-wasi / wasm32-web core module + Component Model emitter |
| `sdust-codegen-llvm` | LLVM backend (real lowering behind `--features llvm`; v0.2) |
| `sdust-debuginfo` | DWARF v4 builder + wasm source-map + `name` section (v0.2) |
| `sdust-fmt` | canonical formatter (Wadler/Lindig pretty-printer) |
| `sdust-driver` | compilation pipeline and `star.toml` manifest loader |
| `sdust-pkg` | package manager: resolver, lockfile, fetchers, publish (v0.2); v0.4 GH Releases registry transport |
| `sdust-lsp` | LSP 3.17 server over stdio (v0.2) |
| `sdust-doc` | doc generator (extract + render markdown/HTML) (v0.2) |
| `sdust-stdlib` | real `std.json` / `tls` / `http` / `fs` / `time` / `test` (v0.2) |
| `sdust-macros` | declarative-macro registry + expander + hygiene (v0.4) |
| `sdust-cli` | the `sdust` binary |

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
| 6 | SIR and interpreter | shipped (`v0.6.0-sir`) |
| 7 | runtime MVP (scheduler, mailboxes, supervisors) | shipped (`v0.7.0-runtime`) |
| 8 | native (Cranelift) and Wasm backends | shipped (`v0.8.0-codegen` / `v0.1.0`) |
| **v0.2** | LSP + pkg + doc + full codegen + stdlib + DWARF + Wasm CM | **shipped (`v0.2.0`)** |
| **v0.3** | Soundness hardening: NLL last-use + field Places, scope-strict + Sendable, mid-turn cancel + OTLP + slab mailboxes, v0.2 cleanup (stdlib install, 20/20 wasm-CM, 5→2 ignored) | **shipped (`v0.3.0`)** |
| **v0.4** | Dogfood demos (3), real GH-Releases registry transport, hygienic declarative macros (SD6001..SD6004), self-host lexer (subset bootstrap via `std.io`), SIR loop terminator fix | **shipped (`v0.4.0`)** |
| **v0.5** | `break` / `continue` HIR + iterator protocol + bounded-fixed-point loop borrows, self-host lexer full diff, dogfood completion (5 gaps: real http.serve, Wasm DOM imports, full Str methods, mem-budget auto-charge, FsCap allowlist), macros completion (`name!(args)`, extended hygiene, cross-file `pub macro`, proc-macro skeleton, stdlib macros), LSP advanced (semantic tokens, rename, inlay hints, code actions, signature help, workspace folders, semantic completion) | **shipped (`v0.5.0`)** |

### Post-v0.5 roadmap

| Slice | Scope | Status |
|---|---|---|
| v0.6 | Labelled break/continue + `Iter[T]` trait, self-host parser + HIR + typeck, `!call_expr` precedence fix, cross-file module resolution for non-macro symbols | planned |
| - | Proc-macro execution (sandboxed SIR sub-context), set-of-scopes hygiene, `format!`-style variadic macros, central SD6xxx catalog merge | planned |
| - | `BuiltinId::Dom` SIR lowering + canonical-ABI return-area bridge, `install_agent_dispatch` runtime wiring, per-call FsCap materialisation from sandbox manifest | planned (finishes the v0.5 dogfood end-to-end) |
| - | Multi-file LSP rename + go-to-def, receiver-chain + method-call-receiver completion, borrow check in the LSP pipeline | planned |
| - | Polonius-style borrows, real cap-name resolution wiring, SIR-side cancellation polling, WASI Preview 2 + user WIT, DWARF v5 + per-instr line program | planned |
| - | `dyn` dispatch + closure capture in compiled code, `escalate` supervisor action | planned |
| - | Multi-core scheduler, PGO/ThinLTO, distributed agents, effect-row polymorphism | future |

## License

Stardust is dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in this work shall be dual-licensed as above,
without any additional terms or conditions.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) and
[docs/contributing.md](docs/contributing.md).
