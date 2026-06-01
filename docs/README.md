# Mighty Documentation

Mighty is a statically typed, ownership-based, agent-first systems
language that compiles to native code and to WebAssembly components.
These docs cover the language at toolchain **v0.42** tracking
spec **v1.0-RC5**.

> **Pre-1.0 warning.** The language surface is frozen for v1.0 but
> the binary distribution is still pre-release (cargo-install
> + PGO release binaries on Linux/macOS-x86_64/Windows; homebrew
> formula prepared but not yet in `homebrew-core`). Treat Mighty
> as ready to play with, not yet ready for production. See the
> [FAQ](faq.md) for the full status breakdown.

## Learn the language

- [Getting started](getting-started.md) — install, scaffold a
  package, run `mty check`, run your first agent, run your first
  test.
- [Tour of Mighty](tour/README.md) — work through the 15 tour
  chapters one at a time. Every chapter has a `Try it:` block that
  runs the corresponding canonical example.
- [Language specification v1.0-RC5](spec/v1.0-rc.md) — the
  normative reference.
- [Spec amendments register (A1..A109)](spec/v0.1-amendments.md) —
  the per-decision archive that the consolidated RC5 spec is built
  on.
- [Conformance coverage](spec/conformance-coverage.md) — what the
  conformance corpus exercises (88% of FROZEN surface as of v0.10).

## Use the tools

- [Reference](reference/README.md)
  - [`mty`](reference/cli/mty.md) — overview of all subcommands.
    - [`mty new`](reference/cli/mty-new.md)
    - [`mty check`](reference/cli/mty-check.md)
    - [`mty fmt`](reference/cli/mty-fmt.md)
    - [`mty run`](reference/cli/mty-run.md)
    - [`mty build`](reference/cli/mty-build.md)
    - [`mty dump`](reference/cli/mty-dump.md)
    - [`mty explain`](reference/cli/mty-explain.md)
    - [`mty doc`](reference/cli/mty-doc.md)
    - [`mty pkg`](reference/cli/mty-pkg.md)
    - [`mty lsp`](reference/cli/mty-lsp.md)
  - [Manifest format](reference/manifest.md) — the `mighty.toml`
    schema.
  - [Diagnostic codes](reference/diagnostics.md) — the `MTxxxx`
    registry. Run `mty explain MTxxxx` for inline help (also
    accepts the legacy `SDxxxx` spelling per amendment A107).
  - [Registry](reference/registry.md) — package index format.
  - [Stdlib reference](reference/stdlib/) —
    [fs](reference/stdlib/fs.md),
    [http](reference/stdlib/http.md),
    [json](reference/stdlib/json.md),
    [test](reference/stdlib/test.md),
    [time](reference/stdlib/time.md),
    [tls](reference/stdlib/tls.md).
  - [WebAssembly Interface Types](reference/wit/) — generated WIT
    artefacts for the component targets.

## Demos and benchmarks

- [Demos index](demos/index.md) — runnable end-to-end programs.
- [Benchmarks](benchmarks/index.md) — methodology + numbers for
  parse throughput, mailbox throughput, agent-send latency, HTTP
  throughput, native compile time, and Wasm size.

## Hack on the compiler

- [Internals](internals/README.md) — pipeline overview and per-crate
  notes. Notable pages:
  [architecture](internals/architecture.md),
  [parser](internals/parser.md),
  [typeck](internals/typeck.md),
  [borrowck](internals/borrowck.md),
  [effects](internals/effects.md),
  [macros](internals/macros.md),
  [runtime](internals/runtime.md),
  [scheduler](internals/scheduler.md),
  [supervisors](internals/supervisors.md),
  [self-hosting](internals/self-hosting.md).
- [Contributing](contributing.md) — workflow, tests, style.
- [FAQ](faq.md)
- [Upstream issues](upstream-issues/) — bugs filed against
  Cranelift / wasmtime / etc. that affect Mighty.

## Status snapshot (v0.42, v0.43 in progress)

| Component                            | State                                            |
|--------------------------------------|--------------------------------------------------|
| Lexer, parser, CST                   | shipped                                          |
| Typed AST + HIR + lowering           | shipped                                          |
| Diagnostics engine                   | shipped                                          |
| Formatter (per-node)                 | shipped; v0.43 starts syntax-aware top-level `const` formatting |
| Type checker                         | shipped                                          |
| Borrow / move / affine checker       | shipped                                          |
| Effect / capability checker          | shipped                                          |
| Codegen (Cranelift) native           | shipped; v0.42 fixed typed log + Vec liveness, v0.43 adds truthful link diagnostics |
| Codegen (Wasm) P2 + cabi_realloc     | shipped (v0.18)                                  |
| Codegen (LLVM)                       | opt-in backend                                   |
| Runtime (scheduler, mailboxes)       | shipped                                          |
| Supervisors + budgets + sandboxes    | shipped                                          |
| Macros (decl + sandboxed proc)       | shipped                                          |
| LSP + DAP                            | shipped (LSP v0.5+, DAP v0.32 Track A)           |
| `mty pkg` (resolve / fetch / publish)| shipped                                          |
| Hot reload (`mty reload`)            | shipped (v0.17/18)                               |
| Replay (`mty replay --byte-identical`)| shipped (v0.17/18)                              |
| Cluster (distributed agents)         | shipped (v0.18 Tier 4.1)                         |
| LLM-agent stack (std.llm/mcp/memory/swarm) | shipped (v0.26–v0.30)                      |
| Extern C + `[[extern_lib]]`          | shipped (v0.36 T2); v0.43 reports real linker failures distinctly from missing linkers |
| String position/range ops + MT5080   | shipped (v0.36 T3)                               |
| Stardust→Mighty rebrand compat       | shipped (v0.36 T4 — STARDUST_* env legacy)       |
| PGO release binaries                 | shipped (v0.36 T5 — Linux + macOS-x86_64 + Win)  |
| `mty find` / `mty fix` / `mty hooks` | shipped (v0.33/v0.34/v0.35)                      |
| Self-host (lexer..min-typeck)        | 40/40 tests passing                              |

Current v0.43 candidates are driven by the Mighty IDE lessons log:
short-circuit logical lowering, interpreter writeback for
statement-form `Vec`/`String` mutators, top-level `const` formatting,
prefix-call parsing, and native link diagnostics.

See
[`CHANGELOG.md`](https://github.com/hassard0/Mighty/blob/main/CHANGELOG.md)
at the repo root for the full release history.
