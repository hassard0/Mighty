# Mighty v0.1 — Release Notes

**Tag:** `v0.1.0`
**Date:** 2026-05-24
**Status:** SHIPPED — first feature-complete release

Mighty is an agent-first systems programming language. v0.1 is the
first release that walks the full spec §31 roadmap: parser through
borrow-check through MtyIR through runtime through native + Wasm
codegen. You can write, type-check, run, and compile real Mighty
programs end-to-end.

## What you can do

```bash
# Compile the toolchain
cargo install --path crates/mty-cli

# Scaffold a new package
mty new hello && cd hello

# Type-check
mty check src/main.sd

# Run via JIT (Cranelift), falls back to interpreter on shapes the
# slice-8 backend doesn't yet cover
mty run src/main.sd

# Build a native object (linker-permitting, a real executable)
mty build src/main.sd

# Build a WebAssembly module
mty build --target wasm32-wasi src/main.sd
wasmtime target/main.wasm

# Format
mty fmt src/

# Inspect IR
mty dump --sir src/main.sd

# Explain any diagnostic code
mty explain MT8005
```

## The eight slices

| Slice | Scope | Tag | Tests added |
|-------|-------|-----|-------------|
| 1 | parser, formatter, HIR, CLI, examples | `v0.1.0-phase1` | 158 |
| 2 | per-node formatter, lambdas, if-let, turbofish | `v0.2.0-phase1-polish` | +40 |
| 3 | type checker, generics MVP, `?` propagation | `v0.3.0-typeck` | +30 |
| 4 | ownership / borrow / affine / arena | `v0.4.0-borrowck` | +25 |
| 5 | effects, capabilities, traits, `dyn`, derives | `v0.5.0-effects` | +20 |
| 6 | MtyIR and interpreter | `v0.6.0-sir` | +17 |
| 7 | runtime MVP (scheduler, mailboxes, supervisors) | `v0.7.0-runtime` | +37 |
| 8 | native (Cranelift) and Wasm backends | `v0.8.0-codegen` | +49 |
| **Total** | | **`v0.1.0`** | **376 passing** |

## Headline numbers

- **376 tests pass** (0 failures, 0 ignored)
- **0 clippy warnings** with `-D warnings`
- **14 crates** in the workspace
- **168 Rust source files**
- **~28 000 lines of Rust** total
- **72 commits on `main`** (slice-leader development branches squash-merged)
- **53 spec amendments (A1..A53)** documenting v0.1 decisions
- **65+ diagnostic codes** across SD0xxx..SD8xxx ranges

## Language features (per spec §31)

All v0.1 surfaces are shipped:

- Logos lexer, rowan CST, Pratt-style expression parser, error recovery
- HIR with name resolution and arena storage
- Wadler/Lindig pretty-printer (`mty fmt`)
- Bidirectional type checker with Hindley-Milner inference
- Generics with MVP monomorphization
- `?`-propagation for `Result` types
- Ownership / move / borrow checker with affine + arena tracking
- Effect system + capabilities (`fs`, `net`, `time`, `rand`, `model`)
- Trait dispatch + `dyn Trait` fat pointers + derives
- Strict protocol enforcement for agent message handling
- Mid-level IR (MtyIR) with basic-block form
- Tree-walking interpreter (`mty run --legacy-interp`)
- Tokio-backed concurrent runtime with mailboxes, supervisors, deadline
  timers, deterministic mode, telemetry, mini HTTP server
- Cranelift JIT for `mty run` (default)
- Cranelift AOT object emission + platform linker for
  `mty build --target native`
- Wasm core-module emission for `wasm32-wasi` and `wasm32-web`
- Real `bumpalo`-backed arena allocator
- `libloading`-backed `extern { fn ... }` resolution

## Toolchain

- **MSRV: Rust 1.85** (bumped from 1.82 in slice 8; cranelift's
  transitive deps require edition2024)
- All-platform: Windows, macOS, Linux
- Cargo workspace; no `build.rs` magic

## Spec amendments

53 amendments (A1..A53) document interpretations made during
implementation. The most consequential:

- **A1** — `k` / `M` suffixes for decimal size literals (slice 2)
- **A21** — Arena lifetime escape detection (slice 4)
- **A30** — Effect-system capability + sandbox model (slice 5)
- **A37** — Slice-7 memory budget approximation
- **A39** — Deterministic mode = current-thread + seeded RNG + logical clock
- **A46** — Cranelift-only native backend (LLVM scaffold-only in v0.1)
- **A47** — Wasm Component Model deferred to v0.2
- **A48** — `mty run` defaults to JIT
- **A52** — Native linker discovery order

See `docs/spec/v0.1-amendments.md` for the full list.

## Deferred to v0.2 / post-v0.1

What's NOT in v0.1, intentionally:

- LSP server
- Package manager + registry
- LLVM backend code generation (scaffold ships in slice 8)
- Full Wasm Component Model + `wit-component` integration
- Full MtyIR coverage in native codegen (ADT construct/destructure,
  `?` propagation, agent dispatch via compiled handlers)
- Per-(fn, type-args) shared-generic monomorphization
- PGO / ThinLTO
- Multi-core work-stealing scheduler
- Cross-machine distributed agents
- Procedural macros
- True NLL / Polonius
- Effect-row polymorphism
- DWARF / Wasm source maps (beyond function-level stubs)
- Strict OTLP wire format for telemetry
- Field-level borrow tracking

These are the v0.2 backlog.

## Backwards compatibility

v0.1.0 establishes the SD-coded diagnostic catalogue, the slice-1
parser surface, the spec amendments doc, and the basic CLI shape.
Future v0.1.x patch releases will not break any of:

- Source-code syntax (slice 1-5 surface)
- Diagnostic codes
- `mty check` / `mty run` / `mty build` / `mty fmt` / `mty dump` /
  `mty explain` / `mty new`
- Public Rust API of `mty-driver`

v0.2 may evolve these.

## Acknowledgments

The slice-by-slice spec-driven build worked because the spec did the
heavy lifting first. Each slice was scoped from spec §31, designed
in `docs/superpowers/specs/`, planned in `docs/superpowers/plans/`,
executed mostly by subagent swarms, gated by review checkpoints,
and tagged. Slice-leaders carried context across pre-emptions; the
test count is the truest measure of "did it land?".

Big thanks to the Cranelift, bumpalo, libloading, wasm-encoder, and
rowan teams — Mighty v0.1 stands on those shoulders.

## What's next

v0.2 picks up the LLVM backend, completes MtyIR coverage in codegen,
and ships the Wasm Component Model wrapper. The aspirational v0.2
tagline: *"every example compiles to a real binary"*.
