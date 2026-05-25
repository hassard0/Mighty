# Mighty Documentation

Mighty is a statically typed, ownership-based, agent-first systems
language that compiles to native code and to WebAssembly components.
These docs cover the language at toolchain **v0.10** tracking
spec **v1.0-RC2**.

> **Pre-1.0 warning.** The language surface is frozen for v1.0 but
> there is no binary release and no semver guarantee yet. Treat
> Mighty as ready to play with, not yet ready for production. See
> the [FAQ](faq.md) for the full status breakdown.

## Learn the language

- [Getting started](getting-started.md) — install, scaffold a
  package, run `mty check`, run your first agent, run your first
  test.
- [Tour of Mighty](tour/README.md) — work through the 15 tour
  chapters one at a time. Every chapter has a `Try it:` block that
  runs the corresponding canonical example.
- [Language specification v1.0-RC2](spec/v1.0-rc.md) — the
  normative reference.
- [Spec amendments register (A1..A109)](spec/v0.1-amendments.md) —
  the per-decision archive that the consolidated RC2 spec is built
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
    registry. Run `mty explain MTxxxx` for inline help.
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

## Status snapshot (v0.10)

| Component                            | State                    |
|--------------------------------------|--------------------------|
| Lexer, parser, CST                   | shipped                  |
| Typed AST + HIR + lowering           | shipped                  |
| Diagnostics engine                   | shipped (977 tests)      |
| Formatter (per-node)                 | shipped (slice 2)        |
| Type checker                         | shipped                  |
| Borrow / move / affine checker       | shipped                  |
| Effect / capability checker          | shipped                  |
| Codegen (Cranelift)                  | shipped, narrow subset   |
| Codegen (Wasm)                       | shipped, narrow subset   |
| Codegen (LLVM)                       | stub (v0.2 target)       |
| Runtime (scheduler, mailboxes)       | shipped (v0.7)           |
| Supervisors + budgets + sandboxes    | shipped (v0.7)           |
| Macros (decl + sandboxed proc)       | shipped (v0.6 + v0.8)    |
| LSP                                  | shipped (v0.5+)          |
| `mty pkg` cross-file resolve         | shipped (v0.10)          |
| `mty test`                           | shipped (folded in v0.3) |
| `mty doc`                            | stub                     |
| `mty bench`                          | stub                     |
| `mty pkg publish`                    | DEFER-V1.1 (RFC-004)     |
| Self-host (lexer..min-typeck)        | 40/40 tests passing      |

See
[`CHANGELOG.md`](https://github.com/hassard0/Mighty/blob/main/CHANGELOG.md)
at the repo root for the full release history.
